//! The scene: what blocks movement and sight, and what the engine draws.
//!
//! The server generates a `World` once per round and sends it whole in
//! `RoundStart`. Clients never run the generators, so native and wasm float
//! or RNG drift cannot desync the scene.

mod city;
mod kitchen;
pub(crate) mod nav;
mod park;

use serde::{Deserialize, Serialize};

use crate::geom::{Aabb2, Footprint, Vec2, Vec3, clip, finite};
use crate::model::SceneKind;
use crate::physics::launch_speed;
use crate::rng::Rng;
use nav::NavGrid;

/// Half the side of the square play area.
pub const HALF_EXTENT: f32 = 20.0;
/// Every world has this many spawn points, enough for a full room.
pub const SPAWNS: usize = 16;
/// The share of walkable cells that must be one connected region.
pub const MIN_CONNECTED: f32 = 0.85;
const MAX_ATTEMPTS: u64 = 32;
pub const PAD_RADIUS: f32 = 0.7;
/// Between a pad's rim and the solid it serves.
const PAD_GAP: f32 = 0.4;
/// How far above the top of its solid a pad's throw peaks.
const PAD_CLEARANCE: f32 = 3.0;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Shape {
    Box,
    Cylinder,
}

/// What a surface is made of. The renderer draws each as a pattern over
/// the prop's colour.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Material {
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
}

/// One drawable. `center` and `size` are world space with Y up. A cylinder's
/// `size.x` is its diameter. Colours are sRGB.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Prop {
    pub shape: Shape,
    pub center: Vec3,
    pub size: Vec3,
    pub color: [u8; 3],
    pub material: Material,
}

/// A colour on a material: how the generators name a finish.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
struct Paint {
    color: [u8; 3],
    material: Material,
}

const fn paint(color: [u8; 3], material: Material) -> Paint {
    Paint { color, material }
}

/// Its footprint extruded from `base` to `height`. Blocks movement, sight,
/// and the camera inside that span; its top can be stood on.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Solid {
    pub shape: Footprint,
    #[serde(deserialize_with = "finite")]
    pub height: f32,
    #[serde(deserialize_with = "finite")]
    pub base: f32,
}

impl Solid {
    /// Standing on the ground, from y = 0 to `height`.
    pub fn ground(shape: Footprint, height: f32) -> Solid {
        Solid {
            shape,
            height,
            base: 0.0,
        }
    }
}

/// A glowing disc on a surface at height `y`. An avatar standing on it is
/// thrown upward at `boost` u/s.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Pad {
    pub center: Vec2,
    #[serde(deserialize_with = "finite")]
    pub radius: f32,
    #[serde(deserialize_with = "finite")]
    pub y: f32,
    #[serde(deserialize_with = "finite")]
    pub boost: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct World {
    pub kind: SceneKind,
    /// Avatars are kept inside this box.
    pub bounds: Aabb2,
    pub solids: Vec<Solid>,
    pub props: Vec<Prop>,
    /// Walkable, mutually reachable, and spread apart. Index 0 is the seeker's.
    pub spawns: Vec<Vec2>,
    pub pads: Vec<Pad>,
}

impl World {
    /// An empty field of the given size, for tests and tools.
    pub fn open(kind: SceneKind) -> World {
        World {
            kind,
            bounds: square(),
            solids: Vec::new(),
            props: Vec::new(),
            spawns: Vec::new(),
            pads: Vec::new(),
        }
    }

    /// Deterministic in `(kind, seed)`. Retries with `seed + 1` until the
    /// walkable area is at least `MIN_CONNECTED` connected.
    pub fn generate(kind: SceneKind, seed: u64) -> World {
        generate_counted(kind, seed).0
    }

    /// Distance from `from` along the segment to `to` where it first enters a
    /// solid, each solid being its footprint extruded from its base to its
    /// height. `Some(0.0)` when `from` starts inside one. Grazing a surface
    /// is not a hit. The follow camera pulls in to this.
    pub fn ray_hit(&self, from: Vec3, to: Vec3) -> Option<f32> {
        let d = Vec3::new(to.x - from.x, to.y - from.y, to.z - from.z);
        let (p, flat) = (Vec2::new(from.x, from.z), Vec2::new(d.x, d.z));
        let nearest = self
            .solids
            .iter()
            .filter_map(|s| {
                let span = s.shape.line_span(p, flat)?;
                let span = clip(span, from.y, d.y, s.base, s.height)?;
                let span = (span.0.max(0.0), span.1.min(1.0));
                (span.0 < span.1).then_some(span.0)
            })
            .min_by(f32::total_cmp)?;
        Some(nearest * (d.x * d.x + d.y * d.y + d.z * d.z).sqrt())
    }

