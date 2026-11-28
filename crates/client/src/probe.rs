//! The plain-JSON state behind `window.__lightseek.state()`, for automated
//! browser checks. Positions are `game` ground coordinates: `x` is world X
//! and `y` is world Z.

use serde::Serialize;

use crate::ui::{Camera, Screen};
use game::{Phase, PlayerId, ViewState};

#[derive(Debug, Serialize)]
pub struct Probe {
    pub screen: String,
    /// "follow", or "spectate" with `watching` set.
    pub camera: String,
    pub watching: Option<u32>,
    pub phase: String,
    pub winner: Option<String>,
    pub you: Option<u32>,
    pub role: Option<String>,
    pub status: Option<String>,
    pub host: Option<u32>,
    pub seeker: Option<u32>,
    pub roster: Vec<Member>,
    pub avatars: Vec<Avatar>,
    pub world: Option<World>,
    pub hiders_left: usize,
    pub seconds_left: f32,
    pub blind: bool,
    /// Your meters, 0 to 1, while you are a living hider.
    pub food: Option<f32>,
    pub water: Option<f32>,
    /// You stand in the danger zone, and the seconds you have left there.
    pub danger: bool,
    pub danger_left: Option<f32>,
    /// The safe zone's half width and ceiling.
    pub zone: Option<[f32; 2]>,
    /// Ripe food as `[x, y]`.
    pub ripe_food: Vec<[f32; 2]>,
    pub gpu: String,
}

#[derive(Debug, Serialize)]
pub struct Member {
    pub id: u32,
    pub name: String,
    pub control: String,
    pub seat: String,
    pub link: String,
}

#[derive(Debug, Serialize)]
pub struct Avatar {
    pub id: u32,
    pub me: bool,
    pub x: f32,
    pub y: f32,
    /// Feet height above the ground.
    pub up: f32,
    pub vy: f32,
    pub yaw: f32,
    pub role: String,
    pub status: String,
}

#[derive(Debug, Serialize)]
pub struct World {
    pub kind: String,
    pub solids: usize,
    pub props: usize,
    /// Each launch pad as `[x, y, height]`.
    pub pads: Vec<[f32; 3]>,
    /// Each spring as `[x, y, radius]`.
    pub springs: Vec<[f32; 3]>,
}

fn name(v: impl std::fmt::Debug) -> String {
    format!("{v:?}")
}

pub fn probe(view: Option<&ViewState<'_>>, camera: Camera, gpu: &str) -> Probe {
    let (camera, watching) = match camera {
        Camera::Follow => ("follow", None),
        Camera::Spectate(PlayerId(id)) => ("spectate", Some(id)),
    };
    let mut p = Probe {
        screen: name(Screen::of(view)),
        camera: camera.into(),
        watching,
        phase: "Connecting".into(),
        winner: None,
        you: None,
        role: None,
        status: None,
        host: None,
        seeker: None,
        roster: Vec::new(),
        avatars: Vec::new(),
        world: None,
        hiders_left: 0,
        seconds_left: 0.0,
        blind: false,
        food: None,
        water: None,
        danger: false,
        danger_left: None,
        zone: None,
        ripe_food: Vec::new(),
        gpu: gpu.into(),
    };
    let Some(v) = view else { return p };
    p.phase = match v.phase {
        Phase::Lobby => "Lobby",
        Phase::Hiding { .. } => "Hiding",
        Phase::Seeking { .. } => "Seeking",
        Phase::Results { winner, .. } => {
            p.winner = Some(name(winner));
            "Results"
        }
    }
    .into();
    p.you = Some(v.you.0);
    p.role = v.me.map(|m| name(m.role));
    p.status = v.me.map(|m| name(m.status));
    p.host = v.host.map(|h| h.0);
    p.seeker = v.seeker.map(|s| s.0);
    p.roster = v
        .roster
        .iter()
        .map(|m| Member {
            id: m.id.0,
            name: m.name.clone(),
            control: name(m.control),
            seat: name(m.seat),
            link: name(m.link),
        })
        .collect();
    p.avatars =
        v.me.iter()
            .chain(&v.others)
            .map(|a| Avatar {
                id: a.id.0,
                me: a.id == v.you,
                x: a.body.pos.x,
                y: a.body.pos.y,
                up: a.body.y,
                vy: a.body.vy,
                yaw: a.body.yaw,
                role: name(a.role),
                status: name(a.status),
            })
            .collect();
    p.world = v.world.map(|w| World {
        kind: name(w.kind),
        solids: w.solids.len(),
        props: w.props.len(),
        pads: w
            .pads
            .iter()
            .map(|d| [d.center.x, d.center.y, d.y])
            .collect(),
        springs: w
            .springs
            .iter()
            .map(|s| [s.center.x, s.center.y, s.radius])
            .collect(),
    });
    p.food = v.vitals.map(|v| v.food);
    p.water = v.vitals.map(|v| v.water);
    p.danger = v.danger;
    p.danger_left = v.vitals.filter(|_| v.danger).map(|v| v.danger_left());
    p.zone = v.zone.map(|z| [z.half, z.ceiling]);
    p.ripe_food = v.food.iter().map(|f| [f.x, f.z]).collect();
    p.hiders_left = v.hiders_left;
    p.seconds_left = v.seconds_left;
    p.blind = v.blind();
    p
}

