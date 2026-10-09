// Atmosphere and life: ambient particles (pollen, fireflies, dust motes, leaves, mist), rain and snow
// with splash rings, and the large soft fog and god ray sprites. Drawn after the scene was peeled and
// blended into the HDR picture (see peel.rs) and before the post-process passes (post.rs), so the
// bloom picks up the fireflies and the tone map treats them like everything else.
//
// Nothing here comes from a vertex buffer. Every thing is one instance of six vertices, and the vertex
// shader derives its whole life from the instance number, a time and the viewer's position: where it
// is, how big, how bright, whether it exists at all. The CPU sends two small uniforms per frame and
// draws a few counted instances per kind (`Layer`, `atmosphere.rs`). Positions are fixed on a torus of
// `2 * NEAR` (or `2 * FAR`) blocks around the viewer, so a thing stays where it is in the world and
// leaves one side of the window as another enters on the other, where its edge fade has made it vanish.
//
// What it reads of the world: the camera and the lighting uniforms of shader.wgsl (the projection, the
// sun and the light colours), the nearest surface of every pixel (the depth of the first peeling
// layer, to hide what is behind the ground and to soften sprites where they cut it) and the column map
// (atmosphere.rs): the height and kind of the top block of every column, so rain lands on the roof and
// not in the room, rings spread on water, fireflies stay over grass and mist over water.
//
// The wind is the grass's (wurfel-sim/src/atmosphere.rs, which rebuilds the gusts of
// wurfel-sim/src/grass.rs in the ground frame): the same direction and the same slow gusts, as a
// closed-form integral so a drifting thing moves smoothly. The constants below marked "sim" are the
// twins of wurfel_sim::atmosphere, and a test checks they agree.
//
// Depth of field (tonemap.wgsl): the post pass blurs by the depth of the surface behind a pixel, which
// knows nothing of a particle in front of it. So a particle spreads itself: out of focus it grows by the
// circle of confusion tonemap.wgsl would give its own depth, and gets fainter by the area it gained.

struct Camera {
    center: vec2<f32>,
    scale: vec2<f32>,
    center_depth: f32,
    mirror_level: f32,
    mirror_on: f32,
    _pad2: f32,
    view: vec4<f32>,
    persp: vec4<f32>,
};

// The first fields of the `Lighting` struct of shader.wgsl: a prefix is enough, the buffer is bigger.
struct Lighting {
    ambient: vec4<f32>,
    sun_color: vec4<f32>,
    moon_color: vec4<f32>,
    sun_faces: vec4<f32>,    // x left, y top, z right: sun diffuse intensity; w: night mix
    moon_faces: vec4<f32>,
    grading: vec4<f32>,      // x exposure
    flat_shades: vec4<f32>,  // y: brightness of a top in the flat look
    misc: vec4<f32>,         // z: minimum light
    fog: vec4<f32>,          // rgb: fog colour
    sun_back: vec4<f32>,
    moon_back: vec4<f32>,
    sun_normal: vec4<f32>,
    moon_normal: vec4<f32>,
    pixel_ambient: vec4<f32>,
    local_light: vec4<f32>,
    sun_dir: vec4<f32>,      // xyz: unit vector towards the sun in the ground frame
};

struct Atmos {
    // x: seconds, y: 1 when the layers hold linear light, zw unused.
    clock: vec4<f32>,
    // xyz: the viewer in the ground frame (blocks); w: 1 when the column map is valid.
    viewer: vec4<f32>,
    // xy: the lattice point (gx, gy) of texel (0, 0) of the column map; z: its size in texels.
    map: vec4<f32>,
    // xy: the direction the wind blows to (ground frame, unit); z: how fast it carries light things
    // (blocks per second at gust 1); w: the gust floor (wurfel_sim::grass::GUST_MIN).
    wind: vec4<f32>,
    // xy: the gust's phase per block along gx and gy (radians); z: radians per second.
    gust: vec4<f32>,
    // x: depth of field strength (0 = off); y: the view depth that is in focus.
    dof: vec4<f32>,
};

// What one draw call draws: x the kind, y the count (informational), z how present it is, 0..1.
struct Layer {
    params: vec4<f32>,
};

@group(0) @binding(0) var<uniform> camera: Camera;
@group(0) @binding(1) var<uniform> lighting: Lighting;
@group(0) @binding(2) var<uniform> atmos: Atmos;
// The depth of the nearest surface of every pixel (1 where there is none), as post.rs reads it.
@group(0) @binding(3) var depth: texture_depth_2d;
// The column map (wurfel_sim::atmosphere): height in the low byte, kind flags above it.
@group(0) @binding(4) var columns: texture_2d<u32>;
@group(1) @binding(0) var<uniform> layer: Layer;

// Kinds (wurfel_sim::atmosphere::Kind).
const POLLEN = 0;
const FIREFLY = 1;
const MOTE = 2;
const LEAF = 3;
const MIST = 4;
const RAIN = 5;
const SPLASH = 6;
const SNOW = 7;
const FOG = 8;
const GOD_RAY = 9;

