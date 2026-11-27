//! Building blocks on a 3 by 3 street grid, with cars, dumpsters, and bus
//! stops in the streets.

use super::{Aabb2, Builder, HALF_EXTENT, Material, Paint, Shape, Vec2, Vec3, paint};

const WALL: [Paint; 4] = [
    paint([176, 168, 156], Material::Concrete),
    paint([150, 140, 132], Material::Brick),
    paint([196, 176, 150], Material::Plaster),
    paint([128, 132, 140], Material::Concrete),
];
const CAR: [Paint; 4] = [
    paint([200, 60, 50], Material::Metal),
    paint([50, 90, 170], Material::Metal),
    paint([230, 200, 70], Material::Metal),
    paint([90, 90, 96], Material::Metal),
];
const DUMPSTER: Paint = paint([40, 100, 60], Material::Metal);
const SHELTER: Paint = paint([120, 150, 170], Material::Metal);
const FENCE: Paint = paint([120, 120, 124], Material::Concrete);

const BLOCK_CENTERS: [f32; 3] = [-13.0, 0.0, 13.0];
const BLOCK_HALF: f32 = 4.5;

pub(super) fn build(b: &mut Builder) {
    b.perimeter(3.0, FENCE);
    for &bx in &BLOCK_CENTERS {
        for &by in &BLOCK_CENTERS {
            let lot = Aabb2::centered(Vec2::new(bx, by), Vec2::new(BLOCK_HALF, BLOCK_HALF));
            building_lot(b, lot);
        }
    }
    b.pads_by(4, b.structure..b.solids.len());
    let area = Aabb2::centered(Vec2::ZERO, Vec2::new(HALF_EXTENT, HALF_EXTENT));
    for _ in 0..b.count(6, 10) {
        let half = b.oriented(1.9, 0.9);
        if let Some(c) = b.place(area, half, 1.0) {
            let finish = b.pick(&CAR);
            b.block(Aabb2::centered(c, half), 0.8, finish);
            let cabin = Vec2::new(half.x * 0.8, half.y * 0.8);
            b.deco(
                Shape::Box,
                Vec3::new(c.x, 1.0, c.y),
                Vec3::new(cabin.x * 1.4, 0.4, cabin.y * 1.4),
                finish,
            );
        }
    }
    for _ in 0..b.count(3, 5) {
        let half = b.oriented(1.4, 0.9);
        if let Some(c) = b.place(area, half, 1.0) {
            b.block(Aabb2::centered(c, half), 1.0, DUMPSTER);
        }
    }
    for _ in 0..2 {
        let half = b.oriented(2.4, 0.2);
        if let Some(c) = b.place(area, half, 1.2) {
            b.block(Aabb2::centered(c, half), 1.8, SHELTER);
            b.deco(
                Shape::Box,
                Vec3::new(c.x, 1.9, c.y),
                Vec3::new(half.x * 2.0 + 0.6, 0.1, half.y * 2.0 + 0.6),
                SHELTER,
            );
        }
    }
}

/// One building, two with an alley between, or an open plaza.
fn building_lot(b: &mut Builder, lot: Aabb2) {
    if b.rng.chance(0.2) {
        return;
    }
    let inset = b.rng.range(0.0, 1.0);
    let lot = lot.grow(-inset);
    let mut parts = vec![lot];
    if b.rng.chance(0.5) {
        let alley = 0.9;
        let c = lot.center();
        parts = if b.rng.chance(0.5) {
            vec![
                Aabb2 {
                    min: lot.min,
                    max: Vec2::new(c.x - alley, lot.max.y),
                },
                Aabb2 {
                    min: Vec2::new(c.x + alley, lot.min.y),
                    max: lot.max,
                },
            ]
        } else {
            vec![
                Aabb2 {
                    min: lot.min,
                    max: Vec2::new(lot.max.x, c.y - alley),
                },
                Aabb2 {
                    min: Vec2::new(lot.min.x, c.y + alley),
                    max: lot.max,
                },
            ]
        };
    }
    for part in parts {
        let h = b.rng.range(4.0, 10.0);
        let finish = b.pick(&WALL);
        b.block(part, h, finish);
    }
}
