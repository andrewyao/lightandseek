//! A house floor built around a kitchen. The kitchen has a U of counters
//! with base and upper cabinets, a stove under a hood, a sink under a
//! window, a tall fridge, and an island with stools. It opens onto a dining
//! room, and a hallway leads to a living room, a pantry of shelving, a
//! laundry, and a storeroom.
//!
//! The plan is laid out in one orientation, then mirrored or turned by a
//! random symmetry of the square. Counters, tables, and seats stay below
//! `rules::EYE_Y`, so they only hide a player from the camera. The fridge,
//! shelving, tall cabinets, and plants block sight.

use super::{Aabb2, Builder, Footprint, HALF_EXTENT, Material, Paint, Shape, Vec2, Vec3, paint};

const WALL: Paint = paint([226, 218, 200], Material::Plaster);
const CABINET: Paint = paint([92, 128, 118], Material::Wood);
const STONE: Paint = paint([222, 220, 212], Material::Concrete);
const STEEL: Paint = paint([178, 182, 188], Material::Metal);
const DARK: Paint = paint([44, 44, 48], Material::Plain);
const GLASS: Paint = paint([150, 196, 220], Material::Plain);
const WOOD: Paint = paint([150, 100, 62], Material::Wood);
const LIGHT_WOOD: Paint = paint([196, 158, 112], Material::Wood);
const SHELF: Paint = paint([120, 84, 54], Material::Wood);
const TILE: [Paint; 2] = [
    paint([236, 232, 224], Material::Tile),
    paint([70, 74, 82], Material::Tile),
];
const FLOOR_WOOD: Paint = paint([176, 138, 98], Material::Wood);
const CONCRETE: Paint = paint([168, 168, 162], Material::Concrete);
const RUG: [Paint; 3] = [
    paint([160, 60, 56], Material::Fabric),
    paint([62, 92, 140], Material::Fabric),
    paint([196, 160, 80], Material::Fabric),
];
const SOFA: Paint = paint([96, 110, 150], Material::Fabric);
const POT: Paint = paint([184, 98, 64], Material::Plaster);
const LEAVES: Paint = paint([64, 130, 70], Material::Foliage);
const WHITE: Paint = paint([240, 240, 236], Material::Plain);
const CARDBOARD: [Paint; 2] = [
    paint([196, 160, 110], Material::Plain),
    paint([176, 140, 96], Material::Plain),
];
const GOODS: [Paint; 6] = [
    paint([210, 70, 60], Material::Plain),
    paint([240, 200, 80], Material::Plain),
    paint([80, 150, 90], Material::Plain),
    paint([70, 110, 190], Material::Plain),
    paint([230, 230, 220], Material::Plain),
    paint([200, 130, 60], Material::Plain),
];

const WALL_HEIGHT: f32 = 2.4;
/// Half a wall's thickness.
const T: f32 = 0.15;
const HALL: f32 = 4.0;
const DOOR: f32 = 2.2;
const COUNTER_DEPTH: f32 = 0.9;
const COUNTER: f32 = 0.5;
const TALL: f32 = 1.9;

pub(super) fn build(b: &mut Builder) {
    let h = HALF_EXTENT;
    b.perimeter(WALL_HEIGHT, WALL);
    let sym = Sym::random(b);
    let k = &mut Kit { b, sym };

    let xa = k.range(-5.0, -3.0);
    let xb = xa + HALL;
    let (yk, yd) = (k.range(-4.0, -2.0), k.range(8.0, 10.0));
    let (yp, yl) = (k.range(-8.0, -6.0), k.range(0.0, 2.0));

    k.wall_x(xa, [yk - 3.0, (yk + yd) / 2.0, yd + 4.0], DOOR);
    k.wall_x(xb, [yp - 3.0, (yp + yl) / 2.0, yl + 4.0], DOOR);
    k.wall_y(yk, -h, xa - T, [-15.0, xa - 6.0], 3.0);
    let living_door = k.range(-14.0, -6.0);
    k.wall_y(yd, -h, xa - T, [living_door], 2.4);
    k.wall_y(yp, xb + T, h, [], DOOR);
    k.wall_y(yl, xb + T, h, [h - 3.5], DOOR);
    k.b.structural();

    kitchen(k, rect(-h, -h, xa - T, yk - T));
    dining(k, rect(-h, yk + T, xa - T, yd - T));
    living(k, rect(-h, yd + T, xa - T, h));
    hallway(k, rect(xa + T, -h, xb - T, h));
    pantry(k, rect(xb + T, -h, h, yp - T));
    laundry(k, rect(xb + T, yp + T, h, yl - T));
    storeroom(k, rect(xb + T, yl + T, h, h));
}

