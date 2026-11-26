// WebGL2-safe: one uniform block, per-instance data in vertex attributes,
// no storage buffers. Colours arrive linear.

struct Globals {
    view_proj: mat4x4<f32>,
    // World to the shadow map's clip space.
    sun_view_proj: mat4x4<f32>,
    eye: vec4<f32>,
    // Unit vector toward the sun.
    sun: vec4<f32>,
    sky: vec4<f32>,
    ground: vec4<f32>,
    // Fog start, fog end, 1 when the target needs sRGB encoding here.
    fog: vec4<f32>,
};

@group(0) @binding(0) var<uniform> g: Globals;
@group(1) @binding(0) var shadow_map: texture_depth_2d;
@group(1) @binding(1) var shadow_sampler: sampler_comparison;

const SUN_COLOR: vec3<f32> = vec3<f32>(1.0, 0.95, 0.86);
const GROUND_HALF: f32 = 400.0;
const WEDGE_LIFT: f32 = 0.02;
// One shadow texel: SHADOW_HALF and SHADOW_RES in render.rs make the map
// 80 u across in 2048 texels.
const SHADOW_TEXEL: f32 = 80.0 / 2048.0;

// `local` is the surface point in the instance's own frame, scaled to world
// units, so patterns stick to a moving shape and keep their physical size.
// `r0`..`r2` turn that frame to world space.
struct Lit {
    @builtin(position) clip: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) color: vec3<f32>,
    @location(2) local: vec3<f32>,
    @location(3) local_n: vec3<f32>,
    @location(4) r0: vec3<f32>,
    @location(5) r1: vec3<f32>,
    @location(6) r2: vec3<f32>,
    @location(7) @interpolate(flat) material: u32,
};

@vertex
fn vs_solid(
    @location(0) pos: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) m0: vec4<f32>,
    @location(3) m1: vec4<f32>,
    @location(4) m2: vec4<f32>,
    @location(5) m3: vec4<f32>,
    @location(6) color: vec4<f32>,
) -> Lit {
    let m = mat4x4<f32>(m0, m1, m2, m3);
    let world = (m * vec4<f32>(pos, 1.0)).xyz;
    // The model is rotation times scale, so each column's length is the
    // scale along that axis.
    let s = vec3<f32>(length(m0.xyz), length(m1.xyz), length(m2.xyz));
    var out: Lit;
    out.clip = g.view_proj * vec4<f32>(world, 1.0);
    out.world = world;
    out.color = color.rgb;
    out.local = pos * s;
    // Scaling a shape by s scales its normals by 1 / s.
    out.local_n = normal / s;
    out.r0 = m0.xyz / s.x;
    out.r1 = m1.xyz / s.y;
    out.r2 = m2.xyz / s.z;
    out.material = u32(color.a + 0.5);
    return out;
}

// The instance as the sun sees it, for the shadow map.
@vertex
fn vs_shadow(
    @location(0) pos: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) m0: vec4<f32>,
    @location(3) m1: vec4<f32>,
    @location(4) m2: vec4<f32>,
    @location(5) m3: vec4<f32>,
    @location(6) color: vec4<f32>,
) -> @builtin(position) vec4<f32> {
    let m = mat4x4<f32>(m0, m1, m2, m3);
    return g.sun_view_proj * m * vec4<f32>(pos, 1.0);
}

// A square under the eye, big enough that fog hides its edge.
@vertex
fn vs_ground(@builtin(vertex_index) i: u32) -> Lit {
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(-1.0, -1.0), vec2<f32>(-1.0, 1.0), vec2<f32>(1.0, 1.0),
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, 1.0), vec2<f32>(1.0, -1.0),
    );
    let c = corners[i] * GROUND_HALF + g.eye.xz;
    let world = vec3<f32>(c.x, 0.0, c.y);
    var out: Lit;
    out.clip = g.view_proj * vec4<f32>(world, 1.0);
    out.world = world;
    out.color = g.ground.rgb;
    out.local = world;
    out.local_n = vec3<f32>(0.0, 1.0, 0.0);
    out.r0 = vec3<f32>(1.0, 0.0, 0.0);
    out.r1 = vec3<f32>(0.0, 1.0, 0.0);
    out.r2 = vec3<f32>(0.0, 0.0, 1.0);
    out.material = u32(g.ground.a + 0.5);
    return out;
}

