// Fast approximate anti-aliasing on the finished picture (display colours, 0..1): finds edges by the
// change of luma and blurs along them. It is the short form of FXAA (Timothy Lottes): no search along
// the edge, so long shallow slopes are only softened, but the jaggies of the block edges and the
// sprite outlines go. Only edges with a clear contrast are touched, so the pixels inside a sprite
// stay sharp.

// An edge must have at least this much luma contrast (and a share of the local maximum).
const EDGE_MIN = 0.0625;
const EDGE_RELATIVE = 0.166;
// Blur along the edge is limited to this many texels.
const SPAN_MAX = 6.0;
const REDUCE_MIN = 1.0 / 128.0;
const REDUCE_MUL = 1.0 / 8.0;

@group(0) @binding(0) var picture: texture_2d<f32>;
@group(0) @binding(1) var picture_sampler: sampler;

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let x = f32(i32(index & 1u) * 4 - 1);
    let y = f32(i32(index >> 1u) * 4 - 1);
    return vec4<f32>(x, y, 0.0, 1.0);
}

fn luma(color: vec3<f32>) -> f32 {
    return dot(color, vec3<f32>(0.299, 0.587, 0.114));
}

fn at(uv: vec2<f32>) -> vec3<f32> {
    return textureSampleLevel(picture, picture_sampler, uv, 0.0).rgb;
}

@fragment
fn fs_main(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    let size = vec2<f32>(textureDimensions(picture));
    let px = 1.0 / size;
    let uv = frag.xy * px;

    let middle = at(uv);
    let l_nw = luma(at(uv + vec2<f32>(-1.0, -1.0) * px));
    let l_ne = luma(at(uv + vec2<f32>(1.0, -1.0) * px));
    let l_sw = luma(at(uv + vec2<f32>(-1.0, 1.0) * px));
    let l_se = luma(at(uv + vec2<f32>(1.0, 1.0) * px));
    let l_m = luma(middle);

    let l_min = min(l_m, min(min(l_nw, l_ne), min(l_sw, l_se)));
    let l_max = max(l_m, max(max(l_nw, l_ne), max(l_sw, l_se)));
    if (l_max - l_min < max(EDGE_MIN, l_max * EDGE_RELATIVE)) {
        return vec4<f32>(middle, 1.0);
    }

    var dir = vec2<f32>(-((l_nw + l_ne) - (l_sw + l_se)), (l_nw + l_sw) - (l_ne + l_se));
    let reduce = max((l_nw + l_ne + l_sw + l_se) * 0.25 * REDUCE_MUL, REDUCE_MIN);
    let scale = 1.0 / (min(abs(dir.x), abs(dir.y)) + reduce);
    dir = clamp(dir * scale, vec2<f32>(-SPAN_MAX), vec2<f32>(SPAN_MAX)) * px;

    let a = 0.5 * (at(uv + dir * (1.0 / 3.0 - 0.5)) + at(uv + dir * (2.0 / 3.0 - 0.5)));
    let b = a * 0.5 + 0.25 * (at(uv + dir * -0.5) + at(uv + dir * 0.5));
    let l_b = luma(b);
    if (l_b < l_min || l_b > l_max) {
        return vec4<f32>(a, 1.0);
    }
    return vec4<f32>(b, 1.0);
}
