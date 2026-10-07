// The glow of very bright things (a torch, the sun on a shiny top, an explosion): the HDR picture is
// cut at a threshold, blurred while it is made smaller step by step, and added back while it is made
// larger again (tonemap.wgsl adds the result to the picture). Three entry points, one per pass:
//
//   fs_prefilter  picture -> level 0 (half size): keeps what is brighter than THRESHOLD
//   fs_down       level n -> level n + 1 (half size again), a 13 tap blur
//   fs_up         level n + 1 -> level n, a tent blur; the pipeline adds it onto what level n holds
//
// Every pass reads one texture with a linear sampler, so one bilinear tap covers four texels.

const THRESHOLD = 1.0;
// The width of the soft edge below the threshold (no hard line where things start to glow).
const SOFT = 0.35;
// A single very bright texel (a sparkle) must not flicker the whole glow: bright samples are weighed
// down by their brightness before they are averaged (Karis).
const MAX_SAMPLE = 8.0;

@group(0) @binding(0) var source: texture_2d<f32>;
@group(0) @binding(1) var source_sampler: sampler;

struct Vertex {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> Vertex {
    // The triangle (-1, -1), (3, -1), (-1, 3) covers the whole target.
    let x = f32(i32(index & 1u) * 4 - 1);
    let y = f32(i32(index >> 1u) * 4 - 1);
    var out: Vertex;
    out.position = vec4<f32>(x, y, 0.0, 1.0);
    out.uv = vec2<f32>((x + 1.0) * 0.5, (1.0 - y) * 0.5);
    return out;
}

fn tap(uv: vec2<f32>) -> vec3<f32> {
    return min(textureSampleLevel(source, source_sampler, uv, 0.0).rgb, vec3<f32>(MAX_SAMPLE));
}

fn bright_part(color: vec3<f32>) -> vec3<f32> {
    let peak = max(max(color.r, color.g), color.b);
    let knee = clamp(peak - THRESHOLD + SOFT, 0.0, 2.0 * SOFT);
    let soft = knee * knee / (4.0 * SOFT);
    let weight = max(soft, peak - THRESHOLD) / max(peak, 1e-4);
    return color * weight;
}

fn karis(color: vec3<f32>) -> f32 {
    return 1.0 / (1.0 + dot(color, vec3<f32>(0.2126, 0.7152, 0.0722)));
}

@fragment
fn fs_prefilter(in: Vertex) -> @location(0) vec4<f32> {
    let px = 1.0 / vec2<f32>(textureDimensions(source));
    // Four bilinear taps = the 4x4 texels that this half size texel covers.
    let a = bright_part(tap(in.uv + px * vec2<f32>(-1.0, -1.0)));
    let b = bright_part(tap(in.uv + px * vec2<f32>(1.0, -1.0)));
    let c = bright_part(tap(in.uv + px * vec2<f32>(-1.0, 1.0)));
    let d = bright_part(tap(in.uv + px * vec2<f32>(1.0, 1.0)));
    let wa = karis(a);
    let wb = karis(b);
    let wc = karis(c);
    let wd = karis(d);
    let sum = (a * wa + b * wb + c * wc + d * wd) / (wa + wb + wc + wd);
    return vec4<f32>(sum, 1.0);
}

@fragment
fn fs_down(in: Vertex) -> @location(0) vec4<f32> {
    let px = 1.0 / vec2<f32>(textureDimensions(source));
    let uv = in.uv;
    // The 13 tap filter of the Call of Duty: Advanced Warfare talk: five overlapping 2x2 boxes.
    let a = tap(uv + px * vec2<f32>(-2.0, 2.0));
    let b = tap(uv + px * vec2<f32>(0.0, 2.0));
    let c = tap(uv + px * vec2<f32>(2.0, 2.0));
    let d = tap(uv + px * vec2<f32>(-2.0, 0.0));
    let e = tap(uv);
    let f = tap(uv + px * vec2<f32>(2.0, 0.0));
    let g = tap(uv + px * vec2<f32>(-2.0, -2.0));
    let h = tap(uv + px * vec2<f32>(0.0, -2.0));
    let i = tap(uv + px * vec2<f32>(2.0, -2.0));
    let j = tap(uv + px * vec2<f32>(-1.0, 1.0));
    let k = tap(uv + px * vec2<f32>(1.0, 1.0));
    let l = tap(uv + px * vec2<f32>(-1.0, -1.0));
    let m = tap(uv + px * vec2<f32>(1.0, -1.0));
    let color = e * 0.125 + (a + c + g + i) * 0.03125 + (b + d + f + h) * 0.0625 + (j + k + l + m) * 0.125;
    return vec4<f32>(color, 1.0);
}

@fragment
fn fs_up(in: Vertex) -> @location(0) vec4<f32> {
    // The source is the smaller level: one of its texels is two of the target's.
    let px = 1.0 / vec2<f32>(textureDimensions(source));
    let uv = in.uv;
    let a = tap(uv + px * vec2<f32>(-1.0, 1.0));
    let b = tap(uv + px * vec2<f32>(0.0, 1.0));
    let c = tap(uv + px * vec2<f32>(1.0, 1.0));
    let d = tap(uv + px * vec2<f32>(-1.0, 0.0));
    let e = tap(uv);
    let f = tap(uv + px * vec2<f32>(1.0, 0.0));
    let g = tap(uv + px * vec2<f32>(-1.0, -1.0));
    let h = tap(uv + px * vec2<f32>(0.0, -1.0));
    let i = tap(uv + px * vec2<f32>(1.0, -1.0));
    let color = (e * 4.0 + (b + d + f + h) * 2.0 + (a + c + g + i)) / 16.0;
    return vec4<f32>(color, 1.0);
}
