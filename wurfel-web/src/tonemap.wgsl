// The post-process pass that puts the picture on the screen (or into the FXAA pass, see fxaa.wgsl).
// The scene was drawn and its layers blended into an HDR texture (see peel.rs), where a lit colour
// can be brighter than 1; the bloom (bloom.wgsl) was made from that texture. This adds the bloom and
// maps the result onto the screen.
//
// `post.params`: x the bloom intensity, y 1 when the HDR texture holds linear light (0: display
// colours, like before the linear mode), z the depth of field strength (0 = off), w unused.
// `post.dof`: x screen pixels per unit of view depth (50 * zoom: one step of x + y moves the ground 50
// pixels down), y the view depth that is in focus (the player's, relative to the camera centre), zw unused.
//
// Depth of field (the miniature look): the nearest surface of every pixel has a depth (the depth buffer
// of the first peeling layer, see peel.rs), and the further it is from the focus plane the bigger its
// circle of confusion. Things in the focus band stay sharp, the rest is blurred by a spiral of taps on
// the HDR picture before the bloom is added. A tap only counts if its own circle reaches the pixel, so
// a sharp thing in the foreground does not smear over a blurry background. The view is orthographic,
// so the blur does not depend on the distance to a camera, only on the distance from the focus plane;
// it is measured in the pixels that distance would move the ground, so zooming keeps the look.

// The default curve: below KNEE nothing changes; above it the brightest channel is compressed
// towards 1 and the others follow, so the hue stays instead of shifting like it does when each
// channel is clipped alone. It works on display colours (linear light is encoded first), so the
// look is the same in both modes. wurfel_sim::light::tone_map is the reference implementation and
// a test checks that the constants agree.

const KNEE = 0.8;
const DESATURATION = 0.25;
// The display gamma. A plain power instead of the sRGB piece-wise curve: the palette and the sprites
// were made for display colours that are multiplied by light, and a power keeps that product the same
// in linear light ((a * l)^2.2 = a^2.2 * l^2.2), so switching the linear mode on does not change the look.
const GAMMA = 2.2;

// The focus band is this share of the screen height on each side of the focus plane; the blur grows to
// its full size over the next FOCUS_FALLOFF. (Measured in the pixels the depth would move the ground.)
const FOCUS_HALF = 0.14;
const FOCUS_FALLOFF = 0.36;
// The blur radius at full strength and on a 1080 pixel high screen (it scales with the height).
const BLUR_MAX = 14.0;
const BLUR_TAPS = 36;
const GOLDEN_ANGLE = 2.39996323;
// Window depth per unit of view depth, as in shader.wgsl (`depth * 0.002`).
const DEPTH_SCALE = 0.002;

struct Post {
    params: vec4<f32>,
    dof: vec4<f32>,
};

@group(0) @binding(0) var hdr: texture_2d<f32>;
@group(0) @binding(1) var bloom: texture_2d<f32>;
@group(0) @binding(2) var bloom_sampler: sampler;
@group(0) @binding(3) var<uniform> post: Post;
// The depth of the nearest surface of every pixel (1 where there is none).
@group(0) @binding(4) var depth: texture_depth_2d;

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    // The triangle (-1, -1), (3, -1), (-1, 3) covers the whole screen.
    let x = f32(i32(index & 1u) * 4 - 1);
    let y = f32(i32(index >> 1u) * 4 - 1);
    return vec4<f32>(x, y, 0.0, 1.0);
}

fn tone_map(input: vec3<f32>) -> vec3<f32> {
    let color = max(input, vec3<f32>(0.0));
    let peak = max(max(color.r, color.g), color.b);
    if (peak <= KNEE) {
        return color;
    }
    let room = 1.0 - KNEE;
    let over = peak - KNEE;
    let new_peak = KNEE + room * over / (over + room);
    let scaled = color * (new_peak / peak);
    let towards_white = 1.0 - 1.0 / (DESATURATION * (peak - new_peak) + 1.0);
    return min(mix(scaled, vec3<f32>(new_peak), towards_white), vec3<f32>(1.0));
}

fn encode(linear: vec3<f32>) -> vec3<f32> {
    return pow(max(linear, vec3<f32>(0.0)), vec3<f32>(1.0 / GAMMA));
}

// The radius, in pixels, of the circle of confusion of the pixel at `texel`.
fn blur_radius(texel: vec2<i32>, height: f32) -> f32 {
    let view_depth = (0.5 - textureLoad(depth, texel, 0)) / DEPTH_SCALE;
    let shift = abs(view_depth - post.dof.y) * post.dof.x;
    let off_focus = smoothstep(0.0, FOCUS_FALLOFF * height, shift - FOCUS_HALF * height);
    return post.params.z * off_focus * BLUR_MAX * height / 1080.0;
}

// A per-pixel number in 0..1 that does not change from frame to frame (interleaved gradient noise,
// Jimenez 2014). It turns every pixel's spiral of taps by a different angle, so what a thin blur
// misses shows as fine, steady grain and not as bands that shimmer when the picture scrolls.
fn pixel_noise(frag: vec2<f32>) -> f32 {
    return fract(52.9829189 * fract(dot(frag, vec2<f32>(0.06711056, 0.00583715))));
}

// The picture at the pixel, blurred by the depth of field.
fn focused(frag: vec2<f32>, size: vec2<f32>) -> vec3<f32> {
    let centre = vec2<i32>(frag);
    let sharp = textureLoad(hdr, centre, 0).rgb;
    let radius = blur_radius(centre, size.y);
    if (radius < 0.5) {
        return sharp;
    }
    let last = vec2<i32>(size) - vec2<i32>(1);
    let turn = pixel_noise(frag) * 6.2831853;
    var sum = sharp;
    var weight = 1.0;
    for (var i = 1; i < BLUR_TAPS; i = i + 1) {
        // A sunflower spiral: even coverage of the disc, and the angle turns a little each tap.
        let t = f32(i) / f32(BLUR_TAPS);
        let angle = f32(i) * GOLDEN_ANGLE + turn;
        let distance = sqrt(t) * radius;
        let at = frag + vec2<f32>(cos(angle), sin(angle)) * distance;
        // The depth comes from the texel under the tap, the colour is filtered between four texels:
        // each tap covers more than a pixel, which a spiral of only so many taps needs.
        let texel = clamp(vec2<i32>(floor(at)), vec2<i32>(0), last);
        let reach = smoothstep(0.0, 1.0, blur_radius(texel, size.y) / max(distance, 1.0));
        sum = sum + textureSampleLevel(hdr, bloom_sampler, at / size, 0.0).rgb * reach;
        weight = weight + reach;
    }
    return sum / weight;
}

@fragment
fn fs_main(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    // The HDR texture has the size of the screen, so the window position is the texel.
    let size = vec2<f32>(textureDimensions(hdr));
    var color = focused(frag.xy, size);
    color = color + textureSampleLevel(bloom, bloom_sampler, frag.xy / size, 0.0).rgb * post.params.x;

    var shown = tone_map(color);
    if (post.params.y > 0.5) {
        shown = tone_map(encode(color));
    }

    // Dither by a third of a step of the 8 bit canvas: the gradients of the sky and the fog would
    // band otherwise.
    let noise = fract(52.9829189 * fract(dot(frag.xy, vec2<f32>(0.06711056, 0.00583715))));
    shown = shown + (noise - 0.5) / 255.0;
    return vec4<f32>(shown, 1.0);
}
