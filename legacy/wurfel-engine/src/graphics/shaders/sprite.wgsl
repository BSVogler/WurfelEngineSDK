// Sprite vertex shader for isometric 2.5D rendering

struct Uniforms {
    view_proj: mat4x4<f32>,
    screen_size: vec2<f32>,
    _padding: vec2<f32>,
}

@group(0) @binding(0)
var<uniform> uniforms: Uniforms;

@group(0) @binding(1)
var sprite_texture: texture_2d<f32>;

@group(0) @binding(2)
var sprite_sampler: sampler;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) tex_coords: vec2<f32>,
    @location(2) color: vec4<f32>,
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) tex_coords: vec2<f32>,
    @location(1) color: vec4<f32>,
}

@vertex
fn vs_main(model: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    
    // Apply view-projection matrix transformation (authentic Wurfel Engine approach)
    // The vertices are in 3D world space and need proper camera transformation
    let world_pos = vec4<f32>(model.position, 1.0);
    out.clip_position = uniforms.view_proj * world_pos;
    
    out.tex_coords = model.tex_coords;
    out.color = model.color;
    
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    // Return vertex color for authentic Wurfel Engine block face coloring
    return in.color;
}