//! Renders park, city, and kitchen with a few avatars to PNGs, from above
//! and through the seeker's follow camera.
//!
//! cargo run -p client --example scenes -- [out-dir]

use std::path::PathBuf;

use client::draw;
use client::gait::Gaits;
use engine::{Offscreen, OrbitCamera, Pose, write_png};
use game::geom::yaw_toward;
use game::{AvatarView, Body, Control, PlayerId, Role, SceneKind, Status, World};
use glam::Vec3;

const SIZE: [u32; 2] = [960, 600];
const SEED: u64 = 11;
/// Phase and stride for each placed avatar in turn: the seeker mid-run,
/// then hiders running, walking, and standing.
const POSES: [(f32, f32); 4] = [(1.6, 1.0), (4.6, 1.0), (0.9, 0.5), (0.0, 0.0)];

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let dir = std::env::args()
        .nth(1)
        .map_or_else(|| PathBuf::from("target"), PathBuf::from);
    let mut off = Offscreen::new(SIZE)?;
    for kind in SceneKind::ALL {
        let world = World::generate(kind, SEED);
        let avatars = place(&world);
        let gaits: Gaits = avatars
            .iter()
            .zip(POSES)
            .map(|(a, (phase, stride))| {
                (
                    a.id,
                    Pose {
                        phase,
                        stride,
                        ..Pose::default()
                    },
                )
            })
            .collect();
        let zone = game::rules::zone(&world, 0.7);
        let scene =
            draw::scene(&world, &avatars, None, &gaits).with_supplies(Some(zone), &world.food);

        let mut top = OrbitCamera::new(Vec3::ZERO);
        top.pitch = 1.4;
        top.distance = 62.0;
        let seeker = avatars[0].body;
        for (name, view) in [
            ("top", top.view(off.aspect())),
            (
                "follow",
                draw::clipped(&world, draw::follow(seeker, seeker.yaw), off.aspect()),
            ),
        ] {
            let path = dir.join(format!("scene-{kind:?}-{name}.png").to_lowercase());
            write_png(&path, SIZE, &off.render(&view, &scene.draw()))?;
            println!("wrote {}", path.display());
        }
    }
    Ok(())
}

/// The seeker on spawn 0 facing the nearest hider, hiders on the next
/// three spawns, and one caught hider who must not show.
fn place(world: &World) -> Vec<AvatarView> {
    let at = |i: usize, role, status, yaw| AvatarView {
        id: PlayerId(i as u32 + 1),
        body: Body {
            pos: world.spawns[i],
            yaw,
            ..Body::default()
        },
        role,
        status,
        control: Control::Human,
    };
    let seeker = world.spawns[0];
    let near = world.spawns[1..4]
        .iter()
        .copied()
        .min_by(|a, b| a.distance(seeker).total_cmp(&b.distance(seeker)))
        .expect("spawns");
    let mut out = vec![at(
        0,
        Role::Seeker,
        Status::Active,
        yaw_toward(near - seeker),
    )];
    for i in 1..4 {
        out.push(at(i, Role::Hider, Status::Active, i as f32));
    }
    out.push(at(4, Role::Hider, Status::Caught, 0.0));
    out
}
