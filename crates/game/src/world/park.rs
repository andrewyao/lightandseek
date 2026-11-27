//! Trees with canopies you can climb into from a launch pad, bushes,
//! hedges, benches, and a pond you cannot wade.

use super::{Aabb2, Builder, HALF_EXTENT, Material, Paint, Shape, Vec2, Vec3, paint};

const TRUNK: Paint = paint([110, 78, 52], Material::Wood);
const LEAVES: [Paint; 3] = [
    paint([58, 128, 60], Material::Foliage),
    paint([74, 142, 58], Material::Foliage),
    paint([46, 110, 70], Material::Foliage),
];
const BUSH: [Paint; 2] = [
    paint([64, 120, 48], Material::Foliage),
    paint([88, 140, 64], Material::Foliage),
];
const HEDGE: Paint = paint([52, 104, 52], Material::Foliage);
const BENCH: Paint = paint([150, 104, 64], Material::Wood);
const WATER: Paint = paint([70, 130, 190], Material::Water);

pub(super) fn build(b: &mut Builder) {
    b.perimeter(1.4, HEDGE);
    let area = Aabb2::centered(Vec2::ZERO, Vec2::new(HALF_EXTENT, HALF_EXTENT));

    let r = b.rng.range(2.5, 4.0);
    let pond = Vec2::new(b.rng.range(-10.0, 10.0), b.rng.range(-10.0, 10.0));
    // Drawn at the surface but solid a little higher, so a step cannot
    // wade in.
    b.solid(
        super::Footprint::Circle {
            center: pond,
            radius: r,
        },
        crate::physics::STEP_UP + 0.05,
    );
    b.deco(
        Shape::Cylinder,
        Vec3::new(pond.x, 0.025, pond.y),
        Vec3::new(r * 2.0, 0.05, r * 2.0),
        WATER,
    );

    for _ in 0..b.count(4, 6) {
        let long = b.rng.range(4.0, 8.0);
        let half = b.oriented(long, 0.6);
        if let Some(c) = b.place(area, half, 1.4) {
            b.block(Aabb2::centered(c, half), 1.4, HEDGE);
        }
    }
    let trees = b.solids.len();
    for _ in 0..b.count(12, 16) {
        let canopy = b.rng.range(1.2, 1.8);
        if let Some(c) = b.place(area, Vec2::new(0.3, 0.3), 1.4) {
            b.pillar(c, 0.3, 1.8, TRUNK);
            let leaves = b.pick(&LEAVES);
            let crown = Aabb2::centered(c, Vec2::new(canopy, canopy));
            b.platform(Shape::Cylinder, crown, 1.8, 3.4, leaves);
        }
    }
    b.pads_by(4, trees..b.solids.len());
    for _ in 0..b.count(12, 18) {
        let r = b.rng.range(0.6, 1.0);
        if let Some(c) = b.place(area, Vec2::new(r, r), 1.2) {
            let h = b.rng.range(0.9, 1.2);
            let finish = b.pick(&BUSH);
            b.pillar(c, r, h, finish);
        }
    }
    for _ in 0..b.count(4, 6) {
        let half = b.oriented(1.6, 0.5);
        if let Some(c) = b.place(area, half, 1.2) {
            b.block(Aabb2::centered(c, half), 0.45, BENCH);
        }
    }
}
