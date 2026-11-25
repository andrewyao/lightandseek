//! Plane and space vectors. The ground plane is `Vec2 { x, y }`, which the
//! engine maps to world `(x, 0, y)` with Y up.
//!
//! Every float that crosses the wire is one of these types or goes through
//! `finite`, so decode rejects NaN and infinity at the leaf.

use std::ops::{Add, Mul, Neg, Sub};

use serde::{Deserialize, Deserializer, Serialize};

#[derive(Copy, Clone, Debug, Default, PartialEq, Serialize)]
pub struct Vec2 {
    pub x: f32,
    pub y: f32,
}

#[derive(Copy, Clone, Debug, Default, PartialEq, Serialize)]
pub struct Vec3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

fn non_finite<E: serde::de::Error>() -> E {
    E::custom("non-finite float")
}

/// `#[serde(deserialize_with = "finite")]` for a bare `f32` field.
pub(crate) fn finite<'de, D: Deserializer<'de>>(d: D) -> Result<f32, D::Error> {
    let v = f32::deserialize(d)?;
    if v.is_finite() {
        Ok(v)
    } else {
        Err(non_finite())
    }
}

#[derive(Deserialize)]
#[serde(rename = "Vec2")]
struct RawVec2 {
    x: f32,
    y: f32,
}

#[derive(Deserialize)]
#[serde(rename = "Vec3")]
struct RawVec3 {
    x: f32,
    y: f32,
    z: f32,
}

impl<'de> Deserialize<'de> for Vec2 {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let RawVec2 { x, y } = RawVec2::deserialize(d)?;
        if x.is_finite() && y.is_finite() {
            Ok(Vec2 { x, y })
        } else {
            Err(non_finite())
        }
    }
}

impl<'de> Deserialize<'de> for Vec3 {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let RawVec3 { x, y, z } = RawVec3::deserialize(d)?;
        if x.is_finite() && y.is_finite() && z.is_finite() {
            Ok(Vec3 { x, y, z })
        } else {
            Err(non_finite())
        }
    }
}

impl Vec2 {
    pub const ZERO: Vec2 = Vec2 { x: 0.0, y: 0.0 };

    pub const fn new(x: f32, y: f32) -> Vec2 {
        Vec2 { x, y }
    }

    pub fn dot(self, o: Vec2) -> f32 {
        self.x * o.x + self.y * o.y
    }

    pub fn length(self) -> f32 {
        self.dot(self).sqrt()
    }

    pub fn distance(self, o: Vec2) -> f32 {
        (self - o).length()
    }

    /// Zero stays zero.
    pub fn normalize_or_zero(self) -> Vec2 {
        let len = self.length();
        if len > 0.0 {
            self * (1.0 / len)
        } else {
            Vec2::ZERO
        }
    }

    pub fn lerp(self, o: Vec2, t: f32) -> Vec2 {
        self + (o - self) * t
    }

    pub fn clamp(self, min: Vec2, max: Vec2) -> Vec2 {
        Vec2::new(self.x.clamp(min.x, max.x), self.y.clamp(min.y, max.y))
    }
}

impl Add for Vec2 {
    type Output = Vec2;
    fn add(self, o: Vec2) -> Vec2 {
        Vec2::new(self.x + o.x, self.y + o.y)
    }
}

impl Sub for Vec2 {
    type Output = Vec2;
    fn sub(self, o: Vec2) -> Vec2 {
        Vec2::new(self.x - o.x, self.y - o.y)
    }
}

impl Mul<f32> for Vec2 {
    type Output = Vec2;
    fn mul(self, s: f32) -> Vec2 {
        Vec2::new(self.x * s, self.y * s)
    }
}

impl Neg for Vec2 {
    type Output = Vec2;
    fn neg(self) -> Vec2 {
        Vec2::new(-self.x, -self.y)
    }
}

impl Vec3 {
    pub const fn new(x: f32, y: f32, z: f32) -> Vec3 {
        Vec3 { x, y, z }
    }
}

