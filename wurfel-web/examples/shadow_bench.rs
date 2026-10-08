//! Times the sun shadow code of `src/shader.wgsl` on the native GPU.
//!
//! `cargo run --release --example shadow_bench [-- <sun>]` (sun: `high` or `low`; both by default).
//!
//! It runs the real functions of the shader (`sun_visibility` and what it calls) for every pixel of a
//! 1920 x 1080 target over a made-up stepped terrain, with the voxel textures and the sun uniform made
//! by the same code the game uses (`voxels.rs`, `sunshadow.rs`), and reads the GPU's own timestamps.
//! Every pixel is a point on the top or a side of a block, laid out as a flat grid (12 pixels a block):
//! this is not the game's picture but the same kind of work, so the numbers compare the methods.

#![allow(dead_code)]

#[path = "../src/sunshadow.rs"]
mod sunshadow;
#[path = "../src/voxels.rs"]
mod voxels;

/// `voxels.rs` builds its grid from the game's render storage; the bench makes its own grid.
mod render_storage {
    pub struct Block;
    impl Block {
        pub fn is_air(&self) -> bool {
            true
        }
    }
    pub struct Cell {
        pub block: Block,
    }
    impl Cell {
        pub fn hides_past_block(&self) -> bool {
            false
        }
        pub fn is_liquid(&self) -> bool {
            false
        }
    }
    pub struct Chunk;
    impl Chunk {
        pub fn top_left(&self) -> (i32, i32) {
            (0, 0)
        }
        pub fn cell<A, B, C>(&self, _: A, _: B, _: C) -> Option<Cell> {
            None
        }
    }
    pub struct RenderStorage;
    impl RenderStorage {
        pub fn chunks(&self) -> std::vec::IntoIter<Chunk> {
            Vec::new().into_iter()
        }
    }
}

use glam::Vec3;
use voxels::{VoxelGrid, SIZE_XY, SIZE_Z};

fn index_of(x: usize, y: usize, z: usize) -> usize {
    x + SIZE_XY * (y + SIZE_XY * z)
}

const WIDTH: u32 = 1920;
const HEIGHT: u32 = 1080;
const PIXELS_PER_BLOCK: f32 = 12.0;
const ROW_OFFSET: i32 = 35;

fn height_at(x: i32, y: i32) -> i32 {
    let (fx, fy) = (x as f32, y as f32);
    let hills = 3.0 * (fx * 0.11).sin() + 3.0 * (fy * 0.09).cos() + 1.5 * ((fx + fy) * 0.23).sin();
    let mut h = (4.0 + hills).round() as i32;
    // A few single pillars, like trees.
    if (x * 7 + y * 13) % 29 == 0 {
        h += 3;
    }
    h.clamp(1, SIZE_Z as i32 - 6)
}

fn scene() -> (VoxelGrid, Vec<f32>) {
    let mut grid = VoxelGrid::empty((0, 0));
    let mut heights = vec![0.0f32; SIZE_XY * SIZE_XY];
    for y in 0..SIZE_XY {
        for x in 0..SIZE_XY {
            let h = height_at(x as i32, y as i32);
            heights[y * SIZE_XY + x] = h as f32;
            for z in 0..h as usize {
                grid.data[index_of(x, y, z)] = 255;
            }
        }
    }
    (grid, heights)
}

