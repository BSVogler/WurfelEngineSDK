//! What happens to the picture after the scene is drawn and its depth peeling layers are blended
//! (`peel.rs`): bloom, the tone map and FXAA.
//!
//! ```text
//! blended HDR picture ──► bloom (bloom.wgsl: cut, blur down, blur up) ─┐
//!        └────────────────────────────────────────────────────────────►├─► tone map (tonemap.wgsl) ─► FXAA (fxaa.wgsl) ─► canvas
//! ```
//!
//! With `linear` the scene shader writes linear light (`shader.wgsl`, the `flat_shades.w` flag), so
//! the layers blend the way light does and the bloom adds light, and the tone map encodes it to the
//! display at the end. The settings come from the graphics section of the menu (see
//! [`PostSettings::from_menu`]) and apply at once, in the running game.
//!
//! The shaders are plain WGSL, the same on every backend; this file builds the passes (browser only)
//! and holds what a native test can check (the settings and that the shaders are valid).

#![cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]

use bytemuck::{Pod, Zeroable};

/// How strong the bloom is when the page does not say (a share of the blurred bright part).
pub const DEFAULT_BLOOM: f32 = 0.1;
/// The most the page can ask for.
pub const MAX_BLOOM: f32 = 2.0;

/// What the post-process passes do. Everything can be switched off on its own.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PostSettings {
    /// Draw the scene in linear light and encode at the end (see the module doc).
    pub linear: bool,
    /// How much of the bloom is added; 0 skips the bloom passes.
    pub bloom: f32,
    /// Smooth the edges of the finished picture.
    pub fxaa: bool,
}

impl Default for PostSettings {
    fn default() -> Self {
        PostSettings { linear: false, bloom: DEFAULT_BLOOM, fxaa: true }
    }
}

impl PostSettings {
    /// The settings from the menu (`wurfelSettings`, see `menu.js`); what is missing keeps its default.
    /// `bloom` is clamped to 0..=[`MAX_BLOOM`] and a value that is not a number counts as missing.
    pub fn from_menu(linear: Option<bool>, bloom: Option<f64>, fxaa: Option<bool>) -> Self {
        let d = PostSettings::default();
        PostSettings {
            linear: linear.unwrap_or(d.linear),
            bloom: bloom.filter(|v| v.is_finite()).map_or(d.bloom, |v| (v as f32).clamp(0.0, MAX_BLOOM)),
            fxaa: fxaa.unwrap_or(d.fxaa),
        }
    }

    /// Without a 16 bit float target the layers are clipped at 1 (see `peel.rs`): linear light would
    /// band in the darks and there is nothing above 1 to glow.
    pub fn limited_by(self, hdr: bool) -> Self {
        if hdr {
            self
        } else {
            PostSettings { linear: false, bloom: 0.0, ..self }
        }
    }

    pub fn uniform(&self) -> PostUniform {
        PostUniform { params: [self.bloom, if self.linear { 1.0 } else { 0.0 }, 0.0, 0.0] }
    }
}

/// `Post` in `tonemap.wgsl`.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub struct PostUniform {
    /// x: bloom intensity, y: 1 for linear light, zw: unused.
    pub params: [f32; 4],
}

/// Browser only: the textures and passes.
#[cfg(target_arch = "wasm32")]
pub mod gpu {
    use super::*;

    /// How many times the bloom halves the picture. Five levels spread the glow over about
    /// 2^6 = 64 pixels of the screen.
    const LEVELS: usize = 5;

    /// Everything that depends on the size of the canvas.
    struct Targets {
        /// The bloom levels: level n is `2^(n + 1)` times smaller than the screen.
        levels: Vec<wgpu::TextureView>,
        /// Reads the blended picture; the first bloom pass.
        prefilter_group: wgpu::BindGroup,
        /// `down_groups[n]` reads level n and is drawn into level n + 1.
        down_groups: Vec<wgpu::BindGroup>,
        /// `up_groups[n]` reads level n + 1 and is added onto level n.
        up_groups: Vec<wgpu::BindGroup>,
        /// The blended picture, the bloom (level 0), the sampler and the settings.
        tonemap_group: wgpu::BindGroup,
        /// What the tone map draws when FXAA follows.
        ldr: wgpu::TextureView,
        fxaa_group: wgpu::BindGroup,
    }

