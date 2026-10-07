// Orthographic isometric projection done by hand so the constants match the Java engine
// (VIEW_WIDTH 200, VIEW_DEPTH 100, VIEW_HEIGHT 122 pixels per block at zoom 1), plus the light
// engine (see wurfel-sim/src/light.rs): every vertex is lit here. The sprites of the sprite atlas
// (a texture array, one layer per page) only multiply that light; a vertex without a sprite shows its
// flat colour instead.
//
// The fragment stage peels the scene: it is drawn once per layer (see peel.rs, after the Java
// engine's depth peeling in GameView.depthPeelingRendering), and layer n keeps only the nearest
// fragment that lies behind the one layer n - 1 kept. That depth comes in as a texture.
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
    // The free camera: x cos and y sin of the yaw, zw the ground point the world turns about.
    view: vec4<f32>,
};

struct Lighting {
    ambient: vec4<f32>,      // rgb: ambient colour, already weighted; w: 1 = lit, 0 = flat look
    sun_color: vec4<f32>,    // rgb: sun light colour; w: specular intensity on top faces
    moon_color: vec4<f32>,   // rgb: moon light colour; w: specular intensity on top faces
    sun_faces: vec4<f32>,    // x left, y top, z right: sun diffuse intensity; w: night mix
    moon_faces: vec4<f32>,   // x left, y top, z right: moon diffuse intensity; w: moon blue
    grading: vec4<f32>,      // x exposure, y ambient occlusion strength, z point light gain, w night grading (1/0)
    flat_shades: vec4<f32>,  // x left, y top, z right: brightness of the flat look; w: 1 = draw linear light (see post.rs)
    misc: vec4<f32>,         // x: number of dynamic point lights, y: time of day, z: minimum light, w: 1 = normal maps
    fog: vec4<f32>,          // rgb: fog colour; w: 1 = fog on
    sun_back: vec4<f32>,     // x: sun diffuse intensity on the -y face, y: on the -x face (free camera)
    moon_back: vec4<f32>,    // the same for the moon
    sun_normal: vec4<f32>,   // xyz: the Java u_sunNormal (Java screen-aligned frame), for the normal maps
    moon_normal: vec4<f32>,  // xyz: the Java u_moonNormal
    pixel_ambient: vec4<f32>,  // rgb: the Java u_ambientColor, unweighted
    local_light: vec4<f32>,  // xyz: the Java u_localLightPos / u_playerpos (blocks); w: 1 when there is one
    lights: array<vec4<f32>, 8>,        // xyz: position (blocks), w: radius
    light_colors: array<vec4<f32>, 8>,  // rgb: colour, w: brightness
};

@group(0) @binding(0) var<uniform> camera: Camera;
@group(0) @binding(1) var<uniform> lighting: Lighting;
@group(1) @binding(0) var atlas: texture_2d_array<f32>;
@group(1) @binding(1) var atlas_sampler: sampler;
// The normal map of the atlas: the same pages, so the same uv and layer (the Java u_normals).
@group(1) @binding(2) var normals: texture_2d_array<f32>;

// One peeling layer's settings; the same four floats as `PeelUniform` in peel.rs.
struct Peel {
    // x: 1 when an earlier layer exists (so `previous_depth` counts), y: the depth margin,
    // z: fragments with an alpha at or below this are discarded, w: unused.
    params: vec4<f32>,
};

@group(2) @binding(0) var<uniform> peel: Peel;
// The depth the previous layer kept (cleared to 1 where it kept nothing).
@group(2) @binding(1) var previous_depth: texture_depth_2d;

const LUMA = vec3<f32>(0.222, 0.707, 0.071);
// wurfel_sim::light::NIGHT_FLOOR: the bluish least light at night.
const NIGHT_FLOOR = vec3<f32>(0.6, 0.7, 0.95);
// Per-side constants of the Java PointLightSource: 0.15 + k * 0.005 for k = 0.1, 0.2, 0.25.
const POINT_SIDE = vec3<f32>(0.1505, 0.151, 0.15125);

// The free camera turns the world about a ground point before the fixed projection.
// wurfel_web::view::View::rotate is the reference.
fn view_pos(p: vec3<f32>) -> vec3<f32> {
    let d = p.xy - camera.view.zw;
    let turned = vec2<f32>(camera.view.x * d.x - camera.view.y * d.y, camera.view.y * d.x + camera.view.x * d.y);
    return vec3<f32>(camera.view.zw + turned, p.z);
}