/// The shadow map as the game would make it for the terrain: the nearest depth to the sun in every texel.
fn shadow_map_depths(u: &sunshadow::SunShadowUniform, size: u32, heights: &[f32]) -> Vec<f32> {
    let mut depths = vec![1.0f32; (size * size) as usize];
    let v3 = |a: [f32; 4]| Vec3::new(a[0], a[1], a[2]);
    let (right, up, dir, center) = (v3(u.right), v3(u.up), v3(u.dir), v3(u.center));
    for y in 1..SIZE_XY - 1 {
        for x in 1..SIZE_XY - 1 {
            let h = heights[y * SIZE_XY + x];
            for sy in 0..6 {
                for sx in 0..6 {
                    let p = Vec3::new(x as f32 - 0.5 + (sx as f32 + 0.5) / 6.0, y as f32 - 0.5 + (sy as f32 + 0.5) / 6.0, h);
                    let rel = p - center;
                    let uv = [rel.dot(right) * u.center[3] * 0.5 + 0.5, 0.5 - rel.dot(up) * u.center[3] * 0.5];
                    if !(0.0..1.0).contains(&uv[0]) || !(0.0..1.0).contains(&uv[1]) {
                        continue;
                    }
                    let (tx, ty) = ((uv[0] * size as f32) as usize, (uv[1] * size as f32) as usize);
                    let depth = 0.5 - rel.dot(dir) * u.dir[3];
                    let at = &mut depths[ty * size as usize + tx];
                    *at = at.min(depth);
                }
            }
        }
    }
    depths
}

fn shader_source() -> String {
    let src = include_str!("../src/shader.wgsl");
    let bindings = {
        let a = src.find("// The sun's shadow map").expect("shadow bindings");
        let b = src.find("@group(1) @binding(0) var atlas").expect("atlas bindings");
        &src[a..b]
    };
    let functions = {
        let a = src.find("// The way a face of the given id looks").expect("face_normal");
        let b = src.find("// Added to `Vertex::occlusion`").expect("end of sun_visibility");
        &src[a..b]
    };
    format!(
        "{bindings}\n{functions}\n{}",
        r#"
struct Bench {
    a: vec4<f32>,  // x: pixels a block, y: column offset x, z: column offset y
};
@group(1) @binding(0) var<uniform> bench: Bench;
@group(1) @binding(1) var heights: texture_2d<f32>;

@vertex
fn vs_main(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let p = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4<f32>(p * 2.0 - 1.0, 0.0, 1.0);
}

fn column_height(x: i32, y: i32) -> f32 {
    return textureLoad(heights, vec2<i32>(x, y), 0).r;
}

@fragment
fn fs_main(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    let col = frag.xy / bench.a.x;
    let cx = i32(floor(col.x)) + i32(bench.a.y);
    let cy = i32(floor(col.y)) + i32(bench.a.z);
    let fx = fract(col.x);
    let fy = fract(col.y);
    let h = column_height(cx, cy);
    var pos = vec3<f32>(f32(cx) - 0.5 + fx, f32(cy) - 0.5 + fy / 0.6, h);
    var face = 1;
    if (fy >= 0.6) {
        let t = (fy - 0.6) / 0.4;
        if (fx < 0.5) {
            let low = column_height(cx, cy + 1);
            if (low < h) {
                face = 0;
                pos = vec3<f32>(f32(cx) - 0.5 + fx * 2.0, f32(cy) + 0.5, h - t * min(h - low, 3.0));
            }
        } else {
            let low = column_height(cx + 1, cy);
            if (low < h) {
                face = 2;
                pos = vec3<f32>(f32(cx) + 0.5, f32(cy) - 0.5 + (fx - 0.5) * 2.0, h - t * min(h - low, 3.0));
            }
        }
    }
    let seen = sun_visibility(pos, face);
    return vec4<f32>(seen, seen, seen, 1.0);
}

@group(1) @binding(2) var lightmap: texture_3d<f32>;
@group(1) @binding(3) var lightmap_sampler: sampler;

// The lightmap: the soft visibility of every surface point, kept in a texture and read back with one
// trilinear read (the real one would be 4 x 4 texels a block face, filled a slice at a time).
@fragment
fn fs_lookup(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    let col = frag.xy / bench.a.x;
    let cx = i32(floor(col.x)) + i32(bench.a.y);
    let cy = i32(floor(col.y)) + i32(bench.a.z);
    let h = column_height(cx, cy);
    let fx = fract(col.x);
    let fy = fract(col.y);
    let pos = vec3<f32>(f32(cx) - 0.5 + fx, f32(cy) - 0.5 + fy / 0.6, h);
    let seen = textureSampleLevel(lightmap, lightmap_sampler, vec3<f32>((pos.x + 0.5) / 160.0, (pos.y + 0.5) / 160.0, (pos.z + 0.5) / 32.0), 0.0).r;
    return vec4<f32>(seen, seen, seen, 1.0);
}

@fragment
fn fs_none(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    let col = frag.xy / bench.a.x;
    let h = column_height(i32(floor(col.x)) + i32(bench.a.y), i32(floor(col.y)) + i32(bench.a.z));
    return vec4<f32>(h * 0.1, 0.0, 0.0, 1.0);
}
"#
    )
}

