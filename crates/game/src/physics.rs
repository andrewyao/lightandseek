//! One fixed step of movement. The server, the bots, and `Replica`
//! prediction all run this same function, so prediction replays exactly what
//! the server does.
//!
//! An avatar is an upright cylinder: a circle of `RADIUS` on the ground
//! plane, from its feet at `Body::y` up to `HEIGHT`. A solid blocks it only
//! where their vertical spans meet, so it walks under anything higher than
//! its head and stands on anything lower than its feet.

use crate::geom::{Footprint, Vec2, facing, right};
use crate::model::{Body, Input};
use crate::rules::DT;
use crate::world::{Solid, World};

/// Avatars are circles of this radius on the ground plane.
pub const RADIUS: f32 = 0.3;
/// From an avatar's feet to the top of its head.
pub const HEIGHT: f32 = 0.98;
/// Downward acceleration, in u/s².
pub const GRAVITY: f32 = 24.0;
/// How high a jump from standing lifts the feet.
pub const JUMP_APEX: f32 = 1.0;
/// Falling never gets faster than this, in u/s.
const MAX_FALL: f32 = 40.0;
/// Ledges up to this far above the feet are walked onto, not into.
pub const STEP_UP: f32 = 0.25;
/// How far past contact a push-out lands, so float error cannot leave the
/// circle a hair inside.
const SKIN: f32 = 1e-3;
/// Feet within this of the floor are standing on it.
const STANDING: f32 = 1e-3;
const SUBSTEPS: usize = 2;
const PUSH_ROUNDS: usize = 4;

/// The upward speed that lifts the feet `apex` above where they leave.
pub fn launch_speed(apex: f32) -> f32 {
    (2.0 * GRAVITY * apex.max(0.0)).sqrt()
}

/// Whether `s` is in the way of an avatar whose feet are at `y`.
pub(crate) fn blocks(s: &Solid, y: f32) -> bool {
    s.base < y + HEIGHT && s.height > y + STEP_UP
}

/// Turns to `input.yaw`, moves `speed * DT` along `input.mv` (already
/// clamped) sliding along whatever blocks at the avatar's height, then
/// falls, lands, or jumps. Never ends inside a solid: a substep whose
/// push-out cannot settle keeps the previous position.
pub fn step(body: Body, input: &Input, speed: f32, world: &World) -> Body {
    let yaw = input.yaw;
    let dir = facing(yaw) * input.mv.x + right(yaw) * input.mv.y;
    let delta = dir * (speed * DT / SUBSTEPS as f32);
    let mut pos = body.pos;
    for _ in 0..SUBSTEPS {
        if let Some(next) = settle(pos + delta, body.y, world) {
            pos = next;
        }
    }
    let (y, vy) = vertical(pos, body.y, body.vy, input.jump, world);
    Body { pos, yaw, y, vy }
}

/// One step for an avatar that may not move: it keeps its yaw and only
/// falls until it lands.
pub fn fall(body: Body, world: &World) -> Body {
    let input = Input {
        yaw: body.yaw,
        ..Input::default()
    };
    step(body, &input, 0.0, world)
}

/// The circle at `p` overlaps `s`'s footprint.
fn over(s: &Solid, p: Vec2) -> bool {
    s.shape.distance(p) < RADIUS
}

fn vertical(p: Vec2, y: f32, vy: f32, jump: bool, world: &World) -> (f32, f32) {
    let under = world.solids.iter().filter(|s| over(s, p));
    let floor = under
        .clone()
        .filter(|s| s.height <= y + STEP_UP)
        .map(|s| s.height)
        .fold(0.0, f32::max);
    let ceiling = under
        .filter(|s| s.base > y + STEP_UP)
        .map(|s| s.base - HEIGHT)
        .fold(f32::INFINITY, f32::min);
    let mut vy = vy;
    if y <= floor + STANDING && vy <= 0.0 {
        let pad = world
            .pads
            .iter()
            .find(|d| (d.y - floor).abs() < STANDING && d.center.distance(p) < d.radius);
        vy = match pad {
            Some(d) => d.boost,
            None if jump => launch_speed(JUMP_APEX),
            None => return (floor, 0.0),
        };
    }
    // Exact for constant gravity, so the apex is the same at any tick rate.
    let mut y = y + vy * DT - 0.5 * GRAVITY * DT * DT;
    vy = (vy - GRAVITY * DT).max(-MAX_FALL);
    if y > ceiling {
        (y, vy) = (ceiling.max(floor), vy.min(0.0));
    }
    if y <= floor {
        (y, vy) = (floor, 0.0);
    }
    (y, vy)
}

