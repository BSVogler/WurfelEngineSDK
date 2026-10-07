// The last pass: the scene was drawn and its layers blended into an HDR texture (see peel.rs), where
// a lit colour can be brighter than 1. This maps it onto the screen. Below KNEE nothing changes; above
// it the brightest channel is compressed towards 1 and the others follow, so the hue stays instead of
// shifting like it does when each channel is clipped alone. wurfel_sim::light::tone_map is the
// reference implementation and a test checks that the constants agree.

const KNEE = 0.8;
const DESATURATION = 0.25;

@group(0) @binding(0) var hdr: texture_2d<f32>;

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

@fragment
fn fs_main(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    // The HDR texture has the size of the screen, so the window position is the texel.
    let texel = textureLoad(hdr, vec2<i32>(frag.xy), 0);
    return vec4<f32>(tone_map(texel.rgb), 1.0);
}