// How much sun reaches `world`: 1 in full light, 0 in shadow. Points
// outside the map are lit. Pushing the point out along its normal keeps a
// surface from shadowing itself.
fn sunlight(world: vec3<f32>, n: vec3<f32>) -> f32 {
    let p = g.sun_view_proj * vec4<f32>(world + n * (1.5 * SHADOW_TEXEL), 1.0);
    let uv = vec2<f32>(p.x * 0.5 + 0.5, 0.5 - p.y * 0.5);
    // Each tap blends 2x2 texels; spreading four of them a texel apart
    // softens edges over about four texels.
    let d = 1.0 / 2048.0;
    var lit = 0.0;
    for (var i = 0; i < 4; i++) {
        let o = vec2<f32>(f32(i & 1) - 0.5, f32(i >> 1u) - 0.5) * 2.0 * d;
        lit += textureSampleCompareLevel(shadow_map, shadow_sampler, uv + o, p.z);
    }
    let inside = all(uv >= vec2<f32>(0.0)) && all(uv <= vec2<f32>(1.0)) && p.z <= 1.0;
    return select(1.0, lit * 0.25, inside);
}

// Material ids, matching `Material` in draw.rs.
const PLAIN: u32 = 0u;
const WOOD: u32 = 1u;
const BRICK: u32 = 2u;
const PLASTER: u32 = 3u;
const CONCRETE: u32 = 4u;
const ASPHALT: u32 = 5u;
const TILE: u32 = 6u;
const GRASS: u32 = 7u;
const FOLIAGE: u32 = 8u;
const FABRIC: u32 = 9u;
const METAL: u32 = 10u;
const WATER: u32 = 11u;

// The step for finite-difference bumps, in world units.
const BUMP_STEP: f32 = 0.004;

fn hash(p: vec2<i32>) -> f32 {
    var h = (bitcast<u32>(p.x) * 0x8da6b343u) ^ (bitcast<u32>(p.y) * 0xd8163841u);
    h = (h ^ (h >> 15u)) * 0x2c1b3c6du;
    h = (h ^ (h >> 12u)) * 0x297a2d39u;
    h = h ^ (h >> 15u);
    return f32(h >> 8u) / 16777216.0;
}

// Smooth value noise in [0, 1].
fn noise(p: vec2<f32>) -> f32 {
    let i = vec2<i32>(floor(p));
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let a = hash(i);
    let b = hash(i + vec2<i32>(1, 0));
    let c = hash(i + vec2<i32>(0, 1));
    let d = hash(i + vec2<i32>(1, 1));
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}

fn fbm(p: vec2<f32>) -> f32 {
    return 0.5 * noise(p) + 0.3 * noise(p * 2.03 + 17.0) + 0.2 * noise(p * 4.01 + 41.0);
}

// A running-bond grid of `size` cells with `gap` joints: the cell, and how
// much of a pixel `fw` across around `uv` is joint. Every other row shifts
// by `shift` cells. Joints thinner than a pixel blend toward their share
// of the surface instead of flickering.
fn bond(uv: vec2<f32>, size: vec2<f32>, gap: f32, shift: f32, fw: f32) -> vec3<f32> {
    let row = floor(uv.y / size.y);
    let x = uv.x / size.x + shift * f32(i32(row) & 1);
    let f = vec2<f32>(fract(x) * size.x, fract(uv.y / size.y) * size.y);
    let sharp = clamp((gap - min(f.x, f.y)) / max(fw, 1e-4) + 0.5, 0.0, 1.0);
    let share = gap / size.x + gap / size.y;
    let joint = mix(sharp, share, smoothstep(0.5 * gap, 2.0 * gap, fw));
    return vec3<f32>(floor(x), row, joint);
}

