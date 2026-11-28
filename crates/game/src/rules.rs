//! The game's numbers, and who may move and catch.

use serde::{Deserialize, Serialize};

use crate::geom::{Vec3, facing, finite};
use crate::model::{Body, Phase, Role, Status, Tick};
use crate::world::{HALF_EXTENT, World};

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
/// `food` is a hider's food level, which slows it when low.
pub fn mobility(phase: Phase, role: Role, status: Status, food: Option<f32>) -> Option<f32> {
    if status != Status::Active {
        return None;
    }
    let pace = food.map_or(1.0, pace);
    match (phase, role) {
        (Phase::Hiding { .. }, Role::Hider) | (Phase::Seeking { .. }, _) => {
            Some(speed(role) * pace)
        }
        _ => None,
    }
}

/// Seconds a full meter lasts.
pub const WATER_SECS: u32 = 75;
pub const FOOD_SECS: u32 = 100;
/// Seconds standing in a spring takes to fill an empty water meter.
pub const DRINK_SECS: u32 = 2;
/// What one food item adds to the food meter.
pub const FOOD_BITE: f32 = 0.5;
/// Below this food level a hider slows to `HUNGRY_PACE`, and at 0 to
/// `STARVING_PACE`. Tiers rather than a slope, so prediction only drifts
/// from the server on the tick a hider crosses one.
pub const HUNGRY: f32 = 0.3;
pub const HUNGRY_PACE: f32 = 0.75;
pub const STARVING_PACE: f32 = 0.5;
/// A hider this long outside the zone, without a break, dies.
pub const DANGER_SECS: u32 = 10;
/// The mean time an eaten food spot takes to regrow.
pub const FOOD_REGROW_SECS: u32 = 40;
/// Food spots are a `u64` bitmask on the wire.
pub const MAX_FOOD: usize = 64;
/// A food item is eaten by feet this close across.
pub const FOOD_REACH: f32 = 0.7;
/// And this close up or down.
pub const FOOD_HEIGHT: f32 = 0.5;
/// The zone's half width and ceiling end at these shares of the start, so
/// its final volume is 0.8² × 0.625 = 40% of the whole play space.
pub const ZONE_FINAL_HALF: f32 = 0.8;
pub const ZONE_FINAL_CEILING: f32 = 0.625;

/// Speed share at `food`.
pub fn pace(food: f32) -> f32 {
    if food >= HUNGRY {
        1.0
    } else if food > 0.0 {
        HUNGRY_PACE
    } else {
        STARVING_PACE
    }
}

/// A hider's meters, each from 0 to 1, and how many ticks in a row it has
/// spent outside the zone.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Vitals {
    #[serde(deserialize_with = "finite")]
    pub food: f32,
    #[serde(deserialize_with = "finite")]
    pub water: f32,
    pub danger: u16,
}

impl Vitals {
    pub const FULL: Vitals = Vitals {
        food: 1.0,
        water: 1.0,
        danger: 0,
    };

    /// Seconds left before the danger zone kills.
    pub fn danger_left(self) -> f32 {
        (secs(DANGER_SECS) as f32 - f32::from(self.danger)).max(0.0) / TICK_HZ as f32
    }
}

/// The safe zone: a box on the world's centre, `half` wide each way and up
/// to `ceiling` above the ground. Everything outside is the danger zone.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Zone {
    #[serde(deserialize_with = "finite")]
    pub half: f32,
    #[serde(deserialize_with = "finite")]
    pub ceiling: f32,
}

impl Zone {
    /// No danger anywhere, as in the lobby.
    pub const EVERYWHERE: Zone = Zone {
        half: f32::MAX,
        ceiling: f32::MAX,
    };

    /// Whether feet at `body` are safe.
    pub fn contains(self, body: Body) -> bool {
        body.pos.x.abs() <= self.half && body.pos.y.abs() <= self.half && body.y <= self.ceiling
    }

    pub fn volume(self) -> f32 {
        4.0 * self.half * self.half * self.ceiling
    }
}