// The torus the close things live on and the one the large soft things live on, in blocks.
const NEAR = 14.0;
const FAR = 26.0;

// sim: rain, splash rings and snow.
const RAIN_SPEED = 12.0;
const RAIN_FALL = 18.0;
const RING_LIFE = 0.5;
const RAIN_SLANT = 0.3;
const SNOW_SPEED = 1.4;
const SNOW_FALL = 14.0;
const SNOW_DRIFT = 0.9;
// The focus band and the blur of tonemap.wgsl (same numbers; a test checks them).
const FOCUS_HALF = 0.14;
const FOCUS_FALLOFF = 0.36;
const BLUR_MAX = 14.0;

const RAIN_CYCLE = RAIN_FALL / RAIN_SPEED + RING_LIFE;
const SNOW_CYCLE = SNOW_FALL / SNOW_SPEED;
const LEAF_CYCLE = 9.0;
// How far a thing may sit behind the surface the depth buffer holds and still show (clip depth: the
// view depth is 0.002 per unit), and over how much it fades where it cuts into the ground.
const DEPTH_BIAS = 0.0004;
const DEPTH_SOFT = 0.006;
const TAU = 6.2831853;

// ------------------------------------------------------------------------------------ projection

// The free camera turns the world about a ground point before the fixed projection.
// shader.wgsl `view_pos` is the reference.
fn view_pos(p: vec3<f32>) -> vec3<f32> {
    let d = p.xy - camera.view.zw;
    let turned = vec2<f32>(camera.view.x * d.x - camera.view.y * d.y, camera.view.y * d.x + camera.view.x * d.y);
    return vec3<f32>(camera.view.zw + turned, p.z);
}

// Pixels (at zoom 1) of the projection per unit of view depth; shader.wgsl `DEPTH_PX`.
const DEPTH_PX = 68.0;

// shader.wgsl `perspective_scale`: 1 on the focus plane, larger towards the viewer, 0 beyond the eye.
fn perspective_scale(depth: f32) -> f32 {
    let distance = 2.0 / (camera.scale.y * camera.persp.x);
    let away = distance - (depth - camera.persp.y) * DEPTH_PX;
    if (away < 0.1 * distance) {
        return 0.0;
    }
    return distance / away;
}

struct Proj {
    // The position on the screen, px at zoom 1, y down, perspective included.
    xy: vec2<f32>,
    // The view depth relative to the camera's centre: larger is nearer.
    depth: f32,
    // The scale of the free camera's perspective at this depth (1 in the fixed camera).
    zoom_in: f32,
};

// The projection of shader.wgsl `vs_main`: the orthographic one and, in the free camera, the perspective.
fn project(world: vec3<f32>) -> Proj {
    let p = view_pos(world);
    let sx = (p.x - p.y) * 100.0;
    let sy = (p.x + p.y) * 50.0 - p.z * 122.0;
    let depth = (p.x + p.y + 0.82 * p.z) - camera.center_depth;
    var zoom_in = 1.0;
    if (camera.persp.x > 0.0) {
        zoom_in = perspective_scale(depth);
    }
    return Proj(camera.center + (vec2<f32>(sx, sy) - camera.center) * zoom_in, depth, zoom_in);
}

// ----------------------------------------------------------------------------------- randomness

fn hash_u(v: u32) -> u32 {
    var h = v;
    h = h ^ (h >> 16u);
    h = h * 0x7feb352du;
    h = h ^ (h >> 15u);
    h = h * 0x846ca68bu;
    return h ^ (h >> 16u);
}

// A number in 0..1 that depends only on the instance `i` and the slot `k`.
fn rnd(i: u32, k: u32) -> f32 {
    return f32(hash_u(i * 0x9e3779b1u + k * 0x85ebca6bu + 0x27d4eb2fu) >> 8u) / 16777216.0;
}

fn hash2(p: vec2<f32>) -> f32 {
    return fract(sin(dot(p, vec2<f32>(127.1, 311.7))) * 43758.5453);
}

fn smooth_noise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    return mix(
        mix(hash2(i), hash2(i + vec2<f32>(1.0, 0.0)), u.x),
        mix(hash2(i + vec2<f32>(0.0, 1.0)), hash2(i + vec2<f32>(1.0, 1.0)), u.x),
        u.y,
    );
}

// ------------------------------------------------------------------------------------------ wind

// wurfel_sim::atmosphere::gust: the grass's gust factor at a ground position.
fn gust_at(t: f32, at: vec2<f32>) -> f32 {
    let wave = sin(atmos.gust.z * t - dot(atmos.gust.xy, at)) * 0.5 + 0.5;
    return atmos.wind.w + (1.0 - atmos.wind.w) * wave;
}