/// The U of counters along the north, west, and east walls, the island, and
/// a checkered floor.
fn kitchen(k: &mut Kit, r: Aabb2) {
    let d = COUNTER_DEPTH;
    k.slab(r, 0.0, 0.02, TILE[0]);
    for x in r.min.x.ceil() as i32..r.max.x.floor() as i32 {
        for y in r.min.y.ceil() as i32..r.max.y.floor() as i32 {
            if (x + y) % 2 == 0 {
                let (x, y) = (x as f32, y as f32);
                k.slab(rect(x, y, x + 1.0, y + 1.0), 0.0, 0.025, TILE[1]);
            }
        }
    }

    let fridge = rect(r.max.x - 1.05, r.min.y, r.max.x, r.min.y + 1.3);
    k.block(fridge, TALL, STEEL);
    let mid = fridge.center().y;
    for y in [mid - 0.12, mid + 0.08] {
        k.slab(
            rect(fridge.min.x - 0.04, y, fridge.min.x, y + 0.04),
            0.7,
            1.5,
            DARK,
        );
    }
    k.slab(
        rect(fridge.min.x - 0.01, mid - 0.01, fridge.min.x, mid + 0.01),
        0.05,
        TALL - 0.05,
        DARK,
    );
    let tall = rect(r.max.x - d, fridge.max.y, r.max.x, fridge.max.y + 0.9);
    k.block(tall, TALL, CABINET);
    let east_end = tall.max.y + k.range(3.0, 4.5);
    k.counter(rect(r.max.x - d, tall.max.y, r.max.x, east_end));
    k.uppers(rect(r.max.x - 0.45, tall.max.y, r.max.x, east_end));

    let run_end = fridge.min.x;
    let len = run_end - r.min.x;
    let stove = r.min.x + len * k.range(0.55, 0.7);
    let sink = r.min.x + len * k.range(0.25, 0.38);
    k.counter(rect(r.min.x, r.min.y, stove - 0.55, r.min.y + d));
    k.counter(rect(stove + 0.55, r.min.y, run_end, r.min.y + d));
    k.stove(rect(stove - 0.55, r.min.y, stove + 0.55, r.min.y + d));
    k.slab(
        rect(sink - 0.45, r.min.y + 0.2, sink + 0.45, r.min.y + 0.7),
        COUNTER + 0.04,
        COUNTER + 0.07,
        STEEL,
    );
    k.disc(
        Vec2::new(sink, r.min.y + 0.12),
        0.04,
        COUNTER,
        COUNTER + 0.35,
        STEEL,
    );
    k.slab(
        rect(sink - 0.8, r.min.y - 0.02, sink + 0.8, r.min.y + 0.02),
        1.0,
        1.8,
        GLASS,
    );
    for (lo, hi) in [
        (r.min.x, sink - 0.9),
        (sink + 0.9, stove - 0.65),
        (stove + 0.65, run_end),
    ] {
        k.uppers(rect(lo, r.min.y, hi, r.min.y + 0.45));
    }

    let leg = k.range(5.0, 6.5);
    k.counter(rect(r.min.x, r.min.y + d, r.min.x + d, r.min.y + leg));
    k.uppers(rect(r.min.x, r.min.y + 0.45, r.min.x + 0.45, r.min.y + leg));
    k.bin(Vec2::new(r.min.x + 0.4, r.min.y + leg + 0.5));

    let c = Vec2::new(
        (r.min.x + r.max.x) / 2.0 + k.range(-1.0, 1.0),
        r.min.y + d + 2.5 + 0.8,
    );
    let island = Aabb2::centered(c, Vec2::new(2.3, 0.8));
    k.block(island, COUNTER, CABINET);
    k.slab(
        rect(
            island.min.x - 0.05,
            island.min.y - 0.05,
            island.max.x + 0.05,
            island.max.y + 0.3,
        ),
        COUNTER,
        COUNTER + 0.06,
        STONE,
    );
    for dx in [-1.2, 1.2] {
        k.slab(
            Aabb2::centered(c + Vec2::new(dx, 0.0), Vec2::new(0.015, 0.015)),
            1.75,
            WALL_HEIGHT,
            DARK,
        );
        k.disc(
            c + Vec2::new(dx, 0.0),
            0.25,
            1.6,
            1.75,
            paint([240, 210, 150], Material::Plain),
        );
    }
    for dx in [-1.4, 0.0, 1.4] {
        let at = Vec2::new(c.x + dx, island.max.y + 0.55);
        k.pillar(at, 0.18, 0.32, DARK);
        k.disc(at, 0.24, 0.32, 0.37, LIGHT_WOOD);
    }
    let nook = rect(r.min.x, island.max.y + 1.0, r.max.x, r.max.y);
    if let Some(c) = k.scatter(nook, Vec2::new(1.3, 1.3), 1.4) {
        k.table(
            c,
            Vec2::new(0.6, 0.6),
            &[(0.0, -1.0), (0.0, 1.0), (-1.0, 0.0), (1.0, 0.0)],
        );
    }
}

