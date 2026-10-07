// The post-process pass that puts the picture on the screen (or into the FXAA pass, see fxaa.wgsl).
// The scene was drawn and its layers blended into an HDR texture (see peel.rs), where a lit colour
// can be brighter than 1; the bloom (bloom.wgsl) was made from that texture. This adds the bloom and
// maps the result onto the screen.
//
// `post.params`: x the bloom intensity, y 1 when the HDR texture holds linear light (0: display
// colours, like before the linear mode), z 1 for the filmic curve, w unused.
//
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

struct Post {
    params: vec4<f32>,
};

@group(0) @binding(0) var hdr: texture_2d<f32>;
@group(0) @binding(1) var bloom: texture_2d<f32>;
@group(0) @binding(2) var bloom_sampler: sampler;
@group(0) @binding(3) var<uniform> post: Post;

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

// Narkowicz's fit of the ACES curve: a toe for the shadows and a long shoulder, on linear light.
fn aces(x: vec3<f32>) -> vec3<f32> {
    let c = max(x, vec3<f32>(0.0));
    return clamp((c * (2.51 * c + 0.03)) / (c * (2.43 * c + 0.59) + 0.14), vec3<f32>(0.0), vec3<f32>(1.0));
}

fn encode(linear: vec3<f32>) -> vec3<f32> {
    return pow(max(linear, vec3<f32>(0.0)), vec3<f32>(1.0 / GAMMA));
}

@fragment
fn fs_main(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    // The HDR texture has the size of the screen, so the window position is the texel.
    let size = vec2<f32>(textureDimensions(hdr));
    var color = textureLoad(hdr, vec2<i32>(frag.xy), 0).rgb;
    color = color + textureSampleLevel(bloom, bloom_sampler, frag.xy / size, 0.0).rgb * post.params.x;

    let linear = post.params.y > 0.5;
    var shown: vec3<f32>;
    if (post.params.z > 0.5) {
        var light = color;
        if (!linear) {
            light = pow(max(color, vec3<f32>(0.0)), vec3<f32>(GAMMA));
        }
        shown = encode(aces(light));
    } else if (linear) {
        shown = tone_map(encode(color));
    } else {
        shown = tone_map(color);
    }

    // Dither by a third of a step of the 8 bit canvas: the gradients of the sky and the fog would
    // band otherwise.
    let noise = fract(52.9829189 * fract(dot(frag.xy, vec2<f32>(0.06711056, 0.00583715))));
    shown = shown + (noise - 0.5) / 255.0;
    return vec4<f32>(shown, 1.0);
}
