//! Shadows of the sun on the block world: a shadow map.
//!
//! Every frame the world is drawn once more, depth only, from the sun (`sunshadow.wgsl`) into a
//! square map around the camera's focus. The scene shader (`shader.wgsl`, `sun_visibility`) then
//! compares each surface point's distance from the sun with what the map holds, and takes the sun's
//! share of the light away where something is in front. Blocks, and standing sprites cut out by their
//! alpha, cast; the light of a vertex is split into the sun's part and the rest in the vertex shader,
//! so a shadow darkens to the ambient light and not to black.
//!
//! The sun is the global light of the light engine (`wurfel_sim::light`), so shadows follow the
//! day: they are long and faint at sunrise, short at noon, and gone at night and when the sun is
//! near the horizon (a long shadow of the whole map's height would only be noise).
//!
//! This file has the maths (native tests) and, for the browser, the map and its pass.

#![cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]

use bytemuck::{Pod, Zeroable};
use glam::Vec3;

/// How sharp the shadows are: texels along one side of the map. The map covers the same area
/// whatever the size, so a larger one makes the stair steps of the edges smaller and costs more
/// memory (4 bytes a texel) and time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ShadowQuality {
    /// 1024 texels: about 10 cm of the world per texel.
    Low,
    /// 2048 texels.
    #[default]
    Medium,
    /// 4096 texels (64 MB of video memory).
    High,
}

impl ShadowQuality {
    pub fn size(self) -> u32 {
        match self {
            ShadowQuality::Low => 1024,
            ShadowQuality::Medium => 2048,
            ShadowQuality::High => 4096,
        }
    }

    /// The menu's `shadowQuality` value; anything else is the default.
    pub fn from_name(name: &str) -> Self {
        match name {
            "low" => ShadowQuality::Low,
            "high" => ShadowQuality::High,
            _ => ShadowQuality::Medium,
        }
    }
}

/// Half the width of the map in blocks: the shadows reach this far from the focus.
pub const RADIUS: f32 = 48.0;
/// Depth range along the sun's ray in blocks (centred on the focus); anything further is clipped.
pub const DEPTH_RANGE: f32 = 256.0;
/// A texel of a map `size` texels wide, in blocks.
pub fn texel(size: u32) -> f32 {
    2.0 * RADIUS / size.max(1) as f32
}

/// The sun this high (the sine of its angle above the horizon) casts full shadows ...
const FULL_AT: f32 = 0.35;
/// ... and below this height none.
const NONE_AT: f32 = 0.1;

/// `SunShadow` in `shader.wgsl` and `sunshadow.wgsl`: five `vec4`s.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub struct SunShadowUniform {
    /// xyz: the map's x axis in the world.
    pub right: [f32; 4],
    /// xyz: the map's y axis.
    pub up: [f32; 4],
    /// xyz: unit vector towards the sun. w: depth per block along it.
    pub dir: [f32; 4],
    /// xyz: the world point in the middle of the map. w: 1 / [`RADIUS`].
    pub center: [f32; 4],
    /// x: strength 0..1 (0: no shadows). y: a texel in blocks. z: the map size in texels. w: unused.
    pub params: [f32; 4],
}

fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// How strong the shadows are: 0 with the sun near or under the horizon (`sun_height` is the z of
/// the unit vector towards the sun) and by night, 1 with the sun high.
pub fn strength(sun_height: f32, night_mix: f32) -> f32 {
    if !sun_height.is_finite() || !night_mix.is_finite() {
        return 0.0;
    }
    smoothstep(NONE_AT, FULL_AT, sun_height) * (1.0 - night_mix.clamp(0.0, 1.0))
}

/// The uniform for a frame: a map centred on `focus` and looking along `to_sun`, with the given
/// `strength` (see [`strength`]; 0 draws nothing and the scene ignores the map), for a map `size` texels wide.
///
/// The centre is moved to a multiple of a texel along the map's axes, so a moving camera does not
/// make the edges of shadows crawl.
pub fn uniform(focus: Vec3, to_sun: Vec3, strength: f32, size: u32) -> SunShadowUniform {
    let texel = texel(size);
    let dir = if to_sun.is_finite() && to_sun.length_squared() > 1e-6 { to_sun.normalize() } else { Vec3::Z };
    let focus = if focus.is_finite() { focus } else { Vec3::ZERO };
    // `right` is level (the horizon of the map); with the sun straight above any horizontal axis will do.
    let right = Vec3::Z.cross(dir).try_normalize().unwrap_or(Vec3::X);
    let up = dir.cross(right);
    let snap = |x: f32| (x / texel).round() * texel;
    let (a, b) = (focus.dot(right), focus.dot(up));
    let center = focus + right * (snap(a) - a) + up * (snap(b) - b);
    SunShadowUniform {
        right: [right.x, right.y, right.z, 0.0],
        up: [up.x, up.y, up.z, 0.0],
        dir: [dir.x, dir.y, dir.z, 1.0 / DEPTH_RANGE],
        center: [center.x, center.y, center.z, 1.0 / RADIUS],
        params: [strength.clamp(0.0, 1.0), texel, size as f32, 0.0],
    }
}