fn settle(mut p: Vec2, y: f32, world: &World) -> Option<Vec2> {
    let r = Vec2::new(RADIUS, RADIUS);
    let (lo, hi) = (world.bounds.min + r, world.bounds.max - r);
    let solids = || world.solids.iter().filter(|s| blocks(s, y));
    p = p.clamp(lo, hi);
    for _ in 0..PUSH_ROUNDS {
        let mut pushed = false;
        for s in solids() {
            if let Some(v) = push_out(&s.shape, p) {
                p = (p + v).clamp(lo, hi);
                pushed = true;
            }
        }
        if !pushed {
            return Some(p);
        }
    }
    solids().all(|s| s.shape.distance(p) >= RADIUS).then_some(p)
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

    fn walk(mv: Vec2, yaw: f32) -> Input {
        Input {
            tick: Tick(0),
            mv,
            yaw,
            jump: false,
        }
    }

    fn jump() -> Input {
        Input {
            jump: true,
            ..Input::default()
        }
    }

    fn slab(world: &mut World, b: Aabb2, base: f32, height: f32) {
        world.solids.push(Solid {
            shape: Footprint::Aabb(b),
            height,
            base,
        });
    }

    /// A 4 u wide slab from y = 1 to y = 3 (in plan), `base` to `height`.
    fn ahead(base: f32, height: f32) -> World {
        let mut world = World::open(SceneKind::Park);
        let b = Aabb2 {
            min: Vec2::new(-2.0, 1.0),
            max: Vec2::new(2.0, 3.0),
        };
        slab(&mut world, b, base, height);
        world
    }

    /// Feet heights for `ticks` steps of `input`, from `body`.
    fn heights(mut body: Body, input: &Input, ticks: usize, world: &World) -> Vec<Body> {
        (0..ticks)
            .map(|_| {
                body = step(body, input, HIDER_SPEED, world);
                body
            })
            .collect()
    }

    fn touches_blocking(world: &World, body: Body) -> Option<Solid> {
        world
            .solids
            .iter()
            .find(|s| blocks(s, body.y) && s.shape.distance(body.pos) < RADIUS - 1e-4)
            .copied()
    }

    #[test]
    fn random_walks_and_jumps_never_end_inside_a_solid_and_still_get_somewhere() {
        for kind in SceneKind::ALL {
            let world = World::generate(kind, 11);
            let mut rng = Rng::new(5);
            let mut travelled = 0.0;
            let mut highest: f32 = 0.0;
            for spawn in &world.spawns {
                let mut body = Body {
                    pos: *spawn,
                    ..Body::default()
                };
                let start = body.pos;
                for t in 0..400 {
                    if t % 20 == 0 {
                        body.yaw = rng.range(-4.0, 4.0);
                    }
                    let input = Input {
                        jump: rng.chance(0.1),
                        ..walk(Vec2::new(1.0, rng.range(-1.0, 1.0)), body.yaw)
                    }
                    .clamped();
                    body = step(body, &input, SEEKER_SPEED, &world);
                    assert_eq!(touches_blocking(&world, body), None, "{kind:?} at {body:?}");
                    assert!(body.y >= 0.0, "{kind:?} under the floor: {body:?}");
                    assert!(world.bounds.grow(-RADIUS + 1e-4).contains(body.pos));
                    highest = highest.max(body.y);
                }
                travelled += body.pos.distance(start);
            }
            assert!(travelled > 50.0, "{kind:?} avatars moved {travelled}");
            assert!(highest > 0.5, "{kind:?} avatars jumped");
        }
    }

    #[test]
    fn a_jump_from_rest_rises_its_apex_and_lands_where_it_left() {
        let world = World::open(SceneKind::Park);
        let path = heights(Body::default(), &jump(), 1, &world);
        let rest = heights(path[0], &Input::default(), 40, &world);
        let apex = rest.iter().map(|b| b.y).fold(path[0].y, f32::max);
        assert!((apex - JUMP_APEX).abs() < 0.05, "apex {apex}");
        let last = rest[39];
        assert_eq!((last.y, last.vy), (0.0, 0.0), "landed");
    }

    #[test]
    fn holding_jump_in_the_air_does_not_jump_again_until_landing() {
        let world = World::open(SceneKind::Park);
        let path = heights(Body::default(), &jump(), 60, &world);
        let takeoffs = path
            .windows(2)
            .filter(|w| w[0].vy <= 0.0 && w[1].vy > 0.0)
            .count();
        let airtime = path.iter().take_while(|b| b.y > 0.0).count();
        assert!(airtime > 8, "one jump lasts {airtime} ticks");
        assert!(takeoffs <= 60 / airtime, "{takeoffs} hops in 60 ticks");
        let apex = path.iter().map(|b| b.y).fold(0.0, f32::max);
        assert!(apex < JUMP_APEX + 0.05, "no double jump: {apex}");
    }

    #[test]
    fn a_jump_lands_on_a_knee_high_ledge_but_not_a_head_high_one() {
        for (height, lands) in [(0.8, true), (1.5, false)] {
            let world = ahead(0.0, height);
            let mut body = Body::default();
            // 12 ticks at 4 u/s ends 2.4 u ahead, on the ledge's far half.
            for t in 0..12 {
                let input = Input {
                    jump: t == 0,
                    ..walk(Vec2::new(1.0, 0.0), 0.0)
                };
                body = step(body, &input, HIDER_SPEED, &world);
            }
            if lands {
                assert_eq!(body.y, height, "on top: {body:?}");
                assert!(body.pos.y > 1.0 + RADIUS, "over the ledge: {body:?}");
            } else {
                assert_eq!(body.y, 0.0, "still on the floor: {body:?}");
                assert!(body.pos.y < 1.0 - RADIUS + 0.01, "stopped at it: {body:?}");
            }
        }
    }

    #[test]
    fn a_low_step_is_walked_onto_and_walking_off_a_ledge_falls() {
        let world = ahead(0.0, STEP_UP * 0.8);
        let mut body = Body::default();
        let fwd = walk(Vec2::new(1.0, 0.0), 0.0);
        for _ in 0..10 {
            body = step(body, &fwd, HIDER_SPEED, &world);
        }
        assert_eq!(body.y, STEP_UP * 0.8, "stepped up: {body:?}");
        let world = ahead(0.0, 2.0);
        let mut body = Body {
            pos: Vec2::new(0.0, 2.5),
            y: 2.0,
            ..Body::default()
        };
        let path = heights(body, &fwd, 30, &world);
        body = path[29];
        assert!(body.pos.y > 3.0 + RADIUS, "walked off: {body:?}");
        assert_eq!(body.y, 0.0, "fell to the floor: {body:?}");
        assert!(
            path.iter().any(|b| b.y > 0.0 && b.y < 2.0),
            "fell, not teleported"
        );
    }

    #[test]
    fn avatars_walk_under_a_high_platform_but_not_a_low_one() {
        for (base, passes) in [(1.2, true), (0.6, false)] {
            let world = ahead(base, base + 0.5);
            let path = heights(Body::default(), &walk(Vec2::new(1.0, 0.0), 0.0), 30, &world);
            let end = path[29].pos.y;
            assert_eq!(end > 3.0, passes, "base {base}: ended at {end}");
            assert!(
                path.iter().all(|b| b.y == 0.0),
                "base {base} stays on the floor"
            );
        }
    }

    #[test]
    fn jumping_under_a_platform_bumps_the_head_and_falls_back() {
        let base = 1.6;
        let world = ahead(base, 2.0);
        let start = Body {
            pos: Vec2::new(0.0, 2.0),
            ..Body::default()
        };
        let first = step(start, &jump(), HIDER_SPEED, &world);
        let path = heights(first, &Input::default(), 30, &world);
        let top = path.iter().map(|b| b.y).fold(first.y, f32::max);
        assert!(top + HEIGHT <= base + 1e-4, "head stays under: {top}");
        assert!(top > base - HEIGHT - 0.1, "reached the underside: {top}");
        assert_eq!(path[29].y, 0.0);
    }

    #[test]
    fn a_pad_throws_an_avatar_to_its_apex_and_lands_it_on_a_counter() {
        let mut world = ahead(0.0, 9.0);
        world.pads.push(crate::world::Pad {
            center: Vec2::new(0.0, 0.0),
            radius: 0.7,
            y: 0.0,
            boost: launch_speed(12.0),
        });
        let still = heights(Body::default(), &Input::default(), 30, &world);
        let apex = still.iter().map(|b| b.y).fold(0.0, f32::max);
        assert!((apex - 12.0).abs() < 0.6, "apex {apex}");

        let mut body = Body::default();
        let mut top = Body::default();
        for t in 0..60 {
            // Rise clear of the counter's face, then steer over its middle.
            let fwd = if t > 3 && body.pos.y < 2.0 { 1.0 } else { 0.0 };
            body = step(body, &walk(Vec2::new(fwd, 0.0), 0.0), HIDER_SPEED, &world);
            if body.y == 9.0 && body.vy == 0.0 {
                top = body;
                break;
            }
        }
        assert_eq!(top.y, 9.0, "landed on the counter: {body:?}");
        assert!((1.0..3.0).contains(&top.pos.y), "on top of it: {top:?}");
    }

    #[test]
    fn walking_diagonally_into_a_wall_slides_along_it() {
        let mut world = World::open(SceneKind::Park);
        world.solids.push(Solid::ground(
            Footprint::Aabb(Aabb2 {
                min: Vec2::new(-10.0, 1.0),
                max: Vec2::new(10.0, 2.0),
            }),
            2.0,
        ));
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