#[derive(Clone, Copy)]
enum Mode {
    /// The shadow code of the shader for every pixel.
    Shadow,
    /// Only the colour: what the picture costs without any shadow.
    None,
    /// One read of a lightmap.
    Lookup,
    /// The shadow code for the first `rows` rows only (a slice of a lightmap refresh).
    Slice(u32),
}

struct Bench {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipeline: wgpu::RenderPipeline,
    none_pipeline: wgpu::RenderPipeline,
    lookup_pipeline: wgpu::RenderPipeline,
    layout0: wgpu::BindGroupLayout,
    group1: wgpu::BindGroup,
    target: wgpu::TextureView,
    sampler: wgpu::Sampler,
    voxels: wgpu::TextureView,
    field: wgpu::TextureView,
    neighbours: wgpu::TextureView,
    query: wgpu::QuerySet,
    resolve: wgpu::Buffer,
    read: wgpu::Buffer,
}

fn texture_3d(device: &wgpu::Device, queue: &wgpu::Queue, label: &str, format: wgpu::TextureFormat, texel: u32, data: &[u8]) -> wgpu::TextureView {
    let size = wgpu::Extent3d { width: SIZE_XY as u32, height: SIZE_XY as u32, depth_or_array_layers: SIZE_Z as u32 };
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D3,
        format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        wgpu::TexelCopyTextureInfo { texture: &texture, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
        data,
        wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(SIZE_XY as u32 * texel), rows_per_image: Some(SIZE_XY as u32) },
        size,
    );
    texture.create_view(&Default::default())
}

/// The distance field with two coarser levels (every cell the mean of 2 x 2 x 2 of the level below).
fn field_with_mips(device: &wgpu::Device, queue: &wgpu::Queue, field: &[f32]) -> wgpu::TextureView {
    let levels = 3u32;
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("field"),
        size: wgpu::Extent3d { width: SIZE_XY as u32, height: SIZE_XY as u32, depth_or_array_layers: SIZE_Z as u32 },
        mip_level_count: levels,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D3,
        format: wgpu::TextureFormat::R16Float,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let mut level = field.to_vec();
    let (mut w, mut h, mut d) = (SIZE_XY, SIZE_XY, SIZE_Z);
    for mip in 0..levels {
        let texels: Vec<u16> = level.iter().map(|&v| voxels::f32_to_f16(v)).collect();
        queue.write_texture(
            wgpu::TexelCopyTextureInfo { texture: &texture, mip_level: mip, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            bytemuck::cast_slice(&texels),
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(w as u32 * 2), rows_per_image: Some(h as u32) },
            wgpu::Extent3d { width: w as u32, height: h as u32, depth_or_array_layers: d as u32 },
        );
        let (nw, nh, nd) = (w / 2, h / 2, d / 2);
        let mut next = vec![0.0f32; nw * nh * nd];
        for z in 0..nd {
            for y in 0..nh {
                for x in 0..nw {
                    let mut sum = 0.0;
                    for (dz, dy, dx) in [(0, 0, 0), (0, 0, 1), (0, 1, 0), (0, 1, 1), (1, 0, 0), (1, 0, 1), (1, 1, 0), (1, 1, 1)] {
                        sum += level[(2 * x + dx) + w * ((2 * y + dy) + h * (2 * z + dz))];
                    }
                    next[x + nw * (y + nh * z)] = sum / 8.0;
                }
            }
        }
        level = next;
        (w, h, d) = (nw, nh, nd);
    }
    texture.create_view(&Default::default())
}