/// Browser only: the map and the pass that fills it.
#[cfg(target_arch = "wasm32")]
pub mod gpu {
    use super::*;
    use crate::mesh::Vertex;

    pub const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

    pub struct SunShadowMap {
        size: u32,
        view: wgpu::TextureView,
        uniform_buffer: wgpu::Buffer,
        group: wgpu::BindGroup,
        pipeline: wgpu::RenderPipeline,
    }

    impl SunShadowMap {
        /// `camera_buffer` is the scene's camera uniform (the free camera's turn is needed to place
        /// the standing sprites); `atlas_layout` the layout of group 1 of the scene (the sprites' alpha).
        pub fn new(device: &wgpu::Device, camera_buffer: &wgpu::Buffer, atlas_layout: &wgpu::BindGroupLayout, size: u32) -> Self {
            let view = Self::create_view(device, size);
            let uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("sun shadow"),
                size: std::mem::size_of::<SunShadowUniform>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let uniform_entry = |binding: u32| wgpu::BindGroupLayoutEntry {
                binding,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
                count: None,
            };
            let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("sun shadow pass"),
                entries: &[uniform_entry(0), uniform_entry(1)],
            });
            let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("sun shadow pass"),
                layout: &layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: camera_buffer.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 1, resource: uniform_buffer.as_entire_binding() },
                ],
            });
            let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("sun shadow"),
                source: wgpu::ShaderSource::Wgsl(include_str!("sunshadow.wgsl").into()),
            });
            let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("sun shadow"),
                bind_group_layouts: &[Some(&layout), Some(atlas_layout)],
                immediate_size: 0,
            });
            let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("sun shadow"),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some("vs_shadow"),
                    compilation_options: Default::default(),
                    buffers: &[Some(Vertex::layout())],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some("fs_shadow"),
                    compilation_options: Default::default(),
                    targets: &[],
                }),
                // Both sides of a face count: the faces of a block that look away from the camera
                // are not in the mesh, and the sun is often on that side.
                primitive: wgpu::PrimitiveState { cull_mode: None, ..Default::default() },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: FORMAT,
                    depth_write_enabled: Some(true),
                    depth_compare: Some(wgpu::CompareFunction::Less),
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: wgpu::MultisampleState::default(),
                multiview_mask: None,
                cache: None,
            });
            SunShadowMap { size, view, uniform_buffer, group, pipeline }
        }

        fn create_view(device: &wgpu::Device, size: u32) -> wgpu::TextureView {
            device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some("sun shadow map"),
                    size: wgpu::Extent3d { width: size, height: size, depth_or_array_layers: 1 },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: FORMAT,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                })
                .create_view(&Default::default())
        }

        /// The map's width in texels.
        pub fn size(&self) -> u32 {
            self.size
        }

        /// Make the map `size` texels wide. Returns true when it changed: the scene's bind group holds the
        /// old texture and has to be made again (with [`Self::view`]).
        pub fn resize(&mut self, device: &wgpu::Device, size: u32) -> bool {
            if size == self.size {
                return false;
            }
            self.size = size;
            self.view = Self::create_view(device, size);
            true
        }

        /// The uniform buffer the scene reads too (binding 4 of its group 0).
        pub fn uniform_buffer(&self) -> &wgpu::Buffer {
            &self.uniform_buffer
        }

        /// The map (binding 5 of the scene's group 0).
        pub fn view(&self) -> &wgpu::TextureView {
            &self.view
        }

        /// Fill the map. `draw` issues the draw calls of what casts (vertex buffers and draws; the
        /// pipeline and groups 0 and 1 are set by the caller of `draw`'s argument, see below).
        /// With a strength of 0 nothing is drawn and the scene does not read the map.
        pub fn render(
            &self,
            encoder: &mut wgpu::CommandEncoder,
            queue: &wgpu::Queue,
            uniform: &SunShadowUniform,
            atlas_group: &wgpu::BindGroup,
            draw: impl FnOnce(&mut wgpu::RenderPass<'_>),
        ) {
            queue.write_buffer(&self.uniform_buffer, 0, bytemuck::bytes_of(uniform));
            if uniform.params[0] <= 0.0 {
                return;
            }
            let mut rp = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("sun shadow"),
                color_attachments: &[],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.view,
                    depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            rp.set_pipeline(&self.pipeline);
            rp.set_bind_group(0, &self.group, &[]);
            rp.set_bind_group(1, atlas_group, &[]);
            draw(&mut rp);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIZE: u32 = 2048;
    const TEXEL_T: f32 = 2.0 * RADIUS / SIZE as f32;

    fn validated(source: &str) -> naga::Module {
        let module = naga::front::wgsl::parse_str(source).unwrap_or_else(|e| panic!("does not parse:\n{}", e.emit_to_string(source)));
        naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::empty())
            .validate(&module)
            .unwrap_or_else(|e| panic!("is not valid:\n{}", e.emit_to_string(source)));
        module
    }

    /// The text of `fn name(...) { ... }` up to its closing brace at the start of a line.
    fn function<'a>(source: &'a str, name: &str) -> &'a str {
        let start = source.find(&format!("fn {name}(")).unwrap_or_else(|| panic!("fn {name} missing"));
        let end = source[start..].find("\n}\n").expect("end of function") + start + 3;
        &source[start..end]
    }

    fn struct_size(module: &naga::Module, name: &str) -> usize {
        let ty = module.types.iter().find(|(_, t)| t.name.as_deref() == Some(name)).unwrap_or_else(|| panic!("struct {name}")).1;
        let naga::TypeInner::Struct { span, .. } = &ty.inner else { panic!("{name} is not a struct") };
        *span as usize
    }

    #[test]
    fn the_uniform_is_five_vec4s_in_both_shaders() {
        assert_eq!(std::mem::size_of::<SunShadowUniform>(), 80);
        for source in [include_str!("shader.wgsl"), include_str!("sunshadow.wgsl")] {
            assert_eq!(struct_size(&validated_or_parsed(source), "SunShadow"), 80);
        }
    }

    /// shader.wgsl is validated by its own test (it needs the whole pipeline's bindings); parse it only.
    fn validated_or_parsed(source: &str) -> naga::Module {
        naga::front::wgsl::parse_str(source).unwrap_or_else(|e| panic!("does not parse:\n{}", e.emit_to_string(source)))
    }

    #[test]
    fn the_shadow_shader_is_valid_and_has_the_two_entry_points() {
        let module = validated(include_str!("sunshadow.wgsl"));
        let entries: Vec<(&str, naga::ShaderStage)> = module.entry_points.iter().map(|e| (e.name.as_str(), e.stage)).collect();
        assert!(entries.contains(&("vs_shadow", naga::ShaderStage::Vertex)), "{entries:?}");
        assert!(entries.contains(&("fs_shadow", naga::ShaderStage::Fragment)), "{entries:?}");
        let bindings: Vec<(u32, u32)> = module.global_variables.iter().filter_map(|(_, v)| v.binding.as_ref().map(|b| (b.group, b.binding))).collect();
        assert_eq!(bindings, vec![(0, 0), (0, 1), (1, 0), (1, 1)]);
    }

    #[test]
    fn the_standing_sprites_are_placed_as_in_the_scene_shader() {
        let scene = include_str!("shader.wgsl");
        let shadow = include_str!("sunshadow.wgsl");
        for name in ["billboard_pos", "view_pos"] {
            assert_eq!(function(scene, name), function(shadow, name), "{name} differs between shader.wgsl and sunshadow.wgsl");
        }
    }

    #[test]
    fn the_axes_are_a_level_right_and_an_up_that_is_perpendicular_to_both() {
        let sun = Vec3::new(0.3, -0.5, 0.8).normalize();
        let u = uniform(Vec3::ZERO, sun, 1.0, SIZE);
        let right = Vec3::new(u.right[0], u.right[1], u.right[2]);
        let up = Vec3::new(u.up[0], u.up[1], u.up[2]);
        assert!(right.z.abs() < 1e-6, "level");
        assert!((right.length() - 1.0).abs() < 1e-5 && (up.length() - 1.0).abs() < 1e-5);
        assert!(right.dot(up).abs() < 1e-5 && right.dot(sun).abs() < 1e-5 && up.dot(sun).abs() < 1e-5);
    }

    #[test]
    fn a_sun_straight_above_still_gets_axes() {
        let u = uniform(Vec3::ZERO, Vec3::Z, 1.0, SIZE);
        let right = Vec3::new(u.right[0], u.right[1], u.right[2]);
        let up = Vec3::new(u.up[0], u.up[1], u.up[2]);
        assert!((right.length() - 1.0).abs() < 1e-5 && (up.length() - 1.0).abs() < 1e-5);
        assert!(right.dot(up).abs() < 1e-5);
    }

    #[test]
    fn bad_input_gives_a_harmless_uniform() {
        for u in [uniform(Vec3::NAN, Vec3::new(f32::NAN, 0.0, 1.0), 1.0, SIZE), uniform(Vec3::ZERO, Vec3::ZERO, 1.0, SIZE)] {
            assert!(u.right.iter().chain(&u.up).chain(&u.dir).chain(&u.center).all(|v| v.is_finite()));
        }
    }

    #[test]
    fn the_centre_sits_on_the_texel_grid_so_shadows_do_not_crawl() {
        let sun = Vec3::new(0.3, -0.5, 0.8).normalize();
        for focus in [Vec3::new(10.123, 20.456, 5.0), Vec3::new(10.124, 20.457, 5.0), Vec3::new(-33.3, 4.9, 12.0)] {
            let u = uniform(focus, sun, 1.0, SIZE);
            let center = Vec3::new(u.center[0], u.center[1], u.center[2]);
            let right = Vec3::new(u.right[0], u.right[1], u.right[2]);
            let up = Vec3::new(u.up[0], u.up[1], u.up[2]);
            for axis in [right, up] {
                let steps = center.dot(axis) / TEXEL_T;
                assert!((steps - steps.round()).abs() < 1e-2, "{steps}");
            }
            assert!(center.distance(focus) < 2.0 * TEXEL_T, "the map stays on the focus");
        }
        // Moving the focus by less than a texel moves the map at most once, in whole texels.
        let right = Vec3::Z.cross(sun).normalize();
        let mut changes = 0;
        let mut last = uniform(Vec3::new(10.0, 20.0, 5.0), sun, 1.0, SIZE).center;
        for step in 1..=100 {
            let moved = uniform(Vec3::new(10.0, 20.0, 5.0) + right * (TEXEL_T * 0.01 * step as f32), sun, 1.0, SIZE).center;
            let moved_by = Vec3::new(moved[0] - last[0], moved[1] - last[1], moved[2] - last[2]).length();
            if moved_by > TEXEL_T * 0.5 {
                changes += 1;
                last = moved;
            }
        }
        assert!(changes <= 1, "the map moved {changes} times within one texel");
    }

    #[test]
    fn the_depth_runs_towards_the_sun_and_the_map_holds_what_is_near() {
        let sun = Vec3::new(0.3, -0.5, 0.8).normalize();
        let u = uniform(Vec3::new(5.0, 5.0, 5.0), sun, 1.0, SIZE);
        let center = Vec3::new(u.center[0], u.center[1], u.center[2]);
        let depth = |p: Vec3| 0.5 - (p - center).dot(sun) * u.dir[3];
        assert!(depth(center + sun * 10.0) < depth(center), "nearer the sun is smaller");
        assert!((depth(center) - 0.5).abs() < 1e-6);
        assert!(depth(center + sun * 100.0) > 0.0 && depth(center - sun * 100.0) < 1.0, "100 blocks each way fit");
    }

    #[test]
    fn shadows_fade_with_the_sun_height_and_the_night() {
        assert_eq!(strength(0.0, 0.0), 0.0);
        assert_eq!(strength(-0.5, 0.0), 0.0);
        assert_eq!(strength(1.0, 0.0), 1.0);
        assert_eq!(strength(1.0, 1.0), 0.0);
        let low = strength(0.2, 0.0);
        assert!(low > 0.0 && low < 1.0);
        assert!(strength(0.3, 0.0) > low, "rises with the sun");
        assert_eq!(strength(f32::NAN, 0.0), 0.0);
    }

    #[test]
    fn the_quality_names_pick_the_map_size_and_unknown_names_the_default() {
        assert_eq!(ShadowQuality::from_name("low").size(), 1024);
        assert_eq!(ShadowQuality::from_name("medium").size(), 2048);
        assert_eq!(ShadowQuality::from_name("high").size(), 4096);
        assert_eq!(ShadowQuality::from_name("ultra"), ShadowQuality::default());
        assert_eq!(ShadowQuality::from_name(""), ShadowQuality::Medium);
        assert!(texel(4096) < texel(2048) && texel(2048) < texel(1024));
        assert_eq!(texel(0), texel(1), "a size of 0 does not divide by zero");
    }

    #[test]
    fn the_uniform_carries_strength_texel_and_size() {
        let u = uniform(Vec3::ZERO, Vec3::Z, 0.5, SIZE);
        assert_eq!(u.params, [0.5, TEXEL_T, SIZE as f32, 0.0]);
        assert_eq!(uniform(Vec3::ZERO, Vec3::Z, 7.0, SIZE).params[0], 1.0);
        assert!((u.center[3] - 1.0 / RADIUS).abs() < 1e-9);
    }
}
