//! A 0.5 u walkable grid with connected components and A*. Server-only: world
//! generation checks connectivity with it and bots path over it.

use std::cmp::Reverse;
use std::collections::{BinaryHeap, VecDeque};

use crate::geom::Vec2;
use crate::physics::{STEP_UP, blocks};
use crate::rng::Rng;
use crate::world::World;

pub(crate) const CELL: f32 = 0.5;
/// Wider than the avatar radius, so a path between cell centres never grazes
/// a corner.
const CLEAR: f32 = 0.4;
const BLOCKED: u32 = u32::MAX;
const STRAIGHT: u32 = 10;
const DIAGONAL: u32 = 14;
/// Spawn spacing counts distance to the seeker at this share, so it starts
/// 8 u from a hider wherever hiders start 6 u apart.
const SEEKER_SHARE: f32 = 0.75;

#[derive(Debug)]
pub(crate) struct NavGrid {
    min: Vec2,
    n: usize,
    /// Component label per cell, `BLOCKED` when not walkable.
    label: Vec<u32>,
    largest: u32,
    walkable: usize,
    largest_size: usize,
}

impl NavGrid {
    pub fn new(world: &World) -> NavGrid {
        let min = world.bounds.min;
        let n = ((world.bounds.max.x - min.x) / CELL).round() as usize;
        let inner = world.bounds.grow(-CLEAR);
        let mut walk: Vec<bool> = (0..n * n)
            .map(|i| inner.contains(center_of(min, n, i)))
            .collect();
        for s in world.solids.iter().filter(|s| blocks(s, 0.0)) {
            let b = s.shape.bounds().grow(CLEAR);
            let lo = |v: f32, o: f32| (((v - o) / CELL).floor().max(0.0) as usize).min(n);
            let (x0, x1) = (lo(b.min.x, min.x), lo(b.max.x, min.x) + 1);
            let (y0, y1) = (lo(b.min.y, min.y), lo(b.max.y, min.y) + 1);
            for y in y0..y1.min(n) {
                for x in x0..x1.min(n) {
                    let i = y * n + x;
                    if walk[i] && s.shape.distance(center_of(min, n, i)) < CLEAR {
                        walk[i] = false;
                    }
                }
            }
        }
        // Bots keep off ground pads, and nobody spawns on one.
        for d in world.pads.iter().filter(|d| d.y <= STEP_UP) {
            for (i, w) in walk.iter_mut().enumerate() {
                if *w && center_of(min, n, i).distance(d.center) < d.radius + CLEAR {
                    *w = false;
                }
            }
        }
        let mut grid = NavGrid {
            min,
            n,
            label: walk.iter().map(|w| if *w { 0 } else { BLOCKED }).collect(),
            largest: BLOCKED,
            walkable: walk.iter().filter(|w| **w).count(),
            largest_size: 0,
        };
        grid.label_components();
        grid
    }

    fn label_components(&mut self) {
        let unvisited = 0;
        let mut next = 1;
        let mut queue = VecDeque::new();
        for start in 0..self.label.len() {
            if self.label[start] != unvisited {
                continue;
            }
            self.label[start] = next;
            queue.push_back(start);
            let mut size = 0;
            while let Some(i) = queue.pop_front() {
                size += 1;
                let next_cells: Vec<usize> = self.neighbors(i).map(|(j, _)| j).collect();
                for j in next_cells {
                    if self.label[j] == unvisited {
                        self.label[j] = next;
                        queue.push_back(j);
                    }
                }
            }
            if size > self.largest_size {
                self.largest_size = size;
                self.largest = next;
            }
            next += 1;
        }
    }

    /// The share of walkable cells in the largest component.
    pub fn connectivity(&self) -> f32 {
        if self.walkable == 0 {
            0.0
        } else {
            self.largest_size as f32 / self.walkable as f32
        }
    }

    pub fn walkable(&self, i: usize) -> bool {
        self.label[i] != BLOCKED
    }

    pub fn center(&self, i: usize) -> Vec2 {
        center_of(self.min, self.n, i)
    }