impl Bench {
    fn new(grid: &VoxelGrid, heights: &[f32]) -> Self {
        let instance = wgpu::Instance::default();
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default())).expect("an adapter");
        println!("GPU: {} ({:?})", adapter.get_info().name, adapter.get_info().backend);
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            required_features: wgpu::Features::TIMESTAMP_QUERY,
            ..Default::default()
        }))
        .expect("a device with timestamp queries");

        let source = shader_source();
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("bench"), source: wgpu::ShaderSource::Wgsl(source.into()) });

        let entry = |binding, ty| wgpu::BindGroupLayoutEntry { binding, visibility: wgpu::ShaderStages::FRAGMENT, ty, count: None };
        let tex3d = wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D3, multisampled: false };
        let layout0 = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("shadow"),
            entries: &[
                entry(4, wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None }),
                entry(5, wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Depth, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false }),
                entry(6, tex3d),
                entry(7, wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering)),
                entry(9, tex3d),
                entry(10, wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Uint, view_dimension: wgpu::TextureViewDimension::D3, multisampled: false }),
            ],
        });
        let layout1 = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("bench"),
            entries: &[
                entry(0, wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None }),
                entry(1, wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: false }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false }),
                entry(2, tex3d),
                entry(3, wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering)),
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: None, bind_group_layouts: &[Some(&layout0), Some(&layout1)], immediate_size: 0 });
        let make = |fragment: &str| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(fragment),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState { module: &shader, entry_point: Some("vs_main"), compilation_options: Default::default(), buffers: &[] },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(fragment),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::TextureFormat::Rgba8Unorm.into())],
                }),
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                multiview_mask: None,
                cache: None,
            })
        };
        let (pipeline, none_pipeline, lookup_pipeline) = (make("fs_main"), make("fs_none"), make("fs_lookup"));

        let voxels_view = texture_3d(&device, &queue, "voxels", wgpu::TextureFormat::R8Unorm, 1, &grid.data);
        let field_view = field_with_mips(&device, &queue, &grid.distance_field());
        let neighbours = texture_3d(&device, &queue, "neighbours", wgpu::TextureFormat::R32Uint, 4, bytemuck::cast_slice(&grid.neighbourhood()));
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor { mag_filter: wgpu::FilterMode::Linear, min_filter: wgpu::FilterMode::Linear, ..Default::default() });

        let heights_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("heights"),
            size: wgpu::Extent3d { width: SIZE_XY as u32, height: SIZE_XY as u32, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R32Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            wgpu::TexelCopyTextureInfo { texture: &heights_texture, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            bytemuck::cast_slice(heights),
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(SIZE_XY as u32 * 4), rows_per_image: Some(SIZE_XY as u32) },
            wgpu::Extent3d { width: SIZE_XY as u32, height: SIZE_XY as u32, depth_or_array_layers: 1 },
        );
        // 4 x 4 texels a block face, 32 layers: 640 x 640 x 32, one byte each.
        let lightmap_view = {
            let size = wgpu::Extent3d { width: 640, height: 640, depth_or_array_layers: SIZE_Z as u32 };
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("lightmap"),
                size,
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D3,
                format: wgpu::TextureFormat::R8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            let data: Vec<u8> = (0..640 * 640 * SIZE_Z).map(|i| ((i * 31) % 251) as u8).collect();
            queue.write_texture(
                wgpu::TexelCopyTextureInfo { texture: &texture, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
                &data,
                wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(640), rows_per_image: Some(640) },
                size,
            );
            texture.create_view(&Default::default())
        };
        let bench_uniform = device.create_buffer(&wgpu::BufferDescriptor { label: Some("bench"), size: 16, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        queue.write_buffer(&bench_uniform, 0, bytemuck::cast_slice(&[PIXELS_PER_BLOCK, 0.0, ROW_OFFSET as f32, 0.0]));
        let group1 = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("bench"),
            layout: &layout1,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: bench_uniform.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&heights_texture.create_view(&Default::default())) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&lightmap_view) },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::Sampler(&sampler) },
            ],
        });

        let target = device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("target"),
                size: wgpu::Extent3d { width: WIDTH, height: HEIGHT, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            })
            .create_view(&Default::default());
        let query = device.create_query_set(&wgpu::QuerySetDescriptor { label: None, ty: wgpu::QueryType::Timestamp, count: 2 });
        let resolve = device.create_buffer(&wgpu::BufferDescriptor { label: None, size: 16, usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC, mapped_at_creation: false });
        let read = device.create_buffer(&wgpu::BufferDescriptor { label: None, size: 16, usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        Bench { device, queue, pipeline, none_pipeline, lookup_pipeline, layout0, group1, target, sampler, voxels: voxels_view, field: field_view, neighbours, query, resolve, read }
    }

    fn depth_map(&self, size: u32, depths: &[f32]) -> wgpu::TextureView {
        let extent = wgpu::Extent3d { width: size, height: size, depth_or_array_layers: 1 };
        let source = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("depths"),
            size: extent,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R32Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo { texture: &source, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            bytemuck::cast_slice(depths),
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(size * 4), rows_per_image: Some(size) },
            extent,
        );
        let map = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("shadow map"),
            size: extent,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Depth32Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let shader = self.device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: None,
            source: wgpu::ShaderSource::Wgsl(
                r#"
@group(0) @binding(0) var depths: texture_2d<f32>;
@vertex fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let p = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4<f32>(p * 2.0 - 1.0, 0.0, 1.0);
}
struct Out { @builtin(frag_depth) depth: f32 };
@fragment fn fs(@builtin(position) frag: vec4<f32>) -> Out {
    return Out(textureLoad(depths, vec2<i32>(frag.xy), 0).r);
}
"#
                .into(),
            ),
        });
        let layout = self.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: false }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false },
                count: None,
            }],
        });
        let pipeline_layout = self.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: None, bind_group_layouts: &[Some(&layout)], immediate_size: 0 });
        let pipeline = self.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: None,
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState { module: &shader, entry_point: Some("vs"), compilation_options: Default::default(), buffers: &[] },
            fragment: Some(wgpu::FragmentState { module: &shader, entry_point: Some("fs"), compilation_options: Default::default(), targets: &[] }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Always),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        let group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&source.create_view(&Default::default())) }],
        });
        let view = map.create_view(&Default::default());
        let mut encoder = self.device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &view,
                    depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &group, &[]);
            pass.draw(0..3, 0..1);
        }
        self.queue.submit([encoder.finish()]);
        view
    }

    /// The best of five rounds of `time_once`: the GPU's clocks and temperature move the numbers around.
    fn time(&self, uniform: &sunshadow::SunShadowUniform, map: &wgpu::TextureView, mode: Mode, frames: usize) -> (f32, f32) {
        (0..5).map(|_| self.time_once(uniform, map, mode, frames)).fold((f32::MAX, f32::MAX), |a, b| (a.0.min(b.0), a.1.min(b.1)))
    }

    /// Milliseconds the GPU needs for one frame with this uniform and map (median of `frames`).
    fn time_once(&self, uniform: &sunshadow::SunShadowUniform, map: &wgpu::TextureView, mode: Mode, frames: usize) -> (f32, f32) {
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor { label: None, size: std::mem::size_of::<sunshadow::SunShadowUniform>() as u64, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        self.queue.write_buffer(&buffer, 0, bytemuck::bytes_of(uniform));
        let group0 = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &self.layout0,
            entries: &[
                wgpu::BindGroupEntry { binding: 4, resource: buffer.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 5, resource: wgpu::BindingResource::TextureView(map) },
                wgpu::BindGroupEntry { binding: 6, resource: wgpu::BindingResource::TextureView(&self.voxels) },
                wgpu::BindGroupEntry { binding: 7, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                wgpu::BindGroupEntry { binding: 9, resource: wgpu::BindingResource::TextureView(&self.field) },
                wgpu::BindGroupEntry { binding: 10, resource: wgpu::BindingResource::TextureView(&self.neighbours) },
            ],
        });
        let period = self.queue.get_timestamp_period() as f64;
        let mut times = Vec::new();
        let mut walls = Vec::new();
        for frame in 0..frames + 3 {
            let mut encoder = self.device.create_command_encoder(&Default::default());
            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: None,
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &self.target,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: Some(wgpu::RenderPassTimestampWrites { query_set: &self.query, beginning_of_pass_write_index: Some(0), end_of_pass_write_index: Some(1) }),
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                pass.set_pipeline(match mode {
                    Mode::None => &self.none_pipeline,
                    Mode::Lookup => &self.lookup_pipeline,
                    _ => &self.pipeline,
                });
                if let Mode::Slice(rows) = mode {
                    pass.set_scissor_rect(0, 0, WIDTH, rows);
                }
                pass.set_bind_group(0, &group0, &[]);
                pass.set_bind_group(1, &self.group1, &[]);
                pass.draw(0..3, 0..1);
            }
            encoder.resolve_query_set(&self.query, 0..2, &self.resolve, 0);
            encoder.copy_buffer_to_buffer(&self.resolve, 0, &self.read, 0, 16);
            let started = std::time::Instant::now();
            self.queue.submit([encoder.finish()]);
            self.read.slice(..).map_async(wgpu::MapMode::Read, |r| r.unwrap());
            self.device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
            if frame >= 3 {
                walls.push(started.elapsed().as_secs_f32() * 1000.0);
            }
            {
                let data = self.read.slice(..).get_mapped_range().unwrap();
                let t: &[u64] = bytemuck::cast_slice(&data);
                if frame >= 3 {
                    times.push((t[1].saturating_sub(t[0]) as f64 * period / 1.0e6) as f32);
                }
            }
            self.read.unmap();
        }
        times.sort_by(|a, b| a.total_cmp(b));
        walls.sort_by(|a, b| a.total_cmp(b));
        (times[times.len() / 2], walls[walls.len() / 2])
    }
}

