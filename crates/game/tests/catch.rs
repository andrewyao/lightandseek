use game::geom::{facing, yaw_toward};
use game::rules::can_catch;
use game::{Aabb2, Body, Footprint, SceneKind, Solid, Vec2, World};

fn wall(height: f32) -> World {
    let mut world = World::open(SceneKind::City);
    world.solids.push(Solid {
        shape: Footprint::Aabb(Aabb2 {
            min: Vec2::new(-1.0, 0.6),
            max: Vec2::new(1.0, 0.9),
        }),
        height,
    });
    world
}

/// The seeker at the origin facing +y, the hider `dist` away at `deg` off it.
fn case(dist: f32, deg: f32, world: &World) -> bool {
    let seeker = Body {
        pos: Vec2::ZERO,
        yaw: 0.0,
    };
    let hider = facing(deg.to_radians()) * dist;
    can_catch(seeker, hider, world)
}

#[test]
fn catch_table() {
    let open = World::open(SceneKind::Park);
    let cases = [
        ("1.9 u facing", 1.9, 0.0, &open, true),
        ("2.1 u facing", 2.1, 0.0, &open, false),
        ("exactly 2.0 u", 2.0, 0.0, &open, false),
        ("1.5 u at 29 degrees", 1.5, 29.0, &open, true),
        ("1.5 u at -29 degrees", 1.5, -29.0, &open, true),
        ("1.5 u at 45 degrees", 1.5, 45.0, &open, false),
        ("1.5 u behind", 1.5, 180.0, &open, false),
        ("1.5 u behind a wall", 1.5, 0.0, &wall(2.0), false),
        ("1.5 u over a knee-high bench", 1.5, 0.0, &wall(0.45), true),
    ];
    for (name, dist, deg, world, want) in cases {
        assert_eq!(case(dist, deg, world), want, "{name}");
    }
}

#[test]
fn facing_survives_unwrapped_yaw() {
    let open = World::open(SceneKind::Park);
    let hider = Vec2::new(1.0, 1.0);
    let toward = yaw_toward(hider);
    for turns in [-3.0, -1.0, 0.0, 2.0, 50.0] {
        let yaw = toward + turns * std::f32::consts::TAU;
        let seeker = Body {
            pos: Vec2::ZERO,
            yaw,
        };
        assert!(can_catch(seeker, hider, &open), "{turns} turns");
    }
}
