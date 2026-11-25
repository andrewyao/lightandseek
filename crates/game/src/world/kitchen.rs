//! A house floor in four rooms, joined by doorways: counters, cabinets, a
//! fridge, an island, and a table with chairs.

use super::{Aabb2, Builder, HALF_EXTENT, Shape, Vec2, Vec3};

const WALL: [u8; 3] = [226, 218, 200];
const COUNTER: [u8; 3] = [190, 190, 186];
const CABINET: [u8; 3] = [150, 110, 76];
const FRIDGE: [u8; 3] = [222, 228, 232];
const WOOD: [u8; 3] = [168, 120, 80];
const CRATE: [u8; 3] = [196, 160, 110];

const WALL_HEIGHT: f32 = 2.4;
const DOOR: f32 = 2.4;

pub(super) fn build(b: &mut Builder) {
    let h = HALF_EXTENT;
    b.perimeter(WALL_HEIGHT, WALL);
    let wx = b.rng.range(-4.0, 4.0);
    let wy = b.rng.range(-4.0, 4.0);

    let doors_y = [b.rng.range(-16.0, wy - 3.0), b.rng.range(wy + 3.0, 16.0)];
    let doors_x = [b.rng.range(-16.0, wx - 3.0), b.rng.range(wx + 3.0, 16.0)];
    for (along, doors, vertical) in [(wx, doors_y, true), (wy, doors_x, false)] {
        let cuts = [-h, doors[0], doors[1], h];
        for k in 0..3 {
            let lo = if k == 0 {
                cuts[0]
            } else {
                cuts[k] + DOOR / 2.0
            };
            let hi = if k == 2 {
                cuts[3]
            } else {
                cuts[k + 1] - DOOR / 2.0
            };
            let (min, max) = if vertical {
                (Vec2::new(along - 0.2, lo), Vec2::new(along + 0.2, hi))
            } else {
                (Vec2::new(lo, along - 0.2), Vec2::new(hi, along + 0.2))
            };
            b.block(Aabb2 { min, max }, WALL_HEIGHT, WALL);
        }
        for d in doors {
            let c = if vertical {
                Vec2::new(along, d)
            } else {
                Vec2::new(d, along)
            };
            b.keep_out.push(Aabb2::centered(
                c,
                Vec2::new(DOOR / 2.0 + 1.0, DOOR / 2.0 + 1.0),
            ));
        }
    }
    b.structural();

    let rooms = [
        (
            Aabb2 {
                min: Vec2::new(-h, -h),
                max: Vec2::new(wx - 0.2, wy - 0.2),
            },
            Vec2::new(-1.0, -1.0),
        ),
        (
            Aabb2 {
                min: Vec2::new(wx + 0.2, -h),
                max: Vec2::new(h, wy - 0.2),
            },
            Vec2::new(1.0, -1.0),
        ),
        (
            Aabb2 {
                min: Vec2::new(-h, wy + 0.2),
                max: Vec2::new(wx - 0.2, h),
            },
            Vec2::new(-1.0, 1.0),
        ),
        (
            Aabb2 {
                min: Vec2::new(wx + 0.2, wy + 0.2),
                max: Vec2::new(h, h),
            },
            Vec2::new(1.0, 1.0),
        ),
    ];
    let island_room = b.rng.below(4);
    let table_room = (island_room + 1 + b.rng.below(3)) % 4;
    let fridge_room = b.rng.below(4);
    for (i, &(room, outer)) in rooms.iter().enumerate() {
        let len = b.rng.range(4.0, 7.0);
        if let Some(c) = against(b, room, outer, len, 0.7) {
            let half = c.1;
            b.block(Aabb2::centered(c.0, half), 0.55, COUNTER);
            b.deco(
                Shape::Box,
                Vec3::new(c.0.x, 1.6, c.0.y),
                Vec3::new(half.x * 2.0, 0.6, half.y * 2.0),
                CABINET,
            );
        }
        for _ in 0..b.count(1, 2) {
            if let Some((c, half)) = against(b, room, outer, 1.2, 0.6) {
                b.block(Aabb2::centered(c, half), 1.3, CABINET);
            }
        }
        if i == fridge_room
            && let Some((c, half)) = against(b, room, outer, 1.0, 0.9)
        {
            b.block(Aabb2::centered(c, half), 1.2, FRIDGE);
        }
        if i == island_room {
            let half = b.oriented(3.0, 1.4);
            if let Some(c) = b.place(room, half, 1.2) {
                b.block(Aabb2::centered(c, half), 0.55, COUNTER);
            }
        }
        if i == table_room {
            table(b, room);
        }
        for _ in 0..b.count(1, 3) {
            let s = b.rng.range(0.35, 0.5);
            if let Some(c) = b.place(room, Vec2::new(s, s), 1.0) {
                b.block(Aabb2::centered(c, Vec2::new(s, s)), 0.8, CRATE);
            }
        }
    }
}

/// A box `len` long and `depth` deep, flush with one of the room's outer
/// walls (`outer` points toward them).
fn against(
    b: &mut Builder,
    room: Aabb2,
    outer: Vec2,
    len: f32,
    depth: f32,
) -> Option<(Vec2, Vec2)> {
    for _ in 0..20 {
        let along_x = b.rng.chance(0.5);
        let half = if along_x {
            Vec2::new(len / 2.0, depth / 2.0)
        } else {
            Vec2::new(depth / 2.0, len / 2.0)
        };
        let c = if along_x {
            let y = if outer.y < 0.0 {
                room.min.y + half.y
            } else {
                room.max.y - half.y
            };
            Vec2::new(b.rng.range(room.min.x + half.x, room.max.x - half.x), y)
        } else {
            let x = if outer.x < 0.0 {
                room.min.x + half.x
            } else {
                room.max.x - half.x
            };
            Vec2::new(x, b.rng.range(room.min.y + half.y, room.max.y - half.y))
        };
        if b.fits(Aabb2::centered(c, half), 1.0) {
            return Some((c, half));
        }
    }
    None
}

fn table(b: &mut Builder, room: Aabb2) {
    let half = b.oriented(2.4, 1.4);
    let Some(c) = b.place(room, half, 1.8) else {
        return;
    };
    b.block(Aabb2::centered(c, half), 0.45, WOOD);
    let chair = Vec2::new(0.25, 0.25);
    let (long, short) = if half.x > half.y {
        (Vec2::new(half.x * 0.5, 0.0), Vec2::new(0.0, half.y + 0.4))
    } else {
        (Vec2::new(0.0, half.y * 0.5), Vec2::new(half.x + 0.4, 0.0))
    };
    for (at, out) in [
        (c + long + short, short),
        (c - long + short, short),
        (c + long - short, -short),
        (c - long - short, -short),
    ] {
        b.block(Aabb2::centered(at, chair), 0.5, WOOD);
        let n = out.normalize_or_zero();
        let back = at + n * 0.22;
        let size = if n.x != 0.0 {
            Vec3::new(0.08, 0.6, 0.5)
        } else {
            Vec3::new(0.5, 0.6, 0.08)
        };
        b.deco(Shape::Box, Vec3::new(back.x, 0.8, back.y), size, WOOD);
    }
}
