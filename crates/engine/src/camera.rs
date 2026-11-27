//! Cameras. Each produces a `View`: the eye plus view and projection
//! matrices. Depth is reversed with an infinite far plane: NDC z is 1 at the
//! near plane and falls toward 0 with distance.

use glam::camera::rh::proj::directx;
use glam::{Mat4, Vec3, Vec4Swizzles};

/// Distance from the eye to the near plane.
pub const NEAR: f32 = 0.05;
pub const FOV_Y: f32 = 60.0 * std::f32::consts::PI / 180.0;
/// Where a follow camera aims: about the followed avatar's eyes.
pub const PIVOT_HEIGHT: f32 = 1.1;
/// The follow camera stops this short of whatever `clip` hits.
const CLIP_MARGIN: f32 = 0.25;
const PITCH_LIMIT: f32 = 1.45;
/// The range of `FollowCamera::look`.
pub const LOOK_DOWN: f32 = -0.8;
pub const LOOK_UP: f32 = 1.1;
/// The pitch a follow camera swings up to as a wall pulls it in. Steeper
/// clears more low cover, but stares at the floor.
const SWING_PITCH: f32 = 0.75;
/// Nearer the pivot than this, a follow camera's own target would fill the
/// bottom of the picture, so callers should not draw it. The swing peaks
/// here and eases back to the base pitch below it: with the target hidden
/// the eye is in effect its eyes, and should look ahead, not at the floor.
pub const CLOSE: f32 = 1.5;
/// Distances `free_distance` tries on the way in, then halvings between the
/// last blocked one and the first clear one.
const SCAN_STEPS: u32 = 24;
const REFINE_STEPS: u32 = 6;

#[derive(Copy, Clone, Debug, PartialEq)]
pub struct View {
    pub eye: Vec3,
    pub view: Mat4,
    pub proj: Mat4,
}

impl View {
    pub fn look(eye: Vec3, target: Vec3, aspect: f32) -> View {
        View::look_to(eye, target - eye, aspect)
    }

    pub fn look_to(eye: Vec3, dir: Vec3, aspect: f32) -> View {
        View {
            eye,
            view: glam::camera::rh::view::look_to_mat4(eye, dir.normalize(), Vec3::Y),
            proj: directx::perspective_infinite_reverse(FOV_Y, aspect.max(1e-3), NEAR),
        }
    }

    pub fn view_proj(&self) -> Mat4 {
        self.proj * self.view
    }

    /// Where `p` lands, in pixels from the top-left corner. None when it is
    /// behind the eye.
    pub fn project(&self, p: Vec3, size: [u32; 2]) -> Option<[f32; 2]> {
        let clip = self.view_proj() * p.extend(1.0);
        if clip.w <= 0.0 {
            return None;
        }
        let ndc = clip.xy() / clip.w;
        Some([
            (ndc.x + 1.0) / 2.0 * size[0] as f32,
            (1.0 - ndc.y) / 2.0 * size[1] as f32,
        ])
    }
}

/// The unit direction a camera with this yaw and pitch looks along. Pitch
/// above 0 looks down.
fn look_dir(yaw: f32, pitch: f32) -> Vec3 {
    let (sy, cy) = yaw.sin_cos();
    let (sp, cp) = pitch.sin_cos();
    Vec3::new(sy * cp, -sp, cy * cp)
}

/// A third-person camera behind and above `target`, looking the way `yaw`
/// faces.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct FollowCamera {
    /// The followed avatar's feet.
    pub target: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    pub distance: f32,
    /// The player's look, in `LOOK_DOWN..=LOOK_UP` radians. Looking down
    /// raises the eye over the target; looking up only tilts the view, so
    /// the eye never drops through the floor.
    pub look: f32,
}

impl FollowCamera {
    pub fn new(target: Vec3, yaw: f32) -> FollowCamera {
        FollowCamera {
            target,
            yaw,
            pitch: 0.35,
            distance: 4.5,
            look: 0.0,
        }
    }

    pub fn pivot(&self) -> Vec3 {
        self.target + Vec3::Y * PIVOT_HEIGHT
    }

    pub fn view(&self, aspect: f32) -> View {
        self.view_at(self.distance, aspect)
    }

    /// The view with the eye `distance` from the pivot. Short of the full
    /// distance the eye swings up and over the target rather than sliding
    /// straight in at head height.
    pub fn view_at(&self, distance: f32, aspect: f32) -> View {
        let dir = self.dir_at(distance);
        let up = self.look.clamp(0.0, LOOK_UP);
        let aim = look_dir(self.yaw, (-dir.y).clamp(-1.0, 1.0).asin() - up);
        View::look_to(self.pivot() - dir * distance, aim, aspect)
    }