// wurfel_sim::atmosphere::drift: how far the wind has carried something since time 0, in blocks along
// the wind, at `speed` times the gust. A difference of two of these is the way over a time span.
fn drift_at(t: f32, at: vec2<f32>, speed: f32) -> f32 {
    let phase = dot(atmos.gust.xy, at);
    let mean = atmos.wind.w + (1.0 - atmos.wind.w) * 0.5;
    let swing = (1.0 - atmos.wind.w) * 0.5 / atmos.gust.z;
    return speed * (mean * t - swing * (cos(atmos.gust.z * t - phase) - cos(phase)));
}

// ------------------------------------------------------------------------------------- the ground

struct Column {
    // The height of the surface in blocks (smoothed between columns by `column_smooth`).
    height: f32,
    grass: f32,
    water: f32,
    tree: f32,
    // 1 when the column is loaded (and the map covers it).
    known: f32,
};

fn decode(v: u32) -> Column {
    var c: Column;
    c.height = f32(v & 255u);
    c.water = f32((v >> 8u) & 1u);
    c.grass = f32((v >> 9u) & 1u);
    c.tree = f32((v >> 10u) & 1u);
    c.known = 1.0 - f32((v >> 15u) & 1u);
    return c;
}

fn column_texel(t: vec2<i32>) -> Column {
    let size = i32(atmos.map.z);
    if (t.x < 0 || t.y < 0 || t.x >= size || t.y >= size || atmos.viewer.w < 0.5) {
        return decode(0x8000u);
    }
    return decode(textureLoad(columns, t, 0).r);
}

// The column whose centre is nearest to the ground position.
fn column_near(p: vec2<f32>) -> Column {
    return column_texel(vec2<i32>(floor(p - atmos.map.xy + vec2<f32>(0.5))));
}

// The columns around the position, blended by distance: a height that changes smoothly while something
// drifts over the steps of the terrain, and kinds that fade at the border of the grass or the water.
fn column_smooth(p: vec2<f32>) -> Column {
    let q = p - atmos.map.xy;
    let base = floor(q);
    let f = q - base;
    let b = vec2<i32>(base);
    let c00 = column_texel(b);
    let c10 = column_texel(b + vec2<i32>(1, 0));
    let c01 = column_texel(b + vec2<i32>(0, 1));
    let c11 = column_texel(b + vec2<i32>(1, 1));
    var c: Column;
    let w00 = (1.0 - f.x) * (1.0 - f.y);
    let w10 = f.x * (1.0 - f.y);
    let w01 = (1.0 - f.x) * f.y;
    let w11 = f.x * f.y;
    c.height = c00.height * w00 + c10.height * w10 + c01.height * w01 + c11.height * w11;
    c.grass = c00.grass * w00 + c10.grass * w10 + c01.grass * w01 + c11.grass * w11;
    c.water = c00.water * w00 + c10.water * w10 + c01.water * w01 + c11.water * w11;
    c.tree = c00.tree * w00 + c10.tree * w10 + c01.tree * w01 + c11.tree * w11;
    c.known = min(min(c00.known, c10.known), min(c01.known, c11.known));
    return c;
}

// ------------------------------------------------------------------------------------- the light

// What a surface facing up gets: the light colours of the lighting uniform the way `shade` of shader.wgsl
// adds them, so the rain, the snow and the leaves are dark at night and warm at dusk.
fn sky_light() -> vec3<f32> {
    if (lighting.ambient.w < 0.5) {
        return vec3<f32>(lighting.flat_shades.y);
    }
    let lit = (lighting.ambient.xyz + lighting.sun_color.xyz * lighting.sun_faces.y + lighting.moon_color.xyz * lighting.moon_faces.y) * lighting.grading.x;
    return max(lit, vec3<f32>(lighting.misc.z));
}

// ---------------------------------------------------------------------------------------- sprites

// How a thing is put on the screen.
const BILLBOARD = 0;  // a rotated rectangle facing the screen, `size` pixels half width and height
const SEGMENT = 1;    // from `tail` to `pos`, `size.x` pixels half width: streaks and beams
const FLAT = 2;       // an ellipse lying on the ground: a circle of `size.x` blocks radius

struct Sprite {
    mode: i32,
    pos: vec3<f32>,
    tail: vec3<f32>,
    size: vec2<f32>,
    rot: f32,
    color: vec3<f32>,
    // 0: not drawn.
    alpha: f32,
    // 1 for what adds light (beams, fireflies), 0 for what is laid over the picture.
    glow: f32,
    seed: f32,
};

// The fraction of an instance that is there: instances switch on one by one as `amount` grows.
fn presence(i: u32, amount: f32) -> f32 {
    return clamp((amount - rnd(i, 9u)) * 8.0, 0.0, 1.0);
}

// A point of the torus of half width `half` around the viewer that stands for `p`.
fn wrap(p: vec2<f32>, half: f32) -> vec2<f32> {
    let size = 2.0 * half;
    let low = atmos.viewer.xy - vec2<f32>(half);
    let rel = p - low;
    return low + rel - size * floor(rel / size);
}