// Material `id` over linear colour `c` at `uv` (world units across the
// surface), seen through a pixel `fw` across: the albedo, and a height in
// [0, 1] for bumps.
fn pattern(id: u32, c: vec3<f32>, uv: vec2<f32>, fw: f32) -> vec4<f32> {
    switch id {
        case WOOD: {
            let grain = fbm(vec2<f32>(uv.x * 0.7, uv.y * 9.0));
            let rings = fract(grain * 5.0);
            let plank = floor(uv.y / 0.16);
            let seam = select(1.0, 0.0, fract(uv.y / 0.16) < 0.04);
            let tone = (0.84 + 0.2 * rings) * (0.9 + 0.2 * hash(vec2<i32>(i32(plank), 7)));
            return vec4<f32>(c * tone * mix(0.6, 1.0, seam), seam * (0.6 + 0.4 * rings));
        }
        case BRICK: {
            let b = bond(uv, vec2<f32>(0.25, 0.085), 0.012, 0.5, fw);
            let brick = c * (0.75 + 0.4 * hash(vec2<i32>(b.xy))) * (0.88 + 0.24 * noise(uv * 30.0));
            let mortar = vec3<f32>(0.4, 0.38, 0.35);
            return vec4<f32>(mix(brick, mortar, b.z), 1.0 - b.z);
        }
        case PLASTER: {
            let n = fbm(uv * 3.0);
            return vec4<f32>(c * (0.95 + 0.08 * n), noise(uv * 35.0));
        }
        case CONCRETE: {
            let blotch = fbm(uv * 1.2);
            let speck = noise(uv * 60.0);
            return vec4<f32>(c * (0.86 + 0.2 * blotch) * (0.94 + 0.12 * speck), speck);
        }
        case ASPHALT: {
            let grit = noise(uv * 55.0);
            let wear = fbm(uv * 0.4);
            return vec4<f32>(c * (0.8 + 0.3 * grit) * (0.85 + 0.3 * wear), grit);
        }
        case TILE: {
            let b = bond(uv, vec2<f32>(0.25, 0.25), 0.008, 0.0, fw);
            let tile = c * (0.95 + 0.08 * hash(vec2<i32>(b.xy)));
            return vec4<f32>(mix(tile, c * 0.55 + 0.08, b.z), 1.0 - b.z);
        }
        case GRASS: {
            let patches = fbm(uv * 0.25);
            let blades = noise(uv * vec2<f32>(45.0, 38.0));
            let dry = smoothstep(0.55, 0.8, fbm(uv * 0.12 + 5.0)) * 0.5;
            let green = c * (0.75 + 0.4 * patches) * (0.85 + 0.3 * blades);
            return vec4<f32>(mix(green, c * vec3<f32>(1.5, 1.25, 0.55), dry), blades);
        }
        case FOLIAGE: {
            let clump = fbm(uv * 4.0);
            let leaf = noise(uv * 18.0);
            return vec4<f32>(c * (0.55 + 0.55 * clump) * (0.9 + 0.2 * leaf), clump * 0.7 + leaf * 0.3);
        }
        case FABRIC: {
            let weave = abs(sin(uv.x * 160.0)) * abs(sin(uv.y * 160.0));
            let tone = 0.92 + 0.1 * fbm(uv * 3.0);
            return vec4<f32>(c * tone * (0.9 + 0.12 * weave), weave);
        }
        case METAL: {
            // Painted or polished: no grain, only faint wear and a ripple
            // in the highlight.
            let smudge = fbm(uv * 2.0);
            return vec4<f32>(c * (0.95 + 0.08 * smudge), smudge);
        }
        case WATER: {
            let ripple = fbm(uv * vec2<f32>(1.6, 2.4));
            return vec4<f32>(c * (0.85 + 0.25 * ripple), ripple);
        }
        default: {
            return vec4<f32>(c, 0.0);
        }
    }
}

// How deep a material's pattern height runs, in world units.
fn depth(id: u32) -> f32 {
    switch id {
        case BRICK: { return 0.004; }
        case TILE: { return 0.0015; }
        case GRASS: { return 0.003; }
        case FOLIAGE: { return 0.03; }
        case WATER: { return 0.01; }
        case WOOD, CONCRETE, ASPHALT: { return 0.001; }
        default: { return 0.0004; }
    }
}

// The finest feature a material draws that `bond` does not filter, in
// world units: brick rows, plank seams, weave, grit. A pattern fades out as
// a pixel grows past it, before it can alias.
fn feature(id: u32) -> f32 {
    switch id {
        case BRICK: { return 0.04; }
        case TILE: { return 0.1; }
        case WOOD, FABRIC: { return 0.01; }
        case FOLIAGE, WATER: { return 0.05; }
        default: { return 0.02; }
    }
}

// Sun highlight strength and sharpness.
fn gloss(id: u32) -> vec2<f32> {
    switch id {
        case TILE: { return vec2<f32>(0.35, 60.0); }
        case METAL: { return vec2<f32>(0.6, 40.0); }
        case WATER: { return vec2<f32>(0.9, 120.0); }
        case WOOD: { return vec2<f32>(0.08, 16.0); }
        case FOLIAGE: { return vec2<f32>(0.06, 10.0); }
        default: { return vec2<f32>(0.0, 1.0); }
    }
}

// The patterned albedo and the bumped normal in the instance's frame.
// Albedo blends the three axis projections by how squarely the surface
// faces each; the bump comes from the projection it faces most.
fn surface(id: u32, c: vec3<f32>, p: vec3<f32>, n: vec3<f32>, fw: f32, detail: f32) -> array<vec3<f32>, 2> {
    let w4 = pow(abs(n), vec3<f32>(4.0));
    let w = w4 / (w4.x + w4.y + w4.z);
    let px = pattern(id, c, p.zy, fw);
    let py = pattern(id, c, p.xz, fw);
    let pz = pattern(id, c, p.xy, fw);
    let albedo = px.rgb * w.x + py.rgb * w.y + pz.rgb * w.z;

    var uv = p.xy;
    var tu = vec3<f32>(1.0, 0.0, 0.0);
    var tv = vec3<f32>(0.0, 1.0, 0.0);
    var h = pz.a;
    if w.y >= w.x && w.y >= w.z {
        uv = p.xz;
        tv = vec3<f32>(0.0, 0.0, 1.0);
        h = py.a;
    } else if w.x >= w.z {
        uv = p.zy;
        tu = vec3<f32>(0.0, 0.0, 1.0);
        h = px.a;
    }
    let du = pattern(id, c, uv + vec2<f32>(BUMP_STEP, 0.0), fw).a - h;
    let dv = pattern(id, c, uv + vec2<f32>(0.0, BUMP_STEP), fw).a - h;
    let slope = depth(id) * detail / BUMP_STEP;
    let bumped = normalize(n - slope * (du * tu + dv * tv));
    return array<vec3<f32>, 2>(mix(c, albedo, detail), bumped);
}