    /// How far the eye can come out from the pivot, up to `distance`, with
    /// nothing solid between them at any point on the way. `clip(from, to)`
    /// returns how far from `from` the segment first hits something solid,
    /// or anything at least its length when it is clear.
    pub fn free_distance(&self, clip: impl Fn(Vec3, Vec3) -> f32) -> f32 {
        let pivot = self.pivot();
        let clear = |d: f32| {
            let reach = d + CLIP_MARGIN;
            clip(pivot, pivot - self.dir_at(d) * reach) >= reach
        };
        if !clear(0.0) {
            // Closer to a wall than the margin: half the gap keeps the eye
            // on this side of it.
            let dir = self.dir_at(0.0);
            return clip(pivot, pivot - dir * CLIP_MARGIN).max(0.0) / 2.0;
        }
        // Outward from the pivot, because each distance swings the eye to
        // its own pitch: a clear spot past a blocked one would have callers
        // easing out through the block.
        let mut ok = 0.0;
        for i in 1..=SCAN_STEPS {
            let d = self.distance * i as f32 / SCAN_STEPS as f32;
            if clear(d) {
                ok = d;
                continue;
            }
            let mut blocked = d;
            for _ in 0..REFINE_STEPS {
                let mid = (ok + blocked) / 2.0;
                if clear(mid) {
                    ok = mid;
                } else {
                    blocked = mid;
                }
            }
            return ok;
        }
        self.distance
    }

    fn dir_at(&self, distance: f32) -> Vec3 {
        let down = -self.look.clamp(LOOK_DOWN, 0.0);
        let base = (self.pitch + down).clamp(-PITCH_LIMIT, PITCH_LIMIT);
        let t = if distance < CLOSE {
            distance / CLOSE
        } else {
            (self.distance - distance) / (self.distance - CLOSE).max(1e-3)
        };
        let t = t.clamp(0.0, 1.0);
        let swing = t * t * (3.0 - 2.0 * t);
        look_dir(self.yaw, base + (SWING_PITCH.max(base) - base) * swing)
    }
}

/// A free camera circling `target`, for spectators.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct OrbitCamera {
    pub target: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    pub distance: f32,
}

impl OrbitCamera {
    pub fn new(target: Vec3) -> OrbitCamera {
        OrbitCamera {
            target,
            yaw: 0.6,
            pitch: 0.7,
            distance: 30.0,
        }
    }

    /// Turns by the given radians, keeping the eye above the ground.
    pub fn orbit(&mut self, d_yaw: f32, d_pitch: f32) {
        self.yaw += d_yaw;
        self.pitch = (self.pitch + d_pitch).clamp(0.05, PITCH_LIMIT);
    }

