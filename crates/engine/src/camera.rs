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
            view: glam::camera::rh::view::look_to_mat4(eye, dir, Vec3::Y),
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
}

impl FollowCamera {
    pub fn new(target: Vec3, yaw: f32) -> FollowCamera {
        FollowCamera {
            target,
            yaw,
            pitch: 0.35,
            distance: 4.5,
        }
    }

    pub fn pivot(&self) -> Vec3 {
        self.target + Vec3::Y * PIVOT_HEIGHT
    }

    pub fn view(&self, aspect: f32) -> View {
        self.view_clipped(aspect, |_, _| f32::INFINITY)
    }

    /// `view`, pulled in toward the pivot so the eye stays in front of any
    /// wall between them. `clip(from, to)` returns how far from `from` the
    /// segment first hits something solid, or anything at least its length
    /// when it is clear.
    pub fn view_clipped(&self, aspect: f32, clip: impl Fn(Vec3, Vec3) -> f32) -> View {
        let pivot = self.pivot();
        let dir = look_dir(self.yaw, self.pitch.clamp(-PITCH_LIMIT, PITCH_LIMIT));
        let hit = clip(pivot, pivot - dir * self.distance).max(0.0);
        // Up against a wall the margin would push the eye through to the
        // pivot's far side; half the gap keeps it in front.
        let distance = self.distance.min((hit - CLIP_MARGIN).max(hit / 2.0));
        View::look_to(pivot - dir * distance, dir, aspect)
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
        let v = cam.view_clipped(1.0, wall);
        assert!(v.eye.z > -2.0, "eye {} stays on the target's side", v.eye);
        assert!(v.eye.distance(cam.pivot()) < cam.distance);
        assert_eq!(cam.view_clipped(1.0, |_, _| 100.0), cam.view(1.0));
    }

    #[test]
    fn a_wall_right_behind_the_target_still_keeps_the_eye_in_front_of_it() {
        let cam = FollowCamera::new(Vec3::ZERO, 0.0);
        for gap in [0.3, 0.05, 0.0] {
            let v = cam.view_clipped(1.0, |_, _| gap);
            let d = v.eye.distance(cam.pivot());
            assert!(d <= gap, "gap {gap}: eye {d} from the pivot");
            assert!(v.view.is_finite(), "gap {gap}");
        }
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