// `n` is the shading normal; `geo` the true surface normal, which decides
// what the shadow map and the highlight see.
fn light(albedo: vec3<f32>, n: vec3<f32>, geo: vec3<f32>, world: vec3<f32>, shine: vec2<f32>) -> vec3<f32> {
    let lit = sunlight(world, geo) * select(0.0, 1.0, dot(geo, g.sun.xyz) > 0.0);
    let sun = max(dot(n, g.sun.xyz), 0.0) * lit * SUN_COLOR;
    let hemi = mix(g.ground.rgb * 0.5, g.sky.rgb * 0.6, n.y * 0.5 + 0.5);
    let half_dir = normalize(g.sun.xyz + normalize(g.eye.xyz - world));
    let highlight = shine.x * pow(max(dot(n, half_dir), 0.0), shine.y) * lit * SUN_COLOR;
    return albedo * (hemi + sun) + highlight;
}

// Lights a patterned surface: the shared tail of both fragment stages.
fn shade(in: Lit, base: vec3<f32>) -> vec4<f32> {
    // How much surface one pixel covers. Taken before any branch, where
    // derivatives are defined.
    let footprint = length(fwidth(in.local));
    let rot = mat3x3<f32>(in.r0, in.r1, in.r2);
    let local_n = normalize(in.local_n);
    let geo = normalize(rot * local_n);
    // fwidth sums both screen axes, so a pixel spans about half the footprint.
    let detail = 1.0 - smoothstep(1.0, 3.0, footprint / feature(in.material));
    var albedo = base;
    var n = geo;
    if in.material != PLAIN {
        let s = surface(in.material, base, in.local, local_n, footprint, detail);
        albedo = s[0];
        n = normalize(rot * s[1]);
    }
    let c = light(albedo, n, geo, in.world, gloss(in.material));
    return vec4<f32>(finish(c, in.world), 1.0);
}

fn encode(c: vec3<f32>) -> vec3<f32> {
    let hi = 1.055 * pow(c, vec3<f32>(1.0 / 2.4)) - 0.055;
    return select(hi, c * 12.92, c <= vec3<f32>(0.0031308));
}

fn finish(c: vec3<f32>, world: vec3<f32>) -> vec3<f32> {
    let f = smoothstep(g.fog.x, g.fog.y, distance(world, g.eye.xyz));
    let fogged = mix(c, g.sky.rgb, f);
    if g.fog.z > 0.5 {
        return encode(fogged);
    }
    return fogged;
}

@fragment
fn fs_lit(in: Lit) -> @location(0) vec4<f32> {
    return shade(in, in.color);
}

// Plain ground gets faint 2 u checks, so motion over it reads.
@fragment
fn fs_ground(in: Lit) -> @location(0) vec4<f32> {
    let cell = floor(in.world.xz / 2.0);
    let odd = abs(cell.x + cell.y) % 2.0;
    let checks = select(1.0, 1.0 - 0.06 * odd, in.material == PLAIN);
    return shade(in, in.color * checks);
}

struct Fan {
    @builtin(position) clip: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) color: vec4<f32>,
    @location(2) r: f32,
};

// `fan` is (fraction across the arc, fraction out along the radius).
@vertex
fn vs_wedge(
    @location(0) fan: vec2<f32>,
    @location(1) center_yaw: vec4<f32>,
    @location(2) half_radius: vec4<f32>,
    @location(3) color: vec4<f32>,
) -> Fan {
    let a = center_yaw.w + (fan.x * 2.0 - 1.0) * half_radius.x;
    let out_dir = vec3<f32>(sin(a), 0.0, cos(a));
    let world = center_yaw.xyz + out_dir * fan.y * half_radius.y + vec3<f32>(0.0, WEDGE_LIFT, 0.0);
    var out: Fan;
    out.clip = g.view_proj * vec4<f32>(world, 1.0);
    out.world = world;
    out.color = color;
    out.r = fan.y;
    return out;
}

@fragment
fn fs_wedge(in: Fan) -> @location(0) vec4<f32> {
    // Stronger toward the rim, so the reach reads at a glance.
    let alpha = in.color.a * mix(0.8, 1.0, in.r * in.r);
    return vec4<f32>(finish(in.color.rgb, in.world), alpha);
}
