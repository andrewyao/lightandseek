//! The shared vocabulary of a room: ids, roles, inputs, phases, the roster.

use serde::{Deserialize, Serialize};

use crate::geom::{Vec2, finite};

/// A player in one room. Never reused within the room. Bots get one too.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct PlayerId(pub u32);

/// One socket, minted by the shell. A reconnect is a new `ConnId` that
/// resumes the same `PlayerId`.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ConnId(pub u64);

/// One fixed 50 ms step. Every deadline is a `Tick`.
#[derive(
    Copy, Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
pub struct Tick(pub u32);

impl Tick {
    pub fn plus(self, ticks: u32) -> Tick {
        Tick(self.0 + ticks)
    }

    /// Ticks from `self` until `later`, 0 if `later` has passed.
    pub fn until(self, later: Tick) -> u32 {
        later.0.saturating_sub(self.0)
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Role {
    Seeker,
    Hider,
}

/// `Caught` hiders spectate until the round ends.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Status {
    Active,
    Caught,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Control {
    Human,
    Bot,
}

/// Where an avatar stands and which way it faces. `yaw` is radians about +Y,
/// see `geom::facing`. `pos` is on the ground plane and `y` is the height of
/// the feet above it; `vy` is vertical speed, 0 while standing.
#[derive(Copy, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Body {
    pub pos: Vec2,
    #[serde(deserialize_with = "finite")]
    pub yaw: f32,
    #[serde(deserialize_with = "finite")]
    pub y: f32,
    #[serde(deserialize_with = "finite")]
    pub vy: f32,
}

/// One client tick of intent. The client sends exactly one per 50 ms.
#[derive(Copy, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Input {
    /// The client's own counter, echoed back as `ack`. Strictly increasing.
    pub tick: Tick,
    /// `x` is forward, `y` is strafe-right. Length at most 1 after `clamped`.
    pub mv: Vec2,
    #[serde(deserialize_with = "finite")]
    pub yaw: f32,
    /// Jump if standing. Holding it hops again on landing.
    pub jump: bool,
}

impl Input {
    /// Scales `mv` down to unit length by the vector, not per axis, so a
    /// diagonal is no faster than straight ahead.
    pub fn clamped(self) -> Input {
        let len = self.mv.length();
        let mv = if len > 1.0 {
            self.mv * (1.0 / len)
        } else {
            self.mv
        };
        Input { mv, ..self }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Phase {
    Lobby,
    /// The seeker is frozen and blind until `until`.
    Hiding {
        until: Tick,
    },
    Seeking {
        until: Tick,
    },
    Results {
        until: Tick,
        winner: Role,
    },
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SceneKind {
    Park,
    City,
    Kitchen,
}

impl SceneKind {
    pub const ALL: [SceneKind; 3] = [SceneKind::Park, SceneKind::City, SceneKind::Kitchen];
}

/// What the host sets in the lobby. `seed`, `hide_secs`, and `seek_secs` are
/// also test knobs.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LobbyConfig {
    pub humans: u8,
    pub bots: u8,
    /// `None` picks a random scene each round.
    pub scene: Option<SceneKind>,
    /// Overrides the room's seed at `Start`, so the rounds that follow are
    /// reproducible.
    pub seed: Option<u64>,
    pub hide_secs: u8,
    pub seek_secs: u16,
}

impl Default for LobbyConfig {
    fn default() -> LobbyConfig {
        LobbyConfig {
            humans: 1,
            bots: 3,
            scene: None,
            seed: None,
            hide_secs: 15,
            seek_secs: 180,
        }
    }
}

/// Whether a member plays this round (or would, if the lobby started now).
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Seat {
    Playing,
    Spectating,
}

/// `Away` is a human inside the reconnect grace period.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Link {
    Online,
    Away,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Member {
    pub id: PlayerId,
    pub name: String,
    pub control: Control,
    pub seat: Seat,
    pub link: Link,
}

/// One avatar in a snapshot.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AvatarState {
    pub id: PlayerId,
    pub body: Body,
    pub role: Role,
    pub status: Status,
}

/// Why the room ignored a `Configure` or `Start`.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Refusal {
    NotHost,
    NotInLobby,
    /// Outside the limits in `rules`.
    BadConfig,
    /// `Start` needs exactly `want` connected humans.
    HumansMissing {
        have: u8,
        want: u8,
    },
    /// `Start` needs at least 2 players.
    TooFewPlayers,
}
