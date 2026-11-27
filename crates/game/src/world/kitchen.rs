//! One kitchen seen by something the size of a beetle: 10 u to the metre,
//! so the play area is a 4 m square kitchen and an avatar stands about
//! 10 cm tall.
//!
//! An L of counters runs along two walls with a sink under a tall window,
//! a range under a hood, and wall cabinets above. A fridge fills the
//! corner, an island stands in the middle, and a table with four chairs
//! sits across the room. The floor is littered with things to hide behind
//! and hop onto. Counters, the island, and the table are a second level:
//! launch pads on the floor throw you onto them, and pads on the counter
//! throw you onto the fridge and the wall cabinets. Under every counter is
//! a toe-kick slot just tall enough to crawl into.
//!
//! The plan is laid out in one orientation, then mirrored or turned by a
//! random symmetry of the square.

use super::{Aabb2, Builder, Footprint, HALF_EXTENT, Material, Paint, Shape, Vec2, Vec3, paint};

const WALL: Paint = paint([232, 224, 206], Material::Plaster);
const CABINET: Paint = paint([92, 128, 118], Material::Wood);
const STONE: Paint = paint([222, 220, 212], Material::Concrete);
const STEEL: Paint = paint([178, 182, 188], Material::Metal);
const DARK: Paint = paint([44, 44, 48], Material::Plain);
const GLASS: Paint = paint([168, 206, 228], Material::Plain);
const SKY: Paint = paint([196, 226, 246], Material::Plain);
const WOOD: Paint = paint([150, 100, 62], Material::Wood);
const LIGHT_WOOD: Paint = paint([196, 158, 112], Material::Wood);
const TILE: [Paint; 2] = [
    paint([236, 232, 224], Material::Tile),
    paint([96, 102, 112], Material::Tile),
];
const CERAMIC: [Paint; 3] = [
    paint([230, 96, 80], Material::Plain),
    paint([90, 140, 210], Material::Plain),
    paint([244, 240, 232], Material::Plain),
];
const CARDBOARD: Paint = paint([214, 170, 92], Material::Plain);
const SPONGE: Paint = paint([246, 214, 70], Material::Fabric);
const SCOURER: Paint = paint([60, 150, 80], Material::Fabric);
const SUGAR: Paint = paint([250, 250, 246], Material::Plain);
const BOOKS: [Paint; 3] = [
    paint([170, 60, 56], Material::Fabric),
    paint([52, 84, 140], Material::Fabric),
    paint([212, 180, 96], Material::Fabric),
];
const FRUIT: [Paint; 3] = [
    paint([214, 50, 40], Material::Plain),
    paint([250, 190, 40], Material::Plain),
    paint([120, 180, 60], Material::Plain),
];

pub(crate) const WALL_HEIGHT: f32 = 40.0;
/// Counter depth.
const DEPTH: f32 = 6.0;
/// The top of the counters and the island.
pub(crate) const COUNTER: f32 = 9.2;
/// The stone top's thickness.
const TOP: f32 = 0.6;
/// The toe-kick slot under every cabinet: this tall, and this deep.
pub(crate) const KICK: f32 = 1.2;
const KICK_DEPTH: f32 = 0.9;
pub(crate) const FRIDGE: f32 = 18.0;
/// Wall cabinets hang from here to `UPPER_TOP`, this deep.
const UPPER_BASE: f32 = 14.0;
pub(crate) const UPPER_TOP: f32 = 21.0;
const UPPER_DEPTH: f32 = 3.5;
const TABLE: f32 = 7.6;
const SEAT: f32 = 4.5;
/// How high above the floor the floor pads throw you, and the table's.
const FLOOR_PAD: f32 = COUNTER + 3.0;
const TABLE_PAD: f32 = TABLE + 2.5;

