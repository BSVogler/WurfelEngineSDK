// Puts the peeled layers (see peel.rs) on the screen: one full-screen triangle per layer, drawn
// from the farthest layer to the nearest with alpha blending (the blend state is on the pipeline).
// A layer pixel is the colour and alpha of one fragment, so a translucent surface shows the layer
// behind it whatever order the geometry was drawn in.

@group(0) @binding(0) var layer: texture_2d<f32>;

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    // The triangle (-1, -1), (3, -1), (-1, 3) covers the whole screen.
    let x = f32(i32(index & 1u) * 4 - 1);
    let y = f32(i32(index >> 1u) * 4 - 1);
    return vec4<f32>(x, y, 0.0, 1.0);
}

@fragment
fn fs_main(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    // The layer has the size of the screen, so the window position is the texel.
    return textureLoad(layer, vec2<i32>(frag.xy), 0);
}