// 1 inside the window, falling to 0 at its border, where the torus wraps.
fn edge(xy: vec2<f32>, half: f32) -> f32 {
    let d = abs(xy - atmos.viewer.xy) / half;
    return 1.0 - smoothstep(0.6, 1.0, max(d.x, d.y));
}

fn at_random(i: u32, k: u32, half: f32) -> vec2<f32> {
    return vec2<f32>(rnd(i, k), rnd(i, k + 1u)) * (2.0 * half);
}

// Pollen: slow warm specks over grass, carried by the wind, bobbing.
fn pollen(i: u32, amount: f32) -> Sprite {
    var s: Sprite;
    let t = atmos.clock.x;
    let base = at_random(i, 1u, NEAR);
    let along = drift_at(t, base, atmos.wind.z * (0.6 + 0.8 * rnd(i, 3u)));
    let wander = vec2<f32>(sin(t * 0.7 + rnd(i, 4u) * TAU), cos(t * 0.55 + rnd(i, 5u) * TAU)) * 0.5;
    let xy = wrap(base + atmos.wind.xy * along + wander, NEAR);
    let c = column_smooth(xy);
    s.mode = BILLBOARD;
    s.pos = vec3<f32>(xy, c.height + 0.4 + 2.0 * rnd(i, 6u) + 0.25 * sin(t * 1.1 + rnd(i, 7u) * TAU));
    s.size = vec2<f32>(1.8 + 1.4 * rnd(i, 8u));
    s.color = vec3<f32>(1.0, 0.93, 0.7) * sky_light() * 1.4;
    let twinkle = 0.7 + 0.3 * sin(t * 2.0 + rnd(i, 10u) * TAU);
    s.alpha = 0.55 * twinkle * presence(i, amount) * edge(xy, NEAR) * c.grass * c.known;
    s.glow = 0.0;
    return s;
}

// Fireflies: wander over grass and water, glow in slow flashes. Bright enough for the bloom.
fn firefly(i: u32, amount: f32) -> Sprite {
    var s: Sprite;
    let t = atmos.clock.x;
    let base = at_random(i, 1u, NEAR);
    let ph = rnd(i, 4u) * TAU;
    let wander = vec2<f32>(sin(t * 0.31 + ph) + 0.5 * sin(t * 0.83 + ph * 2.0), cos(t * 0.27 + ph) + 0.5 * cos(t * 0.71 + ph * 3.0)) * 0.9;
    let xy = wrap(base + wander, NEAR);
    let c = column_smooth(xy);
    let flash = smoothstep(0.55, 1.0, sin(t * (0.6 + 0.7 * rnd(i, 5u)) + rnd(i, 6u) * TAU));
    s.mode = BILLBOARD;
    s.pos = vec3<f32>(xy, c.height + 0.5 + 1.4 * rnd(i, 7u) + 0.35 * sin(t * 0.9 + ph));
    s.size = vec2<f32>(4.5 + 1.5 * rnd(i, 8u));
    s.color = vec3<f32>(0.85, 1.0, 0.3) * 3.0;
    s.alpha = flash * presence(i, amount) * edge(xy, NEAR) * max(c.grass, c.water) * c.known;
    s.glow = 1.0;
    return s;
}

// Dust motes: specks of the sun's colour that glint now and then, anywhere on the ground.
fn mote(i: u32, amount: f32) -> Sprite {
    var s: Sprite;
    let t = atmos.clock.x;
    let base = at_random(i, 1u, NEAR);
    let ph = rnd(i, 4u) * TAU;
    let along = drift_at(t, base, atmos.wind.z * 0.25);
    let wander = vec2<f32>(sin(t * 0.21 + ph), cos(t * 0.19 + ph * 1.7)) * 0.6;
    let xy = wrap(base + atmos.wind.xy * along + wander, NEAR);
    let c = column_smooth(xy);
    let glint = pow(0.5 + 0.5 * sin(t * (0.5 + rnd(i, 5u)) + rnd(i, 6u) * TAU), 3.0);
    s.mode = BILLBOARD;
    s.pos = vec3<f32>(xy, c.height + 0.3 + 3.2 * rnd(i, 7u) + 0.2 * sin(t * 0.6 + ph));
    s.size = vec2<f32>(1.4 + 1.2 * rnd(i, 8u));
    s.color = normalize(lighting.sun_color.xyz + vec3<f32>(0.001)) * 2.2;
    s.alpha = (0.15 + 0.85 * glint) * 0.8 * presence(i, amount) * edge(xy, NEAR) * c.known;
    s.glow = 1.0;
    return s;
}

