//! One fixed step of movement. The server, the bots, and `Replica`
//! prediction all run this same function, so prediction replays exactly what
//! the server does.

use crate::geom::{Footprint, Vec2, facing, right};
use crate::model::{Body, Input};
use crate::rules::DT;
use crate::world::World;

/// Avatars are circles of this radius on the ground plane.
pub const RADIUS: f32 = 0.3;
/// How far past contact a push-out lands, so float error cannot leave the
/// circle a hair inside.
const SKIN: f32 = 1e-3;
const SUBSTEPS: usize = 2;
const PUSH_ROUNDS: usize = 4;

/// Turns to `input.yaw` and moves `speed * DT` along `input.mv` (already
/// clamped), sliding along solids. Never ends inside a solid: a substep whose
/// push-out cannot settle keeps the previous position.
pub fn step(body: Body, input: &Input, speed: f32, world: &World) -> Body {
    let yaw = input.yaw;
    let dir = facing(yaw) * input.mv.x + right(yaw) * input.mv.y;
    let delta = dir * (speed * DT / SUBSTEPS as f32);
    let mut pos = body.pos;
    for _ in 0..SUBSTEPS {
        if let Some(next) = settle(pos + delta, world) {
            pos = next;
        }
    }
    Body { pos, yaw }
}

fn settle(mut p: Vec2, world: &World) -> Option<Vec2> {
    let r = Vec2::new(RADIUS, RADIUS);
    let (lo, hi) = (world.bounds.min + r, world.bounds.max - r);
    p = p.clamp(lo, hi);
    for _ in 0..PUSH_ROUNDS {
        let mut pushed = false;
        for s in &world.solids {
            if let Some(v) = push_out(&s.shape, p) {
                p = (p + v).clamp(lo, hi);
                pushed = true;
            }
        }
        if !pushed {
            return Some(p);
        }
    }
    world
        .solids
        .iter()
        .all(|s| s.shape.distance(p) >= RADIUS)
        .then_some(p)
}

/// The shortest move that takes a circle at `p` out of `shape`.
fn push_out(shape: &Footprint, p: Vec2) -> Option<Vec2> {
    let q = shape.closest(p);
    let d = p - q;
    let dist = d.length();
    if dist >= RADIUS {
        return None;
    }
    if dist > 0.0 {
        return Some(d * ((RADIUS - dist + SKIN) / dist));
    }
    let out = RADIUS + SKIN;
    Some(match *shape {
        Footprint::Aabb(b) => {
            let faces = [
                (p.x - b.min.x, Vec2::new(-(p.x - b.min.x) - out, 0.0)),
                (b.max.x - p.x, Vec2::new(b.max.x - p.x + out, 0.0)),
                (p.y - b.min.y, Vec2::new(0.0, -(p.y - b.min.y) - out)),
                (b.max.y - p.y, Vec2::new(0.0, b.max.y - p.y + out)),
            ];
            faces
                .into_iter()
                .min_by(|a, b| a.0.total_cmp(&b.0))
                .map(|(_, v)| v)
                .unwrap_or_default()
        }
        Footprint::Circle { center, radius } => {
            let dir = (p - center).normalize_or_zero();
            let dir = if dir == Vec2::ZERO {
                Vec2::new(1.0, 0.0)
            } else {
                dir
            };
            center + dir * (radius + out) - p
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::Aabb2;
    use crate::model::{SceneKind, Tick};
    use crate::rng::Rng;
    use crate::rules::{HIDER_SPEED, SEEKER_SPEED};
    use crate::world::Solid;

    fn walk(mv: Vec2, yaw: f32) -> Input {
        Input {
            tick: Tick(0),
            mv,
            yaw,
        }
    }

    #[test]
    fn random_walks_never_end_inside_a_solid_and_still_get_somewhere() {
        for kind in SceneKind::ALL {
            let world = World::generate(kind, 11);
            let mut rng = Rng::new(5);
            let mut travelled = 0.0;
            for spawn in &world.spawns {
                let mut body = Body {
                    pos: *spawn,
                    yaw: 0.0,
                };
                let start = body.pos;
                for t in 0..400 {
                    if t % 20 == 0 {
                        body.yaw = rng.range(-4.0, 4.0);
                    }
                    let input = walk(Vec2::new(1.0, rng.range(-1.0, 1.0)), body.yaw).clamped();
                    body = step(body, &input, SEEKER_SPEED, &world);
                    for s in &world.solids {
                        assert!(
                            s.shape.distance(body.pos) >= RADIUS - 1e-4,
                            "{kind:?} inside {s:?} at {body:?}"
                        );
                    }
                    assert!(world.bounds.grow(-RADIUS + 1e-4).contains(body.pos));
                }
                travelled += body.pos.distance(start);
            }
            assert!(travelled > 50.0, "{kind:?} avatars moved {travelled}");
        }
    }

    #[test]
    fn walking_diagonally_into_a_wall_slides_along_it() {
        let mut world = World::open(SceneKind::Park);
        world.solids.push(Solid {
            shape: Footprint::Aabb(Aabb2 {
                min: Vec2::new(-10.0, 1.0),
                max: Vec2::new(10.0, 2.0),
            }),
            height: 2.0,
        });
        let mut body = Body::default();
        let yaw = std::f32::consts::FRAC_PI_4;
        for _ in 0..40 {
            body = step(body, &walk(Vec2::new(1.0, 0.0), yaw), HIDER_SPEED, &world);
        }
        assert!(
            (body.pos.y - (1.0 - RADIUS)).abs() < 0.01,
            "pressed against the wall: {body:?}"
        );
        assert!(body.pos.x > 4.0, "slid along it: {body:?}");
    }

    #[test]
    fn seeker_covers_exactly_1_1_times_the_hider_distance() {
        let world = World::open(SceneKind::Park);
        let input = walk(Vec2::new(1.0, 0.0), 0.0);
        let (mut h, mut s) = (Body::default(), Body::default());
        for _ in 0..20 {
            h = step(h, &input, HIDER_SPEED, &world);
            s = step(s, &input, SEEKER_SPEED, &world);
        }
        assert!((h.pos.y - 4.0).abs() < 1e-4, "hider makes 4 u/s: {h:?}");
        assert!((s.pos.y / h.pos.y - 1.1).abs() < 1e-5, "{s:?} vs {h:?}");
        assert_eq!(SEEKER_SPEED / HIDER_SPEED, 1.1);
    }

    #[test]
    fn diagonal_input_is_no_faster_than_straight() {
        let world = World::open(SceneKind::Park);
        let diag = walk(Vec2::new(1.0, 1.0), 0.3).clamped();
        let straight = walk(Vec2::new(1.0, 0.0), 0.3);
        let a = step(Body::default(), &diag, HIDER_SPEED, &world);
        let b = step(Body::default(), &straight, HIDER_SPEED, &world);
        assert!((a.pos.length() - b.pos.length()).abs() < 1e-5);
    }
}