fn dining(k: &mut Kit, r: Aabb2) {
    k.slab(r, 0.0, 0.02, FLOOR_WOOD);
    let c = Vec2::new(
        r.center().x + k.range(-1.5, 1.5),
        r.center().y + k.range(-0.8, 0.8),
    );
    let half = Vec2::new(3.0, 1.0);
    let rug = k.pick(&RUG);
    k.slab(Aabb2::centered(c, half).grow(1.4), 0.02, 0.03, rug);
    k.table(
        c,
        half,
        &[
            (-2.0 / 3.0, -1.0),
            (0.0, -1.0),
            (2.0 / 3.0, -1.0),
            (-2.0 / 3.0, 1.0),
            (0.0, 1.0),
            (2.0 / 3.0, 1.0),
            (-1.0, 0.0),
            (1.0, 0.0),
        ],
    );
    k.disc(c, 0.35, 1.6, 1.8, paint([240, 210, 150], Material::Plain));
    k.slab(
        Aabb2::centered(c, Vec2::new(0.02, 0.02)),
        1.8,
        WALL_HEIGHT,
        DARK,
    );

    let y = r.center().y;
    let sideboard = rect(r.min.x, y - 1.75, r.min.x + 0.6, y + 1.75);
    k.block(sideboard, COUNTER, WOOD);
    k.slab(sideboard.grow(0.03), COUNTER, COUNTER + 0.04, LIGHT_WOOD);
    let hutch = rect(r.min.x, r.max.y - 3.0, r.min.x + 0.6, r.max.y - 1.0);
    k.block(hutch, TALL, WOOD);
    k.slab(
        rect(
            hutch.max.x,
            hutch.min.y + 0.15,
            hutch.max.x + 0.02,
            hutch.max.y - 0.15,
        ),
        1.0,
        1.8,
        GLASS,
    );
    k.plant(Vec2::new(r.max.x - 0.7, r.min.y + 0.7));
    k.plant(Vec2::new(r.max.x - 0.7, r.max.y - 0.7));
    if let Some(c) = k.scatter(r, Vec2::new(0.5, 0.35), 1.5) {
        k.block(Aabb2::centered(c, Vec2::new(0.5, 0.35)), 0.5, STEEL);
    }
    for _ in 0..2 {
        if let Some(c) = k.scatter(r, Vec2::new(0.45, 0.45), 1.5) {
            k.plant(c);
        }
    }
}