    #[cfg(test)]
    pub fn cell(&self, p: Vec2) -> Option<usize> {
        let x = ((p.x - self.min.x) / CELL).floor();
        let y = ((p.y - self.min.y) / CELL).floor();
        let n = self.n as f32;
        (x >= 0.0 && y >= 0.0 && x < n && y < n).then(|| y as usize * self.n + x as usize)
    }

    #[cfg(test)]
    pub fn in_largest(&self, p: Vec2) -> bool {
        self.cell(p).is_some_and(|i| self.label[i] == self.largest)
    }

    /// Cells of the largest component, in index order.
    pub fn reachable(&self) -> impl Iterator<Item = usize> + '_ {
        (0..self.label.len()).filter(|i| self.label[*i] == self.largest)
    }

    pub fn random_reachable(&self, rng: &mut Rng) -> Option<usize> {
        let cells: Vec<usize> = self.reachable().collect();
        (!cells.is_empty()).then(|| cells[rng.below(cells.len())])
    }

    /// Blocked or out-of-grid cells among the `r`-ring square around `i`.
    pub fn cover(&self, i: usize, r: i32) -> u32 {
        let (x, y) = ((i % self.n) as i32, (i / self.n) as i32);
        let mut count = 0;
        for dy in -r..=r {
            for dx in -r..=r {
                let (nx, ny) = (x + dx, y + dy);
                let inside = nx >= 0 && ny >= 0 && (nx as usize) < self.n && (ny as usize) < self.n;
                if !inside || !self.walkable(ny as usize * self.n + nx as usize) {
                    count += 1;
                }
            }
        }
        count
    }

    /// 8-connected, without cutting a blocked corner.
    fn neighbors(&self, i: usize) -> impl Iterator<Item = (usize, u32)> + '_ {
        let n = self.n as i32;
        let (x, y) = ((i % self.n) as i32, (i / self.n) as i32);
        let open = move |x: i32, y: i32| {
            x >= 0 && y >= 0 && x < n && y < n && self.label[(y * n + x) as usize] != BLOCKED
        };
        [
            (1, 0),
            (-1, 0),
            (0, 1),
            (0, -1),
            (1, 1),
            (1, -1),
            (-1, 1),
            (-1, -1),
        ]
        .into_iter()
        .filter(move |&(dx, dy)| {
            open(x + dx, y + dy) && (dx == 0 || dy == 0 || (open(x + dx, y) && open(x, y + dy)))
        })
        .map(move |(dx, dy)| {
            let cost = if dx == 0 || dy == 0 {
                STRAIGHT
            } else {
                DIAGONAL
            };
            (((y + dy) * n + (x + dx)) as usize, cost)
        })
    }

    /// The walkable cell nearest `p`, searching a few cells out. Lets an
    /// avatar pressed against a solid still path.
    pub fn nearest_walkable(&self, p: Vec2) -> Option<usize> {
        let x = ((p.x - self.min.x) / CELL).floor() as i32;
        let y = ((p.y - self.min.y) / CELL).floor() as i32;
        let n = self.n as i32;
        let mut best: Option<(f32, usize)> = None;
        for dy in -3..=3 {
            for dx in -3..=3 {
                let (cx, cy) = (x + dx, y + dy);
                if cx < 0 || cy < 0 || cx >= n || cy >= n {
                    continue;
                }
                let i = (cy * n + cx) as usize;
                let d = self.center(i).distance(p);
                if self.walkable(i) && best.is_none_or(|(bd, _)| d < bd) {
                    best = Some((d, i));
                }
            }
        }
        best.map(|(_, i)| i)
    }

    /// A* from the cell nearest `from` to `goal`. Returns cell centres after
    /// the start, ending at `goal`.
    pub fn path(&self, from: Vec2, goal: usize) -> Option<Vec<Vec2>> {
        let start = self.nearest_walkable(from)?;
        if self.label[start] != self.label[goal] {
            return None;
        }
        let n = self.n as i64;
        let h = |i: usize| {
            let (dx, dy) = (
                (i as i64 % n - goal as i64 % n).unsigned_abs() as u32,
                (i as i64 / n - goal as i64 / n).unsigned_abs() as u32,
            );
            let (lo, hi) = (dx.min(dy), dx.max(dy));
            DIAGONAL * lo + STRAIGHT * (hi - lo)
        };
        let mut cost = vec![u32::MAX; self.label.len()];
        let mut prev = vec![usize::MAX; self.label.len()];
        let mut open = BinaryHeap::new();
        cost[start] = 0;
        open.push(Reverse((h(start), start)));
        while let Some(Reverse((_, i))) = open.pop() {
            if i == goal {
                let mut out = Vec::new();
                let mut at = goal;
                while at != start {
                    out.push(self.center(at));
                    at = prev[at];
                }
                out.reverse();
                return Some(out);
            }
            for (j, step) in self.neighbors(i) {
                let c = cost[i] + step;
                if c < cost[j] {
                    cost[j] = c;
                    prev[j] = i;
                    open.push(Reverse((c + h(j), j)));
                }
            }
        }
        None
    }

    /// Up to `k` cell centres in the largest component, by farthest-point
    /// sampling from a random first cell. Each point is the reachable cell
    /// farthest from all before it, so every prefix is as spread out as the
    /// map allows. The first is the seeker's, and distances to it count for
    /// `SEEKER_SHARE`, so it keeps a wider margin than any other pair.
    /// Cells with open ground around them come first, since sampling alone
    /// would start everyone pressed into a corner.
    pub fn spawns(&self, rng: &mut Rng, k: usize) -> Vec<Vec2> {
        let open: Vec<usize> = self
            .reachable()
            .filter(|&i| self.cover(i, 1) == 0)
            .collect();
        let cells: Vec<Vec2> = if open.len() >= k {
            open.into_iter().map(|i| self.center(i)).collect()
        } else {
            self.reachable().map(|i| self.center(i)).collect()
        };
        if cells.is_empty() {
            return Vec::new();
        }
        let mut gap = vec![f32::INFINITY; cells.len()];
        let mut next = rng.below(cells.len());
        let mut out = Vec::with_capacity(k);
        while out.len() < k.min(cells.len()) {
            let p = cells[next];
            let share = if out.is_empty() { SEEKER_SHARE } else { 1.0 };
            out.push(p);
            for (g, c) in gap.iter_mut().zip(&cells) {
                *g = g.min(c.distance(p) * share);
            }
            next = (0..cells.len())
                .max_by(|&a, &b| gap[a].total_cmp(&gap[b]))
                .expect("cells is not empty");
        }
        out
    }
}

