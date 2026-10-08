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
    // The water mirror (reflection.rs): the water level, and 1 in the pass that draws the scene flipped
    // about it. Three scalars, not a vec3: a vec3 would be 16-byte aligned and make this struct 48
    // bytes, but the CPU side (`CameraUniform` in web.rs) is 32.
    mirror_level: f32,
    mirror_on: f32,
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
    sun_dir: vec4<f32>,      // xyz: unit vector towards the sun in the world's ground frame
    clouds: vec4<f32>,       // x: seconds drifted, y: shadow strength (0 = off), z: blocks per repeat, w: cloud height
    lights: array<vec4<f32>, 8>,        // xyz: position (blocks), w: radius
    light_colors: array<vec4<f32>, 8>,  // rgb: colour, w: brightness
};

@group(0) @binding(0) var<uniform> camera: Camera;
@group(0) @binding(1) var<uniform> lighting: Lighting;
// The cloud coverage (clouds.rs): 0 clear sky, 1 the heart of a cloud. Repeats.
@group(0) @binding(2) var cloud_map: texture_2d<f32>;
@group(0) @binding(3) var cloud_sampler: sampler;
// The sun's shadow map (sunshadow.rs, sunshadow.wgsl): the world seen from the sun, as depth.
struct SunShadow {
    right: vec4<f32>,   // xyz: the map's x axis in the world
    up: vec4<f32>,      // xyz: the map's y axis
    dir: vec4<f32>,     // xyz: unit vector towards the sun; w: depth per block along it
    center: vec4<f32>,  // xyz: the world point in the middle of the map; w: 1 / half the width in blocks
    params: vec4<f32>,  // x: strength 0..1 (0 = no shadows); y: a texel in blocks; z: map size in texels; w: 1 = blocks cast through the voxel grid
    grid_origin: vec4<f32>,  // xy: the ground cell that cell (0, 0) of the voxel grid is; z: tangent of the sun's angular radius (0 = hard shadows); w: steps of the ray walk
    grid_dims: vec4<f32>,    // xyz: size of the voxel grid in cells
    soft_quality: vec4<f32>, // x: steps of the soft tracing at most; y: part of a penumbra's width a step is at most; z: steps inside a block at most; w: layers of the grid with anything in them
};
@group(0) @binding(4) var<uniform> sun_shadow: SunShadow;
@group(0) @binding(5) var shadow_map: texture_depth_2d;
// The scene seen flipped about the water level (reflection.rs), drawn from the same camera; empty
// (alpha 0) where nothing is mirrored. A 1 x 1 blank in the pass that draws it.
@group(0) @binding(8) var mirror_image: texture_2d<f32>;
// The block world as opacities, 255 = solid (voxels.rs): the sun's ray is walked through it.
@group(0) @binding(6) var voxels: texture_3d<f32>;
@group(0) @binding(7) var voxel_sampler: sampler;
// The same as a signed distance field (voxels.rs `distance_field`): how far each cell is from the nearest block.
@group(0) @binding(9) var voxel_field: texture_3d<f32>;
// Which of the 27 cells around each are solid, one bit each (voxels.rs `neighbourhood`): an edge is one load.
@group(0) @binding(10) var neighbours: texture_3d<u32>;

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
    // The ambient occlusion of the four corners of the face (mesh.rs `pack_occlusion`): 0 when it has none.
    @location(6) occlusion: f32,
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
    @location(7) ground: vec3<f32>,  // the position in the world (not turned by the free camera)
    // How much of the vertex's light comes from the sun: what a sun shadow takes away.
    @location(8) sun_share: f32,
    // 1 on the surface of water (the fraction the mesher adds to the face id), which mirrors the scene.
    @location(9) @interpolate(flat) water: f32,
    // The four corners' occlusion counts of the face (2 bits each; -1: the face has none) and where in the
    // face this vertex is (0..1 along both sides), for the occlusion per pixel.
    @location(10) @interpolate(flat) ao_corners: f32,
    @location(11) ao_uv: vec2<f32>,
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
fn shade(v: VertexIn, seen: vec3<f32>, sun_share: ptr<function, f32>) -> vec3<f32> {
    let face = i32(v.shade.x + 0.5);
    *sun_share = 0.0;
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
    // A face with all four corners' occlusion gets it per pixel (`pixel_occlusion`), not per vertex.
    var ao = 1.0 - lighting.grading.y * clamp(v.shade.y, 0.0, 1.0);
    if (v.occlusion >= OCCLUSION_FLAG) {
        ao = 1.0;
    }
    let total_light = lit + point * lighting.grading.z;
    let color = v.color * (total_light * ao);
    let sun_light = lighting.sun_color.xyz * sun * lighting.grading.x;
    *sun_share = clamp(dot(sun_light, LUMA) / max(dot(total_light, LUMA), 0.0001), 0.0, 1.0);

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
    let mirroring = camera.mirror_on > 0.5;
    if (mirroring) {
        p.z = 2.0 * camera.mirror_level - p.z;
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
    // Flipped, the tops face down and the flat markers lie on the wrong side: both are not seen.
    let hidden_when_mirrored = mirroring && (face == 1 || face == 3);
    if (!faces_camera(face) || hidden_when_mirrored) {
        out.clip = vec4<f32>(2.0, 2.0, 2.0, 1.0);  // outside the view: the side that looks away
    }
    let seen = view_pos(v.position);
    var sun_share = 0.0;
    out.color = shade(v, seen, &sun_share);
    out.sun_share = sun_share;
    out.uv = v.uv;
    out.layer = v.layer;
    out.albedo = v.color;
    // A sprite's `point` is its screen offset, not light.
    var baked = v.point;
    if (face == 4) {
        baked = vec3<f32>(0.0);
    }
    out.baked = vec4<f32>(baked, 1.0 - lighting.grading.y * clamp(v.shade.y, 0.0, 1.0));
    out.ao_corners = -1.0;
    out.ao_uv = vec2<f32>(0.0);
    if (v.occlusion >= OCCLUSION_FLAG && face != 3 && face != 7) {
        let packed = u32(v.occlusion - OCCLUSION_FLAG + 0.5);
        let corner = packed >> 8u;
        out.ao_corners = f32(packed & 255u);
        // The corners go bottom/left, bottom/right, top/right, top/left.
        out.ao_uv = vec2<f32>(select(0.0, 1.0, corner == 1u || corner == 2u), select(0.0, 1.0, corner >= 2u));
        out.baked.a = 1.0;
    }
    if (face == 3 || face == 7) {
        // Unlit faces have no occlusion: `shade.y` is how see-through they are (0: opaque).
        out.baked.a = 1.0 - clamp(v.shade.y, 0.0, 1.0);
    }
    out.world = seen;
    out.face = f32(face);
    out.water = select(0.0, 1.0, v.shade.x - f32(face) > 0.1);
    out.ground = v.position;
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
// The way a face of the given id looks, in the world (not turned by the free camera).
fn face_normal(face: i32, to_sun: vec3<f32>) -> vec3<f32> {
    if (face == 0) {
        return vec3<f32>(0.0, 1.0, 0.0);
    }
    if (face == 1) {
        return vec3<f32>(0.0, 0.0, 1.0);
    }
    if (face == 2) {
        return vec3<f32>(1.0, 0.0, 0.0);
    }
    if (face == 5) {
        return vec3<f32>(0.0, -1.0, 0.0);
    }
    if (face == 6) {
        return vec3<f32>(-1.0, 0.0, 0.0);
    }
    return to_sun;  // a standing sprite has no particular side
}

// How much of the sun reaches `pos` through the shadow map: 1 in the light, 0 in shadow, in between
// at the soft edge (about five map texels wide).
fn map_visibility(pos: vec3<f32>, face: i32, reach: i32) -> f32 {
    let to_sun = sun_shadow.dir.xyz;
    let n = face_normal(face, to_sun);
    let texel = sun_shadow.params.y;
    // A surface that is turned away from the sun compares against a depth that changes quickly across
    // a texel: the bias grows with the slope (tan of the angle between the normal and the sun).
    let cos_angle = clamp(dot(n, to_sun), 0.0, 1.0);
    let slope = min(sqrt(max(1.0 - cos_angle * cos_angle, 0.0)) / max(cos_angle, 0.1), 4.0);
    let rel = pos + n * texel * 1.5 - sun_shadow.center.xyz;
    let uv = vec2<f32>(
        dot(rel, sun_shadow.right.xyz) * sun_shadow.center.w * 0.5 + 0.5,
        0.5 - dot(rel, sun_shadow.up.xyz) * sun_shadow.center.w * 0.5,
    );
    if (uv.x <= 0.0 || uv.x >= 1.0 || uv.y <= 0.0 || uv.y >= 1.0) {
        return 1.0;  // outside the map: nothing is known, so no shadow
    }
    let depth = 0.5 - dot(rel, to_sun) * sun_shadow.dir.w;
    let bias = (texel * (1.0 + slope) + 0.02) * sun_shadow.dir.w;
    let size = sun_shadow.params.z;
    let last = i32(size) - 1;
    // The comparisons of a 6 x 6 block of texels, weighed so that the 5 x 5 window slides smoothly across
    // texel borders: the edge of a shadow moves by fractions of a texel, and the jumps of the map's own
    // rasterised edge (which change as the sun turns) are spread over five texels instead of three.
    let position = uv * size - vec2<f32>(0.5);
    let corner = vec2<i32>(floor(position));
    let fraction = position - floor(position);
    var lit = 0.0;
    for (var dy = -reach; dy <= reach + 1; dy = dy + 1) {
        var wy = 1.0;
        if (dy == -reach) {
            wy = 1.0 - fraction.y;
        } else if (dy == reach + 1) {
            wy = fraction.y;
        }
        for (var dx = -reach; dx <= reach + 1; dx = dx + 1) {
            var wx = 1.0;
            if (dx == -reach) {
                wx = 1.0 - fraction.x;
            } else if (dx == reach + 1) {
                wx = fraction.x;
            }
            let at = clamp(corner + vec2<i32>(dx, dy), vec2<i32>(0), vec2<i32>(last));
            if (depth <= textureLoad(shadow_map, at, 0) + bias) {
                lit = lit + wx * wy;
            }
        }
    }
    let width = f32(2 * reach + 1);
    return lit / (width * width);
}

// Soft shadows from the shadow map (percentage-closer soft shadows): a search for what blocks the sun in a
// small window finds how far the blocker is, which sets how wide the sun's disc is there (`2 * soft * distance`),
// and that is the width of the window the lit share is counted over. `soft` is the tangent of the sun's angular
// radius, as for the voxel method. A fixed number of reads: 16 for the search and 25 for the filter.
const SEARCH_BLOCKS = 12.0;  // how far from the receiver a blocker is looked for, in blocks along the sun's ray
fn map_visibility_soft(pos: vec3<f32>, face: i32, soft: f32) -> f32 {
    let to_sun = sun_shadow.dir.xyz;
    let n = face_normal(face, to_sun);
    let texel = sun_shadow.params.y;
    let cos_angle = clamp(dot(n, to_sun), 0.0, 1.0);
    let slope = min(sqrt(max(1.0 - cos_angle * cos_angle, 0.0)) / max(cos_angle, 0.1), 4.0);
    let rel = pos + n * texel * 1.5 - sun_shadow.center.xyz;
    let uv = vec2<f32>(
        dot(rel, sun_shadow.right.xyz) * sun_shadow.center.w * 0.5 + 0.5,
        0.5 - dot(rel, sun_shadow.up.xyz) * sun_shadow.center.w * 0.5,
    );
    if (uv.x <= 0.0 || uv.x >= 1.0 || uv.y <= 0.0 || uv.y >= 1.0) {
        return 1.0;
    }
    let depth = 0.5 - dot(rel, to_sun) * sun_shadow.dir.w;
    let bias = (texel * (1.0 + slope) + 0.02) * sun_shadow.dir.w;
    let size = sun_shadow.params.z;
    let last = vec2<i32>(i32(size) - 1);
    let centre = uv * size;
    // The blocker search: a window as wide as the penumbra of a blocker `SEARCH_BLOCKS` away.
    let reach = clamp(soft * SEARCH_BLOCKS / texel, 2.0, 24.0);
    var blockers = 0.0;
    var count = 0.0;
    for (var j = 0; j < 4; j = j + 1) {
        for (var i = 0; i < 4; i = i + 1) {
            let offset = ((vec2<f32>(f32(i), f32(j)) + 0.5) / 4.0 - 0.5) * 2.0 * reach;
            let at = clamp(vec2<i32>(floor(centre + offset)), vec2<i32>(0), last);
            let z = textureLoad(shadow_map, at, 0);
            if (z < depth - bias) {
                blockers = blockers + z;
                count = count + 1.0;
            }
        }
    }
    if (count < 0.5) {
        return 1.0;
    }
    let distance = (depth - blockers / count) / sun_shadow.dir.w;
    let width = clamp(2.0 * soft * distance / texel, 2.0, 2.0 * reach);
    var lit = 0.0;
    for (var j = 0; j < 5; j = j + 1) {
        for (var i = 0; i < 5; i = i + 1) {
            let offset = ((vec2<f32>(f32(i), f32(j)) + 0.5) / 5.0 - 0.5) * width;
            let at = clamp(vec2<i32>(floor(centre + offset)), vec2<i32>(0), last);
            if (depth <= textureLoad(shadow_map, at, 0) + bias) {
                lit = lit + 1.0;
            }
        }
    }
    return lit / 25.0;
}

// The ray of one axis of the cell walk: x the distance along the ray to the next cell border, y the
// distance for one whole cell, and the second function the direction of the step.
fn ray_axis(q: f32, d: f32) -> vec2<f32> {
    if (abs(d) < 0.000001) {
        return vec2<f32>(1.0e30, 1.0e30);
    }
    let edge = select(floor(q), floor(q) + 1.0, d > 0.0);
    return vec2<f32>((edge - q) / d, 1.0 / abs(d));
}

fn ray_step(d: f32) -> i32 {
    return select(-1, 1, d > 0.0);
}

// How much of the sun reaches `pos` through the blocks.
//
// A sun that is a point (softness 0): the ray towards it is walked cell by cell through the voxel grid
// (Amanatides and Woo), and a solid cell ends it. The edges are exact; water lets part of the light through.
//
// A sun that is a disc: a shadow is sharp where it starts and blurs with the distance from what casts it.
// The ray is sphere traced through the distance field (it jumps by the distance to the nearest block), and
// of everything it passes it keeps how much of the sun's disc, which is `2 * soft * t` wide after t, the
// nearest block hides: `0.5 + 0.5 * d / (soft * t)` for a block `d` away from the ray. That is smooth in
// the receiver's position, so the penumbra has no steps. voxels.rs `soft_visibility` is the same on the CPU.
const SPHERE_STEP = 0.5;
const MIN_STEP = 0.03;
// (The steps of the tracing and how fine it looks inside a penumbra, so that the closest approach between
// two samples is hardly missed and the edge does not crawl when the sun moves, come from the voxel quality:
// `soft_quality` of the uniform.)
// A surface's own blur must not reach its own block: the softness is at most this times the cosine of
// the angle between its normal and the sun.
const SELF_CLEARANCE = 0.95;
// Steps inside a block at most (how deep the ray goes is looked for there) and the shortest of them.
const INSIDE_MIN_STEP = 0.03;

// The field at `p`, made exact near the surface for the soft walk (voxels.rs `field_at`).
//
// The field is blended trilinearly from the cell centres, which cannot show the kink at an edge: at a convex
// corner it reads about 0.3 too far, at a concave one too near, even negative in the air. So within a cell of
// a block the exact distances are used.
//
// In the air: the distance to the boxes of the solid cells in the octant of the cell that `p` is nearer to.
// In a block: the sideways distance the ray has to move to leave, over the faces of the cell open to the air.
const EXACT_MARGIN = 0.35;  // the field is off by 0.3 at an edge; further than the sun is wide plus this, the light is saturated
const MIN_ACROSS = 0.25;
// Is the cell at this offset from the middle of the 27 solid? (voxels.rs `neighbourhood`.)
fn solid_around(mask: u32, offset: vec3<i32>) -> bool {
    let k = u32((offset.z + 1) * 9 + (offset.y + 1) * 3 + (offset.x + 1));
    return ((mask >> k) & 1u) != 0u;
}

fn field_at(p: vec3<f32>, size: vec3<f32>, to_sun: vec3<f32>, width: f32) -> f32 {
    // Both loads depend on `p` only, so they are in flight together: the trace is a chain of waits for the
    // texture unit, and a second load that has to wait for the first makes every step twice as long.
    let cell = vec3<i32>(floor(p));
    let d = textureSampleLevel(voxel_field, voxel_sampler, p / size, 0.0).r;
    let mask = textureLoad(neighbours, cell, 0).r;
    // Far from every block (a solid cell's value is below the margin: the field is off by 0.3 at most): no look.
    if (d >= min(width + EXACT_MARGIN, 1.0)) {
        return d;
    }
    let air = ((mask >> 13u) & 1u) == 0u;  // bit 13 is the cell itself
    if (air) {
        if (mask == 0u) {
            return d;  // nothing solid around the cell
        }
        // Only the cells on the side of the cell that `p` is nearer to: the 7 other cells of this octant are
        // enough. The gap to the neighbour on a side is the distance to the face between them, and a diagonal
        // neighbour is as far as the gaps of its sides together (Pythagoras).
        let side = select(vec3<i32>(1), vec3<i32>(-1), p - vec3<f32>(cell) < vec3<f32>(0.5));
        let into = p - vec3<f32>(cell);
        let g = select(1.0 - into, into, side < vec3<i32>(0));
        let g2 = g * g;
        let far = 1.0e30;
        var nearest = far;
        nearest = min(nearest, select(far, g2.x, solid_around(mask, vec3<i32>(side.x, 0, 0))));
        nearest = min(nearest, select(far, g2.y, solid_around(mask, vec3<i32>(0, side.y, 0))));
        nearest = min(nearest, select(far, g2.z, solid_around(mask, vec3<i32>(0, 0, side.z))));
        nearest = min(nearest, select(far, g2.x + g2.y, solid_around(mask, vec3<i32>(side.x, side.y, 0))));
        nearest = min(nearest, select(far, g2.x + g2.z, solid_around(mask, vec3<i32>(side.x, 0, side.z))));
        nearest = min(nearest, select(far, g2.y + g2.z, solid_around(mask, vec3<i32>(0, side.y, side.z))));
        nearest = min(nearest, select(far, g2.x + g2.y + g2.z, solid_around(mask, side)));
        if (nearest < 1.0) {
            return max(sqrt(nearest), 0.0001);
        }
        return d;
    }
    let inside = p - vec3<f32>(cell);
    var leave = 1.0e30;
    for (var axis = 0; axis < 3; axis = axis + 1) {
        var unit = vec3<i32>(0);
        unit[axis] = 1;
        let across = max(sqrt(max(1.0 - to_sun[axis] * to_sun[axis], 0.0)), MIN_ACROSS);
        if (!solid_around(mask, -unit)) {
            leave = min(leave, inside[axis] / across);
        }
        if (!solid_around(mask, unit)) {
            leave = min(leave, (1.0 - inside[axis]) / across);
        }
    }
    return min(min(d, -min(leave, 32.0)), -0.001);
}

fn voxel_visibility(pos: vec3<f32>, face: i32) -> f32 {
    let to_sun = sun_shadow.dir.xyz;
    let n = face_normal(face, to_sun);
    let facing = dot(n, to_sun);
    if (facing <= 0.0) {
        return 1.0;  // turned away from the sun: it has no sun light to take away
    }
    let dims = vec3<i32>(sun_shadow.grid_dims.xyz);
    let size = vec3<f32>(dims);
    // A cell of the grid is a block: the centres are on whole numbers, so a cell starts half a block lower.
    let start = pos + n * 0.02 + to_sun * 0.02;
    let q = start + vec3<f32>(0.5, 0.5, 0.0) - vec3<f32>(sun_shadow.grid_origin.xy, 0.0);
    let soft = min(sun_shadow.grid_origin.z, SELF_CLEARANCE * facing);

    if (soft > 0.001) {
        let soft_steps = i32(sun_shadow.soft_quality.x);
        let width_step = sun_shadow.soft_quality.y;
        let inside_steps = i32(sun_shadow.soft_quality.z);
        let top = sun_shadow.soft_quality.w;
        var visible = 1.0;
        var t = 0.0;
        for (var i = 0; i < soft_steps; i = i + 1) {
            let p = q + to_sun * t;
            if (p.z >= top || p.z < 0.0 || p.x < 0.0 || p.y < 0.0 || p.x >= size.x || p.y >= size.y) {
                break;  // out of the grid: above the highest block, or nothing is known out there
            }
            let d = field_at(p, size, to_sun, soft * t);
            if (d <= 0.0) {
                // The ray is in a block. How deep it goes decides how much of the sun's disc is hidden (a graze
                // hides half of it, a ray that goes through hides all): go on inside until the deepest point is
                // known, so the shadow is continuous across the edge of where the ray hits.
                var deepest = d;
                var deepest_t = t;
                var inside_t = t;
                for (var k = 0; k < inside_steps; k = k + 1) {
                    if (0.5 + 0.5 * deepest / (soft * max(deepest_t, 0.001)) <= 0.0) {
                        break;
                    }
                    inside_t = inside_t + max(-deepest * SPHERE_STEP, INSIDE_MIN_STEP);
                    let inside_p = q + to_sun * inside_t;
                    if (inside_p.z >= top || inside_p.z < 0.0 || inside_p.x < 0.0 || inside_p.y < 0.0 || inside_p.x >= size.x || inside_p.y >= size.y) {
                        break;
                    }
                    let inside = field_at(inside_p, size, to_sun, soft * t);
                    if (inside < deepest) {
                        deepest = inside;
                        deepest_t = inside_t;
                    }
                    if (inside > 0.0) {
                        break;
                    }
                }
                return smoothstep(0.0, 1.0, min(visible, clamp(0.5 + 0.5 * deepest / (soft * max(deepest_t, 0.001)), 0.0, 1.0)));
            }
            // The closest the ray comes to a block is between two samples at most half a step off, so the
            // shadow is within a few percent of the exact value everywhere.
            visible = min(visible, clamp(0.5 + 0.5 * d / (soft * max(t, 0.001)), 0.0, 1.0));
            var step = d * SPHERE_STEP;
            if (d < 2.0 * soft * t) {
                step = min(step, soft * t * width_step);  // near the edge of a shadow: look closer
            }
            t = t + max(step, MIN_STEP);
        }
        // The coverage of the disc by an edge is an S over its width, not a line: no corner at either end.
        return smoothstep(0.0, 1.0, visible);
    }

    let dims_top = i32(sun_shadow.soft_quality.w);
    var cell = vec3<i32>(floor(q));
    let x = ray_axis(q.x, to_sun.x);
    let y = ray_axis(q.y, to_sun.y);
    let z = ray_axis(q.z, to_sun.z);
    var t = vec3<f32>(x.x, y.x, z.x);
    let delta = vec3<f32>(x.y, y.y, z.y);
    let dir_step = vec3<i32>(ray_step(to_sun.x), ray_step(to_sun.y), ray_step(to_sun.z));
    var transmittance = 1.0;
    let steps = i32(sun_shadow.grid_origin.w);
    for (var i = 0; i < steps; i = i + 1) {
        if (cell.z >= dims_top || cell.z < 0 || cell.x < 0 || cell.y < 0 || cell.x >= dims.x || cell.y >= dims.y) {
            break;
        }
        transmittance = transmittance * (1.0 - textureLoad(voxels, cell, 0).r);
        if (transmittance < 0.02) {
            return 0.0;
        }
        if (t.x < t.y && t.x < t.z) {
            cell.x = cell.x + dir_step.x;
            t.x = t.x + delta.x;
        } else if (t.y < t.z) {
            cell.y = cell.y + dir_step.y;
            t.y = t.y + delta.y;
        } else {
            cell.z = cell.z + dir_step.z;
            t.z = t.z + delta.z;
        }
    }
    return transmittance;
}

// How much of the sun reaches `pos`: 1 in the light, 0 in shadow. The blocks' shadows come from the
// shadow map or, with the voxel method, from the grid; the map then only holds the standing sprites.
fn sun_visibility(pos: vec3<f32>, face: i32) -> f32 {
    let strength = sun_shadow.params.x;
    if (strength <= 0.0 || face == 3 || face == 7) {
        return 1.0;
    }
    // The map holds everything (a 5 x 5 window) or, with the voxel method, only the sprites, whose shadows
    // are soft anyway (3 x 3: a quarter of the taps).
    var seen = 1.0;
    if (sun_shadow.params.w > 0.5) {
        seen = min(map_visibility(pos, face, 1), voxel_visibility(pos, face));
    } else if (sun_shadow.grid_origin.z > 0.001) {
        seen = map_visibility_soft(pos, face, sun_shadow.grid_origin.z);
    } else {
        seen = map_visibility(pos, face, 2);
    }
    return mix(1.0, seen, strength);
}

// Added to `Vertex::occlusion` of a face that has its corners' occlusion in it (mesh.rs `OCCLUSION_FLAG`).
const OCCLUSION_FLAG = 1024.0;

// The ambient occlusion factor at this pixel: the occlusion of the four corners of the face, mixed
// bilinearly by where the pixel is on the face. Mixing per vertex over the two triangles of the quad
// instead gives triangular blotches wherever only one corner is occluded.
fn pixel_occlusion(in: VertexOut) -> f32 {
    if (in.ao_corners < 0.0) {
        return 1.0;  // the face has none: the vertex path applied it
    }
    let bits = u32(in.ao_corners + 0.5);
    let a0 = f32(bits & 3u) / 3.0;
    let a1 = f32((bits >> 2u) & 3u) / 3.0;
    let a2 = f32((bits >> 4u) & 3u) / 3.0;
    let a3 = f32((bits >> 6u) & 3u) / 3.0;
    let dark = mix(mix(a0, a1, in.ao_uv.x), mix(a3, a2, in.ao_uv.x), in.ao_uv.y);
    return 1.0 - lighting.grading.y * dark;
}

fn lit_by_normal_map(face: i32, layer: f32) -> bool {
    return lighting.misc.w > 0.5 && layer > -0.5 && face != 3 && face != 7;
}

fn normal_map_color(in: VertexOut, texel: vec4<f32>, sun_seen: f32, occlusion: f32) -> vec3<f32> {
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
    // The sides that look away from the fixed camera (-y and -x, only meshed for the free camera) wear the
    // pictures of the opposite sides, and so their normal maps: the normal on them points the other way
    // round the vertical axis. Without this a turned camera lights these sides as if they faced the
    // opposite way, and the light no longer fits the shadows.
    let side = i32(in.face + 0.5);
    if (side == 5 || side == 6) {
        n = vec3<f32>(-n.x, -n.y, n.z);
    }

    // Clamp the sun light at white, so during the day it appears white, not yellow.
    let sun_light = min(lighting.sun_color.rgb * 2.0, vec3<f32>(1.0)) * max(dot(n, sun_normal), 0.0) * sun_seen;
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
    let lit = (diffuse + ambient + local + baked) * in.baked.a * occlusion;
    return with_fog(max(lit, vec3<f32>(0.0)), in.world);
}

// ------------------------------------------------------------------------------ cloud shadows
// Clouds drift over the world and block part of the sun. The sun's ray from the surface point is
// followed up to the cloud layer and the coverage is read there, so a shadow slides when the sun
// moves and a tall wall's shadow falls away from the sun. Like the sun's shadow map this takes away
// the sun's share of the light only (it multiplies `sun_visibility`), so a cloud adds nothing to a
// place the sun does not reach anyway, and a shaded place darkens to the ambient light, not black.
const WIND = vec2<f32>(0.8, 0.6);

// How much of the sun gets through the clouds to `ground`: 1 under a clear sky.
fn cloud_light(ground: vec3<f32>, face: i32) -> f32 {
    let strength = lighting.clouds.y;
    let sun = lighting.sun_dir.xyz;
    if (strength <= 0.0 || sun.z <= 0.05 || face == 3 || face == 7) {
        return 1.0;
    }
    let rise = max(lighting.clouds.w - ground.z, 0.0) / sun.z;
    let at_cloud = ground.xy + sun.xy * rise - WIND * lighting.clouds.x * 0.6;
    let coverage = textureSampleLevel(cloud_map, cloud_sampler, at_cloud / lighting.clouds.z, 0.0).r;
    // Low sun: the shadows are long and faint. (At night the sun's share is zero anyway.)
    let sun_power = smoothstep(0.05, 0.4, sun.z);
    return 1.0 - coverage * strength * sun_power;
}

// ---------------------------------------------------------------------------- water reflection
// The surface of water mirrors the scene (reflection.rs) and the sun. The normal (the water's normal map
// plus a smooth noise swell) bends where the mirror image is read; Schlick's Fresnel term lets the
// reflection grow towards the horizon, and the water's own colour shows through where it is low.
const WATER_REFLECTIVITY = 0.55;
const WATER_MAP_TILT = 0.6;   // how much of the normal map's slope the reflection follows
const WATER_SWELL = 0.12;     // how steep the noise swell is

// Smooth value noise: random values on a grid, blended with a smoothstep so there are no creases.
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

// The height of the drifting swell: two layers of smooth noise sliding in different directions.
fn swell(ground: vec2<f32>, time: f32) -> f32 {
    return smooth_noise(ground * 1.7 + vec2<f32>(0.35, 0.2) * time) * 0.65
        + smooth_noise(ground * 3.9 - vec2<f32>(0.25, 0.4) * time + vec2<f32>(17.0, 5.0)) * 0.35;
}

// The normal of the water in the world's frame: the normal map of the water's picture (the ripples
// painted into the atlas, which move with its frames) tilted further by the swell.
fn wave_normal(in: VertexOut, time: f32) -> vec3<f32> {
    var tilt = vec2<f32>(0.0);
    if (in.layer > -0.5) {
        let c = textureSampleLevel(normals, atlas_sampler, in.uv, i32(in.layer + 0.5), 0.0).rgb;
        var m = normalize(c * 2.0 - vec3<f32>(1.0));
        m.x = -m.x;  // x is flipped in the texture, as in normal_map_color
        let ground = game_to_iso(m);
        tilt = ground.xy / max(ground.z, 0.2);
    }
    let e = 0.05;
    let h = swell(in.ground.xy, time);
    let slope = vec2<f32>(swell(in.ground.xy + vec2<f32>(e, 0.0), time) - h, swell(in.ground.xy + vec2<f32>(0.0, e), time) - h) / e;
    return normalize(vec3<f32>(tilt * WATER_MAP_TILT - slope * WATER_SWELL, 1.0));
}

fn water_reflection(color: vec3<f32>, in: VertexOut, sun_seen: f32) -> vec3<f32> {
    let n = wave_normal(in, lighting.clouds.x);
    // The view ray (1, 1, 0.82) mirrored about the tilted normal, in the view's frame, then turned back
    // to the world's frame (the free camera turned the world by the yaw).
    let incoming = -normalize(vec3<f32>(1.0, 1.0, 0.82));
    let mirrored = reflect(incoming, n);
    let c = camera.view.x;
    let s = camera.view.y;
    let dir = vec3<f32>(c * mirrored.x + s * mirrored.y, c * mirrored.y - s * mirrored.x, mirrored.z);
    let facing = clamp(dot(n, -incoming), 0.0, 1.0);
    let fresnel = 0.04 + 0.96 * pow(1.0 - facing, 5.0);
    let amount = clamp(fresnel * 3.0 + 0.2, 0.0, 1.0) * WATER_REFLECTIVITY;
    var reflected = color;
    // The mirrored scene over the water's colour, read at this pixel: the waves shift it a little. It only
    // belongs to the water at the level it was flipped about.
    let size = vec2<f32>(textureDimensions(mirror_image));
    if (size.x > 1.5 && abs(in.ground.z - camera.mirror_level) < 0.05) {
        let zoom = camera.scale.x * size.x * 0.5;
        let at = clamp(vec2<i32>(in.clip.xy + n.xy * 40.0 * zoom), vec2<i32>(0), vec2<i32>(size) - vec2<i32>(1));
        var image = textureLoad(mirror_image, at, 0);
        if (lighting.flat_shades.w > 0.5) {
            image = vec4<f32>(pow(max(image.rgb, vec3<f32>(0.0)), vec3<f32>(1.0 / 2.2)), image.a);
        }
        // Seen through water: a little darker and bluer. Where nothing is mirrored the water stays as it is.
        reflected = mix(reflected, image.rgb * vec3<f32>(0.85, 0.93, 1.0), image.a);
    }
    // The sun's glint, where the mirrored ray runs into it (not under a cloud or a shadow).
    let sun = lighting.sun_dir.xyz;
    let glint = pow(max(dot(dir, sun), 0.0), 220.0) * step(0.02, sun.z);
    reflected = reflected + lighting.sun_color.xyz * glint * 4.0 * sun_seen;
    return mix(color, reflected, amount);
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    // Sampled for every fragment (a level sample may sit in non-uniform control flow) and only used
    // when the vertex has a sprite. The sprites are cut out: pixels that are (nearly) transparent are
    // discarded, the rest keep their alpha, which the compositing of the layers blends.
    let texel = textureSampleLevel(atlas, atlas_sampler, in.uv, i32(max(in.layer, 0.0) + 0.5), 0.0);
    let face = i32(in.face + 0.5);
    // What gets thrown away is decided first: the shadows below are the most expensive thing this shader does,
    // and the peeling draws the scene once per layer, so most of what a layer discards (everything the nearer
    // layer shows) used to be traced for nothing. The alpha is what the colour's alpha is below.
    var alpha = 1.0;
    if (in.layer > -0.5) {
        alpha = texel.a;
    }
    if (face == 3 || face == 7) {
        alpha = alpha * in.baked.a;  // markers, shadows, damage cracks and particles can fade
    }
    if (alpha <= peel.params.z) {
        discard;
    }
    // The mirror pass keeps what is above the water: blocks by their height (a side that reaches below
    // the water is cut there), pictures and particles by the point they stand on.
    if (camera.mirror_on > 0.5) {
        var floor_z = camera.mirror_level + 0.01;
        if (face == 4 || face == 7) {
            floor_z = camera.mirror_level - 0.2;
        }
        if (in.ground.z < floor_z) {
            discard;
        }
    }
    // Peeling: drop what this or a nearer layer already shows. The margin keeps the surface
    // of the last layer from showing up again through rounding.
    if (peel.params.x > 0.5) {
        let behind = textureLoad(previous_depth, vec2<i32>(in.clip.xy), 0);
        if (in.clip.z - peel.params.y <= behind) {
            discard;
        }
    }
    // 1: the sun reaches this point, 0: something is in the way. Unlit things (markers, particles) have no sun.
    let sun_seen = sun_visibility(in.ground, face) * cloud_light(in.ground, face);
    // The vertex colour holds the sun's light already; a shadow takes the sun's share of it away.
    let shadowed = 1.0 - in.sun_share * (1.0 - sun_seen);
    let occlusion = pixel_occlusion(in);
    var color = vec4<f32>(in.color * shadowed * occlusion, 1.0);
    if (in.layer > -0.5) {
        color = vec4<f32>(texel.rgb * in.color * shadowed * occlusion, texel.a);
        if (lit_by_normal_map(face, in.layer)) {
            color = vec4<f32>(normal_map_color(in, texel, sun_seen, occlusion), texel.a);
        }
    }
    if (in.water > 0.5) {
        color = vec4<f32>(water_reflection(color.rgb, in, sun_seen), color.a);
    }
    if (face == 3 || face == 7) {
        color.a = color.a * in.baked.a;
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