/// A sofa, armchair, and coffee table facing a TV on the outer wall, with
/// bookshelves.
fn living(k: &mut Kit, r: Aabb2) {
    k.slab(r, 0.0, 0.02, paint([150, 112, 80], Material::Wood));
    let x = r.center().x + k.range(-1.0, 1.0);
    let stand = rect(x - 1.6, r.max.y - 0.6, x + 1.6, r.max.y);
    k.block(stand, 0.4, WOOD);
    k.slab(
        rect(x - 1.2, r.max.y - 0.36, x + 1.2, r.max.y - 0.3),
        0.45,
        1.25,
        DARK,
    );

    let y = r.max.y - 5.5;
    let rug = k.pick(&RUG);
    k.slab(rect(x - 2.6, y - 0.4, x + 2.6, y + 3.0), 0.02, 0.03, rug);
    k.block(rect(x - 2.1, y - 0.5, x + 2.1, y + 0.5), 0.25, SOFA);
    k.block(rect(x - 2.4, y - 0.8, x + 2.4, y - 0.5), 0.55, SOFA);
    for ax in [x - 2.4, x + 2.1] {
        k.block(rect(ax, y - 0.5, ax + 0.3, y + 0.5), 0.38, SOFA);
    }
    k.block(rect(x - 0.9, y + 1.3, x + 0.9, y + 2.2), 0.25, LIGHT_WOOD);
    let arm = Vec2::new(x + 3.8, y + 1.6);
    k.block(Aabb2::centered(arm, Vec2::new(0.5, 0.5)), 0.25, SOFA);
    k.block(
        rect(arm.x + 0.5, arm.y - 0.5, arm.x + 0.8, arm.y + 0.5),
        0.55,
        SOFA,
    );

    for i in 0..2 {
        let y0 = r.min.y + 1.5 + i as f32 * 2.2;
        k.shelving(
            rect(r.min.x, y0, r.min.x + 0.6, y0 + 2.0),
            &[Vec2::new(1.0, 0.0)],
        );
    }
    k.plant(Vec2::new(r.max.x - 0.7, r.max.y - 0.7));

    if let Some(c) = k.scatter(r, Vec2::new(1.0, 0.8), 1.5) {
        k.block(rect(c.x - 1.0, c.y - 0.1, c.x + 1.0, c.y + 0.5), 0.8, DARK);
        k.block(rect(c.x - 0.6, c.y - 0.8, c.x + 0.6, c.y - 0.4), 0.3, DARK);
    }
    if let Some(c) = k.scatter(r, Vec2::new(1.0, 1.0), 1.5) {
        k.table(c, Vec2::new(0.8, 0.45), &[(0.0, -1.0)]);
    }
    for _ in 0..2 {
        if let Some(c) = k.scatter(r, Vec2::new(0.5, 0.8), 1.5) {
            k.block(Aabb2::centered(c, Vec2::new(0.5, 0.5)), 0.25, SOFA);
            k.block(rect(c.x - 0.5, c.y + 0.5, c.x + 0.5, c.y + 0.8), 0.55, SOFA);
        }
    }
    for _ in 0..3 {
        if let Some(c) = k.scatter(r, Vec2::new(0.45, 0.45), 1.5) {
            k.plant(c);
        }
    }
}

fn hallway(k: &mut Kit, r: Aabb2) {
    let c = r.center().x;
    k.slab(
        rect(c - 1.0, r.min.y + 1.5, c + 1.0, r.max.y - 1.5),
        0.0,
        0.02,
        RUG[1],
    );
    k.plant(Vec2::new(c, r.min.y + 0.7));
    k.plant(Vec2::new(c, r.max.y - 0.7));
}

