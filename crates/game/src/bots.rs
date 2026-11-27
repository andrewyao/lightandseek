//! Bot brains. Each tick a brain looks at the room and returns an `Input`,
//! which goes through the same `physics::step` as a human's.

use crate::geom::{Vec2, facing, yaw_toward};
use crate::model::Body;
use crate::model::{AvatarState, Input, Phase, PlayerId, Role, Status, Tick};
use crate::rng::Rng;
use crate::rules::{CATCH_DIST, DT, in_sight, speed};
use crate::world::World;
use crate::world::nav::NavGrid;

/// Half of the seeker bot's 70 degree field of view.
const FOV_HALF: f32 = 35.0 * std::f32::consts::PI / 180.0;
const VIEW_DIST: f32 = 15.0;
/// A hider bot bolts when it sees the seeker this close.
const FLEE_DIST: f32 = 8.0;
/// Exploration memory is a grid of squares this wide.
const REGION: f32 = 5.0;
const WAYPOINT_REACHED: f32 = 0.3;
/// Ticks of no progress before a bot gives up on its path.
const STUCK_TICKS: u32 = 10;
/// A seeker jumps for a hider this far above its feet, within reach.
const JUMP_FOR: f32 = 0.3;

/// What a brain may see of the room this tick.
pub(crate) struct Senses<'a> {
    pub me: AvatarState,
    pub now: Tick,
    pub phase: Phase,
    pub avatars: &'a [AvatarState],
    pub world: &'a World,
    pub nav: &'a NavGrid,
}

impl Senses<'_> {
    fn sees(&self, target: Body, dist: f32, half_angle: Option<f32>) -> bool {
        let to = target.pos - self.me.body.pos;
        let d = to.length();
        d < dist
            && half_angle.is_none_or(|h| facing(self.me.body.yaw).dot(to) >= d * h.cos())
            && in_sight(self.me.body, target, self.world)
    }
}

#[derive(Debug)]
pub(crate) struct Brain {
    mind: Mind,
    path: Vec<Vec2>,
    last_pos: Vec2,
    stuck: u32,
}

#[derive(Debug)]
enum Mind {
    /// Explores, remembering which regions it has stood in.
    Seeker {
        visited: Vec<bool>,
        chasing: Option<PlayerId>,
    },
    Hider(Hide),
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum Hide {
    /// No spot chosen yet.
    Choosing,
    /// Walking to cover or holding it.
    Covered,
    /// Running from a seeker it saw, until `until`.
    Fleeing { until: Tick },
}

impl Brain {
    pub fn new(role: Role) -> Brain {
        let regions = ((2.0 * crate::world::HALF_EXTENT / REGION) as usize).pow(2);
        Brain {
            mind: match role {
                Role::Seeker => Mind::Seeker {
                    visited: vec![false; regions],
                    chasing: None,
                },
                Role::Hider => Mind::Hider(Hide::Choosing),
            },
            path: Vec::new(),
            last_pos: Vec2::ZERO,
            stuck: 0,
        }
    }

