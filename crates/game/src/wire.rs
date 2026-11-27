//! The byte format. Everything here except `Frame` and `WireError` is private.
//!
//! `Room` and `Replica` take bytes and return `Frame`s, so neither shell
//! names a message and postcard stays changeable in this file.
//!
//! Layout: byte 0 is `VERSION`, the rest is postcard. Decode rejects another
//! version, trailing bytes, and non-finite floats (via `geom`'s leaf types).
//!
//! WIRE STABILITY: enum variants and struct fields are append-only.

use serde::{Deserialize, Serialize};

use crate::model::{AvatarState, Input, LobbyConfig, Member, Phase, PlayerId, Refusal, Role, Tick};
use crate::world::World;

pub(crate) const VERSION: u8 = 2;

/// One WebSocket binary message. Opaque to shells.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame(pub(crate) Vec<u8>);

impl Frame {
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WireError {
    Version {
        got: u8,
    },
    Malformed,
    /// A well-formed message that is not valid at this point in the protocol.
    Unexpected,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) enum ClientMsg {
    /// First frame on every socket. `resume` is the id a reloaded client held.
    Hello {
        name: String,
        resume: Option<PlayerId>,
    },
    Configure(LobbyConfig),
    Start,
    Input(Input),
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) enum ServerMsg {
    /// First frame on every socket.
    Welcome {
        you: PlayerId,
        host: Option<PlayerId>,
        config: LobbyConfig,
        roster: Vec<Member>,
    },
    /// The host, config, or roster changed. Sent in any phase.
    Roster {
        host: Option<PlayerId>,
        config: LobbyConfig,
        roster: Vec<Member>,
    },
    /// Also sent to anyone who joins while the round runs.
    RoundStart {
        world: World,
        seeker: PlayerId,
        roster: Vec<Member>,
    },
    /// Every tick during a round. Filtered per recipient: while hiding, the
    /// seeker gets no hiders. `ack` is the last `Input::tick` consumed for the
    /// recipient's avatar.
    Snapshot {
        tick: Tick,
        phase: Phase,
        ack: Tick,
        avatars: Vec<AvatarState>,
    },
    Caught {
        hider: PlayerId,
        by: PlayerId,
    },
    RoundEnd {
        winner: Role,
    },
    Refused(Refusal),
    /// Sent to a socket whose player a newer socket resumed, just before it
    /// closes. Its
    /// client must not resume that player again.
    Replaced,
}

pub(crate) fn encode<T: Serialize>(msg: &T) -> Vec<u8> {
    let mut out = vec![VERSION];
    out.extend(postcard::to_allocvec(msg).expect("in-memory encode cannot fail"));
    out
}

pub(crate) fn frame<T: Serialize>(msg: &T) -> Frame {
    Frame(encode(msg))
}

pub(crate) fn decode<T: for<'de> Deserialize<'de>>(bytes: &[u8]) -> Result<T, WireError> {
    let (&version, body) = bytes.split_first().ok_or(WireError::Malformed)?;
    if version != VERSION {
        return Err(WireError::Version { got: version });
    }
    match postcard::take_from_bytes(body) {
        Ok((msg, [])) => Ok(msg),
        _ => Err(WireError::Malformed),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::{Vec2, Vec3};
    use crate::model::SceneKind;
    use crate::world::{Material, Prop, Shape};

    fn round_start(world: World) -> ServerMsg {
        ServerMsg::RoundStart {
            world,
            seeker: PlayerId(3),
            roster: Vec::new(),
        }
    }

    fn input(mv: Vec2, yaw: f32) -> ClientMsg {
        ClientMsg::Input(Input {
            tick: Tick(9),
            mv,
            yaw,
            jump: true,
        })
    }

    #[test]
    fn a_generated_world_round_trips() {
        for kind in SceneKind::ALL {
            let world = World::generate(kind, 42);
            let bytes = encode(&round_start(world.clone()));
            let Ok(ServerMsg::RoundStart { world: back, .. }) = decode(&bytes) else {
                panic!("{kind:?} did not decode");
            };
            assert_eq!(back, world);
        }
    }

    #[test]
    fn decode_rejects_non_finite_floats_in_inputs_and_worlds() {
        let ok = encode(&input(Vec2::new(1.0, 0.0), 0.5));
        assert!(decode::<ClientMsg>(&ok).is_ok());
        for bad in [
            input(Vec2::new(f32::NAN, 0.0), 0.0),
            input(Vec2::new(0.0, f32::INFINITY), 0.0),
            input(Vec2::ZERO, f32::NAN),
            input(Vec2::ZERO, f32::NEG_INFINITY),
        ] {
            assert_eq!(
                decode::<ClientMsg>(&encode(&bad)).err(),
                Some(WireError::Malformed),
                "{bad:?}"
            );
        }

        let Ok(ClientMsg::Input(back)) = decode::<ClientMsg>(&ok) else {
            panic!("an input decodes");
        };
        assert!(back.jump, "the jump survives");

        let snapshot = |y: f32, vy: f32| ServerMsg::Snapshot {
            tick: Tick(1),
            phase: crate::model::Phase::Lobby,
            ack: Tick(0),
            avatars: vec![AvatarState {
                id: PlayerId(1),
                body: crate::model::Body {
                    y,
                    vy,
                    ..Default::default()
                },
                role: Role::Hider,
                status: crate::model::Status::Active,
            }],
        };
        assert!(decode::<ServerMsg>(&encode(&snapshot(2.0, -3.0))).is_ok());
        for (y, vy) in [(f32::NAN, 0.0), (0.0, f32::INFINITY)] {
            assert_eq!(
                decode::<ServerMsg>(&encode(&snapshot(y, vy))).err(),
                Some(WireError::Malformed),
                "y {y} vy {vy}"
            );
        }

        let mut world = World::generate(SceneKind::Park, 1);
        world.solids[2].base = f32::NAN;
        assert_eq!(
            decode::<ServerMsg>(&encode(&round_start(world))).err(),
            Some(WireError::Malformed)
        );
        let mut world = World::generate(SceneKind::Park, 1);
        world.solids[3].height = f32::NAN;
        assert_eq!(
            decode::<ServerMsg>(&encode(&round_start(world))).err(),
            Some(WireError::Malformed)
        );
        let mut world = World::generate(SceneKind::City, 1);
        world.props[0].center.y = f32::INFINITY;
        assert_eq!(
            decode::<ServerMsg>(&encode(&round_start(world))).err(),
            Some(WireError::Malformed)
        );
    }

    #[test]
    fn decode_rejects_a_material_it_does_not_know() {
        let mut world = World::open(SceneKind::Park);
        world.props.push(Prop {
            shape: Shape::Box,
            center: Vec3::new(0.0, 0.5, 0.0),
            size: Vec3::new(1.0, 1.0, 1.0),
            color: [1, 2, 3],
            material: Material::Water,
        });
        let mut bytes = encode(&world);
        assert_eq!(decode::<World>(&bytes), Ok(world));
        // The prop's material is the last byte before the empty spawn and pad
        // lists.
        let at = bytes.len() - 3;
        assert_eq!(bytes[at], Material::Water as u8);
        bytes[at] = 200;
        assert_eq!(decode::<World>(&bytes).err(), Some(WireError::Malformed));
    }

    #[test]
    fn decode_rejects_trailing_bytes_wrong_version_and_empty() {
        let mut bytes = encode(&ClientMsg::Start);
        assert!(decode::<ClientMsg>(&bytes).is_ok());
        bytes.push(0);
        assert_eq!(
            decode::<ClientMsg>(&bytes).err(),
            Some(WireError::Malformed)
        );

        let mut bytes = encode(&ClientMsg::Start);
        bytes[0] = VERSION + 1;
        assert_eq!(
            decode::<ClientMsg>(&bytes).err(),
            Some(WireError::Version { got: VERSION + 1 })
        );
        assert_eq!(decode::<ClientMsg>(&[]).err(), Some(WireError::Malformed));
    }
}