/// Shelving along two walls and in rows, with two-unit aisles.
fn pantry(k: &mut Kit, r: Aabb2) {
    k.slab(r, 0.0, 0.02, CONCRETE);
    k.shelving(
        rect(r.min.x + 2.0, r.min.y, r.max.x, r.min.y + 0.6),
        &[Vec2::new(0.0, 1.0)],
    );
    k.shelving(
        rect(r.max.x - 0.6, r.min.y + 0.6, r.max.x, r.max.y),
        &[Vec2::new(-1.0, 0.0)],
    );
    let (y0, y1) = (r.min.y + 2.6, r.max.y - 2.2);
    let mut x = r.min.x + 2.0;
    while x + 1.0 <= r.max.x - 2.6 {
        k.shelving(
            rect(x, y0, x + 1.0, y1),
            &[Vec2::new(-1.0, 0.0), Vec2::new(1.0, 0.0)],
        );
        x += 3.0;
    }
    for i in 0..3 {
        let at = Vec2::new(r.max.x - 1.5 - i as f32 * 0.8, r.max.y - 0.5);
        k.block(
            Aabb2::centered(at, Vec2::new(0.3, 0.3)),
            0.5,
            CARDBOARD[i % 2],
        );
    }
}

fn laundry(k: &mut Kit, r: Aabb2) {
    k.slab(r, 0.0, 0.02, paint([196, 210, 214], Material::Tile));
    let y = r.center().y - 1.5;
    for i in 0..2 {
        let y0 = y + i as f32 * 0.95;
        let m = rect(r.max.x - 0.9, y0, r.max.x, y0 + 0.9);
        k.block(m, 0.55, WHITE);
        k.slab(
            rect(m.min.x - 0.02, m.min.y + 0.2, m.min.x, m.max.y - 0.2),
            0.12,
            0.42,
            DARK,
        );
    }
    k.counter(rect(r.max.x - COUNTER_DEPTH, y + 1.9, r.max.x, y + 3.4));
    k.pillar(Vec2::new(r.max.x - 0.6, r.min.y + 0.6), 0.45, 1.8, WHITE);
    k.shelving(
        rect(r.min.x + 2.5, r.min.y, r.min.x + 9.5, r.min.y + 0.5),
        &[Vec2::new(0.0, 1.0)],
    );
    let run = rect(
        r.min.x + 2.5,
        r.max.y - COUNTER_DEPTH,
        r.max.x - 5.5,
        r.max.y,
    );
    k.counter(run);
    k.uppers(rect(run.min.x, r.max.y - 0.45, run.max.x, r.max.y));
    k.block(
        rect(run.max.x, r.max.y - 0.7, run.max.x + 1.0, r.max.y),
        TALL,
        WHITE,
    );
    for _ in 0..3 {
        if let Some(c) = k.scatter(r, Vec2::new(0.9, 0.5), 1.5) {
            k.block(Aabb2::centered(c, Vec2::new(0.9, 0.5)), 0.45, LIGHT_WOOD);
        }
    }
    for _ in 0..2 {
        if let Some(c) = k.scatter(r, Vec2::new(0.3, 0.3), 1.2) {
            k.pillar(c, 0.3, 0.4, paint([214, 200, 160], Material::Fabric));
        }
    }
}

