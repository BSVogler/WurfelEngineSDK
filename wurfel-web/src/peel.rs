//! Depth peeling, the way the Java engine orders what it draws (`GameView.depthPeelingRendering`
//! with `fragment_DP.fs`).
//!
//! # The algorithm
//!
//! One depth buffer shows only the nearest surface of every pixel, and with no blending a
//! translucent surface (water, the soft edge of a sprite) can neither show what is behind it nor be
//! drawn in the right order relative to it. Depth peeling draws the whole scene once per layer,
//! in any order, and peels the scene one surface at a time:
//!
//! - Layer 0 is the plain nearest surface of every pixel.
//! - Layer `n` discards every fragment that is not behind (by more than [`EPSILON`]) the fragment
//!   layer `n - 1` kept at that pixel, and keeps the nearest of the rest. The depth of layer
//!   `n - 1` is a texture the fragment shader reads; two depth buffers swap roles each layer, one
//!   written and one read (`depthTexture` and `depthTexture1` in Java).
//! - Fragments that are (nearly) transparent are discarded in every layer ([`MIN_ALPHA`], 0.1 as in
//!   Java), the others keep their alpha.
//! - The layers are blended onto the screen from the farthest to the nearest.
//!
//! The Java engine uses [`LAYERS`] = 2 as well. A pixel with three translucent surfaces on top of
//! each other shows the nearest two; what lies behind them is lost.
//!
//! # Depth
//!
//! The depth is a linear function of the position in the world, the same one as in Java (where it
//! is `y + z * PROJECTIONFACTORZ`, see `AbstractGameObject.getDepth`): along the view direction of
//! the isometric camera. Here that is `x + y + 0.82 z` in the ground frame (`sprites::depth`), and
//! the vertex shader maps it to the window depth with [`clip_depth`], nearer is smaller.
//!
//! Everything in this file is plain arithmetic, tested natively, and mirrored by `shader.wgsl`
//! (the per-fragment test) and `web.rs` (the passes); `gpu` below holds the textures and passes.

#![cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]

use bytemuck::{Pod, Zeroable};

/// How many surfaces per pixel are kept (`numDPLayers` in Java).
pub const LAYERS: usize = 2;

/// Fragments with an alpha at or below this are discarded in every layer (Java: `<= 0.1`).
pub const MIN_ALPHA: f32 = 0.1;

/// The depth margin of the peeling test, in window depth. Java uses 0.00008 of a 2200 unit range
/// (`gl_FragCoord.z-0.00008 <= previous`); this is 0.01 depth units of [`DEPTH_SCALE`], well below
/// the distance between two surfaces and well above the rounding of the 32 bit depth buffer.
pub const EPSILON: f32 = 0.01 * DEPTH_SCALE;

/// What the depth buffers are cleared to: the far plane, and "nothing kept here" for the layer after.
pub const CLEAR_DEPTH: f32 = 1.0;

/// Window depth per unit of view depth (`depth * 0.002` in the vertex shader).
pub const DEPTH_SCALE: f32 = 0.002;

/// The window depth of a view depth (`sprites::depth` minus the camera's), nearer is smaller.
#[cfg(test)]
pub fn clip_depth(depth: f32) -> f32 {
    0.5 - depth * DEPTH_SCALE
}

/// What one pass of layer `n` reads and writes. The two depth buffers alternate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LayerPass {
    /// The layer, 0 for the nearest surface.
    pub layer: usize,
    /// Index (0 or 1) of the depth buffer this layer writes.
    pub depth_write: usize,
    /// Index of the depth buffer holding the layer before; meaningless (and ignored by the
    /// shader, see [`LayerPass::uniform`]) for layer 0, it is only bound because the shader needs
    /// some texture there.
    pub depth_read: usize,
}

impl LayerPass {
    pub fn new(layer: usize) -> Self {
        LayerPass { layer, depth_write: layer % 2, depth_read: (layer + 1) % 2 }
    }

    /// The `Peel` uniform of the shader for this pass.
    pub fn uniform(&self) -> PeelUniform {
        PeelUniform { params: [if self.layer > 0 { 1.0 } else { 0.0 }, EPSILON, MIN_ALPHA, 0.0] }
    }
}

/// All passes, nearest layer first.
pub fn passes() -> impl Iterator<Item = LayerPass> {
    (0..LAYERS).map(LayerPass::new)
}

/// The layers in the order they are blended onto the screen: farthest first.
pub fn composite_order() -> impl Iterator<Item = usize> {
    (0..LAYERS).rev()
}

