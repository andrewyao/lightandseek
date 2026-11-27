//! The game's numbers, and who may move and catch.

use crate::geom::{Vec3, facing};
use crate::model::{Body, Phase, Role, Status};
use crate::world::World;

pub const TICK_HZ: u32 = 20;
/// Seconds per tick. Every step in the game is exactly this long.
pub const DT: f32 = 1.0 / TICK_HZ as f32;

pub const HIDER_SPEED: f32 = 4.0;
pub const SEEKER_SPEED: f32 = HIDER_SPEED * 1.1;

/// Horizontal centre-to-centre distance, strictly less than.
pub const CATCH_DIST: f32 = 2.0;
/// The hider must be within this many radians of the seeker's yaw.
pub const CATCH_HALF_ANGLE: f32 = std::f32::consts::PI / 6.0;
/// Vertical distance between feet, strictly less than, for a catch.
pub const CATCH_HEIGHT: f32 = 1.0;
/// Sight lines run at chest height above the feet. Solids lower than this
/// do not block someone standing on the ground.
pub const EYE_Y: f32 = 0.6;

/// A disconnected human keeps their avatar this long.
pub const GRACE_TICKS: u32 = 10 * TICK_HZ;
pub const RESULTS_TICKS: u32 = 10 * TICK_HZ;

pub const MAX_HUMANS: u8 = 8;
pub const MAX_BOTS: u8 = 8;
pub const MAX_NAME: usize = 24;

pub fn speed(role: Role) -> f32 {
    match role {
        Role::Seeker => SEEKER_SPEED,
        Role::Hider => HIDER_SPEED,
    }
}

pub fn secs(s: u32) -> u32 {
    s * TICK_HZ
}

/// The speed an avatar may move at in `phase`, or `None` while it is frozen.
pub fn mobility(phase: Phase, role: Role, status: Status) -> Option<f32> {
    match (phase, role, status) {
        (_, _, Status::Caught) => None,
        (Phase::Hiding { .. }, Role::Hider, _) | (Phase::Seeking { .. }, _, _) => Some(speed(role)),
        _ => None,
    }
}

/// Where `body` sees from: chest height above its feet.
pub fn eye(body: Body) -> Vec3 {
    Vec3::new(body.pos.x, body.y + EYE_Y, body.pos.y)
}

/// Nothing solid between the two avatars' eyes.
pub fn in_sight(a: Body, b: Body, world: &World) -> bool {
    world.ray_hit(eye(a), eye(b)).is_none()
}

/// Close enough across and up, facing the hider, and in sight.
pub fn can_catch(seeker: Body, hider: Body, world: &World) -> bool {
    let to = hider.pos - seeker.pos;
    let dist = to.length();
    dist < CATCH_DIST
        && (hider.y - seeker.y).abs() < CATCH_HEIGHT
        && (dist == 0.0 || facing(seeker.yaw).dot(to) >= dist * CATCH_HALF_ANGLE.cos())
        && in_sight(seeker, hider, world)
}