pub(super) fn build(b: &mut Builder) {
    let h = HALF_EXTENT;
    b.perimeter(WALL_HEIGHT, WALL);
    let sym = Sym::random(b);
    let k = &mut Kit { b, sym };

    floor(k);
    baseboards(k);

    // The north run, wall to fridge, with a sink and a range in it.
    let fridge = rect(h - 7.0, -h, h, -h + 7.0);
    let north = rect(-h, -h, fridge.min.x, -h + DEPTH);
    let sink = k.range(-14.0, -11.0);
    let range = k.range(-1.0, 3.0);
    let (range_lo, range_hi) = (range - 3.5, range + 3.5);
    k.counter(
        rect(north.min.x, north.min.y, range_lo, north.max.y),
        Side::South,
    );
    k.counter(
        rect(range_hi, north.min.y, north.max.x, north.max.y),
        Side::South,
    );
    k.stove(rect(range_lo, north.min.y, range_hi, north.max.y));
    k.sink(Vec2::new(sink, north.min.y + DEPTH / 2.0));
    k.window(sink);
    k.fridge(fridge);

    // The west run, from the corner to past the middle.
    let west_end = k.range(4.0, 8.0);
    let west = rect(-h, north.max.y, -h + DEPTH, west_end);
    k.counter(west, Side::East);

    // Wall cabinets over the west run, and over the north run between the
    // range and the fridge.
    k.uppers(rect(
        -h,
        north.max.y + 1.0,
        -h + UPPER_DEPTH,
        west_end - 1.0,
    ));
    let right_uppers = rect(range_hi + 1.0, -h, fridge.min.x, -h + UPPER_DEPTH);
    k.uppers(right_uppers);

    // Pads onto the counters, and from the counter onto the fridge and the
    // wall cabinets.
    k.pad(Vec2::new(sink + 4.0, north.max.y + 1.6), 0.0, FLOOR_PAD);
    k.pad(
        Vec2::new(west.max.x + 1.6, (west.min.y + west.max.y) / 2.0),
        0.0,
        FLOOR_PAD,
    );
    // Beside both the fridge and the wall cabinets: high enough for either.
    k.pad(
        Vec2::new(fridge.min.x - 1.4, north.max.y - 1.4),
        COUNTER,
        UPPER_TOP - COUNTER + 3.0,
    );
    k.pad(
        Vec2::new(
            (right_uppers.min.x + right_uppers.max.x) / 2.0,
            north.max.y - 1.2,
        ),
        COUNTER,
        UPPER_TOP - COUNTER + 3.0,
    );

    north_top(k, north, range_lo, range_hi, sink, fridge.min.x);
    west_top(k, west);

    let island = Aabb2::centered(
        Vec2::new(k.range(0.0, 4.0), k.range(-4.0, -2.0)),
        Vec2::new(6.0, 3.0),
    );
    k.island(island);
    k.pad(
        Vec2::new(island.center().x - 3.0, island.max.y + 1.6),
        0.0,
        FLOOR_PAD,
    );

    let table = Aabb2::centered(
        Vec2::new(k.range(5.0, 8.0), k.range(10.0, 12.0)),
        Vec2::new(6.0, 4.0),
    );
    k.table(table);
    k.pad(
        Vec2::new(table.max.x + 1.6, table.min.y + 1.0),
        0.0,
        TABLE_PAD,
    );

    k.bin(Vec2::new(h - 2.0, h - 2.0));
    clutter(k);
}

/// Big checkered tiles.
fn floor(k: &mut Kit) {
    let h = HALF_EXTENT;
    k.slab(rect(-h, -h, h, h), 0.0, 0.02, TILE[0]);
    let cell = 4.0;
    let n = (2.0 * h / cell).ceil() as i32;
    for i in 0..n {
        for j in 0..n {
            if (i + j) % 2 == 0 {
                let (x, y) = (-h + i as f32 * cell, -h + j as f32 * cell);
                let t = rect(x, y, (x + cell).min(h), (y + cell).min(h));
                k.slab(t, 0.02, 0.03, TILE[1]);
            }
        }
    }
}

fn baseboards(k: &mut Kit) {
    let h = HALF_EXTENT;
    let board = paint([250, 248, 240], Material::Wood);
    for r in [
        rect(-h, -h, h, -h + 0.08),
        rect(-h, h - 0.08, h, h),
        rect(-h, -h, -h + 0.08, h),
        rect(h - 0.08, -h, h, h),
    ] {
        k.slab(r, 0.0, 1.0, board);
    }
}

