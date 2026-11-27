//! Renders a hand-built sample scene to a PNG.
//!
//! cargo run -p engine --example scene -- [out.png]

use std::f32::consts::{FRAC_PI_2, FRAC_PI_6, PI};
use std::path::PathBuf;

use engine::{
    Environment, Instance, Offscreen, OrbitCamera, Pose, SceneDraw, Shape, Wedge, avatar_instances,
    write_png,
};
use glam::Vec3;

const SIZE: [u32; 2] = [960, 600];

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out = std::env::args()
        .nth(1)
        .map_or_else(|| PathBuf::from("target/engine-scene.png"), PathBuf::from);

    let block = |x, z, size: Vec3, color| {
        Instance::new(Shape::Box, Vec3::new(x, size.y / 2.0, z), size, 0.0, color)
    };
    let mut instances = vec![
        block(-6.5, -5.0, Vec3::new(4.0, 3.0, 3.0), [0.82, 0.72, 0.58]),
        block(5.5, -7.0, Vec3::new(3.0, 4.5, 3.0), [0.55, 0.6, 0.7]),
        block(-0.6, 3.2, Vec3::new(3.0, 0.9, 0.8), [0.62, 0.42, 0.26]),
        block(3.6, 2.4, Vec3::new(0.8, 0.9, 2.2), [0.62, 0.42, 0.26]),
    ];
    for (x, z) in [(-4.0, 1.5), (4.5, -2.5), (-2.0, -6.0)] {
        instances.push(Instance::new(
            Shape::Cylinder,
            Vec3::new(x, 0.75, z),
            Vec3::new(0.35, 1.5, 0.35),
            0.0,
            [0.45, 0.3, 0.18],
        ));
        instances.push(Instance::new(
            Shape::Sphere,
            Vec3::new(x, 2.1, z),
            Vec3::splat(1.8),
            0.0,
            [0.3, 0.55, 0.25],
        ));
    }
    // The seeker stands facing the camera, one hider runs side-on, and one
    // walks away with the counter hiding its lower half.
    let seeker = (Vec3::new(0.0, 0.0, -1.0), 0.0);
    let running = Pose {
        phase: FRAC_PI_2,
        stride: 1.0,
        ..Pose::default()
    };
    let walking = Pose {
        phase: 2.0,
        stride: 0.5,
        ..Pose::default()
    };
    for ((feet, yaw), color, pose) in [
        (seeker, [0.9, 0.2, 0.18], Pose::default()),
        (
            (Vec3::new(2.4, 0.0, 0.2), FRAC_PI_2),
            [0.25, 0.45, 0.95],
            running,
        ),
        ((Vec3::new(-0.8, 0.0, 2.5), PI), [0.95, 0.8, 0.2], walking),
    ] {
        instances.extend(avatar_instances(feet, yaw, color, pose));
    }
    let wedges = [Wedge {
        center: seeker.0,
        yaw: seeker.1,
        half_angle: FRAC_PI_6,
        radius: 2.0,
        color: [0.95, 0.05, 0.05, 0.8],
    }];

    let mut off = Offscreen::new(SIZE)?;
    let mut cam = OrbitCamera::new(Vec3::new(0.6, 0.4, 0.8));
    cam.yaw = PI + 0.35;
    cam.pitch = 0.42;
    cam.distance = 7.5;
    let rgba = off.render(
        &cam.view(off.aspect()),
        &SceneDraw {
            instances: &instances,
            wedges: &wedges,
            env: Environment::default(),
        },
    );
    write_png(&out, SIZE, &rgba)?;
    println!("wrote {}", out.display());
    Ok(())
}