    /// Whether a level ray at height `y` from `a` reaches `b`. Solids no
    /// taller than `y` do not block.
    pub fn ray_clear(&self, a: Vec2, b: Vec2, y: f32) -> bool {
        self.ray_hit(Vec3::new(a.x, y, a.y), Vec3::new(b.x, y, b.y))
            .is_none()
    }
}

fn square() -> Aabb2 {
    Aabb2::centered(Vec2::ZERO, Vec2::new(HALF_EXTENT, HALF_EXTENT))
}

/// The world and how many seeds it tried.
pub(crate) fn generate_counted(kind: SceneKind, seed: u64) -> (World, u64) {
    let mut best: Option<(f32, World)> = None;
    for attempt in 0..MAX_ATTEMPTS {
        let mut rng = Rng::new(seed.wrapping_add(attempt));
        let mut b = Builder::new(&mut rng);
        match kind {
            SceneKind::Park => park::build(&mut b),
            SceneKind::City => city::build(&mut b),
            SceneKind::Kitchen => kitchen::build(&mut b),
        }
        let (solids, props, pads) = (b.solids, b.props, b.pads);
        let mut world = World {
            kind,
            bounds: square(),
            solids,
            props,
            spawns: Vec::new(),
            pads,
        };
        let grid = NavGrid::new(&world);
        let connected = grid.connectivity();
        world.spawns = grid.spawns(&mut rng, SPAWNS);
        if connected >= MIN_CONNECTED && world.spawns.len() == SPAWNS {
            return (world, attempt + 1);
        }
        if best.as_ref().is_none_or(|(c, _)| connected > *c) {
            best = Some((connected, world));
        }
    }
    let (_, world) = best.expect("MAX_ATTEMPTS > 0");
    (world, MAX_ATTEMPTS)
}

/// Accumulates matching props and solids, and keeps placements apart.
struct Builder<'a> {
    rng: &'a mut Rng,
    solids: Vec<Solid>,
    props: Vec<Prop>,
    pads: Vec<Pad>,
    /// Regions nothing may be placed in, such as doorways.
    keep_out: Vec<Aabb2>,
    /// `solids[..structure]` are walls. Furniture may sit flush against them.
    structure: usize,
}

