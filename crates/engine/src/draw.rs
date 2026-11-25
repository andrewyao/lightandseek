//! What a frame draws, in terms that know nothing about the game.
//!
//! Y is up and the ground is the plane y = 0. A yaw is radians about +Y;
//! yaw 0 faces +Z and `facing(yaw)` gives the direction. Colours are sRGB in
//! [0, 1], the way a colour picker shows them.

use glam::{Mat4, Quat, Vec3};

/// A unit mesh centred on the origin. Every shape but the capsule fits the
/// cube [-0.5, 0.5]^3, so a model matrix scaling by `size` makes a shape
/// `size` across.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum Shape {
    Box,
    /// Axis along Y, radius 0.5, height 1.
    Cylinder,
    /// Axis along Y, radius 0.5, a straight section of 1 and a hemisphere at
    /// each end: 2 tall in all.
    Capsule,
    /// Radius 0.5.
    Sphere,
}

impl Shape {
    pub const ALL: [Shape; 4] = [Shape::Box, Shape::Cylinder, Shape::Capsule, Shape::Sphere];
}

/// One opaque, lit shape.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Instance {
    pub shape: Shape,
    /// Rotation, scale, and translation only: no shear. Normals rely on it.
    pub model: Mat4,
    pub color: [f32; 3],
}

impl Instance {
    /// `shape` scaled to `size`, turned by `yaw`, centred on `center`.
    pub fn new(shape: Shape, center: Vec3, size: Vec3, yaw: f32, color: [f32; 3]) -> Instance {
        Instance {
            shape,
            model: Mat4::from_scale_rotation_translation(size, Quat::from_rotation_y(yaw), center),
            color,
        }
    }
}

/// A translucent fan lying on the ground, such as a seeker's catch cone.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Wedge {
    /// The apex. The fan lies in the horizontal plane through it.
    pub center: Vec3,
    /// The direction the fan opens toward.
    pub yaw: f32,
    /// Radians either side of `yaw`.
    pub half_angle: f32,
    pub radius: f32,
    /// sRGB with straight alpha.
    pub color: [f32; 4],
}

/// Sky, ground, sun, and fog.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Environment {
    /// The clear colour and the colour fog fades toward.
    pub sky: [f32; 3],
    pub ground: [f32; 3],
    /// Toward the sun. Need not be unit length.
    pub sun: Vec3,
    /// Fog starts at the first distance from the eye and is total at the
    /// second.
    pub fog: [f32; 2],
}

impl Default for Environment {
    fn default() -> Environment {
        Environment {
            sky: [0.62, 0.78, 0.92],
            ground: [0.42, 0.6, 0.32],
            sun: Vec3::new(0.45, 1.0, 0.3),
            fog: [25.0, 90.0],
        }
    }
}

/// Everything one frame draws besides the ground plane.
#[derive(Copy, Clone, Debug)]
pub struct SceneDraw<'a> {
    pub instances: &'a [Instance],
    pub wedges: &'a [Wedge],
    pub env: Environment,
}

/// The unit direction a yaw faces, on the ground plane.
pub fn facing(yaw: f32) -> Vec3 {
    Vec3::new(yaw.sin(), 0.0, yaw.cos())
}

/// From an avatar's feet to the top of its head.
pub const AVATAR_HEIGHT: f32 = 0.98;
const BODY_RADIUS: f32 = 0.28;
const BODY_HEIGHT: f32 = 0.72;
const HEAD_RADIUS: f32 = 0.18;
const HEAD_Y: f32 = AVATAR_HEIGHT - HEAD_RADIUS;
const VISOR: [f32; 3] = [0.07, 0.08, 0.11];

/// A player figure standing at `feet`, facing `yaw`: a capsule body, a
/// sphere head, and a dark visor on the side it faces.
pub fn avatar_instances(feet: Vec3, yaw: f32, color: [f32; 3]) -> [Instance; 3] {
    let head = feet + Vec3::Y * HEAD_Y;
    // The capsule mesh is 2 tall, so its Y scale is half the body height.
    let body = Instance::new(
        Shape::Capsule,
        feet + Vec3::Y * (BODY_HEIGHT / 2.0),
        Vec3::new(2.0 * BODY_RADIUS, BODY_HEIGHT / 2.0, 2.0 * BODY_RADIUS),
        yaw,
        color,
    );
    let skull = Instance::new(Shape::Sphere, head, Vec3::splat(2.0 * HEAD_RADIUS), yaw, color);
    let visor = Instance::new(
        Shape::Box,
        head + facing(yaw) * (HEAD_RADIUS * 0.8),
        Vec3::new(0.27, 0.09, 0.12),
        yaw,
        VISOR,
    );
    [body, skull, visor]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn centre(i: &Instance) -> Vec3 {
        i.model.transform_point3(Vec3::ZERO)
    }

    #[test]
    fn the_visor_sits_on_the_side_the_avatar_faces() {
        for yaw in [0.0, 1.0, -2.5, 3.0] {
            let [_, head, visor] = avatar_instances(Vec3::new(3.0, 0.0, -1.0), yaw, [1.0; 3]);
            let out = centre(&visor) - centre(&head);
            assert!(out.normalize().dot(facing(yaw)) > 0.99, "yaw {yaw}");
            // Its front face pokes out of the head, so it shows.
            let front = visor.model.transform_point3(Vec3::new(0.0, 0.0, 0.5));
            assert!(front.distance(centre(&head)) > HEAD_RADIUS);
        }
    }

    #[test]
    fn an_avatar_stands_on_its_feet_about_one_unit_tall() {
        let feet = Vec3::new(1.0, 0.0, 2.0);
        let [body, head, _] = avatar_instances(feet, 0.4, [1.0; 3]);
        let bottom = body.model.transform_point3(Vec3::new(0.0, -1.0, 0.0));
        let top = head.model.transform_point3(Vec3::new(0.0, 0.5, 0.0));
        assert!(bottom.abs_diff_eq(feet, 1e-5), "{bottom}");
        assert!((top.y - AVATAR_HEIGHT).abs() < 1e-5);
    }

    #[test]
    fn yaw_zero_faces_plus_z_and_turns_toward_plus_x() {
        assert!(facing(0.0).abs_diff_eq(Vec3::Z, 1e-6));
        assert!(facing(std::f32::consts::FRAC_PI_2).abs_diff_eq(Vec3::X, 1e-6));
    }
}