/// Stacks of boxes, rows of shelving, and a chest freezer.
fn storeroom(k: &mut Kit, r: Aabb2) {
    k.slab(r, 0.0, 0.02, CONCRETE);
    k.shelving(
        rect(r.min.x + 2.0, r.max.y - 0.6, r.max.x - 2.0, r.max.y),
        &[Vec2::new(0.0, -1.0)],
    );
    k.block(
        rect(r.max.x - 0.8, r.min.y + 2.5, r.max.x, r.min.y + 4.1),
        0.5,
        WHITE,
    );
    for _ in 0..2 {
        if let Some(c) = k.scatter(r, Vec2::new(3.0, 0.5), 2.0) {
            k.shelving(
                Aabb2::centered(c, Vec2::new(3.0, 0.5)),
                &[Vec2::new(0.0, -1.0), Vec2::new(0.0, 1.0)],
            );
        }
    }
    for _ in 0..6 {
        let Some(c) = k.scatter(r, Vec2::new(1.1, 1.1), 1.5) else {
            continue;
        };
        for (dx, dy) in [(-0.55, -0.55), (0.55, -0.55), (-0.55, 0.55), (0.55, 0.55)] {
            if k.b.rng.chance(0.3) {
                continue;
            }
            let half = k.range(0.35, 0.5);
            let height = k.range(0.5, 1.4);
            let finish = k.pick(&CARDBOARD);
            let at = Vec2::new(c.x + dx, c.y + dy);
            k.block(Aabb2::centered(at, Vec2::new(half, half)), height, finish);
        }
    }
}

/// One of the square's eight symmetries.
#[derive(Copy, Clone, Debug)]
struct Sym {
    swap: bool,
    flip_x: bool,
    flip_y: bool,
}

impl Sym {
    fn random(b: &mut Builder) -> Sym {
        let n = b.rng.below(8);
        Sym {
            swap: n & 1 != 0,
            flip_x: n & 2 != 0,
            flip_y: n & 4 != 0,
        }
    }

    fn pt(self, p: Vec2) -> Vec2 {
        let p = if self.swap { Vec2::new(p.y, p.x) } else { p };
        Vec2::new(
            if self.flip_x { -p.x } else { p.x },
            if self.flip_y { -p.y } else { p.y },
        )
    }

    fn rect(self, r: Aabb2) -> Aabb2 {
        let (a, b) = (self.pt(r.min), self.pt(r.max));
        rect(a.x, a.y, b.x, b.y)
    }
}

fn rect(x0: f32, y0: f32, x1: f32, y1: f32) -> Aabb2 {
    Aabb2 {
        min: Vec2::new(x0.min(x1), y0.min(y1)),
        max: Vec2::new(x0.max(x1), y0.max(y1)),
    }
}

/// Places furniture in plan coordinates through the layout's symmetry.
struct Kit<'a, 'b> {
    b: &'a mut Builder<'b>,
    sym: Sym,
}

impl Kit<'_, '_> {
    fn range(&mut self, lo: f32, hi: f32) -> f32 {
        self.b.rng.range(lo, hi)
    }

    fn pick<T: Copy>(&mut self, xs: &[T]) -> T {
        self.b.pick(xs)
    }

    fn block(&mut self, r: Aabb2, height: f32, paint: Paint) {
        self.b.block(self.sym.rect(r), height, paint);
    }

    fn pillar(&mut self, c: Vec2, radius: f32, height: f32, paint: Paint) {
        self.b.pillar(self.sym.pt(c), radius, height, paint);
    }

    /// A drawn box over `r` from `y0` to `y1`.
    fn slab(&mut self, r: Aabb2, y0: f32, y1: f32, paint: Paint) {
        let r = self.sym.rect(r);
        let c = r.center();
        self.b.deco(
            Shape::Box,
            Vec3::new(c.x, (y0 + y1) / 2.0, c.y),
            Vec3::new(r.max.x - r.min.x, y1 - y0, r.max.y - r.min.y),
            paint,
        );
    }

    /// A drawn cylinder from `y0` to `y1`.
    fn disc(&mut self, c: Vec2, radius: f32, y0: f32, y1: f32, paint: Paint) {
        let c = self.sym.pt(c);
        self.b.deco(
            Shape::Cylinder,
            Vec3::new(c.x, (y0 + y1) / 2.0, c.y),
            Vec3::new(radius * 2.0, y1 - y0, radius * 2.0),
            paint,
        );
    }