/// The soft walk of `voxels.rs` on every 7th pixel of the bench's picture: where do its steps go?
fn count_steps(grid: &VoxelGrid, heights: &[f32], sun: Vec3, soft: f32, quality: [f32; 3]) {
    use std::sync::atomic::Ordering::Relaxed;
    let field = grid.distance_field();
    for s in &voxels::STATS {
        s.store(0, Relaxed);
    }
    let h = |x: i32, y: i32| heights[(y as usize) * SIZE_XY + x as usize];
    let sun = sun.normalize();
    let mut px = 0;
    for py in (0..HEIGHT).step_by(7) {
        for pxx in (0..WIDTH).step_by(7) {
            px += 1;
            let (cx, cy) = ((pxx as f32 / PIXELS_PER_BLOCK).floor() as i32, (py as f32 / PIXELS_PER_BLOCK).floor() as i32 + ROW_OFFSET);
            let (fx, fy) = (pxx as f32 / PIXELS_PER_BLOCK % 1.0, py as f32 / PIXELS_PER_BLOCK % 1.0);
            let top = h(cx, cy);
            let (mut pos, mut normal) = (Vec3::new(cx as f32 - 0.5 + fx, cy as f32 - 0.5 + fy / 0.6, top), Vec3::Z);
            if fy >= 0.6 {
                let t = (fy - 0.6) / 0.4;
                if fx < 0.5 && h(cx, cy + 1) < top {
                    pos = Vec3::new(cx as f32 - 0.5 + fx * 2.0, cy as f32 + 0.5, top - t * (top - h(cx, cy + 1)).min(3.0));
                    normal = Vec3::Y;
                } else if fx >= 0.5 && h(cx + 1, cy) < top {
                    pos = Vec3::new(cx as f32 + 0.5, cy as f32 - 0.5 + (fx - 0.5) * 2.0, top - t * (top - h(cx + 1, cy)).min(3.0));
                    normal = Vec3::X;
                }
            }
            grid.soft_visibility_with(&field, pos, sun, normal, soft, quality);
        }
    }
    let n = |i: usize| voxels::STATS[i].load(Relaxed) as f64;
    println!(
        "  per pixel (of {px} sampled): {:.1} steps, {:.1} of them within 1 cell of a block, {:.2} exact looks, {:.2} steps inside blocks",
        n(1) / n(0),
        n(2) / n(0),
        n(3) / n(0),
        n(4) / n(0)
    );
}