/// Mirror of the `Peel` struct in `shader.wgsl`.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
pub struct PeelUniform {
    /// x: 1 when an earlier layer exists, y: [`EPSILON`], z: [`MIN_ALPHA`], w: unused.
    pub params: [f32; 4],
}

/// Does a layer keep a fragment of window depth `z` and `alpha`? `previous` is the depth the layer
/// before kept at that pixel (`CLEAR_DEPTH` if it kept none), `None` for layer 0. This is the test
/// at the end of `fs_main`.
#[cfg(test)]
pub fn keeps(z: f32, alpha: f32, previous: Option<f32>) -> bool {
    alpha > MIN_ALPHA && previous.is_none_or(|behind| z - EPSILON > behind)
}

/// A fragment of one pixel: its window depth and straight (not premultiplied) colour with alpha.
#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Fragment {
    pub z: f32,
    pub color: [f32; 4],
}

/// What the [`LAYERS`] passes keep at one pixel, given all the fragments drawn there in any order:
/// the reference of the GPU algorithm. A layer keeps the nearest fragment it does not discard; on
/// equal depth the one drawn first wins (the depth test is `Less`).
#[cfg(test)]
pub fn peel_pixel(fragments: &[Fragment]) -> [Option<Fragment>; LAYERS] {
    let mut kept = [None; LAYERS];
    for pass in passes() {
        let previous = match pass.layer {
            0 => None,
            n => Some(kept[n - 1].map_or(CLEAR_DEPTH, |f: Fragment| f.z)),
        };
        let mut nearest: Option<Fragment> = None;
        for f in fragments.iter().filter(|f| keeps(f.z, f.color[3], previous)) {
            if nearest.is_none_or(|n| f.z < n.z) {
                nearest = Some(*f);
            }
        }
        kept[pass.layer] = nearest;
    }
    kept
}

/// The colour of a pixel: the layers blended over `background`, farthest first, with the standard
/// alpha blending of the composite pass.
#[cfg(test)]
pub fn composite(layers: &[Option<Fragment>; LAYERS], background: [f32; 3]) -> [f32; 3] {
    let mut out = background;
    for n in composite_order() {
        if let Some(f) = layers[n] {
            let a = f.color[3];
            for c in 0..3 {
                out[c] = f.color[c] * a + out[c] * (1.0 - a);
            }
        }
    }
    out
}

/// The textures and passes. Needs a GPU, so browser only.
#[cfg(target_arch = "wasm32")]
pub mod gpu {
    use super::*;

    pub const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
    /// The format the layers and their blend are kept in, so that a lit colour above 1 survives
    /// until the tone map instead of being clipped to the 8 bits of the canvas.
    pub const HDR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;

    /// Everything that depends on the size of the canvas.
    struct Targets {
        depth: [wgpu::TextureView; 2],
        /// The colour of each layer (straight alpha, cleared to transparent).
        colors: Vec<wgpu::TextureView>,
        /// Group 2 of the scene shader per layer: the peel uniform and the previous layer's depth.
        peel_groups: Vec<wgpu::BindGroup>,
        /// Group 0 of the composite shader per layer: that layer's colour.
        composite_groups: Vec<wgpu::BindGroup>,
        /// The layers blended together, in the HDR format; the post-process passes read it (`post.rs`).
        blended: wgpu::TextureView,
    }

    pub struct Peeling {
        /// The format of the layers and of the blended picture (see [`HDR_FORMAT`]).
        color_format: wgpu::TextureFormat,
        /// Group 2 of the scene pipeline.
        peel_layout: wgpu::BindGroupLayout,
        composite_layout: wgpu::BindGroupLayout,
        composite_pipeline: wgpu::RenderPipeline,
        uniforms: Vec<wgpu::Buffer>,
        targets: Targets,
    }