/// A toaster and a knife block between the sink and the range, a kettle
/// past the range, and jars along the wall to the fridge.
fn north_top(k: &mut Kit, run: Aabb2, range_lo: f32, range_hi: f32, sink: f32, end: f32) {
    let y = run.min.y + DEPTH / 2.0;
    let toaster = (sink + 4.5 + range_lo - 1.0) / 2.0;
    if toaster - 1.5 > sink + 3.5 && toaster + 1.5 < range_lo - 3.0 {
        k.raised(
            rect(toaster - 1.5, y - 1.0, toaster + 1.5, y + 1.0),
            2.2,
            STEEL,
        );
        k.slab(
            rect(toaster - 1.0, y - 0.3, toaster + 1.0, y + 0.3),
            COUNTER + 2.2,
            COUNTER + 2.25,
            DARK,
        );
    }
    k.raised(
        rect(
            range_lo - 2.6,
            run.min.y + 0.6,
            range_lo - 1.4,
            run.min.y + 1.8,
        ),
        2.6,
        WOOD,
    );
    k.raised_disc(Vec2::new(range_hi + 2.6, y), 1.1, 2.4, CERAMIC[2]);
    k.slab(
        rect(range_hi + 3.5, y - 0.15, range_hi + 4.4, y + 0.15),
        COUNTER + 1.6,
        COUNTER + 1.9,
        DARK,
    );
    let mut x = range_hi + 5.5;
    while x < end - 1.0 {
        let jar = k.pick(&CERAMIC);
        let tall = k.range(1.2, 2.2);
        k.raised_disc(Vec2::new(x, run.min.y + 1.0), 0.6, tall, jar);
        x += k.range(1.6, 2.4);
    }
}

/// A cutting board, flat enough to walk onto, and a bowl of fruit.
fn west_top(k: &mut Kit, run: Aabb2) {
    let x = (run.min.x + run.max.x) / 2.0;
    let y = k.range(run.min.y + 3.0, run.max.y - 7.0);
    k.slab(
        rect(x - 1.5, y - 2.0, x + 1.5, y + 2.0),
        COUNTER,
        COUNTER + 0.15,
        LIGHT_WOOD,
    );
    let bowl = Vec2::new(x, y + 4.5);
    k.raised_disc(bowl, 1.5, 0.6, CERAMIC[1]);
    for (dx, dy) in [(-0.5, -0.3), (0.5, 0.0), (0.0, 0.6)] {
        let f = k.pick(&FRUIT);
        k.disc(
            bowl + Vec2::new(dx, dy),
            0.45,
            COUNTER + 0.6,
            COUNTER + 1.3,
            f,
        );
    }
}