    pub fn think(&mut self, s: &Senses, rng: &mut Rng) -> Input {
        let me = s.me;
        if me.status == Status::Caught {
            return still(me.body.yaw);
        }
        if !self.path.is_empty() && me.body.pos.distance(self.last_pos) < 0.02 {
            self.stuck += 1;
        } else {
            self.stuck = 0;
        }
        if self.stuck > STUCK_TICKS {
            self.path.clear();
            self.stuck = 0;
        }
        self.last_pos = me.body.pos;
        match &mut self.mind {
            Mind::Seeker { visited, chasing } => {
                let Phase::Seeking { .. } = s.phase else {
                    return still(me.body.yaw);
                };
                if let Some(r) = region(me.body.pos) {
                    visited[r] = true;
                }
                let target = s
                    .avatars
                    .iter()
                    .filter(|a| a.role == Role::Hider && a.status == Status::Active)
                    .filter(|a| {
                        let fov = (*chasing != Some(a.id)).then_some(FOV_HALF);
                        s.sees(a.body, VIEW_DIST, fov)
                    })
                    .min_by(|a, b| {
                        let (da, db) = (
                            a.body.pos.distance(me.body.pos),
                            b.body.pos.distance(me.body.pos),
                        );
                        da.total_cmp(&db)
                    });
                if let Some(t) = target {
                    *chasing = Some(t.id);
                    self.path.clear();
                    let above = t.body.y - me.body.y;
                    let near = t.body.pos.distance(me.body.pos) < CATCH_DIST;
                    return Input {
                        jump: near && above > JUMP_FOR,
                        ..toward(me.body.pos, t.body.pos)
                    };
                }
                if chasing.take().is_some() {
                    self.path.clear();
                }
                if self.path.is_empty() {
                    self.path = explore(s, visited, rng).unwrap_or_default();
                }
            }
            Mind::Hider(hide) => {
                let seeker = s.avatars.iter().find(|a| a.role == Role::Seeker);
                let threat = seeker
                    .filter(|_| matches!(s.phase, Phase::Seeking { .. }))
                    .filter(|k| s.sees(k.body, FLEE_DIST, None));
                match (*hide, threat) {
                    (Hide::Fleeing { until }, _) if s.now < until && !self.path.is_empty() => {}
                    (_, Some(k)) => {
                        self.path = flee(s, k.body.pos, rng).unwrap_or_default();
                        *hide = Hide::Fleeing {
                            until: s.now.plus(20),
                        };
                    }
                    (Hide::Choosing, None) => {
                        let from = seeker.map_or(me.body.pos, |k| k.body.pos);
                        self.path = cover(s, from, rng).unwrap_or_default();
                        *hide = Hide::Covered;
                    }
                    (Hide::Fleeing { .. }, None) => *hide = Hide::Covered,
                    (Hide::Covered, None) => {}
                }
            }
        }
        self.follow(me.body.pos, me.body.yaw)
    }