/// Up to `k` of `cells`, each the one farthest from all chosen before it and
/// from `taken`. Starts from a random cell when `taken` is empty.
pub(crate) fn spread(rng: &mut Rng, cells: &[Vec2], k: usize, taken: &[Vec2]) -> Vec<Vec2> {
    if cells.is_empty() {
        return Vec::new();
    }
    let mut gap: Vec<f32> = cells
        .iter()
        .map(|c| {
            taken
                .iter()
                .map(|t| c.distance(*t))
                .fold(f32::INFINITY, f32::min)
        })
        .collect();
    let farthest = |gap: &[f32]| {
        (0..gap.len())
            .max_by(|&a, &b| gap[a].total_cmp(&gap[b]))
            .expect("cells is not empty")
    };
    let mut next = if taken.is_empty() {
        rng.below(cells.len())
    } else {
        farthest(&gap)
    };
    let mut out = Vec::with_capacity(k);
    while out.len() < k.min(cells.len()) {
        let p = cells[next];
        out.push(p);
        for (g, c) in gap.iter_mut().zip(cells) {
            *g = g.min(c.distance(p));
        }
        next = farthest(&gap);
    }
    out
}

fn center_of(min: Vec2, n: usize, i: usize) -> Vec2 {
    Vec2::new(
        min.x + ((i % n) as f32 + 0.5) * CELL,
        min.y + ((i / n) as f32 + 0.5) * CELL,
    )
}
