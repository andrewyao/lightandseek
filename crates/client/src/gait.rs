//! Each player's walk cycle, read off how their avatar actually moves, so
//! the figure's limbs keep pace with its feet.

use std::collections::{BTreeMap, BTreeSet};
use std::f32::consts::{PI, TAU};

use engine::Pose;
use game::rules::HIDER_SPEED;
use game::{AvatarView, PlayerId, Status, Vec2};

/// Frames further apart than this (a backgrounded tab) re-anchor the gait
/// instead of reading a speed off the gap.
const MAX_GAP_MS: f64 = 100.0;
/// Further than this between frames is a respawn, not a step.
const TELEPORT: f32 = 1.5;
/// Speed smoothing, applied twice. Your own avatar moves in 50 ms jumps, so
/// this spans a couple of them, and the second pass irons out the ripple
/// the first leaves.
const SMOOTH_MS: f64 = 100.0;

#[derive(Copy, Clone, Debug)]
struct Gait {
    pos: Vec2,
    at_ms: f64,
    /// Metres per second, smoothed once.
    rough: f32,
    /// Metres per second, smoothed twice.
    speed: f32,
    /// Radians, in [0, TAU).
    phase: f32,
}

impl Gait {
    fn at(pos: Vec2, at_ms: f64) -> Gait {
        Gait {
            pos,
            at_ms,
            rough: 0.0,
            speed: 0.0,
            phase: 0.0,
        }
    }

    fn step(&mut self, pos: Vec2, now_ms: f64) {
        let dt_ms = now_ms - self.at_ms;
        let moved = pos.distance(self.pos);
        if dt_ms <= 0.0 && moved == 0.0 {
            return;
        }
        if moved > TELEPORT {
            self.rough = 0.0;
            self.speed = 0.0;
        } else if dt_ms > 0.0 && dt_ms <= MAX_GAP_MS {
            let sample = moved / (dt_ms / 1000.0) as f32;
            let k = 1.0 - (-dt_ms / SMOOTH_MS).exp() as f32;
            self.rough += (sample - self.rough) * k;
            self.speed += (self.rough - self.speed) * k;
            let advance = (dt_ms / 1000.0) as f32 * self.speed * PI / engine::stride_length();
            self.phase = (self.phase + advance).rem_euclid(TAU);
        }
        self.pos = pos;
        self.at_ms = now_ms;
    }
}

/// Walk cycles of every active player, kept across frames.
#[derive(Clone, Debug, Default)]
pub struct Gaits(BTreeMap<PlayerId, Gait>);

impl Gaits {
    /// Folds in this frame's `avatars`. Players no longer active are
    /// forgotten.
    pub fn update(&mut self, avatars: &[AvatarView], now_ms: f64) {
        let active: BTreeSet<PlayerId> = avatars
            .iter()
            .filter(|a| a.status == Status::Active)
            .map(|a| a.id)
            .collect();
        self.0.retain(|id, _| active.contains(id));
        for a in avatars.iter().filter(|a| a.status == Status::Active) {
            self.0
                .entry(a.id)
                .and_modify(|g| g.step(a.body.pos, now_ms))
                .or_insert_with(|| Gait::at(a.body.pos, now_ms));
        }
    }

    /// How `id`'s figure stands this frame. Unknown players stand at rest.
    pub fn pose(&self, id: PlayerId) -> Pose {
        self.0.get(&id).map_or_else(Pose::default, |g| Pose {
            phase: g.phase,
            stride: (g.speed / HIDER_SPEED).clamp(0.0, 1.0),
        })
    }
}

/// Fixed poses, for stills. The next `update` for a player re-anchors it
/// where it then stands.
impl FromIterator<(PlayerId, Pose)> for Gaits {
    fn from_iter<I: IntoIterator<Item = (PlayerId, Pose)>>(poses: I) -> Gaits {
        Gaits(
            poses
                .into_iter()
                .map(|(id, pose)| {
                    let speed = pose.stride.clamp(0.0, 1.0) * HIDER_SPEED;
                    let gait = Gait {
                        rough: speed,
                        speed,
                        phase: pose.phase.rem_euclid(TAU),
                        ..Gait::at(Vec2::ZERO, f64::NEG_INFINITY)
                    };
                    (id, gait)
                })
                .collect(),
        )
    }
}

#[cfg(test)]
mod tests {
    use game::{Body, Control, Role};

    use super::*;

    fn avatar(id: u32, x: f32) -> AvatarView {
        AvatarView {
            id: PlayerId(id),
            body: Body {
                pos: Vec2::new(x, 0.0),
                ..Body::default()
            },
            role: Role::Hider,
            status: Status::Active,
            control: Control::Human,
        }
    }

    const FRAME_MS: f64 = 1000.0 / 60.0;
    const TICK_MS: f64 = 50.0;