// Leaves: let go of a tree, tumble down the wind and fade out where they land.
fn leaf(i: u32, amount: f32) -> Sprite {
    var s: Sprite;
    let t = atmos.clock.x;
    let u = t / LEAF_CYCLE + rnd(i, 2u);
    let n = floor(u);
    let tc = (u - n) * LEAF_CYCLE;
    let k = i ^ (u32(max(n, 0.0)) * 0x9e3779b9u);
    let spawn = wrap(at_random(k, 11u, NEAR), NEAR);
    let c = column_near(spawn);
    let ph = rnd(k, 14u) * TAU;
    let along = drift_at(t, spawn, atmos.wind.z * 1.5) - drift_at(t - tc, spawn, atmos.wind.z * 1.5);
    let sway = vec2<f32>(-atmos.wind.y, atmos.wind.x) * sin(tc * 1.7 + ph) * 0.6;
    let xy = spawn + atmos.wind.xy * along + sway;
    let ground = column_smooth(xy).height;
    let z = c.height - 0.4 - 1.2 * rnd(k, 15u) - 0.45 * tc;
    let life = smoothstep(0.0, 0.6, tc) * (1.0 - smoothstep(LEAF_CYCLE - 1.0, LEAF_CYCLE, tc)) * smoothstep(ground, ground + 0.3, z);
    s.mode = BILLBOARD;
    s.pos = vec3<f32>(xy, z);
    let squash = 0.35 + 0.65 * abs(cos(tc * 2.3 + ph));
    s.size = vec2<f32>(4.5, 3.0 * squash);
    s.rot = tc * (1.5 + 2.0 * rnd(k, 16u)) + ph;
    let kind = rnd(k, 17u);
    var tint = vec3<f32>(0.35, 0.55, 0.15);
    if (kind > 0.6) {
        tint = vec3<f32>(0.62, 0.55, 0.12);
    }
    if (kind > 0.85) {
        tint = vec3<f32>(0.75, 0.38, 0.1);
    }
    s.color = tint * sky_light();
    s.alpha = 0.95 * life * presence(i, amount) * edge(xy, NEAR) * c.tree * c.known;
    s.glow = 0.0;
    return s;
}

// Mist: wide pale patches that hang over water and wander a little over it.
fn mist(i: u32, amount: f32) -> Sprite {
    var s: Sprite;
    let t = atmos.clock.x;
    let base = at_random(i, 1u, FAR);
    let ph = rnd(i, 4u) * TAU;
    let wander = (atmos.wind.xy * sin(t * 0.05 + ph) * 1.5 + vec2<f32>(-atmos.wind.y, atmos.wind.x) * cos(t * 0.04 + ph * 1.3));
    let xy = wrap(base + wander, FAR);
    // The patch is there where the water is, and reaches a block or two over the shore.
    let c = column_smooth(xy);
    let reach = max(
        max(column_smooth(xy + vec2<f32>(1.5, 0.0)).water, column_smooth(xy - vec2<f32>(1.5, 0.0)).water),
        max(column_smooth(xy + vec2<f32>(0.0, 1.5)).water, column_smooth(xy - vec2<f32>(0.0, 1.5)).water),
    );
    s.mode = FLAT;
    s.pos = vec3<f32>(xy, c.height + 0.3 + 0.4 * rnd(i, 6u));
    s.size = vec2<f32>(1.4 + 1.6 * rnd(i, 7u), 0.0);
    s.color = mix(lighting.fog.xyz, vec3<f32>(1.0), 0.6) * sky_light();
    s.alpha = 0.16 * presence(i, amount) * edge(xy, FAR) * max(c.water, 0.8 * reach) * c.known;
    s.glow = 0.0;
    s.seed = rnd(i, 12u) * 40.0;
    return s;
}

// The column a drop of this cycle lands in, in the window around the viewer.
fn drop_column(i: u32, k: u32) -> vec2<f32> {
    return wrap(at_random(i ^ (k * 0x9e3779b9u), 21u, NEAR), NEAR);
}

// Rain streaks: fall at RAIN_SPEED, slanted by the wind, until they land.
fn rain(i: u32, amount: f32) -> Sprite {
    var s: Sprite;
    let t = atmos.clock.x;
    let u = t / RAIN_CYCLE + rnd(i, 2u);
    let n = floor(u);
    let tc = (u - n) * RAIN_CYCLE;
    let land = drop_column(i, u32(max(n, 0.0)));
    let c = column_near(land);
    let h = RAIN_FALL - RAIN_SPEED * tc;
    let slant = RAIN_SLANT * gust_at(t, land);
    let head = vec3<f32>(land - atmos.wind.xy * slant * h, c.height + h);
    // The streak is as long as the drop moves in a twentieth of a second, and points along its path.
    let way = normalize(vec3<f32>(-atmos.wind.xy * slant, 1.0));
    s.mode = SEGMENT;
    s.pos = head;
    s.tail = head + way * 0.9;
    s.size = vec2<f32>(1.5, 0.0);
    s.color = vec3<f32>(0.62, 0.74, 0.92) * sky_light();
    s.alpha = select(0.0, 0.34, h > 0.0) * presence(i, amount) * edge(land, NEAR) * c.known;
    s.glow = 0.0;
    return s;
}