    pub fn view(&self, aspect: f32) -> View {
        let eye = self.target - look_dir(self.yaw, self.pitch) * self.distance;
        View::look(eye, self.target, aspect)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::draw::facing;

    #[test]
    fn the_follow_camera_sits_behind_and_above_looking_the_way_the_target_faces() {
        for yaw in [0.0, 1.2, -2.8] {
            let cam = FollowCamera::new(Vec3::new(4.0, 0.0, -3.0), yaw);
            let v = cam.view(1.5);
            let off = v.eye - cam.target;
            assert!(off.y > PIVOT_HEIGHT, "above");
            assert!(off.dot(facing(yaw)) < -3.0, "behind");
            assert!((v.eye.distance(cam.pivot()) - cam.distance).abs() < 1e-4);
            // The followed avatar is in the middle of the picture.
            let [x, y] = v.project(cam.pivot(), [200, 100]).unwrap();
            assert!((x - 100.0).abs() < 0.01 && (y - 50.0).abs() < 0.01);
        }
    }

    #[test]
    fn a_wall_between_the_target_and_the_eye_pulls_the_eye_in_front_of_it() {
        let cam = FollowCamera::new(Vec3::ZERO, 0.0);
        // A wall at z = -2, behind a target facing +Z.
        let wall = |from: Vec3, to: Vec3| {
            let d = to - from;
            let t = (-2.0 - from.z) / d.z;
            if (0.0..=1.0).contains(&t) {
                t * d.length()
            } else {
                f32::INFINITY
            }
        };
        let v = cam.view_at(cam.free_distance(wall), 1.0);
        assert!(v.eye.z > -2.0, "eye {} stays on the target's side", v.eye);
        assert!(v.eye.distance(cam.pivot()) < cam.distance);
        assert_eq!(cam.free_distance(|_, _| 100.0), cam.distance);
    }

    #[test]
    fn a_wall_right_behind_the_target_still_keeps_the_eye_in_front_of_it() {
        let cam = FollowCamera::new(Vec3::ZERO, 0.0);
        for gap in [0.3, 0.05, 0.0] {
            let v = cam.view_at(cam.free_distance(|_, _| gap), 1.0);
            let d = v.eye.distance(cam.pivot());
            assert!(d <= gap, "gap {gap}: eye {d} from the pivot");
            assert!(v.view.is_finite(), "gap {gap}");
        }
    }

    #[test]
    fn a_low_wall_behind_the_target_is_looked_over_rather_than_clipped_to() {
        let cam = FollowCamera::new(Vec3::ZERO, 0.0);
        // Chest-high to the eye: z in [-1.4, -1.0], 1.6 tall, behind a
        // target facing +Z.
        let solid = |p: Vec3| (-1.4..=-1.0).contains(&p.z) && p.y < 1.6;
        let wall = |from: Vec3, to: Vec3| {
            let n = 400;
            (0..=n)
                .map(|i| i as f32 / n as f32)
                .find(|&t| solid(from.lerp(to, t)))
                .map_or(f32::INFINITY, |t| t * from.distance(to))
        };
        let straight_in = 1.0 / cam.pitch.cos() - CLIP_MARGIN;
        let d = cam.free_distance(wall);
        let eye = cam.view_at(d, 1.0).eye;
        assert!(d > 2.0 * straight_in, "{d} vs {straight_in} straight in");
        assert!(eye.z < -1.4 && eye.y > 1.6, "eye {eye} is over the wall");
    }

    #[test]
    fn the_eye_swings_up_as_it_comes_in_then_looks_ahead_once_close() {
        let cam = FollowCamera::new(Vec3::ZERO, 0.0);
        let pitch = |d: f32| {
            let v = cam.view_at(d, 1.0);
            let toward = (cam.pivot() - v.eye).normalize();
            -toward.y.asin()
        };
        assert!((pitch(cam.distance) - cam.pitch).abs() < 1e-5);
        assert!(pitch(2.5) > cam.pitch + 0.15);
        assert!((pitch(CLOSE) - SWING_PITCH).abs() < 1e-5);
        assert!(pitch(0.3) < pitch(CLOSE), "looks ahead again once close");
        assert!((pitch(1e-3) - cam.pitch).abs() < 1e-3);
    }

    #[test]
    fn looking_up_from_the_floor_brings_a_tall_wall_into_frame() {
        // Facing +Z with a wall 20 u ahead; a point 20 u up it.
        let high = Vec3::new(0.0, 20.0, 20.0);
        let in_frame = |look: f32| {
            let cam = FollowCamera {
                look,
                ..FollowCamera::new(Vec3::ZERO, 0.0)
            };
            cam.view(1.5)
                .project(high, [300, 200])
                .is_some_and(|[x, y]| (0.0..=300.0).contains(&x) && (0.0..=200.0).contains(&y))
        };
        assert!(!in_frame(0.0), "level look cannot see it");
        assert!(in_frame(LOOK_UP), "looking up can");
    }

    #[test]
    fn no_look_takes_the_eye_below_the_floor() {
        for i in 0..=40 {
            let look = LOOK_DOWN + (LOOK_UP - LOOK_DOWN) * i as f32 / 40.0;
            let cam = FollowCamera {
                look,
                ..FollowCamera::new(Vec3::ZERO, 0.4)
            };
            for d in [0.2, 1.0, CLOSE, 3.0, cam.distance] {
                let eye = cam.view_at(d, 1.0).eye;
                assert!(eye.y > 0.2, "look {look} at {d}: eye {eye}");
            }
        }
    }

    #[test]
    fn looking_down_raises_the_eye() {
        let level = FollowCamera::new(Vec3::ZERO, 0.0);
        let down = FollowCamera {
            look: LOOK_DOWN,
            ..level
        };
        assert!(down.view(1.0).eye.y > level.view(1.0).eye.y + 1.0);
    }

    #[test]
    fn how_far_away_the_target_is_does_not_change_the_view() {
        let eye = Vec3::new(3.0, 2.0, 5.0);
        let d = Vec3::new(-0.4, -0.3, -1.0);
        let near = View::look(eye, eye + d, 1.6);
        let far = View::look(eye, eye + 4.0 * d, 1.6);
        assert!(near.view.abs_diff_eq(far.view, 1e-5));
        // A view matrix is a rotation and a translation: no scale.
        let x = near.view.transform_vector3(Vec3::X);
        assert!((x.length() - 1.0).abs() < 1e-5);
    }

    #[test]
    fn nearer_points_get_greater_depth() {
        let v = OrbitCamera::new(Vec3::ZERO).view(1.0);
        let depth = |p: Vec3| {
            let c = v.view_proj() * p.extend(1.0);
            c.z / c.w
        };
        let toward_eye = (v.eye - Vec3::ZERO).normalize();
        assert!(depth(toward_eye * 5.0) > depth(Vec3::ZERO));
        assert!(depth(Vec3::ZERO) > depth(-toward_eye * 50.0));
        assert!(depth(-toward_eye * 50.0) > 0.0);
    }

    #[test]
    fn orbiting_never_takes_the_eye_below_the_ground() {
        let mut cam = OrbitCamera::new(Vec3::ZERO);
        cam.orbit(0.3, -10.0);
        assert!(cam.view(1.0).eye.y > 0.0);
        cam.orbit(0.0, 10.0);
        assert!(cam.view(1.0).eye.y < cam.distance);
    }
}