/// Things on the floor: a fallen cereal box, a mug, a pet bowl, a sponge,
/// a spoon, a pile of cookbooks reached by a stair of sugar cubes, and
/// crumbs. Each lands somewhere clear, if it can.
fn clutter(k: &mut Kit) {
    let h = HALF_EXTENT;
    let room = rect(-h + DEPTH + 1.0, -h + DEPTH + 1.0, h - 1.0, h - 1.0);

    if let Some(c) = k.scatter(room, Vec2::new(3.0, 1.0), 1.2) {
        k.block(Aabb2::centered(c, Vec2::new(3.0, 1.0)), 2.0, CARDBOARD);
        k.slab(
            Aabb2::centered(c + Vec2::new(0.6, 0.0), Vec2::new(1.4, 1.02)),
            0.6,
            1.4,
            CERAMIC[0],
        );
    }
    if let Some(c) = k.scatter(room, Vec2::new(0.8, 0.5), 1.2) {
        let mug = k.pick(&CERAMIC);
        k.pillar(c, 0.5, 1.0, mug);
        k.slab(
            Aabb2::centered(c + Vec2::new(0.6, 0.0), Vec2::new(0.12, 0.08)),
            0.25,
            0.75,
            mug,
        );
    }
    if let Some(c) = k.scatter(room, Vec2::new(1.1, 1.1), 1.2) {
        k.pillar(c, 1.1, 0.5, CERAMIC[1]);
    }
    if let Some(c) = k.scatter(room, Vec2::new(0.9, 0.6), 1.0) {
        k.block(Aabb2::centered(c, Vec2::new(0.9, 0.6)), 0.4, SPONGE);
        k.slab(Aabb2::centered(c, Vec2::new(0.9, 0.6)), 0.4, 0.55, SCOURER);
    }
    if let Some(c) = k.scatter(room, Vec2::new(2.2, 0.5), 1.0) {
        k.slab(Aabb2::centered(c, Vec2::new(1.4, 0.12)), 0.0, 0.1, STEEL);
        k.disc(c + Vec2::new(1.7, 0.0), 0.45, 0.0, 0.12, STEEL);
    }
    if let Some(c) = k.scatter(room, Vec2::new(3.5, 2.0), 1.2) {
        // Three cubes, each a jump higher than the last, up to the books.
        for (i, step) in [0.7, 1.4, 2.1].into_iter().enumerate() {
            let x = c.x - 3.5 + i as f32;
            k.block(rect(x, c.y - 0.5, x + 1.0, c.y + 0.5), step, SUGAR);
        }
        let books = rect(c.x - 0.5, c.y - 2.0, c.x + 3.5, c.y + 2.0);
        let mut y0 = 0.0;
        for (i, finish) in BOOKS.into_iter().enumerate() {
            let y1 = y0 + 0.7;
            k.slab(books.grow(-0.15 * i as f32), y0, y1, finish);
            y0 = y1;
        }
        k.b.solid(Footprint::Aabb(k.sym.rect(books)), y0);
    }
    for _ in 0..12 {
        if let Some(c) = k.scatter(room, Vec2::new(0.2, 0.2), 0.5) {
            k.slab(
                Aabb2::centered(c, Vec2::new(0.15, 0.1)),
                0.0,
                0.12,
                CARDBOARD,
            );
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

/// Which way a cabinet faces the room, in plan coordinates.
#[derive(Copy, Clone, Debug)]
enum Side {
    South,
    East,
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

    /// A box that blocks from `y0` to `y1`.
    fn platform(&mut self, r: Aabb2, y0: f32, y1: f32, paint: Paint) {
        self.b.platform(Shape::Box, self.sym.rect(r), y0, y1, paint);
    }

    /// A box standing on the counter top, `tall` high.
    fn raised(&mut self, r: Aabb2, tall: f32, paint: Paint) {
        self.platform(r, COUNTER, COUNTER + tall, paint);
    }

    /// A cylinder standing on the counter top, `tall` high.
    fn raised_disc(&mut self, c: Vec2, radius: f32, tall: f32, paint: Paint) {
        let r = Aabb2::centered(self.sym.pt(c), Vec2::new(radius, radius));
        self.b
            .platform(Shape::Cylinder, r, COUNTER, COUNTER + tall, paint);
    }

    fn pad(&mut self, c: Vec2, y: f32, apex: f32) {
        self.b.pad(self.sym.pt(c), y, apex);
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

    /// A centre in `room` where a box of `half` extents stays `gap` from
    /// everything else, if one turns up.
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

    /// A base cabinet over a plinth set back from the front on `side`,
    /// under a stone top. Doors and handles on the face.
    fn counter(&mut self, r: Aabb2, side: Side) {
        let plinth = match side {
            Side::South => rect(r.min.x, r.min.y, r.max.x, r.max.y - KICK_DEPTH),
            Side::East => rect(r.min.x, r.min.y, r.max.x - KICK_DEPTH, r.max.y),
        };
        self.block(plinth, KICK, DARK);
        self.platform(r, KICK, COUNTER - TOP, CABINET);
        self.platform(r, COUNTER - TOP, COUNTER, STONE);
        self.fronts(r, side, KICK, COUNTER - TOP);
    }

    /// Door seams and handles every few units along a cabinet face.
    fn fronts(&mut self, r: Aabb2, side: Side, y0: f32, y1: f32) {
        let (lo, hi) = match side {
            Side::South => (r.min.x, r.max.x),
            Side::East => (r.min.y, r.max.y),
        };
        let doors = ((hi - lo) / 4.5).round().max(1.0) as i32;
        let w = (hi - lo) / doors as f32;
        let face = |s0: f32, s1: f32, out: f32| match side {
            Side::South => rect(s0, r.max.y, s1, r.max.y + out),
            Side::East => rect(r.max.x, s0, r.max.x + out, s1),
        };
        for i in 0..doors {
            let a = lo + i as f32 * w;
            self.slab(face(a, a + 0.08, 0.04), y0, y1, DARK);
            let mid = a + w / 2.0;
            self.slab(face(mid - 0.8, mid + 0.8, 0.15), y1 - 1.2, y1 - 0.9, STEEL);
        }
    }

    /// A steel range in the run: burners, an oven door, and a hood above
    /// whose top can be stood on.
    fn stove(&mut self, r: Aabb2) {
        self.block(r, COUNTER, STEEL);
        let c = r.center();
        for (dx, dy) in [(-1.6, -1.2), (1.6, -1.2), (-1.6, 1.2), (1.6, 1.2)] {
            let at = Vec2::new(c.x + dx, c.y + dy);
            self.disc(at, 1.0, COUNTER, COUNTER + 0.15, DARK);
        }
        self.slab(
            rect(r.min.x + 0.6, r.max.y, r.max.x - 0.6, r.max.y + 0.06),
            1.0,
            6.5,
            DARK,
        );
        self.slab(
            rect(r.min.x + 1.0, r.max.y, r.max.x - 1.0, r.max.y + 0.3),
            6.9,
            7.3,
            STEEL,
        );
        self.platform(
            rect(r.min.x - 0.3, r.min.y, r.max.x + 0.3, r.min.y + 4.5),
            16.0,
            18.5,
            STEEL,
        );
        self.slab(
            rect(c.x - 1.5, r.min.y, c.x + 1.5, r.min.y + 2.5),
            18.5,
            WALL_HEIGHT,
            STEEL,
        );
    }

    /// A basin and a tall tap at `c` on the counter top.
    fn sink(&mut self, c: Vec2) {
        let basin = Aabb2::centered(c + Vec2::new(0.0, 0.4), Vec2::new(3.0, 1.8));
        self.slab(basin, COUNTER, COUNTER + 0.04, STEEL);
        self.slab(basin.grow(-0.3), COUNTER + 0.04, COUNTER + 0.06, DARK);
        let tap = c + Vec2::new(0.0, -2.2);
        let r = Aabb2::centered(self.sym.pt(tap), Vec2::new(0.3, 0.3));
        self.b
            .platform(Shape::Cylinder, r, COUNTER, COUNTER + 4.0, STEEL);
        self.slab(
            Aabb2::centered(tap + Vec2::new(0.0, 1.0), Vec2::new(0.2, 1.2)),
            COUNTER + 3.6,
            COUNTER + 4.0,
            STEEL,
        );
    }

    /// A tall window over the sink, on the north wall.
    fn window(&mut self, x: f32) {
        let y = -HALF_EXTENT;
        self.slab(rect(x - 5.0, y, x + 5.0, y + 0.05), 13.0, 32.0, SKY);
        for (x0, x1) in [(x - 5.4, x - 5.0), (x - 0.2, x + 0.2), (x + 5.0, x + 5.4)] {
            self.slab(rect(x0, y, x1, y + 0.3), 12.6, 32.4, WALL);
        }
        for (y0, y1) in [(12.6, 13.0), (22.3, 22.7), (32.0, 32.4)] {
            self.slab(rect(x - 5.4, y, x + 5.4, y + 0.3), y0, y1, WALL);
        }
        self.slab(rect(x - 5.4, y, x + 5.4, y + 1.0), 12.4, 12.6, GLASS);
    }

    fn fridge(&mut self, r: Aabb2) {
        self.block(r, FRIDGE, STEEL);
        let front = r.max.y;
        self.slab(
            rect(r.min.x, front, r.max.x, front + 0.05),
            11.8,
            12.0,
            DARK,
        );
        for (y0, y1) in [(5.0, 11.0), (12.8, 16.5)] {
            self.slab(
                rect(r.min.x + 0.8, front, r.min.x + 1.1, front + 0.4),
                y0,
                y1,
                DARK,
            );
        }
    }

    /// Wall cabinets: they block from `UPPER_BASE` to `UPPER_TOP`, and their
    /// tops can be stood on.
    fn uppers(&mut self, r: Aabb2) {
        if r.max.x <= r.min.x || r.max.y <= r.min.y {
            return;
        }
        self.platform(r, UPPER_BASE, UPPER_TOP, CABINET);
    }

    /// Cabinet and stone like a counter, with a toe kick all round.
    fn island(&mut self, r: Aabb2) {
        self.block(r.grow(-KICK_DEPTH), KICK, DARK);
        self.platform(r, KICK, COUNTER - TOP, CABINET);
        self.platform(r.grow(0.4), COUNTER - TOP, COUNTER, STONE);
        self.fronts(r, Side::South, KICK, COUNTER - TOP);
    }

    /// A table on four legs, with a chair on every side.
    fn table(&mut self, r: Aabb2) {
        self.platform(r, TABLE - 0.4, TABLE, WOOD);
        let inset = 0.6;
        for (x, y) in [
            (r.min.x + inset, r.min.y + inset),
            (r.max.x - inset, r.min.y + inset),
            (r.min.x + inset, r.max.y - inset),
            (r.max.x - inset, r.max.y - inset),
        ] {
            self.pillar(Vec2::new(x, y), 0.35, TABLE - 0.4, WOOD);
        }
        let c = r.center();
        let across = (r.max.x - r.min.x) / 2.0;
        let deep = (r.max.y - r.min.y) / 2.0;
        for out in [
            Vec2::new(0.0, -1.0),
            Vec2::new(0.0, 1.0),
            Vec2::new(-1.0, 0.0),
            Vec2::new(1.0, 0.0),
        ] {
            let edge = if out.x == 0.0 { deep } else { across };
            self.chair(c + out * (edge + 1.2), out);
        }
    }

    /// A seat on four legs with its back on the `out` side.
    fn chair(&mut self, at: Vec2, out: Vec2) {
        let half = 1.8;
        let seat = Aabb2::centered(at, Vec2::new(half, half));
        self.platform(seat, SEAT - 0.4, SEAT, LIGHT_WOOD);
        for (dx, dy) in [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
            let leg = at + Vec2::new(dx, dy) * (half - 0.3);
            self.pillar(leg, 0.2, SEAT - 0.4, LIGHT_WOOD);
        }
        let back = at + out * (half - 0.2);
        let span = if out.x != 0.0 {
            Vec2::new(0.2, half)
        } else {
            Vec2::new(half, 0.2)
        };
        self.platform(Aabb2::centered(back, span), SEAT, SEAT + 4.5, LIGHT_WOOD);
    }

    fn bin(&mut self, at: Vec2) {
        self.pillar(at, 1.5, 6.0, STEEL);
        self.disc(at, 1.6, 6.0, 6.3, DARK);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Body, Input, SceneKind};
    use crate::physics::{GRAVITY, HEIGHT, RADIUS, blocks, step};
    use crate::world::World;

    const _: () = assert!(COUNTER >= 8.0 * HEIGHT, "counters tower over avatars");

    fn kitchens() -> impl Iterator<Item = World> {
        (0..30).map(|seed| World::generate(SceneKind::Kitchen, seed))
    }

    fn apex(d: &crate::world::Pad) -> f32 {
        d.y + d.boost * d.boost / (2.0 * GRAVITY)
    }

    #[test]
    fn the_walls_tower_and_the_counters_stand_eight_avatars_tall() {
        for w in kitchens() {
            let top = w.solids.iter().map(|s| s.height).fold(0.0, f32::max);
            assert!(top >= 35.0, "walls {top}");
            assert!(
                w.solids.iter().any(|s| s.height == COUNTER),
                "a counter top"
            );
            assert!(w.solids.iter().any(|s| s.height == FRIDGE), "a fridge");
        }
    }

    #[test]
    fn some_pad_reaches_the_counter_the_fridge_and_the_wall_cabinets() {
        for w in kitchens() {
            for (top, what) in [
                (COUNTER, "counter"),
                (FRIDGE, "fridge"),
                (UPPER_TOP, "wall cabinets"),
            ] {
                let reaches = w.pads.iter().any(|d| {
                    apex(d) > top + 1.0
                        && w.solids
                            .iter()
                            .any(|s| s.height == top && s.shape.distance(d.center) < d.radius + 1.0)
                });
                assert!(reaches, "no pad onto the {what}");
            }
            let floor_pads = w.pads.iter().filter(|d| d.y == 0.0).count();
            assert!(floor_pads >= 3, "{floor_pads} floor pads");
        }
    }

    #[test]
    fn there_is_room_to_crawl_under_the_counters() {
        for w in kitchens() {
            let mut slot = None;
            'search: for i in -200..200 {
                for j in -200..200 {
                    let p = Vec2::new(i as f32 * 0.1, j as f32 * 0.1);
                    let overhead = w
                        .solids
                        .iter()
                        .any(|s| s.base == KICK && s.shape.distance(p) == 0.0);
                    let clear = w
                        .solids
                        .iter()
                        .filter(|s| blocks(s, 0.0))
                        .all(|s| s.shape.distance(p) >= RADIUS + 0.01);
                    if overhead && clear {
                        slot = Some(p);
                        break 'search;
                    }
                }
            }
            let Some(p) = slot else {
                panic!("no toe-kick slot");
            };
            let body = Body {
                pos: p,
                ..Body::default()
            };
            let after = step(body, &Input::default(), 0.0, &w);
            assert_eq!((after.pos, after.y), (p, 0.0), "an avatar fits at {p:?}");
        }
    }
}