#[cfg(test)]
mod tests {
    use game::{Event, LobbyConfig, Replica, Room, SceneKind, Step};

    use super::*;

    /// Plays the wire between a real `Room` and one `Replica`.
    fn deliver(rep: &mut Option<Replica>, step: Step) {
        for (_, frame) in step.send {
            match rep {
                None => *rep = Some(Replica::new(frame.as_bytes()).unwrap()),
                Some(r) => {
                    let _: Option<Event> = r.receive(frame.as_bytes(), 0.0).unwrap();
                }
            }
        }
    }

    #[test]
    fn the_probe_reports_a_running_round_as_json() {
        let mut room = Room::new(5);
        let conn = game::ConnId(1);
        let mut rep = None;
        deliver(
            &mut rep,
            room.join(conn, Replica::hello("ann", None).as_bytes())
                .unwrap(),
        );
        let config = LobbyConfig {
            humans: 1,
            bots: 1,
            scene: Some(SceneKind::City),
            ..LobbyConfig::default()
        };
        deliver(
            &mut rep,
            room.frame(conn, Replica::configure(config).as_bytes())
                .unwrap(),
        );
        deliver(
            &mut rep,
            room.frame(conn, Replica::start().as_bytes()).unwrap(),
        );
        deliver(&mut rep, room.tick());
        let rep = rep.unwrap();

        let view = rep.view(0.0);
        let json = serde_json::to_value(probe(Some(&view), Camera::Follow, "ok")).unwrap();
        assert_eq!(json["screen"], "Game");
        assert_eq!(json["phase"], "Hiding");
        assert_eq!(json["you"], rep.you().0);
        assert_eq!(json["roster"][0]["name"], "ann");
        assert_eq!(json["world"]["kind"], "City");
        assert_eq!(json["world"]["solids"], room.world().unwrap().solids.len());
        let me = &json["avatars"][0];
        assert_eq!(me["me"], true);
        assert_eq!(json["role"], me["role"]);
        assert_eq!(json["status"], "Active");
        let world = room.world().unwrap();
        assert_eq!(
            json["ripe_food"].as_array().unwrap().len(),
            world.food.len()
        );
        assert_eq!(
            json["world"]["springs"].as_array().unwrap().len(),
            world.springs.len()
        );
        assert_eq!(json["zone"][0], game::rules::zone(world, 0.0).half);
        assert_eq!(json["danger"], false);
        assert_eq!(json["food"].is_number(), me["role"] == "Hider");

        let none = serde_json::to_value(probe(None, Camera::Follow, "")).unwrap();
        assert_eq!(none["screen"], "Connecting");
    }
}
