// The sun's shadow map: the world drawn as seen from the sun, depth only. shader.wgsl compares a
// surface point's depth along the sun's ray with what is stored here (`sun_visibility`).
//
// The map is orthographic, like the sun's light: x and y are the point's coordinates along
// `sun.right` and `sun.up` (blocks, scaled to -1..1 over the map), the depth is 0.5 at the middle
// of the map and gets smaller towards the sun. The same vertices as the scene are used. Everything
// that has a face the sun can see casts: blocks and standing sprites (cut out by their alpha); markers,
// particles and the flat shadow spots (face 3 and 7) do not.

// The same as in shader.wgsl.
struct Camera {
    center: vec2<f32>,
    scale: vec2<f32>,
    center_depth: f32,
    _pad0: f32,
    _pad1: f32,
    _pad2: f32,
    view: vec4<f32>,
};

struct SunShadow {
    right: vec4<f32>,
    up: vec4<f32>,
    dir: vec4<f32>,
    center: vec4<f32>,
    params: vec4<f32>,
    grid_origin: vec4<f32>,
    grid_dims: vec4<f32>,  // w: 1 = only standing sprites cast (the blocks are in the voxel grid)
};

@group(0) @binding(0) var<uniform> camera: Camera;
@group(0) @binding(1) var<uniform> sun: SunShadow;
@group(1) @binding(0) var atlas: texture_2d_array<f32>;
@group(1) @binding(1) var atlas_sampler: sampler;

// Sprites with an alpha at or below this do not cast (the cut-out edge of a leaf or a player).
const CAST_ALPHA = 0.5;

// The free camera turns the world about a ground point before the fixed projection (shader.wgsl).
fn view_pos(p: vec3<f32>) -> vec3<f32> {
    let d = p.xy - camera.view.zw;
    let turned = vec2<f32>(camera.view.x * d.x - camera.view.y * d.y, camera.view.y * d.x + camera.view.x * d.y);
    return vec3<f32>(camera.view.zw + turned, p.z);
}

// The way back: from the turned view space into the world.
fn world_pos(p: vec3<f32>) -> vec3<f32> {
    let d = p.xy - camera.view.zw;
    let turned = vec2<f32>(camera.view.x * d.x + camera.view.y * d.y, -camera.view.y * d.x + camera.view.x * d.y);
    return vec3<f32>(camera.view.zw + turned, p.z);
}

// The same function as in shader.wgsl (a test compares them).
fn billboard_pos(anchor: vec3<f32>, dx: f32, dy: f32, toward: f32) -> vec3<f32> {
    let s = anchor.x + anchor.y + 1.0;
    let across = anchor.x - anchor.y + dx / 100.0;
    let z = anchor.z + (50.0 - dy) / 122.0;
    let e = 2.0 * 50.0 / 122.0;
    let d = toward / (2.0 + 0.82 * e);
    return vec3<f32>((s + across) / 2.0 + d, (s - across) / 2.0 + d, z + e * d);
}

struct VertexIn {
    @location(0) position: vec3<f32>,
    @location(1) color: vec3<f32>,
    @location(2) shade: vec2<f32>,
    @location(3) point: vec3<f32>,
    @location(4) uv: vec2<f32>,
    @location(5) layer: f32,
};

struct VertexOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) @interpolate(flat) layer: f32,
};

@vertex
fn vs_shadow(v: VertexIn) -> VertexOut {
    let face = i32(v.shade.x + 0.5);
    var out: VertexOut;
    out.uv = v.uv;
    out.layer = v.layer;
    if (face == 3 || face == 7 || (sun.grid_dims.w > 0.5 && face != 4)) {
        out.clip = vec4<f32>(2.0, 2.0, 2.0, 1.0);  // does not cast: outside the map
        return out;
    }
    var p = v.position;
    if (face == 4) {
        // A standing sprite is a card that faces the camera: build it in view space, bring it back.
        p = world_pos(billboard_pos(view_pos(p), v.point.x, v.point.y, 0.0));
    }
    let rel = p - sun.center.xyz;
    out.clip = vec4<f32>(
        dot(rel, sun.right.xyz) * sun.center.w,
        dot(rel, sun.up.xyz) * sun.center.w,
        0.5 - dot(rel, sun.dir.xyz) * sun.dir.w,
        1.0,
    );
    return out;
}

@fragment
fn fs_shadow(in: VertexOut) {
    if (in.layer > -0.5) {
        let texel = textureSampleLevel(atlas, atlas_sampler, in.uv, i32(in.layer + 0.5), 0.0);
        if (texel.a <= CAST_ALPHA) {
            discard;
        }
    }
}
