use game::geom::{facing, yaw_toward};
use game::rules::can_catch;
use game::{Aabb2, Body, Footprint, SceneKind, Solid, Vec2, Vec3, World};

#[test]
fn ray_hit_reports_distance_to_the_first_solid_respecting_heights() {
    let mut world = wall(2.0);
    world.solids.push(Solid {
        shape: Footprint::Circle {
            center: Vec2::new(0.0, 5.0),
            radius: 1.0,
        },
        height: 3.0,
    });
    let at = Vec3::new;
    let hit = world
        .ray_hit(at(0.0, 1.0, -2.0), at(0.0, 1.0, 10.0))
        .unwrap();
    assert!((hit - 2.6).abs() < 1e-5, "the wall face at z = 0.6: {hit}");
    let hit = world
        .ray_hit(at(0.0, 2.5, -2.0), at(0.0, 2.5, 10.0))
        .unwrap();
    assert!(
        (hit - 6.0).abs() < 1e-5,
        "over the wall, into the pillar at z = 4: {hit}"
    );
    assert_eq!(
        world.ray_hit(at(0.0, 3.5, -2.0), at(0.0, 3.5, 10.0)),
        None,
        "over both"
    );
    let down = world
        .ray_hit(at(0.0, 4.0, 0.75), at(0.0, -1.0, 0.75))
        .unwrap();
    assert!(
        (down - 2.0).abs() < 1e-5,
        "straight down onto the wall top: {down}"
    );
    assert_eq!(
        world.ray_hit(at(0.0, 1.0, 0.75), at(0.0, 1.0, 3.0)),
        Some(0.0),
        "starts inside"
    );
    assert_eq!(
        world.ray_hit(at(0.0, 1.0, -2.0), at(0.0, 1.0, 0.0)),
        None,
        "stops short"
    );
}

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

/// The engine maps `Vec2 (x, y)` to world `(x, 0, y)` with Y up and relies
/// on yaw 0 facing +Z and strafe-right being `facing × up`.
#[test]
fn yaw_convention_matches_the_engine() {
    use game::geom::right;
    let close = |a: Vec2, b: Vec2| a.distance(b) < 1e-6;
    let quarter = std::f32::consts::FRAC_PI_2;
    assert!(close(facing(0.0), Vec2::new(0.0, 1.0)));
    assert!(close(facing(quarter), Vec2::new(1.0, 0.0)));
    assert!(close(right(0.0), Vec2::new(-1.0, 0.0)));
    assert!(close(right(quarter), Vec2::new(0.0, 1.0)));
    assert!((yaw_toward(Vec2::new(1.0, 0.0)) - quarter).abs() < 1e-6);
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