// The ring of the drop that landed: an ellipse on the ground (a circle seen from the camera) that grows
// and fades, bigger on water than on the ground.
fn splash(i: u32, amount: f32) -> Sprite {
    var s: Sprite;
    let t = atmos.clock.x;
    let u = t / RAIN_CYCLE + rnd(i, 2u);
    let n = floor(u);
    let tc = (u - n) * RAIN_CYCLE;
    let land = drop_column(i, u32(max(n, 0.0)));
    let c = column_near(land);
    let age = tc - RAIN_FALL / RAIN_SPEED;
    let life = clamp(age / RING_LIFE, 0.0, 1.0);
    let on_water = c.water;
    s.mode = FLAT;
    s.pos = vec3<f32>(land, c.height + 0.03);
    s.size = vec2<f32>(mix(0.05 + 0.22 * life, 0.08 + 0.5 * life, on_water), 0.0);
    s.color = vec3<f32>(0.8, 0.88, 1.0) * sky_light();
    let bright = (1.0 - life) * (1.0 - life) * mix(0.35, 0.6, on_water);
    s.alpha = select(0.0, bright, age >= 0.0 && age < RING_LIFE) * presence(i, amount) * edge(land, NEAR) * c.known;
    s.glow = 0.0;
    s.seed = on_water;
    return s;
}

// Snowflakes: fall slowly, flutter, drift with the wind and fade where they reach the ground.
fn snow(i: u32, amount: f32) -> Sprite {
    var s: Sprite;
    let t = atmos.clock.x;
    let u = t / SNOW_CYCLE + rnd(i, 2u);
    let n = floor(u);
    let tc = (u - n) * SNOW_CYCLE;
    let spawn = drop_column(i, u32(max(n, 0.0)));
    let c = column_near(spawn);
    let ph = rnd(i, 14u) * TAU;
    let along = drift_at(t, spawn, SNOW_DRIFT) - drift_at(t - tc, spawn, SNOW_DRIFT);
    let flutter = vec2<f32>(sin(tc * 1.3 + ph), cos(tc * 0.9 + ph * 1.7)) * 0.35;
    let xy = spawn + atmos.wind.xy * along + flutter;
    let z = c.height + SNOW_FALL - SNOW_SPEED * tc;
    let ground = column_smooth(xy).height;
    let life = smoothstep(0.0, 0.8, tc) * smoothstep(ground, ground + 0.4, z);
    s.mode = BILLBOARD;
    s.pos = vec3<f32>(xy, z);
    s.size = vec2<f32>(1.5 + 1.5 * rnd(i, 8u));
    s.color = vec3<f32>(1.0) * sky_light() * 1.1;
    s.alpha = 0.85 * life * presence(i, amount) * edge(xy, NEAR) * c.known;
    s.glow = 0.0;
    return s;
}

// Fog banks: big soft patches low over the ground that drift slowly with the wind, warm and thin when the
// sun is low. `amount` carries the sun's angle (CPU), the distance fade is the window's edge.
fn fog_bank(i: u32, amount: f32) -> Sprite {
    var s: Sprite;
    let t = atmos.clock.x;
    let base = at_random(i, 1u, FAR);
    let along = drift_at(t, base, atmos.wind.z * 0.4);
    let xy = wrap(base + atmos.wind.xy * along, FAR);
    let c = column_smooth(xy);
    s.mode = FLAT;
    s.pos = vec3<f32>(xy, c.height + 0.6 + 1.0 * rnd(i, 6u));
    s.size = vec2<f32>(2.5 + 3.5 * rnd(i, 7u), 0.0);
    let low_sun = 1.0 - clamp(lighting.sun_dir.z, 0.0, 1.0);
    let haze = mix(lighting.fog.xyz, vec3<f32>(1.0), 0.5);
    s.color = mix(haze, normalize(lighting.sun_color.xyz + vec3<f32>(0.001)), 0.35 * low_sun) * sky_light();
    s.alpha = 0.09 * presence(i, amount) * edge(xy, FAR) * c.known;
    s.glow = 0.0;
    s.seed = rnd(i, 12u) * 40.0;
    return s;
}

// God rays: slanting beams along the sun's direction, bright where they leave the sky and gone at the
// ground, added to the picture. They stand where they are in the world, so they slide with the camera.
fn god_ray(i: u32, amount: f32) -> Sprite {
    var s: Sprite;
    let t = atmos.clock.x;
    let sun = lighting.sun_dir.xyz;
    let base = at_random(i, 1u, FAR);
    let ph = rnd(i, 4u) * TAU;
    let xy = wrap(base + vec2<f32>(sin(t * 0.07 + ph), cos(t * 0.06 + ph)) * 0.8, FAR);
    let c = column_smooth(xy);
    let foot = vec3<f32>(xy, c.height);
    let reach = min(7.0 / max(sun.z, 0.05), 18.0);
    s.mode = SEGMENT;
    s.pos = foot + sun * reach;
    s.tail = foot;
    s.size = vec2<f32>(35.0 + 70.0 * rnd(i, 7u), 0.0);
    s.color = normalize(lighting.sun_color.xyz + vec3<f32>(0.001)) * vec3<f32>(1.0, 0.92, 0.75) * 1.6;
    let breathe = 0.7 + 0.3 * sin(t * 0.3 + ph);
    s.alpha = 0.09 * breathe * presence(i, amount) * edge(xy, FAR) * c.known * select(0.0, 1.0, sun.z > 0.03);
    s.glow = 1.0;
    s.seed = rnd(i, 12u) * 40.0;
    return s;
}

