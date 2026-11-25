//! The unit meshes behind each `Shape`. Round shapes are lathed: a profile of
//! (radius, y) points with their normals, swept around the Y axis.

use std::f32::consts::{FRAC_PI_2, PI, TAU};

use crate::draw::Shape;

/// Position, then normal.
pub type Vertex = [f32; 6];

#[derive(Debug)]
pub struct MeshData {
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u16>,
}

const SEGMENTS: usize = 32;
const RINGS: usize = 16;

pub fn build(shape: Shape) -> MeshData {
    match shape {
        Shape::Box => cube(),
        Shape::Cylinder => lathe(&[
            [0.0, 0.5, 0.0, 1.0],
            [0.5, 0.5, 0.0, 1.0],
            [0.5, 0.5, 1.0, 0.0],
            [0.5, -0.5, 1.0, 0.0],
            [0.5, -0.5, 0.0, -1.0],
            [0.0, -0.5, 0.0, -1.0],
        ]),
        Shape::Sphere => lathe(&arc(0.0, PI, RINGS, 0.0)),
        Shape::Capsule => {
            let mut profile = arc(0.0, FRAC_PI_2, RINGS / 2, 0.5);
            profile.extend(arc(FRAC_PI_2, PI, RINGS / 2, -0.5));
            lathe(&profile)
        }
    }
}

/// Points on a radius 0.5 arc from polar angle `from` to `to`, lifted by
/// `dy`. Polar angle 0 is straight up.
fn arc(from: f32, to: f32, steps: usize, dy: f32) -> Vec<[f32; 4]> {
    (0..=steps)
        .map(|i| {
            let t = from + (to - from) * i as f32 / steps as f32;
            let (s, c) = t.sin_cos();
            [0.5 * s, 0.5 * c + dy, s, c]
        })
        .collect()
}

/// Sweeps `profile` (radius, y, normal radial, normal y), ordered top to
/// bottom, around Y. Repeating a point with a new normal makes a crease.
fn lathe(profile: &[[f32; 4]]) -> MeshData {
    let mut vertices = Vec::with_capacity(profile.len() * (SEGMENTS + 1));
    for &[r, y, nr, ny] in profile {
        for j in 0..=SEGMENTS {
            let (s, c) = (j as f32 / SEGMENTS as f32 * TAU).sin_cos();
            vertices.push([r * s, y, r * c, nr * s, ny, nr * c]);
        }
    }
    let row = SEGMENTS as u16 + 1;
    let mut indices = Vec::new();
    for k in 0..profile.len() as u16 - 1 {
        for j in 0..SEGMENTS as u16 {
            let (a, b) = (k * row + j, k * row + j + 1);
            let (c, d) = (a + row, b + row);
            indices.extend([a, c, d, a, d, b]);
        }
    }
    MeshData { vertices, indices }
}

fn cube() -> MeshData {
    let mut vertices = Vec::with_capacity(24);
    let mut indices = Vec::with_capacity(36);
    for axis in 0..3 {
        for sign in [-1.0f32, 1.0] {
            let mut n = [0.0; 3];
            n[axis] = sign;
            // Two axes spanning the face, ordered so u x v points along n.
            let (u, v) = ((axis + 1) % 3, (axis + 2) % 3);
            let base = vertices.len() as u16;
            for (du, dv) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
                let mut p = [0.0; 3];
                p[axis] = 0.5 * sign;
                p[u] = 0.5 * du * sign;
                p[v] = 0.5 * dv;
                vertices.push([p[0], p[1], p[2], n[0], n[1], n[2]]);
            }
            indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
        }
    }
    MeshData { vertices, indices }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Vec3;

    fn pos(v: &Vertex) -> Vec3 {
        Vec3::from_slice(&v[..3])
    }

    /// Back-face culling depends on it, and so does lighting.
    #[test]
    fn every_triangle_winds_counter_clockwise_seen_from_outside() {
        for shape in Shape::ALL {
            let m = build(shape);
            let mut real = 0;
            for t in m.indices.chunks(3) {
                let [a, b, c] = [0, 1, 2].map(|i| pos(&m.vertices[t[i] as usize]));
                let face = (b - a).cross(c - a);
                // Lathe poles and creases leave slivers with no area.
                if face.length() < 1e-6 {
                    continue;
                }
                real += 1;
                let n: Vec3 = t
                    .iter()
                    .map(|&i| Vec3::from_slice(&m.vertices[i as usize][3..]))
                    .sum();
                assert!(face.dot(n) > 0.0, "{shape:?} triangle {t:?}");
            }
            assert!(real >= 12, "{shape:?}");
        }
    }

    #[test]
    fn unit_shapes_have_their_documented_extents() {
        for (shape, half) in [
            (Shape::Box, Vec3::splat(0.5)),
            (Shape::Cylinder, Vec3::splat(0.5)),
            (Shape::Sphere, Vec3::splat(0.5)),
            (Shape::Capsule, Vec3::new(0.5, 1.0, 0.5)),
        ] {
            let m = build(shape);
            let max = m.vertices.iter().map(pos).fold(Vec3::MIN, Vec3::max);
            let min = m.vertices.iter().map(pos).fold(Vec3::MAX, Vec3::min);
            assert!(max.abs_diff_eq(half, 1e-5), "{shape:?} max {max}");
            assert!(min.abs_diff_eq(-half, 1e-5), "{shape:?} min {min}");
            for v in &m.vertices {
                assert!((Vec3::from_slice(&v[3..]).length() - 1.0).abs() < 1e-5);
            }
        }
    }
}
