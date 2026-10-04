// Orthographic isometric projection done by hand so the constants match the Java engine
// (VIEW_WIDTH 200, VIEW_DEPTH 100, VIEW_HEIGHT 122 pixels per block at zoom 1), plus the light
// engine (see wurfel-sim/src/light.rs): every vertex is lit here. The sprites of the sprite atlas
// (a texture array, one layer per page) only multiply that light; a vertex without a sprite shows its
// flat colour instead.
//
// All uniform structs use vec4 (or scalars) only. A WGSL vec3 has 16-byte alignment, which would
// pad the structs differently from the plain f32 arrays on the Rust side (lighting.rs checks the
// layout against this file in a test).

struct Camera {
    center: vec2<f32>,    // screen position (px, y down) that sits in the middle of the canvas
    scale: vec2<f32>,     // 2 * zoom / canvas size in px
    center_depth: f32,
    // Three scalars, not a vec3: a vec3 would be 16-byte aligned and make this struct 48 bytes,
    // but the CPU side (`CameraUniform` in web.rs) is 32.
    _pad0: f32,
    _pad1: f32,
    _pad2: f32,
};

struct Lighting {
    ambient: vec4<f32>,      // rgb: ambient colour, already weighted; w: 1 = lit, 0 = flat look
    sun_color: vec4<f32>,    // rgb: sun light colour; w: specular intensity on top faces
    moon_color: vec4<f32>,   // rgb: moon light colour; w: specular intensity on top faces
    sun_faces: vec4<f32>,    // x left, y top, z right: sun diffuse intensity; w: night mix
    moon_faces: vec4<f32>,   // x left, y top, z right: moon diffuse intensity; w: moon blue
    grading: vec4<f32>,      // x exposure, y ambient occlusion strength, z point light gain, w night grading (1/0)
    flat_shades: vec4<f32>,  // x left, y top, z right: brightness of the flat look
    misc: vec4<f32>,         // x: number of dynamic point lights, y: time of day, z: minimum light, w: unused
    lights: array<vec4<f32>, 8>,        // xyz: position (blocks), w: radius
    light_colors: array<vec4<f32>, 8>,  // rgb: colour, w: brightness
};

@group(0) @binding(0) var<uniform> camera: Camera;
@group(0) @binding(1) var<uniform> lighting: Lighting;
@group(1) @binding(0) var atlas: texture_2d_array<f32>;
@group(1) @binding(1) var atlas_sampler: sampler;

const LUMA = vec3<f32>(0.222, 0.707, 0.071);
// Per-side constants of the Java PointLightSource: 0.15 + k * 0.005 for k = 0.1, 0.2, 0.25.
const POINT_SIDE = vec3<f32>(0.1505, 0.151, 0.15125);

struct VertexIn {
    @location(0) position: vec3<f32>,
    @location(1) color: vec3<f32>,   // the block's flat colour
    @location(2) shade: vec2<f32>,   // x: face (0 left, 1 top, 2 right, 3 unlit), y: ambient occlusion 0..1
    @location(3) point: vec3<f32>,   // light baked from static point lights
    @location(4) uv: vec2<f32>,      // atlas coordinates of the sprite
    @location(5) layer: f32,         // atlas page, or -1: no sprite, show `color`
};

struct VertexOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) color: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) @interpolate(flat) layer: f32,
};

// The left, top or right component of a vec4; face 4 (a sprite standing in the world) takes the
// average of the three, since it has no particular orientation.
fn pick3(v: vec4<f32>, face: i32) -> f32 {
    if (face == 0) {
        return v.x;
    }
    if (face == 1) {
        return v.y;
    }
    if (face == 4) {
        return (v.x + v.y + v.z) / 3.0;
    }
    return v.z;
}