fn make_sprite(kind: i32, i: u32, amount: f32) -> Sprite {
    var s: Sprite;
    switch (kind) {
        case 0: { s = pollen(i, amount); }
        case 1: { s = firefly(i, amount); }
        case 2: { s = mote(i, amount); }
        case 3: { s = leaf(i, amount); }
        case 4: { s = mist(i, amount); }
        case 5: { s = rain(i, amount); }
        case 6: { s = splash(i, amount); }
        case 7: { s = snow(i, amount); }
        case 8: { s = fog_bank(i, amount); }
        default: { s = god_ray(i, amount); }
    }
    return s;
}

// ---------------------------------------------------------------------------------- depth of field

// The radius, in px at zoom 1, of the circle of confusion tonemap.wgsl gives something at this view
// depth. (`blur_radius` there works in screen pixels: dividing by the zoom, which is
// `scale.y * height / 2`, leaves the height out of it.)
fn blur_px(view_depth: f32) -> f32 {
    if (atmos.dof.x <= 0.0) {
        return 0.0;
    }
    let shift = abs(view_depth - atmos.dof.y) * 25.0 * camera.scale.y;
    let off_focus = smoothstep(0.0, FOCUS_FALLOFF, shift - FOCUS_HALF);
    return atmos.dof.x * off_focus * BLUR_MAX * 2.0 / (1080.0 * camera.scale.y);
}

// ------------------------------------------------------------------------------------- the stages

struct VertexOut {
    @builtin(position) clip: vec4<f32>,
    // Across the quad, -1..1 both ways; for a segment y runs from the tail (-1) to the head (1).
    @location(0) uv: vec2<f32>,
    @location(1) color: vec3<f32>,
    // x: kind, y: opacity, z: 1 for light that is added, w: seed.
    @location(2) @interpolate(flat) look: vec4<f32>,
};

fn hidden() -> VertexOut {
    var out: VertexOut;
    out.clip = vec4<f32>(2.0, 2.0, 2.0, 1.0);
    out.uv = vec2<f32>(0.0);
    out.color = vec3<f32>(0.0);
    out.look = vec4<f32>(0.0);
    return out;
}

fn clip_of(xy: vec2<f32>, depth: f32) -> vec4<f32> {
    return vec4<f32>((xy.x - camera.center.x) * camera.scale.x, -(xy.y - camera.center.y) * camera.scale.y, 0.5 - depth * 0.002, 1.0);
}

@vertex
fn vs_main(@builtin(vertex_index) vi: u32, @builtin(instance_index) instance: u32) -> VertexOut {
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, -1.0), vec2<f32>(1.0, 1.0),
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, 1.0), vec2<f32>(-1.0, 1.0),
    );
    let c = corners[vi];
    let kind = i32(layer.params.x + 0.5);
    let sprite = make_sprite(kind, instance, layer.params.z);
    if (sprite.alpha <= 0.002) {
        return hidden();
    }

    let head = project(sprite.pos);
    if (head.zoom_in <= 0.0) {
        return hidden();
    }
    var out: VertexOut;
    out.uv = c;
    out.color = sprite.color;
    var alpha = sprite.alpha;
    var xy = head.xy;
    var depth = head.depth;

    if (sprite.mode == BILLBOARD) {
        // The sprite spreads by its own circle of confusion and gets fainter by the area it gained.
        let blur = blur_px(head.depth);
        let half = sqrt(sprite.size * sprite.size + vec2<f32>(blur * blur));
        alpha = alpha * clamp(sprite.size.x * sprite.size.y / (half.x * half.y), 0.15, 1.0);
        let cr = cos(sprite.rot);
        let sr = sin(sprite.rot);
        let local = c * half;
        let turned = vec2<f32>(cr * local.x - sr * local.y, sr * local.x + cr * local.y);
        xy = head.xy + turned * head.zoom_in;
        // A little towards the viewer, so a speck on the ground is not cut by the block it floats over.
        depth = head.depth + 0.05;
    } else if (sprite.mode == SEGMENT) {
        let tail = project(sprite.tail);
        if (tail.zoom_in <= 0.0) {
            return hidden();
        }
        var along = head.xy - tail.xy;
        let length_px = max(length(along), 0.001);
        along = along / length_px;
        let across = vec2<f32>(-along.y, along.x);
        let blur = blur_px(head.depth);
        let width = sqrt(sprite.size.x * sprite.size.x + blur * blur);
        alpha = alpha * clamp(sprite.size.x / width, 0.15, 1.0);
        // From the tail (uv.y = -1) to the head (uv.y = 1); the ends reach a little past the points.
        let along_t = (c.y * 0.5 + 0.5);
        let at = mix(tail.xy, head.xy, along_t) + along * c.y * width * head.zoom_in;
        xy = at + across * c.x * width * head.zoom_in;
        depth = mix(tail.depth, head.depth, along_t) + 0.05;
    } else {
        // A circle on the ground is an ellipse of 2:1 on the screen (a block is 100 px wide and 50 px high
        // per half step), and the depth along it follows the ground: a pixel lower on the screen is nearer.
        let r = sprite.size.x;
        let half = vec2<f32>(141.4, 70.7) * r;
        xy = head.xy + c * half * head.zoom_in;
        depth = head.depth + c.y * half.y / 50.0;
    }
    out.clip = clip_of(xy, depth);
    out.look = vec4<f32>(f32(kind), alpha, sprite.glow, sprite.seed);
    return out;
}