    fn follow(&mut self, pos: Vec2, yaw: f32) -> Input {
        while self
            .path
            .first()
            .is_some_and(|w| w.distance(pos) < WAYPOINT_REACHED)
        {
            self.path.remove(0);
        }
        match self.path.first() {
            Some(&w) => toward(pos, w),
            None => still(yaw),
        }
    }
}

fn still(yaw: f32) -> Input {
    Input {
        tick: Tick(0),
        mv: Vec2::ZERO,
        yaw,
        jump: false,
    }
}

fn toward(from: Vec2, to: Vec2) -> Input {
    Input {
        tick: Tick(0),
        mv: Vec2::new(1.0, 0.0),
        yaw: yaw_toward(to - from),
        jump: false,
    }
}

fn region(p: Vec2) -> Option<usize> {
    let n = (2.0 * crate::world::HALF_EXTENT / REGION) as i32;
    let x = ((p.x + crate::world::HALF_EXTENT) / REGION).floor() as i32;
    let y = ((p.y + crate::world::HALF_EXTENT) / REGION).floor() as i32;
    (x >= 0 && y >= 0 && x < n && y < n).then(|| (y * n + x) as usize)
}

/// A path to a reachable cell, preferring regions not yet visited.
fn explore(s: &Senses, visited: &[bool], rng: &mut Rng) -> Option<Vec<Vec2>> {
    let mut best: Option<(f32, usize)> = None;
    for _ in 0..12 {
        let c = s.nav.random_reachable(rng)?;
        let at = s.nav.center(c);
        let fresh = region(at).is_some_and(|r| !visited[r]);
        let score = if fresh { 100.0 } else { 0.0 } - at.distance(s.me.body.pos) * 0.5;
        if best.is_none_or(|(b, _)| score > b) {
            best = Some((score, c));
        }
    }
    s.nav.path(s.me.body.pos, best?.1)
}

/// A path to a cell surrounded by solids and far from `seeker`, close enough
/// to reach before hiding ends.
fn cover(s: &Senses, seeker: Vec2, rng: &mut Rng) -> Option<Vec<Vec2>> {
    let left = match s.phase {
        Phase::Hiding { until } => s.now.until(until),
        _ => 0,
    };
    let reach = (left as f32 * speed(Role::Hider) * DT * 0.6).max(3.0);
    let mut best: Option<(f32, usize)> = None;
    for _ in 0..40 {
        let c = s.nav.random_reachable(rng)?;
        let at = s.nav.center(c);
        if at.distance(s.me.body.pos) > reach {
            continue;
        }
        let score = s.nav.cover(c, 2) as f32 + at.distance(seeker) * 0.3;
        if best.is_none_or(|(b, _)| score > b) {
            best = Some((score, c));
        }
    }
    s.nav.path(s.me.body.pos, best?.1)
}

/// A path to the sampled cell farthest from `seeker` that does not lie past it.
fn flee(s: &Senses, seeker: Vec2, rng: &mut Rng) -> Option<Vec<Vec2>> {
    let me = s.me.body.pos;
    let away = (me - seeker).normalize_or_zero();
    let mut best: Option<(f32, usize)> = None;
    for _ in 0..30 {
        let c = s.nav.random_reachable(rng)?;
        let at = s.nav.center(c);
        if (at - me).dot(away) < 0.0 {
            continue;
        }
        let score = at.distance(seeker);
        if best.is_none_or(|(b, _)| score > b) {
            best = Some((score, c));
        }
    }
    s.nav.path(me, best?.1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::SceneKind;
    use crate::physics;
    use crate::rules::{SEEKER_SPEED, can_catch};

    fn avatar(id: u32, role: Role, pos: Vec2) -> AvatarState {
        AvatarState {
            id: PlayerId(id),
            body: Body {
                pos,
                ..Body::default()
            },
            role,
            status: Status::Active,
        }
    }

    #[test]
    fn a_bot_seeker_catches_a_stationary_hider_in_an_open_field() {
        let world = World::open(SceneKind::Park);
        let nav = NavGrid::new(&world);
        for (seed, hider_at) in [
            (1, Vec2::new(12.0, -9.0)),
            (2, Vec2::new(-15.0, 14.0)),
            (3, Vec2::new(0.0, -6.0)),
        ] {
            let mut rng = Rng::new(seed);
            let mut brain = Brain::new(Role::Seeker);
            let mut seeker = avatar(1, Role::Seeker, Vec2::ZERO);
            let hider = avatar(2, Role::Hider, hider_at);
            let phase = Phase::Seeking {
                until: Tick(10_000),
            };
            let caught_at = (0..1200).find(|&t| {
                let avatars = [seeker, hider];
                let senses = Senses {
                    me: seeker,
                    now: Tick(t),
                    phase,
                    avatars: &avatars,
                    world: &world,
                    nav: &nav,
                };
                let input = brain.think(&senses, &mut rng).clamped();
                seeker.body = physics::step(seeker.body, &input, SEEKER_SPEED, &world);
                can_catch(seeker.body, hider.body, &world)
            });
            assert!(
                caught_at.is_some(),
                "seed {seed}: hider at {hider_at:?} never caught"
            );
        }
    }

    #[test]
    fn a_bot_seeker_waits_while_hiders_hide() {
        let world = World::open(SceneKind::Park);
        let nav = NavGrid::new(&world);
        let mut brain = Brain::new(Role::Seeker);
        let seeker = avatar(1, Role::Seeker, Vec2::ZERO);
        let avatars = [seeker, avatar(2, Role::Hider, Vec2::new(0.0, 1.0))];
        let senses = Senses {
            me: seeker,
            now: Tick(0),
            phase: Phase::Hiding { until: Tick(100) },
            avatars: &avatars,
            world: &world,
            nav: &nav,
        };
        assert_eq!(brain.think(&senses, &mut Rng::new(1)).mv, Vec2::ZERO);
    }
}
