//! What a frame draws, in terms that know nothing about the game.
//!
//! Y is up and the ground is the plane y = 0. A yaw is radians about +Y;
//! yaw 0 faces +Z and `facing(yaw)` gives the direction. Colours are sRGB in
//! [0, 1], the way a colour picker shows them.

use std::f32::consts::PI;

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

/// The pattern a surface shows over its colour. Patterns are generated in
/// the shader at a fixed physical scale, so a wider prop shows more bricks
/// rather than bigger ones.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Default)]
pub enum Material {
    #[default]
    Plain,
    Wood,
    Brick,
    Plaster,
    Concrete,
    Asphalt,
    Tile,
    Grass,
    Foliage,
    Fabric,
    Metal,
    Water,
    /// Lit from within: unshaded, casts no shadow, barely fogged.
    Glow,
}

impl Material {
    /// The id the shader switches on.
    pub(crate) fn id(self) -> f32 {
        f32::from(self as u8)
    }
}

/// One opaque, lit shape.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Instance {
    pub shape: Shape,
    /// Rotation, scale, and translation only: no shear. Normals rely on it.
    pub model: Mat4,
    pub color: [f32; 3],
    pub material: Material,
}

impl Instance {
    /// A plain `shape` scaled to `size`, turned by `yaw`, centred on
    /// `center`.
    pub fn new(shape: Shape, center: Vec3, size: Vec3, yaw: f32, color: [f32; 3]) -> Instance {
        Instance {
            shape,
            model: Mat4::from_scale_rotation_translation(size, Quat::from_rotation_y(yaw), center),
            color,
            material: Material::Plain,
        }
    }