// Where the corner `(dx, dy)` pixels from `anchor` (in view space) lies on the wall that faces the
// camera; `toward` moves it nearer without moving it on the screen.
// wurfel_web::sprites::billboard_point is the reference.
fn billboard_pos(anchor: vec3<f32>, dx: f32, dy: f32, toward: f32) -> vec3<f32> {
    let s = anchor.x + anchor.y + 1.0;
    let across = anchor.x - anchor.y + dx / 100.0;
    let z = anchor.z + (50.0 - dy) / 122.0;
    let e = 2.0 * 50.0 / 122.0;
    let d = toward / (2.0 + 0.82 * e);
    return vec3<f32>((s + across) / 2.0 + d, (s - across) / 2.0 + d, z + e * d);
}

// Does the side with this face id look towards the camera? The camera looks from +x +y, and the
// world is turned by the yaw (cos in view.x, sin in view.y): the normal of the face turns with it.
fn faces_camera(face: i32) -> bool {
    let c = camera.view.x;
    let s = camera.view.y;
    if (face == 0) {
        return c - s > 0.0;   // left: +y
    }
    if (face == 2) {
        return c + s > 0.0;   // right: +x
    }
    if (face == 5) {
        return s - c > 0.0;   // -y
    }
    if (face == 6) {
        return -c - s > 0.0;  // -x
    }
    return true;
}

struct VertexIn {
    @location(0) position: vec3<f32>,
    @location(1) color: vec3<f32>,   // the block's flat colour
    @location(2) shade: vec2<f32>,   // x: face (0 left, 1 top, 2 right, 3 unlit), y: ambient occlusion 0..1
    @location(3) point: vec3<f32>,   // light baked from static point lights
    @location(4) uv: vec2<f32>,      // atlas coordinates of the sprite
    @location(5) layer: f32,         // atlas page, or -1: no sprite, show `color`
};