// A soft round speck: a bright core in a halo.
fn speck(d2: f32) -> f32 {
    return exp(-d2 * 5.0) * 0.8 + exp(-d2 * 18.0) * 0.6;
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    let kind = i32(in.look.x + 0.5);
    let t = atmos.clock.x;
    let d2 = dot(in.uv, in.uv);
    var shape = 0.0;
    var soft = false;
    if (kind == POLLEN || kind == FIREFLY || kind == MOTE || kind == SNOW) {
        shape = speck(d2) * select(1.0, 0.0, d2 > 1.0);
    } else if (kind == LEAF) {
        // A leaf: an ellipse with a darker rib.
        let rib = 1.0 - 0.35 * exp(-in.uv.y * in.uv.y * 30.0);
        shape = (1.0 - smoothstep(0.7, 1.0, sqrt(d2))) * rib;
    } else if (kind == RAIN) {
        // Brighter towards the head, thin across.
        let across = 1.0 - smoothstep(0.0, 1.0, abs(in.uv.x));
        let along = smoothstep(-1.0, 1.0, in.uv.y);
        shape = across * along;
    } else if (kind == SPLASH) {
        // A ring: a band at 0.75 of the radius.
        let r = sqrt(d2);
        shape = 1.0 - smoothstep(0.0, 0.22, abs(r - 0.78));
        if (in.look.w > 0.5) {
            // On water a fainter second ring follows the first inside.
            shape = shape + 0.4 * (1.0 - smoothstep(0.0, 0.16, abs(r - 0.45)));
        }
        shape = shape * select(1.0, 0.0, r > 1.0);
    } else if (kind == MIST || kind == FOG) {
        // A soft blob whose edge is eaten by slow noise.
        let r2 = min(d2, 1.0);
        let n = smooth_noise(in.uv * 2.2 + vec2<f32>(in.look.w, in.look.w * 0.7) + vec2<f32>(t * 0.02, -t * 0.015));
        let blob = (1.0 - r2) * (1.0 - r2);
        shape = blob * (0.45 + 0.9 * n);
        soft = true;
    } else {
        // A beam: soft across, bright at the sky end (uv.y = 1) and fading to nothing at the ground,
        // with slow streaks of noise along it.
        let across = 1.0 - smoothstep(0.0, 1.0, abs(in.uv.x));
        let along = smoothstep(-1.0, 0.6, in.uv.y) * (1.0 - smoothstep(0.7, 1.0, in.uv.y));
        let streak = 0.55 + 0.9 * smooth_noise(vec2<f32>(in.uv.x * 3.0 + in.look.w, t * 0.15 + in.look.w * 0.3));
        shape = across * across * along * streak;
        soft = true;
    }

    // Behind the surface the depth buffer holds, nothing shows; soft things fade out where they cut it.
    let scene = textureLoad(depth, vec2<i32>(in.clip.xy), 0);
    let margin = scene - in.clip.z;
    if (margin < -DEPTH_BIAS) {
        discard;
    }
    var fade = 1.0;
    if (soft) {
        fade = smoothstep(-DEPTH_BIAS, DEPTH_SOFT, margin);
    }

    let a = clamp(shape * in.look.y * fade, 0.0, 1.0);
    if (a <= 0.001) {
        discard;
    }
    var rgb = in.color;
    if (atmos.clock.y > 0.5) {
        // The scene is blended in linear light (see shader.wgsl); a power keeps `albedo * light` as it was.
        rgb = pow(max(rgb, vec3<f32>(0.0)), vec3<f32>(2.2));
    }
    // Premultiplied: what is laid over the picture covers it by `a`, what is added leaves it alone.
    let covers = select(a, 0.0, in.look.z > 0.5);
    return vec4<f32>(rgb * a, covers);
}