    fn wall_x<const N: usize>(&mut self, x: f32, doors: [f32; N], width: f32) {
        let h = HALF_EXTENT;
        for (lo, hi) in gaps(-h, h, &doors, width) {
            self.block(rect(x - T, lo, x + T, hi), WALL_HEIGHT, WALL);
        }
        for d in doors {
            let r = rect(x - T, d - width / 2.0, x + T, d + width / 2.0);
            self.door(r);
        }
    }

    fn wall_y<const N: usize>(&mut self, y: f32, x0: f32, x1: f32, doors: [f32; N], width: f32) {
        for (lo, hi) in gaps(x0, x1, &doors, width) {
            self.block(rect(lo, y - T, hi, y + T), WALL_HEIGHT, WALL);
        }
        for d in doors {
            let r = rect(d - width / 2.0, y - T, d + width / 2.0, y + T);
            self.door(r);
        }
    }

    /// A lintel over the gap `r`, and room to walk through it.
    fn door(&mut self, r: Aabb2) {
        self.slab(r, 2.0, WALL_HEIGHT, WALL);
        let clear = self.sym.rect(r.grow(1.2));
        self.b.keep_out.push(clear);
    }

    /// A centre in `room` where a box of `half` extents clears doorways and
    /// stays `gap` from other furniture, if one turns up.
    fn scatter(&mut self, room: Aabb2, half: Vec2, gap: f32) -> Option<Vec2> {
        for _ in 0..40 {
            let c = Vec2::new(
                self.range(room.min.x + half.x, room.max.x - half.x),
                self.range(room.min.y + half.y, room.max.y - half.y),
            );
            if self.b.fits(self.sym.rect(Aabb2::centered(c, half)), gap) {
                return Some(c);
            }
        }
        None
    }

    /// A base cabinet under a stone top.
    fn counter(&mut self, r: Aabb2) {
        self.block(r, COUNTER, CABINET);
        self.slab(r.grow(0.03), COUNTER, COUNTER + 0.06, STONE);
    }

    /// Wall cabinets above head height. Drawn only, so they never block.
    fn uppers(&mut self, r: Aabb2) {
        if r.max.x > r.min.x && r.max.y > r.min.y {
            self.slab(r, 1.3, 1.85, CABINET);
        }
    }

    /// A range in the counter run along the plan's north wall, with burners,
    /// an oven door facing south, and a hood above.
    fn stove(&mut self, r: Aabb2) {
        self.block(r, COUNTER + 0.02, STEEL);
        let c = r.center();
        for (dx, dy) in [(-0.25, -0.18), (0.25, -0.18), (-0.25, 0.18), (0.25, 0.18)] {
            let at = Vec2::new(c.x + dx, c.y + dy);
            self.disc(at, 0.13, COUNTER + 0.02, COUNTER + 0.04, DARK);
        }
        self.slab(
            rect(r.min.x + 0.1, r.max.y, r.max.x - 0.1, r.max.y + 0.02),
            0.08,
            0.4,
            DARK,
        );
        self.slab(
            rect(r.min.x - 0.05, r.min.y, r.max.x + 0.05, r.min.y + 0.6),
            1.45,
            1.7,
            STEEL,
        );
        self.slab(
            rect(c.x - 0.25, r.min.y, c.x + 0.25, r.min.y + 0.35),
            1.7,
            WALL_HEIGHT,
            STEEL,
        );
    }

    /// A table of `half` extents with a chair at each `(x, y)`, given as a
    /// fraction of `half` with one coordinate at ±1, so it sits on that side.
    fn table(&mut self, c: Vec2, half: Vec2, seats: &[(f32, f32)]) {
        self.block(Aabb2::centered(c, half), 0.42, WOOD);
        for &(fx, fy) in seats {
            let out = if fy.abs() == 1.0 {
                Vec2::new(0.0, fy)
            } else {
                Vec2::new(fx, 0.0)
            };
            let at = Vec2::new(c.x + fx * half.x, c.y + fy * half.y) + out * 0.4;
            self.chair(at, out);
        }
    }