    impl Peeling {
        /// The layers are kept in `color_format`, which is [`HDR_FORMAT`] when the device can render
        /// and blend into it and otherwise the canvas format (then colours above 1 are clipped before
        /// the tone map, as they were before it existed).
        pub fn new(
            device: &wgpu::Device,
            queue: &wgpu::Queue,
            color_format: wgpu::TextureFormat,
            width: u32,
            height: u32,
        ) -> Self {
            let peel_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("peel layout"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Depth,
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                ],
            });
            let composite_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("composite layout"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                }],
            });
            let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("composite"),
                source: wgpu::ShaderSource::Wgsl(include_str!("composite.wgsl").into()),
            });
            let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("composite"),
                bind_group_layouts: &[Some(&composite_layout)],
                immediate_size: 0,
            });
            let composite_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("composite"),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_main"),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("fs_main"),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: color_format,
                        blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                multiview_mask: None,
                cache: None,
            });
            let uniforms: Vec<wgpu::Buffer> = passes()
                .map(|pass| {
                    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                        label: Some("peel"),
                        size: std::mem::size_of::<PeelUniform>() as u64,
                        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                        mapped_at_creation: false,
                    });
                    queue.write_buffer(&buffer, 0, bytemuck::bytes_of(&pass.uniform()));
                    buffer
                })
                .collect();
            let targets = Self::create_targets(device, &peel_layout, &composite_layout, &uniforms, color_format, width, height);
            Peeling { color_format, peel_layout, composite_layout, composite_pipeline, uniforms, targets }
        }

        /// Group 2 of the scene pipeline.
        pub fn peel_layout(&self) -> &wgpu::BindGroupLayout {
            &self.peel_layout
        }

        /// Group 2 of the scene pipeline for a pass that peels nothing (the first layer's).
        pub fn first_peel_group(&self) -> &wgpu::BindGroup {
            &self.targets.peel_groups[0]
        }

        /// The canvas changed size.
        pub fn resize(&mut self, device: &wgpu::Device, width: u32, height: u32) {
            self.targets = Self::create_targets(device, &self.peel_layout, &self.composite_layout, &self.uniforms, self.color_format, width, height);
        }

        fn create_targets(
            device: &wgpu::Device,
            peel_layout: &wgpu::BindGroupLayout,
            composite_layout: &wgpu::BindGroupLayout,
            uniforms: &[wgpu::Buffer],
            color_format: wgpu::TextureFormat,
            width: u32,
            height: u32,
        ) -> Targets {
            let texture = |label: &str, format: wgpu::TextureFormat| {
                device
                    .create_texture(&wgpu::TextureDescriptor {
                        label: Some(label),
                        size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
                        mip_level_count: 1,
                        sample_count: 1,
                        dimension: wgpu::TextureDimension::D2,
                        format,
                        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                        view_formats: &[],
                    })
                    .create_view(&Default::default())
            };
            let depth = [texture("peel depth 0", DEPTH_FORMAT), texture("peel depth 1", DEPTH_FORMAT)];
            let colors: Vec<wgpu::TextureView> = passes().map(|_| texture("peel colour", color_format)).collect();
            let peel_groups = passes()
                .map(|pass| {
                    device.create_bind_group(&wgpu::BindGroupDescriptor {
                        label: Some("peel"),
                        layout: peel_layout,
                        entries: &[
                            wgpu::BindGroupEntry { binding: 0, resource: uniforms[pass.layer].as_entire_binding() },
                            wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&depth[pass.depth_read]) },
                        ],
                    })
                })
                .collect();
            let composite_groups = colors
                .iter()
                .map(|view| {
                    device.create_bind_group(&wgpu::BindGroupDescriptor {
                        label: Some("composite"),
                        layout: composite_layout,
                        entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(view) }],
                    })
                })
                .collect();
            let blended = texture("blended", color_format);
            Targets { depth, colors, peel_groups, composite_groups, blended }
        }

        /// The format the scene pipeline must draw in.
        pub fn color_format(&self) -> wgpu::TextureFormat {
            self.color_format
        }

        /// The depth of the nearest surface of every pixel (layer 0's depth buffer, which the later
        /// layers do not write), for the depth of field. A new texture after [`Self::resize`].
        pub fn nearest_depth(&self) -> &wgpu::TextureView {
            &self.targets.depth[0]
        }

        /// The blended picture, which [`Self::render`] leaves for the post-process passes (`post.rs`).
        /// A new texture after [`Self::resize`].
        pub fn blended(&self) -> &wgpu::TextureView {
            &self.targets.blended
        }

        /// Draw the scene once per layer and blend the layers over `background` into [`Self::blended`].
        /// `draw` issues the draw calls of the scene (pipeline, groups 0 and 1, buffers); it runs
        /// once per layer and this sets group 2.
        pub fn render(
            &self,
            encoder: &mut wgpu::CommandEncoder,
            background: wgpu::Color,
            timer: Option<&crate::gputime::GpuTimer>,
            mut draw: impl FnMut(&mut wgpu::RenderPass<'_>),
        ) {
            for pass in passes() {
                let mut rp = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("peel layer"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &self.targets.colors[pass.layer],
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Store },
                    })],
                    depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                        view: &self.targets.depth[pass.depth_write],
                        depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(CLEAR_DEPTH), store: wgpu::StoreOp::Store }),
                        stencil_ops: None,
                    }),
                    timestamp_writes: crate::gputime::GpuTimer::writes(timer, "scene (depth peeling)"),
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                rp.set_bind_group(2, &self.targets.peel_groups[pass.layer], &[]);
                draw(&mut rp);
            }
            let mut rp = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("composite"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.targets.blended,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(background), store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: crate::gputime::GpuTimer::writes(timer, "scene composite"),
                occlusion_query_set: None,
                multiview_mask: None,
            });
            rp.set_pipeline(&self.composite_pipeline);
            for layer in composite_order() {
                rp.set_bind_group(0, &self.targets.composite_groups[layer], &[]);
                rp.draw(0..3, 0..1);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Vec3;

    fn frag(z: f32, color: [f32; 4]) -> Fragment {
        Fragment { z, color }
    }

    const RED: [f32; 4] = [1.0, 0.0, 0.0, 1.0];
    const BLUE: [f32; 4] = [0.0, 0.0, 1.0, 1.0];
    const BG: [f32; 3] = [0.0, 0.0, 0.0];

    #[test]
    fn the_depth_buffers_swap_roles_every_layer() {
        let all: Vec<LayerPass> = passes().collect();
        assert_eq!(all.len(), LAYERS);
        for pass in &all {
            // A texture can not be written and read in the same pass.
            assert_ne!(pass.depth_write, pass.depth_read);
        }
        // What a layer reads is what the layer before wrote.
        for pair in all.windows(2) {
            assert_eq!(pair[1].depth_read, pair[0].depth_write);
        }
    }

    #[test]
    fn only_the_first_layer_ignores_the_previous_depth() {
        assert_eq!(LayerPass::new(0).uniform().params[0], 0.0);
        for layer in 1..LAYERS {
            assert_eq!(LayerPass::new(layer).uniform().params[0], 1.0);
        }
        let u = LayerPass::new(1).uniform();
        assert_eq!((u.params[1], u.params[2]), (EPSILON, MIN_ALPHA));
    }

    #[test]
    fn the_layers_are_blended_from_far_to_near() {
        assert_eq!(composite_order().collect::<Vec<_>>(), (0..LAYERS).rev().collect::<Vec<_>>());
        assert_eq!(composite_order().last(), Some(0));
    }

    #[test]
    fn nearer_is_smaller_and_the_margin_is_small_in_view_depth() {
        let near = Vec3::new(5.0, 5.0, 3.0);
        let far = Vec3::new(4.0, 4.0, 3.0);
        assert!(clip_depth(crate::sprites::depth(near)) < clip_depth(crate::sprites::depth(far)));
        // The margin is a hundredth of a view depth unit (a block diagonal step is 2) and
        // far below the 0.05 a standing sprite is moved toward the camera (`sprites::billboard`).
        assert!((EPSILON / DEPTH_SCALE - 0.01).abs() < 1e-6);
        // The 32 bit depth buffer resolves the margin around the middle of the range.
        assert!(EPSILON > 100.0 * f32::EPSILON * 0.5);
        // Everything within 240 view depth units of the camera is inside the depth range.
        assert!(clip_depth(240.0) > 0.0 && clip_depth(-240.0) < 1.0);
    }

    #[test]
    fn the_shader_uses_the_same_depth_scale_and_the_peel_layout() {
        let src = include_str!("shader.wgsl");
        assert!(src.contains("depth * 0.002"), "DEPTH_SCALE no longer matches the vertex shader");
        let module = naga::front::wgsl::parse_str(src).unwrap();
        let peel = module.types.iter().find(|(_, t)| t.name.as_deref() == Some("Peel")).expect("struct Peel").1;
        let naga::TypeInner::Struct { members, span } = &peel.inner else { panic!("Peel is not a struct") };
        assert_eq!(members.len(), 1);
        assert_eq!(*span as usize, std::mem::size_of::<PeelUniform>());
    }

    #[test]
    fn an_opaque_surface_is_layer_zero_and_nothing_is_behind_it() {
        let layers = peel_pixel(&[frag(0.5, RED)]);
        assert_eq!(layers[0], Some(frag(0.5, RED)));
        assert!(layers[1..].iter().all(|l| l.is_none()));
        assert_eq!(composite(&layers, BG), [1.0, 0.0, 0.0]);
    }

    #[test]
    fn the_order_of_drawing_does_not_matter() {
        let water = frag(0.40, [0.2, 0.4, 0.8, 0.5]);
        let ground = frag(0.45, RED);
        let a = peel_pixel(&[water, ground]);
        let b = peel_pixel(&[ground, water]);
        assert_eq!(a, b);
        assert_eq!(a[0], Some(water));
        assert_eq!(a[1], Some(ground));
    }

    #[test]
    fn a_translucent_surface_shows_what_is_behind_it() {
        let water = frag(0.40, [0.0, 0.0, 1.0, 0.5]);
        let ground = frag(0.45, [1.0, 0.0, 0.0, 1.0]);
        let c = composite(&peel_pixel(&[ground, water]), BG);
        assert_eq!(c, [0.5, 0.0, 0.5]);
        // Without the layer behind, the translucent surface would only mix with the background.
        let alone = composite(&peel_pixel(&[water]), BG);
        assert_eq!(alone, [0.0, 0.0, 0.5]);
    }

    #[test]
    fn transparent_fragments_are_skipped_in_every_layer() {
        let hole = frag(0.1, [1.0, 1.0, 1.0, MIN_ALPHA]);
        let layers = peel_pixel(&[hole, frag(0.5, RED), frag(0.6, BLUE)]);
        assert_eq!(layers[0], Some(frag(0.5, RED)));
        assert_eq!(layers[1], Some(frag(0.6, BLUE)));
    }

    #[test]
    fn a_fragment_within_the_margin_of_the_layer_before_is_the_same_surface() {
        let layers = peel_pixel(&[frag(0.5, RED), frag(0.5 + EPSILON * 0.5, BLUE), frag(0.5 + EPSILON * 3.0, [0.0, 1.0, 0.0, 1.0])]);
        assert_eq!(layers[0], Some(frag(0.5, RED)));
        assert_eq!(layers[1].map(|f| f.color), Some([0.0, 1.0, 0.0, 1.0]));
    }

    #[test]
    fn equal_depth_keeps_the_first_drawn_like_the_less_test() {
        let layers = peel_pixel(&[frag(0.5, RED), frag(0.5, BLUE)]);
        assert_eq!(layers[0], Some(frag(0.5, RED)));
        assert_eq!(layers[1], None);
    }

    #[test]
    fn more_surfaces_than_layers_lose_the_farthest() {
        let fragments: Vec<Fragment> = (0..LAYERS + 2).map(|i| frag(0.3 + 0.1 * i as f32, [i as f32 / 10.0, 0.0, 0.0, 0.5])).collect();
        let layers = peel_pixel(&fragments);
        for (n, layer) in layers.iter().enumerate() {
            assert_eq!(*layer, Some(fragments[n]));
        }
    }

    #[test]
    fn an_empty_pixel_shows_the_background() {
        assert_eq!(composite(&peel_pixel(&[]), [0.1, 0.2, 0.3]), [0.1, 0.2, 0.3]);
    }

    #[test]
    fn the_keep_test_matches_the_shader_rule() {
        // Layer 0: everything not transparent.
        assert!(keeps(0.7, 1.0, None));
        assert!(!keeps(0.7, 0.1, None));
        // Later layers: strictly behind the previous depth by more than the margin.
        assert!(keeps(0.5 + 2.0 * EPSILON, 1.0, Some(0.5)));
        assert!(!keeps(0.5 + 0.5 * EPSILON, 1.0, Some(0.5)));
        assert!(!keeps(0.4, 1.0, Some(0.5)));
        // Nothing kept before (cleared to 1): nothing can be behind it.
        assert!(!keeps(0.9, 1.0, Some(CLEAR_DEPTH)));
    }

    #[test]
    fn composite_shader_parses_and_validates() {
        let src = include_str!("composite.wgsl");
        let module = naga::front::wgsl::parse_str(src).unwrap_or_else(|e| panic!("composite.wgsl does not parse:\n{}", e.emit_to_string(src)));
        naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::empty())
            .validate(&module)
            .unwrap_or_else(|e| panic!("composite.wgsl is not valid:\n{}", e.emit_to_string(src)));
        let entry_points: Vec<_> = module.entry_points.iter().map(|e| (e.name.as_str(), e.stage)).collect();
        assert!(entry_points.contains(&("vs_main", naga::ShaderStage::Vertex)), "{entry_points:?}");
        assert!(entry_points.contains(&("fs_main", naga::ShaderStage::Fragment)), "{entry_points:?}");
        let bindings: Vec<(u32, u32)> = module.global_variables.iter().filter_map(|(_, g)| g.binding.as_ref().map(|b| (b.group, b.binding))).collect();
        assert_eq!(bindings, vec![(0, 0)]);
    }
}