fn main() {
    let which = std::env::args().nth(1);
    let (grid, heights) = scene();
    let bench = Bench::new(&grid, &heights);
    let focus = Vec3::new(80.0, 80.0, 5.0);
    let suns = [("high sun", Vec3::new(-0.2, -0.1, 0.97)), ("low sun", Vec3::new(-0.6, -0.3, 0.74))];
    let top = grid.top();
    let frames = 40;
    for (name, sun) in suns {
        if which.as_deref().is_some_and(|w| !name.starts_with(w)) {
            continue;
        }
        println!("\n{name}  (to_sun {:?}), {WIDTH}x{HEIGHT}, grid top {top} layers", sun.normalize());
        let map_uniform = sunshadow::uniform(focus, sun, 1.0, 2048, sunshadow::RADIUS);
        let map = bench.depth_map(2048, &shadow_map_depths(&map_uniform, 2048, &heights));
        let sprite_map = bench.depth_map(1024, &vec![1.0; 1024 * 1024]);
        let sprite_uniform = sunshadow::uniform(focus, sun, 1.0, 1024, sunshadow::SPRITE_RADIUS);
        count_steps(&grid, &heights, sun, 0.12, sunshadow::VoxelQuality::Medium.params()[..3].try_into().unwrap());
        // The GPU clocks up under load: spin it up before the first number.
        bench.time_once(&sprite_uniform.with_voxels((0, 0), 0.12, sunshadow::VoxelQuality::High, top), &sprite_map, Mode::Shadow, 300);
        let row = |label: &str, (_, wall): (f32, f32)| println!("  {label:<44} {wall:>7.2} ms");
        row("baseline (colour only)", bench.time(&map_uniform, &map, Mode::None, frames));
        row("shadow map (5x5 window)", bench.time(&map_uniform, &map, Mode::Shadow, frames));
        row("shadow map, soft 0.12 (PCSS, 16 + 25 reads)", bench.time(&map_uniform.with_map_softness(0.12), &map, Mode::Shadow, frames));
        row("lightmap lookup (1 trilinear read a pixel)", bench.time(&map_uniform, &map, Mode::Lookup, frames));
        let soft_medium = sprite_uniform.with_voxels((0, 0), 0.12, sunshadow::VoxelQuality::Medium, top);
        // A lightmap of 4 x 4 texels a block face, three faces, 1/16 of it a frame: 77 000 traces, 1/27 of the screen's pixels.
        row("lightmap refresh slice (soft Medium, 1/27 of the pixels)", bench.time(&soft_medium, &sprite_map, Mode::Slice(HEIGHT / 27), frames));
        row("analytic, hard (softness 0)", bench.time(&sprite_uniform.with_voxels((0, 0), 0.0, sunshadow::VoxelQuality::Medium, top), &sprite_map, Mode::Shadow, frames));
        for (label, quality) in [("Low", sunshadow::VoxelQuality::Low), ("Medium", sunshadow::VoxelQuality::Medium), ("High", sunshadow::VoxelQuality::High)] {
            row(&format!("analytic, soft 0.12, {label}"), bench.time(&sprite_uniform.with_voxels((0, 0), 0.12, quality, top), &sprite_map, Mode::Shadow, frames));
        }
    }
}