    pub struct Post {
        sampled_layout: wgpu::BindGroupLayout,
        tonemap_layout: wgpu::BindGroupLayout,
        sampler: wgpu::Sampler,
        uniform: wgpu::Buffer,
        prefilter_pipeline: wgpu::RenderPipeline,
        down_pipeline: wgpu::RenderPipeline,
        up_pipeline: wgpu::RenderPipeline,
        tonemap_pipeline: wgpu::RenderPipeline,
        fxaa_pipeline: wgpu::RenderPipeline,
        /// The format of the blended picture and of the bloom.
        color_format: wgpu::TextureFormat,
        screen_format: wgpu::TextureFormat,
        targets: Targets,
    }

    fn fullscreen_pipeline(
        device: &wgpu::Device,
        label: &str,
        module: &wgpu::ShaderModule,
        layout: &wgpu::PipelineLayout,
        fragment: &str,
        format: wgpu::TextureFormat,
        blend: Option<wgpu::BlendState>,
    ) -> wgpu::RenderPipeline {
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(label),
            layout: Some(layout),
            vertex: wgpu::VertexState { module, entry_point: Some("vs_main"), compilation_options: Default::default(), buffers: &[] },
            fragment: Some(wgpu::FragmentState {
                module,
                entry_point: Some(fragment),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState { format, blend, write_mask: wgpu::ColorWrites::ALL })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        })
    }

    impl Post {
        /// `color_format` is the format of the blended picture (`peel::gpu::HDR_FORMAT`, or the
        /// canvas format where the device can not blend into it), `screen_format` the canvas.
        pub fn new(
            device: &wgpu::Device,
            color_format: wgpu::TextureFormat,
            screen_format: wgpu::TextureFormat,
            blended: &wgpu::TextureView,
            width: u32,
            height: u32,
        ) -> Self {
            let texture_entry = |binding: u32, filterable: bool| wgpu::BindGroupLayoutEntry {
                binding,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            };
            let sampler_entry = |binding: u32| wgpu::BindGroupLayoutEntry {
                binding,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            };
            // A picture and a sampler: what the bloom passes and FXAA read.
            let sampled_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("post sampled"),
                entries: &[texture_entry(0, true), sampler_entry(1)],
            });
            let tonemap_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("tone map"),
                entries: &[
                    texture_entry(0, false),
                    texture_entry(1, true),
                    sampler_entry(2),
                    wgpu::BindGroupLayoutEntry {
                        binding: 3,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
                        count: None,
                    },
                ],
            });
            let sampled_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("post sampled"),
                bind_group_layouts: &[Some(&sampled_layout)],
                immediate_size: 0,
            });
            let tonemap_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("tone map"),
                bind_group_layouts: &[Some(&tonemap_layout)],
                immediate_size: 0,
            });
            let module = |label: &str, source: &str| {
                device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some(label), source: wgpu::ShaderSource::Wgsl(source.into()) })
            };
            let bloom = module("bloom", include_str!("bloom.wgsl"));
            let tonemap = module("tone map", include_str!("tonemap.wgsl"));
            let fxaa = module("fxaa", include_str!("fxaa.wgsl"));
            let add = wgpu::BlendComponent { src_factor: wgpu::BlendFactor::One, dst_factor: wgpu::BlendFactor::One, operation: wgpu::BlendOperation::Add };
            let prefilter_pipeline = fullscreen_pipeline(device, "bloom prefilter", &bloom, &sampled_pipeline_layout, "fs_prefilter", color_format, None);
            let down_pipeline = fullscreen_pipeline(device, "bloom down", &bloom, &sampled_pipeline_layout, "fs_down", color_format, None);
            let up_pipeline = fullscreen_pipeline(
                device,
                "bloom up",
                &bloom,
                &sampled_pipeline_layout,
                "fs_up",
                color_format,
                Some(wgpu::BlendState { color: add, alpha: add }),
            );
            let tonemap_pipeline = fullscreen_pipeline(device, "tone map", &tonemap, &tonemap_pipeline_layout, "fs_main", screen_format, None);
            let fxaa_pipeline = fullscreen_pipeline(device, "fxaa", &fxaa, &sampled_pipeline_layout, "fs_main", screen_format, None);

            let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("post sampler"),
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                ..Default::default()
            });
            let uniform = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("post"),
                size: std::mem::size_of::<PostUniform>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let targets = Self::create_targets(device, &sampled_layout, &tonemap_layout, &sampler, &uniform, color_format, screen_format, blended, width, height);
            Post {
                sampled_layout,
                tonemap_layout,
                sampler,
                uniform,
                prefilter_pipeline,
                down_pipeline,
                up_pipeline,
                tonemap_pipeline,
                fxaa_pipeline,
                color_format,
                screen_format,
                targets,
            }
        }

        /// The canvas changed size; `blended` is the new blended picture of `Peeling`.
        pub fn resize(&mut self, device: &wgpu::Device, blended: &wgpu::TextureView, width: u32, height: u32) {
            self.targets = Self::create_targets(
                device,
                &self.sampled_layout,
                &self.tonemap_layout,
                &self.sampler,
                &self.uniform,
                self.color_format,
                self.screen_format,
                blended,
                width,
                height,
            );
        }

        #[allow(clippy::too_many_arguments)]
        fn create_targets(
            device: &wgpu::Device,
            sampled_layout: &wgpu::BindGroupLayout,
            tonemap_layout: &wgpu::BindGroupLayout,
            sampler: &wgpu::Sampler,
            uniform: &wgpu::Buffer,
            color_format: wgpu::TextureFormat,
            screen_format: wgpu::TextureFormat,
            blended: &wgpu::TextureView,
            width: u32,
            height: u32,
        ) -> Targets {
            let texture = |label: &str, format: wgpu::TextureFormat, width: u32, height: u32| {
                device
                    .create_texture(&wgpu::TextureDescriptor {
                        label: Some(label),
                        size: wgpu::Extent3d { width: width.max(1), height: height.max(1), depth_or_array_layers: 1 },
                        mip_level_count: 1,
                        sample_count: 1,
                        dimension: wgpu::TextureDimension::D2,
                        format,
                        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                        view_formats: &[],
                    })
                    .create_view(&Default::default())
            };
            let sampled_group = |label: &str, view: &wgpu::TextureView| {
                device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some(label),
                    layout: sampled_layout,
                    entries: &[
                        wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(view) },
                        wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(sampler) },
                    ],
                })
            };
            let levels: Vec<wgpu::TextureView> =
                (0..LEVELS).map(|n| texture("bloom level", color_format, width >> (n + 1), height >> (n + 1))).collect();
            let prefilter_group = sampled_group("bloom prefilter", blended);
            let down_groups = (0..LEVELS - 1).map(|n| sampled_group("bloom down", &levels[n])).collect();
            let up_groups = (0..LEVELS - 1).map(|n| sampled_group("bloom up", &levels[n + 1])).collect();
            let tonemap_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("tone map"),
                layout: tonemap_layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(blended) },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&levels[0]) },
                    wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(sampler) },
                    wgpu::BindGroupEntry { binding: 3, resource: uniform.as_entire_binding() },
                ],
            });
            let ldr = texture("tone mapped", screen_format, width, height);
            let fxaa_group = sampled_group("fxaa", &ldr);
            Targets { levels, prefilter_group, down_groups, up_groups, tonemap_group, ldr, fxaa_group }
        }

        /// Run the passes on the blended picture and draw the result onto `screen`.
        pub fn render(&self, encoder: &mut wgpu::CommandEncoder, queue: &wgpu::Queue, screen: &wgpu::TextureView, settings: &PostSettings) {
            queue.write_buffer(&self.uniform, 0, bytemuck::bytes_of(&settings.uniform()));
            let targets = &self.targets;

            if settings.bloom > 0.0 {
                let pass = |encoder: &mut wgpu::CommandEncoder,
                            pipeline: &wgpu::RenderPipeline,
                            group: &wgpu::BindGroup,
                            target: &wgpu::TextureView,
                            load: wgpu::LoadOp<wgpu::Color>| {
                    let mut rp = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some("bloom"),
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                            view: target,
                            depth_slice: None,
                            resolve_target: None,
                            ops: wgpu::Operations { load, store: wgpu::StoreOp::Store },
                        })],
                        depth_stencil_attachment: None,
                        timestamp_writes: None,
                        occlusion_query_set: None,
                        multiview_mask: None,
                    });
                    rp.set_pipeline(pipeline);
                    rp.set_bind_group(0, group, &[]);
                    rp.draw(0..3, 0..1);
                };
                let clear = wgpu::LoadOp::Clear(wgpu::Color::BLACK);
                pass(encoder, &self.prefilter_pipeline, &targets.prefilter_group, &targets.levels[0], clear);
                for n in 0..LEVELS - 1 {
                    pass(encoder, &self.down_pipeline, &targets.down_groups[n], &targets.levels[n + 1], clear);
                }
                for n in (0..LEVELS - 1).rev() {
                    pass(encoder, &self.up_pipeline, &targets.up_groups[n], &targets.levels[n], wgpu::LoadOp::Load);
                }
            }

            let fullscreen = |encoder: &mut wgpu::CommandEncoder, label: &str, pipeline: &wgpu::RenderPipeline, group: &wgpu::BindGroup, target: &wgpu::TextureView| {
                let mut rp = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some(label),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: target,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                rp.set_pipeline(pipeline);
                rp.set_bind_group(0, group, &[]);
                rp.draw(0..3, 0..1);
            };
            if settings.fxaa {
                fullscreen(encoder, "tone map", &self.tonemap_pipeline, &targets.tonemap_group, &targets.ldr);
                fullscreen(encoder, "fxaa", &self.fxaa_pipeline, &targets.fxaa_group, screen);
            } else {
                fullscreen(encoder, "tone map", &self.tonemap_pipeline, &targets.tonemap_group, screen);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn validated(label: &str, source: &str) -> naga::Module {
        let module = naga::front::wgsl::parse_str(source).unwrap_or_else(|e| panic!("{label} does not parse:\n{}", e.emit_to_string(source)));
        naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::empty())
            .validate(&module)
            .unwrap_or_else(|e| panic!("{label} is not valid:\n{}", e.emit_to_string(source)));
        module
    }

    fn entry_points(module: &naga::Module) -> Vec<(String, naga::ShaderStage)> {
        module.entry_points.iter().map(|e| (e.name.clone(), e.stage)).collect()
    }

    fn bindings(module: &naga::Module) -> Vec<(u32, u32)> {
        module.global_variables.iter().filter_map(|(_, v)| v.binding.as_ref().map(|b| (b.group, b.binding))).collect()
    }

    #[test]
    fn the_tone_map_shader_binds_the_picture_the_bloom_the_sampler_and_the_settings() {
        let module = validated("tonemap.wgsl", include_str!("tonemap.wgsl"));
        let entries = entry_points(&module);
        assert!(entries.contains(&("vs_main".into(), naga::ShaderStage::Vertex)), "{entries:?}");
        assert!(entries.contains(&("fs_main".into(), naga::ShaderStage::Fragment)), "{entries:?}");
        assert_eq!(bindings(&module), vec![(0, 0), (0, 1), (0, 2), (0, 3)]);
        // `Post` is one vec4, like `PostUniform`.
        let post = module.types.iter().find(|(_, t)| t.name.as_deref() == Some("Post")).expect("struct Post").1;
        let naga::TypeInner::Struct { span, .. } = &post.inner else { panic!("Post is not a struct") };
        assert_eq!(*span as usize, std::mem::size_of::<PostUniform>());
    }

    #[test]
    fn the_tone_map_constants_are_the_reference_ones() {
        let source = include_str!("tonemap.wgsl");
        assert!(source.contains(&format!("const KNEE = {:.1};", wurfel_sim::light::TONE_KNEE)));
        assert!(source.contains(&format!("const DESATURATION = {:.2};", wurfel_sim::light::TONE_DESATURATION)));
    }

    #[test]
    fn the_scene_shader_encodes_with_the_same_gamma_as_the_tone_map() {
        assert!(include_str!("tonemap.wgsl").contains("const GAMMA = 2.2;"));
        assert!(include_str!("shader.wgsl").contains("vec3<f32>(2.2)"));
    }

    #[test]
    fn the_bloom_shader_has_one_pass_per_entry_point_and_reads_one_texture() {
        let module = validated("bloom.wgsl", include_str!("bloom.wgsl"));
        let entries = entry_points(&module);
        for name in ["fs_prefilter", "fs_down", "fs_up"] {
            assert!(entries.contains(&(name.into(), naga::ShaderStage::Fragment)), "{name} missing: {entries:?}");
        }
        assert!(entries.contains(&("vs_main".into(), naga::ShaderStage::Vertex)), "{entries:?}");
        assert_eq!(bindings(&module), vec![(0, 0), (0, 1)]);
    }

    #[test]
    fn the_fxaa_shader_reads_one_texture_with_a_sampler() {
        let module = validated("fxaa.wgsl", include_str!("fxaa.wgsl"));
        let entries = entry_points(&module);
        assert!(entries.contains(&("vs_main".into(), naga::ShaderStage::Vertex)), "{entries:?}");
        assert!(entries.contains(&("fs_main".into(), naga::ShaderStage::Fragment)), "{entries:?}");
        assert_eq!(bindings(&module), vec![(0, 0), (0, 1)]);
    }

    #[test]
    fn the_new_look_is_the_default() {
        let d = PostSettings::default();
        assert!(!d.linear, "the art was made for blending display colours: linear light is opt-in");
        assert!(d.fxaa && d.bloom > 0.0);
        assert_eq!(PostSettings::from_menu(None, None, None), d);
    }

    #[test]
    fn each_menu_value_changes_one_thing() {
        let d = PostSettings::default();
        assert_eq!(PostSettings::from_menu(None, None, Some(false)), PostSettings { fxaa: false, ..d });
        assert_eq!(PostSettings::from_menu(Some(false), None, None), PostSettings { linear: false, ..d });
        assert_eq!(PostSettings::from_menu(None, Some(0.0), None), PostSettings { bloom: 0.0, ..d });
        assert_eq!(PostSettings::from_menu(None, Some(0.25), None), PostSettings { bloom: 0.25, ..d });
    }

    #[test]
    fn nonsense_from_the_menu_is_ignored_or_clamped() {
        let d = PostSettings::default();
        assert_eq!(PostSettings::from_menu(None, Some(f64::NAN), None), d);
        assert_eq!(PostSettings::from_menu(None, Some(f64::INFINITY), None), d);
        assert_eq!(PostSettings::from_menu(None, Some(99.0), None).bloom, MAX_BLOOM);
        assert_eq!(PostSettings::from_menu(None, Some(-3.0), None).bloom, 0.0);
    }

    #[test]
    fn without_a_float_target_there_is_no_linear_light_and_no_bloom() {
        let limited = PostSettings::default().limited_by(false);
        assert!(!limited.linear);
        assert_eq!(limited.bloom, 0.0);
        assert!(limited.fxaa, "FXAA works on any target");
        assert_eq!(PostSettings::default().limited_by(true), PostSettings::default());
    }

    #[test]
    fn the_uniform_carries_the_settings() {
        let u = PostSettings { linear: true, bloom: 0.25, fxaa: true }.uniform();
        assert_eq!(u.params, [0.25, 1.0, 0.0, 0.0]);
        assert_eq!(PostSettings { linear: false, bloom: 0.0, fxaa: false }.uniform().params, [0.0; 4]);
    }
}