// Light from the moving point lights (the player's torch, projectiles...), the Java falloff
// (1 + brightness) / distance^2 with the per-side Lambert term, zero beyond the radius. They do
// not cast shadows; the baked ones do.
fn dynamic_light(p: vec3<f32>, face: i32) -> vec3<f32> {
    var normal = vec3<f32>(0.0, 0.0, 1.0);
    if (face == 0) {
        normal = vec3<f32>(0.0, 1.0, 0.0);
    } else if (face == 2) {
        normal = vec3<f32>(1.0, 0.0, 0.0);
    }
    let side = pick3(vec4<f32>(POINT_SIDE, 0.0), face);
    var total = vec3<f32>(0.0);
    let count = u32(clamp(lighting.misc.x, 0.0, 8.0));
    for (var i: u32 = 0u; i < count; i = i + 1u) {
        let light = lighting.lights[i];
        let to_light = light.xyz - p;
        let dist = length(to_light);
        if (dist < light.w) {
            var lambert = 1.0;
            if (dist > 0.0001) {
                lambert = dot(to_light / dist, normal);
            }
            if (lambert > 0.0) {
                let d = max(dist, 0.5);
                let strength = (1.0 + lighting.light_colors[i].w) / (d * d) * lambert * side;
                total = total + lighting.light_colors[i].xyz * strength;
            }
        }
    }
    return total;
}

// The final colour of a vertex. wurfel_sim::light::shade_vertex is the reference implementation.
fn shade(v: VertexIn) -> vec3<f32> {
    let face = i32(v.shade.x + 0.5);
    if (face == 3) {
        return v.color;  // not lit: markers
    }
    if (lighting.ambient.w < 0.5) {
        return v.color * pick3(lighting.flat_shades, face);  // lighting switched off
    }

    var sun = pick3(lighting.sun_faces, face);
    var moon = pick3(lighting.moon_faces, face);
    if (face == 1) {
        sun = sun + lighting.sun_color.w;
        moon = moon + lighting.moon_color.w;
    }
    let lit = max(
        (lighting.ambient.xyz + lighting.sun_color.xyz * sun + lighting.moon_color.xyz * moon) * lighting.grading.x,
        vec3<f32>(lighting.misc.z),
    );

    let point = v.point + dynamic_light(v.position, face);
    let ao = 1.0 - lighting.grading.y * clamp(v.shade.y, 0.0, 1.0);
    let color = v.color * ((lit + point * lighting.grading.z) * ao);

    // At night: less saturation and more contrast, like the Java fragment shader.
    let night_mix = lighting.sun_faces.w;
    if (lighting.grading.w > 0.5 && night_mix > 0.0) {
        let grey = dot(color, LUMA);
        let desaturated = color - 0.6 * (color - vec3<f32>(grey));
        let contrast = max(1.0 + 0.4 * lighting.moon_faces.w, 0.0);
        let night = (desaturated - vec3<f32>(0.5)) * contrast + vec3<f32>(0.5);
        return max(mix(color, night, night_mix), vec3<f32>(0.0));
    }
    return max(color, vec3<f32>(0.0));
}

@vertex
fn vs_main(v: VertexIn) -> VertexOut {
    let p = v.position;
    let sx = (p.x - p.y) * 100.0;
    let sy = (p.x + p.y) * 50.0 - p.z * 122.0;

    // Looking along (1, 1, 0.82) in world space: a larger value is closer to the viewer. This is
    // exact for an orthographic camera, so the depth buffer sorts all faces correctly.
    let depth = (p.x + p.y + 0.82 * p.z) - camera.center_depth;

    var out: VertexOut;
    out.clip = vec4<f32>(
        (sx - camera.center.x) * camera.scale.x,
        -(sy - camera.center.y) * camera.scale.y,
        0.5 - depth * 0.002,
        1.0,
    );
    out.color = shade(v);
    out.uv = v.uv;
    out.layer = v.layer;
    return out;
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    // Sampled for every fragment (a level sample may sit in non-uniform control flow) and only used
    // when the vertex has a sprite. The sprites are cut out: pixels that are nearly transparent are
    // discarded, so the depth buffer still sorts everything.
    let texel = textureSampleLevel(atlas, atlas_sampler, in.uv, i32(max(in.layer, 0.0) + 0.5), 0.0);
    if (in.layer < -0.5) {
        return vec4<f32>(in.color, 1.0);
    }
    // A low threshold keeps the anti-aliased edge pixels of the art, which closes the hairline gaps
    // between neighbouring faces that a 0.5 cut would leave.
    if (texel.a < 0.2) {
        discard;
    }
    return vec4<f32>(texel.rgb * in.color, 1.0);
}