    /// A seat facing away from `out`, with its back on that side.
    fn chair(&mut self, at: Vec2, out: Vec2) {
        let seat = Aabb2::centered(at, Vec2::new(0.24, 0.24));
        self.block(seat, 0.26, WOOD);
        let back = at + out * 0.2;
        let half = if out.x != 0.0 {
            Vec2::new(0.04, 0.24)
        } else {
            Vec2::new(0.24, 0.04)
        };
        self.slab(Aabb2::centered(back, half), 0.26, 0.6, WOOD);
    }

    /// A storage unit that blocks sight, with boards and goods on each face
    /// whose outward normal is in `fronts`.
    fn shelving(&mut self, r: Aabb2, fronts: &[Vec2]) {
        self.block(r, TALL, SHELF);
        for level in 0..4 {
            let y = 0.1 + level as f32 * 0.45;
            self.slab(r.grow(0.03), y, y + 0.04, LIGHT_WOOD);
            if level < 3 {
                for &n in fronts {
                    self.goods(r, n, y + 0.04);
                }
            }
        }
    }

    /// A row of jars and boxes standing at `y`, poking out of the face of `r`
    /// that faces `n`.
    fn goods(&mut self, r: Aabb2, n: Vec2, y: f32) {
        let along_x = n.x == 0.0;
        let (lo, hi) = if along_x {
            (r.min.x, r.max.x)
        } else {
            (r.min.y, r.max.y)
        };
        let face = match (n.x > 0.0, n.x < 0.0, n.y > 0.0) {
            (true, _, _) => r.max.x,
            (_, true, _) => r.min.x,
            (_, _, true) => r.max.y,
            _ => r.min.y,
        };
        let mut s = lo + 0.15;
        while s < hi - 0.3 {
            let w = self.range(0.18, 0.32);
            let tall = self.range(0.12, 0.32);
            let finish = self.pick(&GOODS);
            if self.b.rng.chance(0.6) {
                let (a, b) = (face - 0.08, face + 0.04);
                let (a, b) = if n.x < 0.0 || n.y < 0.0 {
                    (2.0 * face - b, 2.0 * face - a)
                } else {
                    (a, b)
                };
                let r = if along_x {
                    rect(s, a, s + w, b)
                } else {
                    rect(a, s, b, s + w)
                };
                if self.b.rng.chance(0.4) {
                    let c = r.center();
                    self.disc(c, w / 2.0, y, y + tall, finish);
                } else {
                    self.slab(r, y, y + tall, finish);
                }
            }
            s += w + 0.14;
        }
    }

    /// A potted plant tall enough to hide behind.
    fn plant(&mut self, at: Vec2) {
        let at_world = self.sym.pt(at);
        self.b.solid(
            Footprint::Circle {
                center: at_world,
                radius: 0.45,
            },
            1.3,
        );
        self.disc(at, 0.35, 0.0, 0.45, POT);
        self.disc(at, 0.55, 0.45, 1.4, LEAVES);
    }

    fn bin(&mut self, at: Vec2) {
        self.pillar(at, 0.28, 0.45, STEEL);
    }
}

/// The spans of `lo..hi` left after cutting a `width` gap at each door.
fn gaps(lo: f32, hi: f32, doors: &[f32], width: f32) -> Vec<(f32, f32)> {
    let mut cuts: Vec<f32> = doors.to_vec();
    cuts.sort_by(f32::total_cmp);
    let mut out = Vec::new();
    let mut from = lo;
    for d in cuts {
        out.push((from, d - width / 2.0));
        from = d + width / 2.0;
    }
    out.push((from, hi));
    out.retain(|(a, b)| b > a);
    out
}