    /// Your own avatar at full speed as the browser sees it: a 0.2 m step
    /// every 50 ms tick, sampled at 60 frames a second. Returns each
    /// frame's pose after the first half second.
    fn run_at_full_speed(gaits: &mut Gaits, frames: usize) -> Vec<Pose> {
        let step = HIDER_SPEED * (TICK_MS / 1000.0) as f32;
        let mut poses = Vec::new();
        for f in 0..frames {
            let now = f as f64 * FRAME_MS;
            let x = (now / TICK_MS).floor() as f32 * step;
            gaits.update(&[avatar(1, x)], now);
            if now > 500.0 {
                poses.push(gaits.pose(PlayerId(1)));
            }
        }
        poses
    }

    #[test]
    fn running_in_tick_sized_jumps_gives_a_full_steady_stride() {
        let mut gaits = Gaits::default();
        let poses = run_at_full_speed(&mut gaits, 120);
        let strides = poses.iter().map(|p| p.stride);
        let low = strides.clone().fold(f32::INFINITY, f32::min);
        let high = strides.fold(0.0, f32::max);
        assert!(low > 0.92 && high <= 1.0, "stride {low}..{high}");
    }

    #[test]
    fn the_phase_advances_every_frame_not_only_on_ticks() {
        let mut gaits = Gaits::default();
        let poses = run_at_full_speed(&mut gaits, 120);
        let advances: Vec<f32> = poses
            .windows(2)
            .map(|w| (w[1].phase - w[0].phase).rem_euclid(TAU))
            .collect();
        let mean = advances.iter().sum::<f32>() / advances.len() as f32;
        // Feet don't skate: per second, the cycle covers the ground moved.
        let per_second = mean as f64 / FRAME_MS * 1000.0;
        let expected = f64::from(HIDER_SPEED * PI / engine::stride_length());
        assert!(
            (per_second / expected - 1.0).abs() < 0.1,
            "{per_second} vs {expected}"
        );
        for a in advances {
            assert!(
                a > 0.6 * mean && a < 1.4 * mean,
                "{a} against a mean of {mean}"
            );
        }
        assert!(poses.iter().all(|p| (0.0..TAU).contains(&p.phase)));
    }

    #[test]
    fn a_player_who_stops_comes_to_rest() {
        let mut gaits = Gaits::default();
        run_at_full_speed(&mut gaits, 60);
        let x = (60.0 * FRAME_MS / TICK_MS).floor() as f32 * 0.2;
        for f in 60..120 {
            gaits.update(&[avatar(1, x)], f as f64 * FRAME_MS);
        }
        assert!(
            gaits.pose(PlayerId(1)).stride < 0.02,
            "{:?}",
            gaits.pose(PlayerId(1))
        );
    }

    #[test]
    fn a_respawn_across_the_map_is_not_a_sprint() {
        let mut gaits = Gaits::default();
        gaits.update(&[avatar(1, 0.0)], 0.0);
        gaits.update(&[avatar(1, 30.0)], FRAME_MS);
        assert_eq!(gaits.pose(PlayerId(1)).stride, 0.0);
        gaits.update(&[avatar(1, 30.0)], 2.0 * FRAME_MS);
        assert_eq!(gaits.pose(PlayerId(1)).stride, 0.0);
    }

    #[test]
    fn a_long_pause_between_frames_is_not_a_sprint() {
        let mut gaits = Gaits::default();
        gaits.update(&[avatar(1, 0.0)], 0.0);
        gaits.update(&[avatar(1, 1.2)], 3000.0);
        assert_eq!(gaits.pose(PlayerId(1)).stride, 0.0);
    }

    #[test]
    fn players_who_leave_or_are_caught_are_forgotten() {
        let mut gaits = Gaits::default();
        gaits.update(&[avatar(1, 0.0), avatar(2, 0.0)], 0.0);
        gaits.update(&[avatar(1, 0.05), avatar(2, 0.05)], FRAME_MS);
        let mut caught = avatar(2, 0.1);
        caught.status = Status::Caught;
        gaits.update(&[avatar(1, 0.1), caught], 2.0 * FRAME_MS);
        assert!(gaits.pose(PlayerId(1)).stride > 0.0);
        assert_eq!(gaits.pose(PlayerId(2)), Pose::default());
        assert_eq!(gaits.0.keys().copied().collect::<Vec<_>>(), [PlayerId(1)]);
        gaits.update(&[], 3.0 * FRAME_MS);
        assert!(gaits.0.is_empty());
    }

    #[test]
    fn unknown_players_stand_at_rest() {
        assert_eq!(Gaits::default().pose(PlayerId(9)), Pose::default());
    }

    #[test]
    fn fixed_poses_hold_until_the_player_moves() {
        let pose = Pose {
            phase: 1.0,
            stride: 0.5,
        };
        let mut gaits: Gaits = [(PlayerId(3), pose)].into_iter().collect();
        assert!((gaits.pose(PlayerId(3)).stride - 0.5).abs() < 1e-6);
        assert!((gaits.pose(PlayerId(3)).phase - 1.0).abs() < 1e-6);
        gaits.update(&[avatar(3, 40.0)], 1000.0);
        gaits.update(&[avatar(3, 40.05)], 1000.0 + FRAME_MS);
        assert!(
            gaits.pose(PlayerId(3)).stride < 0.5,
            "re-anchored, not a sprint"
        );
    }
}