/// Forward on the ground plane for `yaw` radians about +Y. Yaw 0 faces +y
/// (world +Z), yaw π/2 faces +x.
pub fn facing(yaw: f32) -> Vec2 {
    Vec2::new(yaw.sin(), yaw.cos())
}

/// Strafe-right for `yaw`, which is `facing × up` in world space.
pub fn right(yaw: f32) -> Vec2 {
    Vec2::new(-yaw.cos(), yaw.sin())
}

/// The yaw whose `facing` points along `dir`.
pub fn yaw_toward(dir: Vec2) -> f32 {
    dir.x.atan2(dir.y)
}

#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Aabb2 {
    pub min: Vec2,
    pub max: Vec2,
}

impl Aabb2 {
    pub fn centered(center: Vec2, half: Vec2) -> Aabb2 {
        Aabb2 {
            min: center - half,
            max: center + half,
        }
    }

    pub fn center(&self) -> Vec2 {
        (self.min + self.max) * 0.5
    }

    pub fn contains(&self, p: Vec2) -> bool {
        p.x >= self.min.x && p.x <= self.max.x && p.y >= self.min.y && p.y <= self.max.y
    }

    pub fn grow(&self, by: f32) -> Aabb2 {
        Aabb2 {
            min: self.min - Vec2::new(by, by),
            max: self.max + Vec2::new(by, by),
        }
    }

    pub fn overlaps(&self, o: &Aabb2) -> bool {
        self.min.x < o.max.x && o.min.x < self.max.x && self.min.y < o.max.y && o.min.y < self.max.y
    }
}

/// A solid's shape on the ground plane.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Footprint {
    Aabb(Aabb2),
    Circle {
        center: Vec2,
        #[serde(deserialize_with = "finite")]
        radius: f32,
    },
}

impl Footprint {
    /// The point of the footprint closest to `p`. `p` itself when inside.
    pub fn closest(&self, p: Vec2) -> Vec2 {
        match *self {
            Footprint::Aabb(b) => p.clamp(b.min, b.max),
            Footprint::Circle { center, radius } => {
                let d = p - center;
                if d.length() <= radius {
                    p
                } else {
                    center + d.normalize_or_zero() * radius
                }
            }
        }
    }

    /// Distance from `p` to the footprint, 0 inside.
    pub fn distance(&self, p: Vec2) -> f32 {
        p.distance(self.closest(p))
    }

    pub fn bounds(&self) -> Aabb2 {
        match *self {
            Footprint::Aabb(b) => b,
            Footprint::Circle { center, radius } => {
                Aabb2::centered(center, Vec2::new(radius, radius))
            }
        }
    }

    /// Whether the segment `a..b` passes through the footprint's interior.
    pub fn hits_segment(&self, a: Vec2, b: Vec2) -> bool {
        match *self {
            Footprint::Aabb(bx) => segment_hits_aabb(a, b, &bx),
            Footprint::Circle { center, radius } => {
                let ab = b - a;
                let len2 = ab.dot(ab);
                let t = if len2 > 0.0 {
                    ((center - a).dot(ab) / len2).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                (a + ab * t).distance(center) < radius
            }
        }
    }
}

/// Slab test.
fn segment_hits_aabb(a: Vec2, b: Vec2, bx: &Aabb2) -> bool {
    let d = b - a;
    let mut t0: f32 = 0.0;
    let mut t1: f32 = 1.0;
    for (p, dir, lo, hi) in [
        (a.x, d.x, bx.min.x, bx.max.x),
        (a.y, d.y, bx.min.y, bx.max.y),
    ] {
        if dir == 0.0 {
            if p <= lo || p >= hi {
                return false;
            }
        } else {
            let (mut n, mut f) = ((lo - p) / dir, (hi - p) / dir);
            if n > f {
                std::mem::swap(&mut n, &mut f);
            }
            t0 = t0.max(n);
            t1 = t1.min(f);
            if t0 >= t1 {
                return false;
            }
        }
    }
    true
}
