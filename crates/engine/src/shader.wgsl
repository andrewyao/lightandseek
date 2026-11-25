// WebGL2-safe: one uniform block, per-instance data in vertex attributes,
// no storage buffers. Colours arrive linear.

struct Globals {
    view_proj: mat4x4<f32>,
    eye: vec4<f32>,
    // Unit vector toward the sun.
    sun: vec4<f32>,
    sky: vec4<f32>,
    ground: vec4<f32>,
    // Fog start, fog end, 1 when the target needs sRGB encoding here.
    fog: vec4<f32>,
};

@group(0) @binding(0) var<uniform> g: Globals;

const SUN_COLOR: vec3<f32> = vec3<f32>(1.0, 0.95, 0.86);
const GROUND_HALF: f32 = 400.0;
const WEDGE_LIFT: f32 = 0.02;

struct Lit {
    @builtin(position) clip: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) color: vec3<f32>,
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
    // For rotation times scale, the inverse transpose is m with each column
    // divided by its squared length.
    let s2 = vec3<f32>(dot(m0.xyz, m0.xyz), dot(m1.xyz, m1.xyz), dot(m2.xyz, m2.xyz));
    var out: Lit;
    out.clip = g.view_proj * vec4<f32>(world, 1.0);
    out.world = world;
    out.normal = (m * vec4<f32>(normal / s2, 0.0)).xyz;
    out.color = color.rgb;
    return out;
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
    out.normal = vec3<f32>(0.0, 1.0, 0.0);
    out.color = g.ground.rgb;
    return out;
}

fn light(base: vec3<f32>, n: vec3<f32>) -> vec3<f32> {
    let sun = max(dot(n, g.sun.xyz), 0.0) * SUN_COLOR;
    let hemi = mix(g.ground.rgb * 0.5, g.sky.rgb * 0.6, n.y * 0.5 + 0.5);
    return base * (hemi + sun);
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
    return vec4<f32>(finish(light(in.color, normalize(in.normal)), in.world), 1.0);
}

// Faint 2 u checks, so motion over open ground reads.
@fragment
fn fs_ground(in: Lit) -> @location(0) vec4<f32> {
    let cell = floor(in.world.xz / 2.0);
    let odd = abs(cell.x + cell.y) % 2.0;
    let base = in.color * (1.0 - 0.06 * odd);
    return vec4<f32>(finish(light(base, in.normal), in.world), 1.0);
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