    pub fn with_material(self, material: Material) -> Instance {
        Instance { material, ..self }
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
    pub ground_material: Material,
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
            ground_material: Material::Plain,
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

/// Where a figure is in its gait cycle.
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct Pose {
    /// Radians through the cycle. At a quarter turn the left leg is fully
    /// forward; half a turn later the right one is.
    pub phase: f32,
    /// 0 stands at rest, 1 is a full running stride.
    pub stride: f32,
    /// 0 on the ground, 1 in the air: knees tucked, arms up, and no foot
    /// held to the ground.
    pub air: f32,
}

/// From an avatar's feet to the top of its head.
pub const AVATAR_HEIGHT: f32 = 0.98;
const HEAD_RADIUS: f32 = 0.18;
const HEAD_Y: f32 = AVATAR_HEIGHT - HEAD_RADIUS;
const VISOR: [f32; 3] = [0.07, 0.08, 0.11];
const SHOE: [f32; 3] = [0.1, 0.09, 0.09];
/// How much darker than the body the limbs are.
const LIMB_SHADE: f32 = 0.72;

const TORSO_RADIUS: f32 = 0.15;
const TORSO_LOW: f32 = 0.39;
const TORSO_HIGH: f32 = 0.55;
const SHOULDER: Vec3 = Vec3::new(0.17, 0.56, 0.0);
const ARM_RADIUS: f32 = 0.045;
const UPPER_ARM: f32 = 0.13;
const FOREARM: f32 = 0.13;
const HIP: Vec3 = Vec3::new(0.085, 0.36, 0.0);
const THIGH_RADIUS: f32 = 0.065;
const SHIN_RADIUS: f32 = 0.055;
/// The ankle's height above the sole, which sits flat on the ground.
const ANKLE_Y: f32 = 0.06;
const SHOE_SIZE: Vec3 = Vec3::new(0.09, 0.07, 0.15);
const SHOE_FORWARD: f32 = 0.02;
const THIGH: f32 = (HIP.y - ANKLE_Y) / 2.0;
const SHIN: f32 = THIGH;

/// Hip swing either side of straight down at full stride.
const HIP_SWING: f32 = 0.6;
/// Knee bend at full stride: always, plus more while the leg swings forward.
const KNEE_BASE: f32 = 0.15;
const KNEE_SWING: f32 = 0.9;
const ARM_SWING: f32 = 0.45;
const ELBOW_REST: f32 = 0.12;
const ELBOW_STRIDE: f32 = 0.45;
/// Arms hang this far out from the body.
const ARM_SPLAY: f32 = 0.08;
const LEAN: f32 = 0.1;
/// In the air the thighs swing up this far, the knees fold this far, and
/// the arms reach forward and up this far.
const TUCK_HIP: f32 = 1.0;
const TUCK_KNEE: f32 = 1.7;
const REACH: f32 = 2.5;

/// Ground covered per half gait cycle at full stride: the distance a planted
/// foot travels back from fully forward to fully behind.
pub fn stride_length() -> f32 {
    2.0 * (THIGH + SHIN) * HIP_SWING.sin()
}

/// How many parts `avatar_instances` draws.
const AVATAR_PARTS: usize = 13;

/// A player figure standing at `feet`, facing `yaw`, mid-`pose`: torso, head,
/// a dark visor on the side it faces, then left and right arms (upper,
/// fore), left and right legs (thigh, shin), and two shoes. The lower foot
/// stays on the ground.
pub fn avatar_instances(
    feet: Vec3,
    yaw: f32,
    color: [f32; 3],
    pose: Pose,
) -> [Instance; AVATAR_PARTS] {
    let s = pose.stride.clamp(0.0, 1.0);
    let a = pose.air.clamp(0.0, 1.0);
    let mix = |ground: f32, air: f32| ground + (air - ground) * a;
    let limb = color.map(|c| c * LIMB_SHADE);
    // Joints in the figure's frame, +x to its left, y up, +z forward, before the
    // body drops to keep a foot planted.
    let leg = |side: f32, phase: f32| {
        let hip_angle = mix(s * HIP_SWING * phase.sin(), TUCK_HIP);
        let knee = mix(
            s * (KNEE_BASE + KNEE_SWING * phase.cos().max(0.0)),
            TUCK_KNEE,
        );
        let hip = Vec3::new(side * HIP.x, HIP.y, 0.0);
        let knee_at = hip + swing(hip_angle, 0.0) * THIGH;
        let ankle = knee_at + swing(hip_angle - knee, 0.0) * SHIN;
        (hip, knee_at, ankle)
    };
    let arm = |side: f32, phase: f32| {
        let angle = mix(-s * ARM_SWING * phase.sin(), REACH);
        let elbow = mix(ELBOW_REST + s * ELBOW_STRIDE, ELBOW_REST);
        let shoulder = Vec3::new(side * SHOULDER.x, SHOULDER.y, 0.0);
        let elbow_at = shoulder + swing(angle, side * ARM_SPLAY) * UPPER_ARM;
        let hand = elbow_at + swing(angle + elbow, side * ARM_SPLAY) * FOREARM;
        (shoulder, elbow_at, hand)
    };
    let (l_hip, l_knee, l_ankle) = leg(1.0, pose.phase);
    let (r_hip, r_knee, r_ankle) = leg(-1.0, pose.phase + PI);
    let (l_shoulder, l_elbow, l_hand) = arm(1.0, pose.phase);
    let (r_shoulder, r_elbow, r_hand) = arm(-1.0, pose.phase + PI);

    let drop = (l_ankle.y.min(r_ankle.y) - ANKLE_Y) * (1.0 - a);
    let place = Mat4::from_translation(feet - Vec3::Y * drop) * Mat4::from_rotation_y(yaw);
    let pelvis = Vec3::Y * HIP.y;
    let lean = Mat4::from_translation(pelvis)
        * Mat4::from_rotation_x(s * LEAN)
        * Mat4::from_translation(-pelvis);
    let upper = place * lean;

    let head = Vec3::Y * HEAD_Y;
    let shoe = |ankle: Vec3| Instance {
        shape: Shape::Box,
        model: Mat4::from_translation(Vec3::new(
            ankle.x,
            ankle.y - ANKLE_Y + SHOE_SIZE.y / 2.0,
            ankle.z + SHOE_FORWARD,
        )) * Mat4::from_scale(SHOE_SIZE),
        color: SHOE,
        material: Material::Plain,
    };
    let local = [
        (
            upper,
            segment(
                Vec3::Y * TORSO_LOW,
                Vec3::Y * TORSO_HIGH,
                TORSO_RADIUS,
                color,
            ),
        ),
        (
            upper,
            Instance::new(
                Shape::Sphere,
                head,
                Vec3::splat(2.0 * HEAD_RADIUS),
                0.0,
                color,
            ),
        ),
        (
            upper,
            Instance::new(
                Shape::Box,
                head + Vec3::Z * (HEAD_RADIUS * 0.8),
                Vec3::new(0.27, 0.09, 0.12),
                0.0,
                VISOR,
            ),
        ),
        (upper, segment(l_shoulder, l_elbow, ARM_RADIUS, limb)),
        (upper, segment(l_elbow, l_hand, ARM_RADIUS, limb)),
        (upper, segment(r_shoulder, r_elbow, ARM_RADIUS, limb)),
        (upper, segment(r_elbow, r_hand, ARM_RADIUS, limb)),
        (place, segment(l_hip, l_knee, THIGH_RADIUS, limb)),
        (place, segment(l_knee, l_ankle, SHIN_RADIUS, limb)),
        (place, segment(r_hip, r_knee, THIGH_RADIUS, limb)),
        (place, segment(r_knee, r_ankle, SHIN_RADIUS, limb)),
        (place, shoe(l_ankle)),
        (place, shoe(r_ankle)),
    ];
    local.map(|(to, i)| Instance {
        model: to * i.model,
        ..i
    })
}

/// A unit direction pointing down, swung `forward` radians toward +z and
/// tilted `out` radians toward +x.
fn swing(forward: f32, out: f32) -> Vec3 {
    let (sf, cf) = forward.sin_cos();
    let (so, co) = out.sin_cos();
    Vec3::new(so, -cf * co, sf * co)
}

/// A capsule of `radius` from `a` to `b`, overshooting each end by `radius`
/// so neighbouring segments overlap at the joint instead of pinching.
fn segment(a: Vec3, b: Vec3, radius: f32, color: [f32; 3]) -> Instance {
    let along = b - a;
    let dir = along.try_normalize().unwrap_or(Vec3::Y);
    // The capsule mesh is 2 tall, so its Y scale is half the length.
    let length = along.length() + 2.0 * radius;
    // The capsule is symmetric, so aim it upward and the arc never has to
    // turn Y to its exact opposite.
    let up = if dir.y < 0.0 { -dir } else { dir };
    Instance {
        shape: Shape::Capsule,
        model: Mat4::from_scale_rotation_translation(
            Vec3::new(2.0 * radius, length / 2.0, 2.0 * radius),
            Quat::from_rotation_arc(Vec3::Y, up),
            (a + b) / 2.0,
        ),
        color,
        material: Material::Plain,
    }
}

#[cfg(test)]
mod tests {
    use std::f32::consts::FRAC_PI_2;

    use super::*;

    fn centre(i: &Instance) -> Vec3 {
        i.model.transform_point3(Vec3::ZERO)
    }

    /// Every vertex of `i` as the renderer places it.
    fn points(i: &Instance) -> Vec<Vec3> {
        crate::mesh::build(i.shape)
            .vertices
            .iter()
            .map(|v| i.model.transform_point3(Vec3::from_slice(&v[..3])))
            .collect()
    }

    fn lowest(parts: &[Instance]) -> f32 {
        parts
            .iter()
            .flat_map(points)
            .map(|p| p.y)
            .fold(f32::INFINITY, f32::min)
    }

    fn walking(phase: f32) -> Pose {
        Pose {
            phase,
            stride: 1.0,
            ..Pose::default()
        }
    }

    const TORSO: usize = 0;
    const HEAD: usize = 1;
    const VISOR_PART: usize = 2;
    const L_UPPER_ARM: usize = 3;
    const L_HAND: usize = 4;
    const R_UPPER_ARM: usize = 5;
    const R_HAND: usize = 6;
    const L_THIGH: usize = 7;
    const R_THIGH: usize = 9;
    const L_SHIN: usize = 8;
    const R_SHIN: usize = 10;
    const L_SHOE: usize = 11;
    const R_SHOE: usize = 12;

    /// How far forward along `facing(yaw)` part `i` sits from the feet.
    fn ahead(parts: &[Instance], i: usize, feet: Vec3, yaw: f32) -> f32 {
        (centre(&parts[i]) - feet).dot(facing(yaw))
    }

    /// A dense sweep of the gait cycle, quarter turns included.
    fn phases() -> impl Iterator<Item = f32> {
        (0..48).map(|i| i as f32 * std::f32::consts::TAU / 48.0)
    }

    #[test]
    fn in_the_air_the_knees_come_up_the_hands_go_up_and_the_feet_leave_the_ground() {
        let stand = avatar_instances(Vec3::ZERO, 0.0, [1.0; 3], Pose::default());
        let air = Pose {
            air: 1.0,
            ..Pose::default()
        };
        let jump = avatar_instances(Vec3::ZERO, 0.0, [1.0; 3], air);
        // Parts: 3..7 arms (upper, fore), 7..11 legs (thigh, shin), 11..13 shoes.
        let knee_y = |parts: &[Instance]| centre(&parts[8]).y;
        let hand_y = |parts: &[Instance]| centre(&parts[4]).y;
        assert!(knee_y(&jump) > knee_y(&stand) + 0.08, "knees tuck");
        assert!(hand_y(&jump) > hand_y(&stand) + 0.2, "arms reach up");
        assert!(
            lowest(&jump) > lowest(&stand) + 0.05,
            "feet leave the ground"
        );
    }

    #[test]
    fn the_visor_sits_on_the_side_the_avatar_faces() {
        for yaw in [0.0, 1.0, -2.5, 3.0] {
            for pose in [Pose::default(), walking(1.0)] {
                let parts = avatar_instances(Vec3::new(3.0, 0.0, -1.0), yaw, [1.0; 3], pose);
                let (head, visor) = (parts[HEAD], parts[VISOR_PART]);
                let out = centre(&visor) - centre(&head);
                let flat = Vec3::new(out.x, 0.0, out.z);
                assert!(flat.normalize().dot(facing(yaw)) > 0.99, "yaw {yaw}");
                // Its front face pokes out of the head, so it shows.
                let front = visor.model.transform_point3(Vec3::new(0.0, 0.0, 0.5));
                assert!(front.distance(centre(&head)) > HEAD_RADIUS);
            }
        }
    }

    #[test]
    fn at_rest_an_avatar_stands_on_its_feet_about_one_unit_tall() {
        let feet = Vec3::new(1.0, 0.0, 2.0);
        let parts = avatar_instances(feet, 0.4, [1.0; 3], Pose::default());
        assert!(lowest(&parts).abs() < 1e-3, "{}", lowest(&parts));
        let top = parts
            .iter()
            .flat_map(points)
            .map(|p| p.y)
            .fold(f32::MIN, f32::max);
        assert!((top - AVATAR_HEIGHT).abs() < 1e-3, "{top}");
        let head_top = parts[HEAD].model.transform_point3(Vec3::new(0.0, 0.5, 0.0));
        assert!((head_top.y - AVATAR_HEIGHT).abs() < 1e-5);
        assert!(centre(&parts[TORSO]).x == feet.x && centre(&parts[TORSO]).z == feet.z);
    }

    #[test]
    fn mid_stride_the_lower_foot_stays_planted_on_the_ground() {
        for phase in phases() {
            for stride in [0.3, 1.0] {
                let parts = avatar_instances(
                    Vec3::ZERO,
                    0.7,
                    [1.0; 3],
                    Pose {
                        phase,
                        stride,
                        ..Pose::default()
                    },
                );
                let low = lowest(&parts);
                assert!(
                    low.abs() < 1e-3,
                    "phase {phase} stride {stride}: lowest {low}"
                );
                let soles = lowest(&parts[L_SHOE..=L_SHOE]).min(lowest(&parts[R_SHOE..=R_SHOE]));
                assert!(soles.abs() < 1e-3, "phase {phase}: a shoe is on the ground");
            }
        }
    }

    #[test]
    fn the_left_foot_is_ahead_when_the_left_hip_swings_forward() {
        for yaw in [0.0, 2.0, -1.0] {
            let feet = Vec3::new(-2.0, 0.0, 4.0);
            let parts = avatar_instances(feet, yaw, [1.0; 3], walking(FRAC_PI_2));
            assert!(ahead(&parts, L_SHOE, feet, yaw) > 0.1, "yaw {yaw}");
            assert!(ahead(&parts, R_SHOE, feet, yaw) < -0.1, "yaw {yaw}");
        }
    }

    #[test]
    fn legs_swing_in_antiphase_and_each_arm_opposes_its_leg() {
        let feet = Vec3::ZERO;
        let mut left_forward = false;
        let mut right_forward = false;
        for phase in phases() {
            let parts = avatar_instances(feet, 0.3, [1.0; 3], walking(phase));
            let at = |i| ahead(&parts, i, feet, 0.3);
            // Mirror images half a cycle apart.
            let later = avatar_instances(feet, 0.3, [1.0; 3], walking(phase + PI));
            let later_at = |i| ahead(&later, i, feet, 0.3);
            assert!(
                (at(L_SHIN) - later_at(R_SHIN)).abs() < 1e-4,
                "phase {phase}"
            );
            assert!(
                (at(L_HAND) - later_at(R_HAND)).abs() < 1e-4,
                "phase {phase}"
            );
            let (l_leg, r_leg) = (at(L_THIGH), at(R_THIGH));
            if (l_leg - r_leg).abs() > 0.02 {
                assert!(
                    (l_leg - r_leg).signum() != (at(L_UPPER_ARM) - at(R_UPPER_ARM)).signum(),
                    "phase {phase}: the arm on the forward leg's side swings back"
                );
                left_forward |= l_leg > r_leg;
                right_forward |= r_leg > l_leg;
            }
        }
        assert!(left_forward && right_forward, "each leg leads in turn");
    }

    #[test]
    fn standing_still_the_legs_are_straight_and_side_by_side() {
        let parts = avatar_instances(
            Vec3::ZERO,
            0.0,
            [1.0; 3],
            Pose {
                phase: 1.3,
                stride: 0.0,
                ..Pose::default()
            },
        );
        assert_eq!(
            parts,
            avatar_instances(Vec3::ZERO, 0.0, [1.0; 3], Pose::default())
        );
        let (l, r) = (centre(&parts[L_SHOE]), centre(&parts[R_SHOE]));
        assert!((l.z - r.z).abs() < 1e-6 && (l.y - r.y).abs() < 1e-6);
    }

    #[test]
    fn no_part_reaches_past_the_collision_radius() {
        // game::physics::RADIUS: anything further out would poke through
        // the walls a hider stands against.
        for phase in phases() {
            for yaw in [0.0, 0.8] {
                let feet = Vec3::new(5.0, 0.0, -3.0);
                let parts = avatar_instances(feet, yaw, [1.0; 3], walking(phase));
                for (i, part) in parts.iter().enumerate() {
                    for p in points(part) {
                        let reach = Vec3::new(p.x - feet.x, 0.0, p.z - feet.z).length();
                        assert!(reach < 0.3, "phase {phase}, part {i}: {reach}");
                    }
                }
            }
        }
    }

    #[test]
    fn every_part_is_rotated_and_scaled_without_shear() {
        for phase in phases() {
            for i in avatar_instances(Vec3::new(1.0, 0.0, 1.0), 2.2, [1.0; 3], walking(phase)) {
                let m = i.model;
                let [x, y, z] = [m.x_axis, m.y_axis, m.z_axis].map(|c| c.truncate().normalize());
                for (a, b) in [(x, y), (y, z), (z, x)] {
                    assert!(a.dot(b).abs() < 1e-4, "phase {phase}: {:?}", i.shape);
                }
            }
        }
    }

    #[test]
    fn limbs_are_darker_than_the_body_and_shoes_are_dark() {
        let red = [0.9, 0.16, 0.12];
        let parts = avatar_instances(Vec3::ZERO, 0.0, red, Pose::default());
        assert_eq!(parts[TORSO].color, red);
        assert_eq!(parts[HEAD].color, red);
        let shin = parts[L_SHIN].color;
        assert!(
            shin[0] < red[0] && shin[0] > 2.0 * shin[1],
            "still reads red: {shin:?}"
        );
        assert!(parts[L_SHOE].color.iter().all(|&c| c < 0.2));
    }

    #[test]
    fn a_full_stride_covers_about_forty_centimetres_per_step() {
        assert!(
            (0.3..0.45).contains(&stride_length()),
            "{}",
            stride_length()
        );
    }

    #[test]
    fn yaw_zero_faces_plus_z_and_turns_toward_plus_x() {
        assert!(facing(0.0).abs_diff_eq(Vec3::Z, 1e-6));
        assert!(facing(std::f32::consts::FRAC_PI_2).abs_diff_eq(Vec3::X, 1e-6));
    }
}