struct VertexOut {
    // In the fragment stage this is the window position: clip.xy in pixels, clip.z the depth.
    @builtin(position) clip: vec4<f32>,
    @location(0) color: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) @interpolate(flat) layer: f32,
    // What the normal map path needs (Java v_color and v_pos): the flat colour the sprite is tinted
    // with, the baked point light (rgb) and the ambient occlusion factor (a), and where the vertex is.
    @location(3) albedo: vec3<f32>,
    @location(4) baked: vec4<f32>,
    @location(5) world: vec3<f32>,
    @location(6) @interpolate(flat) face: f32,
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
    if (face == 5 || face == 6) {
        return (v.x + v.z) / 2.0;  // the sides away from the fixed camera look like the left and right
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
    } else if (face == 5) {
        normal = vec3<f32>(0.0, -1.0, 0.0);
    } else if (face == 6) {
        normal = vec3<f32>(-1.0, 0.0, 0.0);
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

// Fog: what lies far behind the camera's centre is blended towards the fog colour (the Java shader
// added the colour, which brightened the distance). wurfel_sim::light::fog_mix is the reference.
// `behind` is in Java game units: 100 per block row.
const FOG_MAX = 0.6;
fn with_fog(color: vec3<f32>, seen: vec3<f32>) -> vec3<f32> {
    if (lighting.fog.w < 0.5) {
        return color;
    }
    let behind = (camera.center_depth - (seen.x + seen.y)) * 100.0;
    let java = min(exp(max(behind - 40.8, 0.0) * 0.001), 2.5) - 1.0;
    // The fog colour lightened towards white (haze is a light veil), dimmer at night.
    let haze = mix(lighting.fog.xyz, vec3<f32>(1.0), 0.6) * (1.0 - 0.75 * clamp(lighting.sun_faces.w, 0.0, 1.0));
    return mix(color, haze, java / 1.5 * FOG_MAX);
}

// The final colour of a vertex. wurfel_sim::light::shade_vertex is the reference implementation.
fn shade(v: VertexIn, seen: vec3<f32>) -> vec3<f32> {
    let face = i32(v.shade.x + 0.5);
    if (face == 3 || face == 7) {
        return v.color;  // not lit: markers and particles
    }
    if (lighting.ambient.w < 0.5) {
        return v.color * pick3(lighting.flat_shades, face);  // lighting switched off
    }

    var sun = pick3(lighting.sun_faces, face);
    var moon = pick3(lighting.moon_faces, face);
    if (face == 5) {
        sun = lighting.sun_back.x;
        moon = lighting.moon_back.x;
    } else if (face == 6) {
        sun = lighting.sun_back.y;
        moon = lighting.moon_back.y;
    }
    if (face == 1) {
        sun = sun + lighting.sun_color.w;
        moon = moon + lighting.moon_color.w;
    }
    let night_mix = lighting.sun_faces.w;
    var floor_light = vec3<f32>(lighting.misc.z);
    if (lighting.grading.w > 0.5) {
        floor_light = mix(floor_light, NIGHT_FLOOR, night_mix);
    }
    let lit = max(
        (lighting.ambient.xyz + lighting.sun_color.xyz * sun + lighting.moon_color.xyz * moon) * lighting.grading.x,
        floor_light,
    );

    // A sprite's `point` is its screen offset, not light.
    var baked = v.point;
    if (face == 4) {
        baked = vec3<f32>(0.0);
    }
    let point = baked + dynamic_light(v.position, face);
    let ao = 1.0 - lighting.grading.y * clamp(v.shade.y, 0.0, 1.0);
    let color = v.color * ((lit + point * lighting.grading.z) * ao);

    // At night: less saturation and more contrast, like the Java fragment shader.
    if (lighting.grading.w > 0.5 && night_mix > 0.0) {
        let grey = dot(color, LUMA);
        let desaturated = color - 0.6 * (color - vec3<f32>(grey));
        let contrast = max(1.0 + 0.4 * lighting.moon_faces.w, 0.0);
        let night = (desaturated - vec3<f32>(0.5)) * contrast + vec3<f32>(0.5);
        return with_fog(max(mix(color, night, night_mix), vec3<f32>(0.0)), seen);
    }
    return with_fog(max(color, vec3<f32>(0.0)), seen);
}

@vertex
fn vs_main(v: VertexIn) -> VertexOut {
    let face = i32(v.shade.x + 0.5);
    var p = view_pos(v.position);
    if (face == 4 || face == 7) {
        p = billboard_pos(p, v.point.x, v.point.y, v.point.z);
    }
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
    if (!faces_camera(face)) {
        out.clip = vec4<f32>(2.0, 2.0, 2.0, 1.0);  // outside the view: the side that looks away
    }
    let seen = view_pos(v.position);
    out.color = shade(v, seen);
    out.uv = v.uv;
    out.layer = v.layer;
    out.albedo = v.color;
    // A sprite's `point` is its screen offset, not light.
    var baked = v.point;
    if (face == 4) {
        baked = vec3<f32>(0.0);
    }
    out.baked = vec4<f32>(baked, 1.0 - lighting.grading.y * clamp(v.shade.y, 0.0, 1.0));
    if (face == 3 || face == 7) {
        // Unlit faces have no occlusion: `shade.y` is how see-through they are (0: opaque).
        out.baked.a = 1.0 - clamp(v.shade.y, 0.0, 1.0);
    }
    out.world = seen;
    out.face = f32(face);
    return out;
}


// ------------------------------------------------------------------------ normal map lighting
// A port of the Java fragment_NM.fs. The sprite's normal comes from the normal map, in the Java
// screen-aligned frame (x right, y towards the viewer, z up); the light directions are brought into
// it. The Java shader works in game units (100 per block), so distances are scaled to match.

const SQRT_HALF = 0.70710678;
const GAME_UNITS_PER_BLOCK = 100.0;

// The isometric ground frame of the world (x towards the lower right, y towards the lower left)
// and the Java screen-aligned frame: wurfel_sim::entity::{screen_to_iso, iso_to_screen}.
fn game_to_iso(g: vec3<f32>) -> vec3<f32> {
    return vec3<f32>(SQRT_HALF * (g.x + g.y), SQRT_HALF * (g.y - g.x), g.z);
}

fn iso_to_game(v: vec3<f32>) -> vec3<f32> {
    return vec3<f32>(SQRT_HALF * (v.x - v.y), SQRT_HALF * (v.x + v.y), v.z);
}

// A direction in the Java frame as the free camera sees it: the world turns by the yaw.
fn turn_game_direction(g: vec3<f32>) -> vec3<f32> {
    let d = game_to_iso(g);
    let c = camera.view.x;
    let s = camera.view.y;
    return iso_to_game(vec3<f32>(c * d.x - s * d.y, s * d.x + c * d.y, d.z));
}

// Is this fragment lit per pixel? Blocks (faces 0, 1, 2 and the free camera's 5, 6) and standing
// sprites (4) are; markers, particles and models (3, 7) keep their vertex colour.
fn lit_by_normal_map(face: i32, layer: f32) -> bool {
    return lighting.misc.w > 0.5 && layer > -0.5 && face != 3 && face != 7;
}

fn normal_map_color(in: VertexOut, texel: vec4<f32>) -> vec3<f32> {
    // The sun and the moon belong to the world. A block's normal map is in the world's frame, which
    // does not turn with the free camera, so the light is used as it is. Only a standing sprite
    // always faces the camera: its normal map is in the view frame, so the light turns into it.
    let billboard = i32(in.face + 0.5) == 4;
    var sun_normal = lighting.sun_normal.xyz;
    var moon_normal = lighting.moon_normal.xyz;
    if (billboard) {
        sun_normal = turn_game_direction(sun_normal);
        moon_normal = turn_game_direction(moon_normal);
    }

    var diffuse = texel.rgb * in.albedo;
    let normal_color = textureSampleLevel(normals, atlas_sampler, in.uv, i32(max(in.layer, 0.0) + 0.5), 0.0).rgb;
    var n = normalize(normal_color * 2.0 - vec3<f32>(1.0));
    n.x = -n.x;  // x is flipped in the texture

    // Clamp the sun light at white, so during the day it appears white, not yellow.
    let sun_light = min(lighting.sun_color.rgb * 2.0, vec3<f32>(1.0)) * max(dot(n, sun_normal), 0.0);
    diffuse = diffuse * max(sun_light * 3.5, vec3<f32>(1.0));  // allow very bright light
    let moon_light = lighting.moon_color.rgb * 2.0 * max(dot(n, moon_normal), 0.0);
    diffuse = diffuse * max(moon_light, vec3<f32>(1.0));

    // The night colour: less saturation, more contrast.
    let night_mix = lighting.sun_faces.w * lighting.grading.w;
    let grey = dot(diffuse, LUMA);
    var night = diffuse - 0.6 * (diffuse - vec3<f32>(grey));
    night = (night - vec3<f32>(0.5)) * max(1.0 + 0.4 * lighting.moon_faces.w, 0.0) + vec3<f32>(0.5);
    diffuse = diffuse * (1.0 - night_mix) + night * night_mix;

    // The one light the Java shader lights per pixel: the focus entity, a Blinn-Phong light with a
    // fixed colour. The distance is at least a tenth of a block (Java divides by zero there).
    var local = vec3<f32>(0.0);
    if (lighting.local_light.w > 0.5) {
        var to_light = view_pos(lighting.local_light.xyz) - in.world;
        if (!billboard) {
            // `to_light` is in view space; a block's normal is in the world's frame.
            to_light = vec3<f32>(camera.view.x * to_light.x + camera.view.y * to_light.y, camera.view.x * to_light.y - camera.view.y * to_light.x, to_light.z);
        }
        let l = normalize(iso_to_game(to_light));
        let dist = max(length(to_light) * GAME_UNITS_PER_BLOCK, 0.1 * GAME_UNITS_PER_BLOCK);
        let view_dir = normalize(vec3<f32>(0.0, 0.5, 1.0));
        let h = normalize(l + view_dir);
        let specular = pow(max(dot(h, n), 0.0), 16.0);
        let color = vec3<f32>(0.3, 0.3, 0.2);
        local = (color * max(dot(n, l), 0.0) + color * specular) * (200000.0 / (dist * dist));
    }

    // The light baked from the static point lights and the occlusion are vertex data, like the Java
    // PointLightSource, which wrote them into the vertex colours.
    let ambient = lighting.pixel_ambient.rgb * diffuse;
    let baked = texel.rgb * in.albedo * in.baked.rgb * lighting.grading.z;
    let lit = (diffuse + ambient + local + baked) * in.baked.a;
    return with_fog(max(lit, vec3<f32>(0.0)), in.world);
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    // Sampled for every fragment (a level sample may sit in non-uniform control flow) and only used
    // when the vertex has a sprite. The sprites are cut out: pixels that are (nearly) transparent are
    // discarded, the rest keep their alpha, which the compositing of the layers blends.
    let texel = textureSampleLevel(atlas, atlas_sampler, in.uv, i32(max(in.layer, 0.0) + 0.5), 0.0);
    var color = vec4<f32>(in.color, 1.0);
    if (in.layer > -0.5) {
        color = vec4<f32>(texel.rgb * in.color, texel.a);
        if (lit_by_normal_map(i32(in.face + 0.5), in.layer)) {
            color = vec4<f32>(normal_map_color(in, texel), texel.a);
        }
    }
    let face = i32(in.face + 0.5);
    if (face == 3 || face == 7) {
        color.a = color.a * in.baked.a;  // markers, shadows, damage cracks and particles can fade
    }
    if (color.a <= peel.params.z) {
        discard;
    }
    // Peeling: drop what this or a nearer layer already shows. The margin keeps the surface
    // of the last layer from showing up again through rounding.
    if (peel.params.x > 0.5) {
        let behind = textureLoad(previous_depth, vec2<i32>(in.clip.xy), 0);
        if (in.clip.z - peel.params.y <= behind) {
            discard;
        }
    }
    if (lighting.flat_shades.w > 0.5) {
        // The colours above are display colours (the palette, the sprites and the light were made for
        // them). The layers are blended and tone mapped in linear light, so translucent surfaces and
        // the bloom mix the way light does; post.rs encodes it again at the end. A power keeps
        // `albedo * light` the same product on both sides, so the look does not change.
        color = vec4<f32>(pow(max(color.rgb, vec3<f32>(0.0)), vec3<f32>(2.2)), color.a);
    }
    return color;
}