/// The zone `progress` of the way through the seek, 0 to 1. It shrinks
/// slowly at first and faster toward the end.
pub fn zone(world: &World, progress: f32) -> Zone {
    let p = progress.clamp(0.0, 1.0);
    let p = p * p;
    let shrink = |from: f32, share: f32| from * (1.0 - p * (1.0 - share));
    Zone {
        half: shrink(HALF_EXTENT, ZONE_FINAL_HALF),
        ceiling: shrink(world.ceiling, ZONE_FINAL_CEILING),
    }
}

/// The zone at `now` in `phase` of a seek `seek_ticks` long. Whole outside
/// the seek.
pub fn zone_at(world: &World, phase: Phase, seek_ticks: u32, now: Tick) -> Zone {
    match phase {
        Phase::Seeking { until } if seek_ticks > 0 => {
            let left = now.until(until).min(seek_ticks);
            zone(world, 1.0 - left as f32 / seek_ticks as f32)
        }
        _ => zone(world, 0.0),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::SceneKind;

    #[test]
    fn the_zone_shrinks_slowly_then_fast_to_forty_percent() {
        let world = World::generate(SceneKind::Kitchen, 1);
        let whole = zone(&world, 0.0);
        assert_eq!(whole.half, HALF_EXTENT);
        assert_eq!(whole.ceiling, world.ceiling);
        let end = zone(&world, 1.0);
        let share = end.volume() / whole.volume();
        assert!((share - 0.4).abs() < 1e-3, "final share {share}");
        let lost = |a: f32, b: f32| zone(&world, a).volume() - zone(&world, b).volume();
        assert!(
            lost(0.0, 0.5) * 2.0 < lost(0.5, 1.0),
            "faster toward the end"
        );
        let mut last = whole.volume();
        for i in 1..=20 {
            let v = zone(&world, i as f32 / 20.0).volume();
            assert!(v < last, "shrinks every step");
            last = v;
        }
    }

    #[test]
    fn the_zone_follows_the_seek_clock() {
        let world = World::generate(SceneKind::City, 1);
        let seek = secs(100);
        let at = |phase, now| zone_at(&world, phase, seek, Tick(now));
        let until = Tick(1000 + seek);
        assert_eq!(at(Phase::Hiding { until }, 0), zone(&world, 0.0));
        assert_eq!(at(Phase::Seeking { until }, 1000), zone(&world, 0.0));
        assert_eq!(
            at(Phase::Seeking { until }, 1000 + seek / 2),
            zone(&world, 0.5)
        );
        assert_eq!(at(Phase::Seeking { until }, 1000 + seek), zone(&world, 1.0));
    }

    #[test]
    fn the_zone_holds_feet_inside_its_box_and_under_its_ceiling() {
        let z = Zone {
            half: 10.0,
            ceiling: 4.0,
        };
        let at = |x: f32, y: f32, h: f32| Body {
            pos: crate::geom::Vec2::new(x, y),
            y: h,
            ..Body::default()
        };
        assert!(z.contains(at(9.9, -9.9, 3.9)));
        assert!(!z.contains(at(10.1, 0.0, 0.0)));
        assert!(!z.contains(at(0.0, -10.1, 0.0)));
        assert!(!z.contains(at(0.0, 0.0, 4.1)));
    }

    #[test]
    fn only_a_living_hider_moves_and_hunger_slows_it_in_tiers() {
        let seek = Phase::Seeking { until: Tick(9) };
        let hider = |status, food| mobility(seek, Role::Hider, status, Some(food));
        assert_eq!(hider(Status::Active, 1.0), Some(HIDER_SPEED));
        assert_eq!(hider(Status::Active, HUNGRY), Some(HIDER_SPEED));
        assert_eq!(hider(Status::Active, 0.1), Some(HIDER_SPEED * HUNGRY_PACE));
        assert_eq!(
            hider(Status::Active, 0.0),
            Some(HIDER_SPEED * STARVING_PACE)
        );
        assert_eq!(hider(Status::Caught, 1.0), None);
        assert_eq!(hider(Status::Died(crate::model::Cause::Thirst), 1.0), None);
        assert_eq!(
            mobility(seek, Role::Seeker, Status::Active, None),
            Some(SEEKER_SPEED)
        );
    }
}