impl<'a> Builder<'a> {
    fn new(rng: &'a mut Rng) -> Builder<'a> {
        Builder {
            rng,
            solids: Vec::new(),
            props: Vec::new(),
            pads: Vec::new(),
            keep_out: Vec::new(),
            structure: 0,
        }
    }

    /// Marks every solid so far as a wall.
    fn structural(&mut self) {
        self.structure = self.solids.len();
    }

    /// A box that blocks, drawn from the ground to `height`.
    fn block(&mut self, b: Aabb2, height: f32, paint: Paint) {
        let c = b.center();
        self.solids.push(Solid::ground(Footprint::Aabb(b), height));
        self.props.push(Prop {
            shape: Shape::Box,
            center: Vec3::new(c.x, height / 2.0, c.y),
            size: Vec3::new(b.max.x - b.min.x, height, b.max.y - b.min.y),
            color: paint.color,
            material: paint.material,
        });
    }

    /// A cylinder that blocks, drawn from the ground to `height`.
    fn pillar(&mut self, center: Vec2, radius: f32, height: f32, paint: Paint) {
        self.solids
            .push(Solid::ground(Footprint::Circle { center, radius }, height));
        self.props.push(Prop {
            shape: Shape::Cylinder,
            center: Vec3::new(center.x, height / 2.0, center.y),
            size: Vec3::new(radius * 2.0, height, radius * 2.0),
            color: paint.color,
            material: paint.material,
        });
    }

    /// A box that blocks from `y0` to `y1`, drawn there: a table top, a
    /// shelf, a tree canopy. Avatars walk under it if `y0` clears their heads.
    fn platform(&mut self, shape: Shape, b: Aabb2, y0: f32, y1: f32, paint: Paint) {
        let c = b.center();
        let footprint = match shape {
            Shape::Box => Footprint::Aabb(b),
            Shape::Cylinder => Footprint::Circle {
                center: c,
                radius: (b.max.x - b.min.x) / 2.0,
            },
        };
        self.solids.push(Solid {
            shape: footprint,
            height: y1,
            base: y0,
        });
        self.deco(
            shape,
            Vec3::new(c.x, (y0 + y1) / 2.0, c.y),
            Vec3::new(b.max.x - b.min.x, y1 - y0, b.max.y - b.min.y),
            paint,
        );
    }

    /// A launch pad on the surface at `y` that throws feet `apex` above it.
    /// Keeps other things off it.
    fn pad(&mut self, center: Vec2, y: f32, apex: f32) {
        self.pads.push(Pad {
            center,
            radius: PAD_RADIUS,
            y,
            boost: launch_speed(apex),
        });
        self.keep_out
            .push(Aabb2::centered(center, Vec2::new(PAD_RADIUS, PAD_RADIUS)));
    }

    /// A ground pad beside each of up to `n` of the tallest solids in
    /// `candidates`, each throwing high enough to land on top of its solid.
    fn pads_by(&mut self, n: usize, candidates: std::ops::Range<usize>) {
        let mut tall: Vec<usize> = candidates.filter(|&i| i >= self.structure).collect();
        tall.sort_by(|&a, &b| self.solids[b].height.total_cmp(&self.solids[a].height));
        let mut placed = 0;
        for i in tall {
            if placed == n {
                break;
            }
            let s = self.solids[i];
            let half = Vec2::new(PAD_RADIUS, PAD_RADIUS);
            let b = s.shape.bounds();
            let c = b.center();
            let off = PAD_RADIUS + PAD_GAP;
            let sides = [
                Vec2::new(c.x, b.min.y - off),
                Vec2::new(c.x, b.max.y + off),
                Vec2::new(b.min.x - off, c.y),
                Vec2::new(b.max.x + off, c.y),
            ];
            let start = self.rng.below(sides.len());
            let spot = (0..sides.len())
                .map(|k| sides[(start + k) % sides.len()])
                .find(|&p| self.fits(Aabb2::centered(p, half), 0.3));
            if let Some(p) = spot {
                self.pad(p, 0.0, s.height + PAD_CLEARANCE);
                placed += 1;
            }
        }
    }

    /// Blocks without a prop of its own. The caller draws it in parts.
    fn solid(&mut self, shape: Footprint, height: f32) {
        self.solids.push(Solid::ground(shape, height));
    }

    /// Drawn only. Canopies, roofs, shelves above head height.
    fn deco(&mut self, shape: Shape, center: Vec3, size: Vec3, paint: Paint) {
        self.props.push(Prop {
            shape,
            center,
            size,
            color: paint.color,
            material: paint.material,
        });
    }

    /// Whether `b` lies inside the play area, clear of walls and keep-out
    /// regions, and at least `gap` from every other solid.
    fn fits(&self, b: Aabb2, gap: f32) -> bool {
        let area = square();
        let grown = b.grow(gap);
        let (walls, rest) = self.solids.split_at(self.structure);
        area.contains(b.min)
            && area.contains(b.max)
            && !self.keep_out.iter().any(|k| k.overlaps(&b))
            && !walls.iter().any(|s| s.shape.bounds().overlaps(&b))
            && !rest.iter().any(|s| s.shape.bounds().overlaps(&grown))
    }

    /// A random center in `region` where a box of `half` extents fits.
    fn place(&mut self, region: Aabb2, half: Vec2, gap: f32) -> Option<Vec2> {
        for _ in 0..40 {
            let c = Vec2::new(
                self.rng.range(region.min.x + half.x, region.max.x - half.x),
                self.rng.range(region.min.y + half.y, region.max.y - half.y),
            );
            if self.fits(Aabb2::centered(c, half), gap) {
                return Some(c);
            }
        }
        None
    }

    /// Half extents of a `long` by `thick` box, turned along x or y at random.
    fn oriented(&mut self, long: f32, thick: f32) -> Vec2 {
        if self.rng.chance(0.5) {
            Vec2::new(long / 2.0, thick / 2.0)
        } else {
            Vec2::new(thick / 2.0, long / 2.0)
        }
    }

    fn count(&mut self, lo: usize, hi: usize) -> usize {
        lo + self.rng.below(hi - lo + 1)
    }

    fn pick<T: Copy>(&mut self, xs: &[T]) -> T {
        xs[self.rng.below(xs.len())]
    }

    /// Walls just outside the play area, so the camera and sight stop there.
    fn perimeter(&mut self, height: f32, paint: Paint) {
        let (h, t) = (HALF_EXTENT, 0.5);
        for b in [
            Aabb2 {
                min: Vec2::new(-h - t, -h - t),
                max: Vec2::new(h + t, -h),
            },
            Aabb2 {
                min: Vec2::new(-h - t, h),
                max: Vec2::new(h + t, h + t),
            },
            Aabb2 {
                min: Vec2::new(-h - t, -h),
                max: Vec2::new(-h, h),
            },
            Aabb2 {
                min: Vec2::new(h, -h),
                max: Vec2::new(h + t, h),
            },
        ] {
            self.block(b, height, paint);
        }
        self.structural();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::physics::{RADIUS, blocks};

    #[test]
    fn every_kind_is_deterministic_connected_and_spawnable_across_200_seeds() {
        for kind in SceneKind::ALL {
            let mut worst_attempts = 0;
            for seed in 0..200 {
                let (world, attempts) = generate_counted(kind, seed);
                worst_attempts = worst_attempts.max(attempts);
                assert!(
                    attempts < MAX_ATTEMPTS,
                    "{kind:?} seed {seed} never connected"
                );
                assert_eq!(world, World::generate(kind, seed), "{kind:?} seed {seed}");

                let grid = NavGrid::new(&world);
                assert!(
                    grid.connectivity() >= MIN_CONNECTED,
                    "{kind:?} seed {seed}: {}",
                    grid.connectivity()
                );
                assert_eq!(world.spawns.len(), SPAWNS);
                for (i, s) in world.spawns.iter().enumerate() {
                    assert!(grid.in_largest(*s), "{kind:?} {seed} spawn {i} reachable");
                    assert!(
                        world
                            .solids
                            .iter()
                            .filter(|x| blocks(x, 0.0))
                            .all(|x| x.shape.distance(*s) > RADIUS),
                        "{kind:?} {seed} spawn {i} clear"
                    );
                }
            }
            assert!(
                worst_attempts <= 4,
                "{kind:?} needed {worst_attempts} attempts"
            );
        }
    }

    #[test]
    fn spawns_are_spread_and_the_seekers_is_far_from_every_other() {
        for kind in SceneKind::ALL {
            for seed in 0..100 {
                let s = World::generate(kind, seed).spawns;
                for i in 0..s.len() {
                    for j in i + 1..s.len() {
                        let d = s[i].distance(s[j]);
                        let want = if i == 0 { 8.0 } else { 6.0 };
                        assert!(d >= want, "{kind:?} seed {seed}: spawns {i}-{j} {d}");
                    }
                }
            }
        }
    }

    #[test]
    fn every_world_has_pads_that_reach_the_top_of_what_they_stand_by() {
        for kind in SceneKind::ALL {
            for seed in 0..60 {
                let w = World::generate(kind, seed);
                assert!(w.pads.len() >= 3, "{kind:?} {seed}: {} pads", w.pads.len());
                for d in &w.pads {
                    assert!(
                        w.solids
                            .iter()
                            .filter(|s| blocks(s, d.y))
                            .all(|s| s.shape.distance(d.center) >= d.radius),
                        "{kind:?} {seed}: pad at {:?} is clear",
                        d.center
                    );
                    let apex = d.y + d.boost * d.boost / (2.0 * crate::physics::GRAVITY);
                    let beside = w
                        .solids
                        .iter()
                        .filter(|s| s.shape.distance(d.center) < d.radius + 1.0)
                        .map(|s| s.height)
                        .fold(0.0, f32::max);
                    assert!(beside > d.y, "{kind:?} {seed}: pad stands by something");
                    assert!(
                        apex > beside + 1.0,
                        "{kind:?} {seed}: apex {apex} over {beside}"
                    );
                }
                for s in &w.spawns {
                    assert!(
                        w.pads.iter().all(|d| d.center.distance(*s) > d.radius),
                        "{kind:?} {seed}: spawn {s:?} on a pad"
                    );
                }
            }
        }
    }

    #[test]
    fn kinds_and_seeds_produce_different_scenes() {
        let park = World::generate(SceneKind::Park, 1);
        assert_ne!(park, World::generate(SceneKind::Park, 2));
        assert_ne!(park.solids, World::generate(SceneKind::City, 1).solids);
        for kind in SceneKind::ALL {
            let w = World::generate(kind, 3);
            assert!(w.solids.len() > 10, "{kind:?} has obstacles");
            assert!(w.props.len() >= w.solids.len(), "every solid is drawn");
        }
    }
}
