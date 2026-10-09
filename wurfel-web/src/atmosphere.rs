//! Atmosphere and life: ambient particles, weather, fog and god ray sprites (`atmosphere.wgsl`), and
//! the settings that switch them. The maths that is pure is `wurfel_sim::atmosphere`.
//!
//! Every thing is one instance of a quad whose place, size and brightness the vertex shader derives
//! from the instance number and a time uniform, so the CPU only decides how many instances of which
//! kind to draw ([`plan`]), writes two small uniforms and keeps the column map (the height and kind of
//! the top block of every column near the viewer) up to date, a few rows per frame ([`Columns`]).
//! The pass draws into the blended HDR picture after the scene and before the post-process passes, and
//! reads the nearest depth of every pixel to hide what is behind the ground.
//!

#![cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]

use bytemuck::{Pod, Zeroable};
use wurfel_sim::atmosphere::{self as sim, Kind, Strengths, Weather};
use wurfel_sim::World;

/// The column map is `SIZE` x `SIZE` columns, rebuilt when the viewer is `STEP` columns from its middle.
pub const SIZE: usize = 96;
pub const STEP: i32 = 16;
/// Rows of the column map filled per frame.
const ROWS_PER_FRAME: usize = 12;

/// The menu's and the page address's choices.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Settings {
    /// Pollen, fireflies, dust motes, leaves and mist.
    pub ambient: bool,
    /// 0 to [`sim::MAX_DENSITY`]; 1 is the default.
    pub ambient_density: f32,
    pub weather: Weather,
    pub weather_density: f32,
    /// The fog banks and god rays.
    pub volumetrics: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { ambient: true, ambient_density: 1.0, weather: Weather::Clear, weather_density: 1.0, volumetrics: true }
    }
}

/// `0`, `off` and `false` are off, anything else on (the way `?grass=` reads).
pub fn parse_flag(text: &str) -> bool {
    !matches!(text.trim().to_ascii_lowercase().as_str(), "0" | "off" | "false" | "no")
}

/// A density from the menu or the address, clamped; not a number gives `None`.
pub fn parse_density(text: &str) -> Option<f32> {
    text.trim().parse::<f32>().ok().filter(|v| v.is_finite()).map(clamp_density)
}

pub fn clamp_density(v: f32) -> f32 {
    v.clamp(0.0, sim::MAX_DENSITY)
}

/// What one draw call draws.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Draw {
    pub kind: Kind,
    pub count: u32,
    /// How present it is, 0..1 (the shader switches instances on one by one as it grows).
    pub amount: f32,
}

/// The draw calls of a frame for the settings and the sun's height; kinds with nothing to show are left out.
pub fn plan(settings: &Settings, sun_z: f32) -> Vec<Draw> {
    let strengths = Strengths::at(sun_z, settings.weather);
    let mut out = Vec::new();
    let mut add = |kind: Kind, density: f32, amount: f32| {
        let count = kind.count(density);
        if count > 0 && amount > 0.01 {
            out.push(Draw { kind, count, amount: amount.min(1.0) });
        }
    };
    if settings.volumetrics {
        add(Kind::Fog, settings.ambient_density, strengths.fog);
        add(Kind::GodRay, settings.ambient_density, strengths.god_rays);
    }
    if settings.ambient {
        let d = settings.ambient_density;
        add(Kind::Mist, d, strengths.mist);
        add(Kind::Mote, d, strengths.motes);
        add(Kind::Pollen, d, strengths.pollen);
        add(Kind::Leaf, d, strengths.leaves);
        add(Kind::Firefly, d, strengths.fireflies);
    }
    match settings.weather {
        Weather::Clear => {}
        Weather::Rain => {
            add(Kind::Splash, settings.weather_density, 1.0);
            add(Kind::Rain, settings.weather_density, 1.0);
        }
        Weather::Snow => add(Kind::Snow, settings.weather_density, 1.0),
    }
    out
}

/// `Atmos` in `atmosphere.wgsl`.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub struct AtmosUniform {
    /// x: seconds, y: 1 for linear light.
    pub clock: [f32; 4],
    /// xyz: the viewer in the ground frame, w: 1 when the column map is valid.
    pub viewer: [f32; 4],
    /// xy: the lattice point of texel (0, 0), z: the size of the map.
    pub map: [f32; 4],
    /// xy: the wind's direction, z: how fast it carries light things, w: the gust floor.
    pub wind: [f32; 4],
    /// xy: the gust's phase per block, z: radians per second.
    pub gust: [f32; 4],
    /// x: depth of field strength, y: the view depth in focus.
    pub dof: [f32; 4],
}

impl AtmosUniform {
    pub fn new(time: f32, linear: bool, viewer: [f32; 3], map: Option<(i32, i32)>, dof: f32, focus_depth: f32, wind: (f32, f32)) -> Self {
        let phase = sim::gust_phase_per_block();
        let origin = map.unwrap_or((0, 0));
        AtmosUniform {
            clock: [time, if linear { 1.0 } else { 0.0 }, 0.0, 0.0],
            viewer: [viewer[0], viewer[1], viewer[2], if map.is_some() { 1.0 } else { 0.0 }],
            map: [origin.0 as f32, origin.1 as f32, SIZE as f32, 0.0],
            wind: [wind.0, wind.1, sim::DRIFT_SPEED, wurfel_sim::grass::GUST_MIN],
            gust: [phase.0, phase.1, sim::GUST_RATE, 0.0],
            dof: [dof, focus_depth, 0.0, 0.0],
        }
    }
}

/// `Layer` in `atmosphere.wgsl`.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub struct LayerUniform {
    pub params: [f32; 4],
}

impl Draw {
    pub fn uniform(&self) -> LayerUniform {
        LayerUniform { params: [self.kind as u32 as f32, self.count as f32, self.amount, 0.0] }
    }
}

/// Keeps the column map current: builds a new one a few rows per frame when the viewer has moved
/// on or the terrain changed, and hands it over when it is complete.
#[derive(Default)]
pub struct Columns {
    /// The origin of the map the GPU holds.
    pub origin: Option<(i32, i32)>,
    terrain: u64,
    job: Option<Job>,
}

struct Job {
    origin: (i32, i32),
    terrain: u64,
    next_row: usize,
    data: Vec<u16>,
}

impl Columns {
    /// Advance by one frame. `viewer` is in the ground frame. Returns a finished map to upload (and the
    /// origin it belongs to).
    pub fn update(&mut self, world: &World, terrain_version: u64, viewer: (f32, f32)) -> Option<((i32, i32), Vec<u16>)> {
        let wanted = sim::map_origin(viewer, SIZE, STEP);
        if self.job.is_none() && (self.origin != Some(wanted) || self.terrain != terrain_version) {
            self.job = Some(Job { origin: wanted, terrain: terrain_version, next_row: 0, data: vec![0; SIZE * SIZE] });
        }
        let job = self.job.as_mut()?;
        let end = (job.next_row + ROWS_PER_FRAME).min(SIZE);
        sim::fill_rows(world, job.origin, SIZE, job.next_row..end, &mut job.data);
        job.next_row = end;
        if end < SIZE {
            return None;
        }
        let done = self.job.take()?;
        self.origin = Some(done.origin);
        self.terrain = done.terrain;
        Some((done.origin, done.data))
    }
}

#[cfg(target_arch = "wasm32")]
pub mod gpu {
    use super::*;

    /// Draws the atmosphere into the blended picture.
    pub struct Atmosphere {
        pipeline: wgpu::RenderPipeline,
        layout: wgpu::BindGroupLayout,
        layer_group: wgpu::BindGroup,
        atmos_buffer: wgpu::Buffer,
        layer_buffer: wgpu::Buffer,
        stride: u64,
        columns: wgpu::Texture,
        group: wgpu::BindGroup,
        camera: wgpu::Buffer,
        lighting: wgpu::Buffer,
    }

    fn uniform_entry(binding: u32, visibility: wgpu::ShaderStages) -> wgpu::BindGroupLayoutEntry {
        wgpu::BindGroupLayoutEntry {
            binding,
            visibility,
            ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
            count: None,
        }
    }

    impl Atmosphere {
        /// `camera` and `lighting` are the buffers of the scene shader, `depth` the nearest depth of
        /// [`crate::peel::gpu::Peeling`], `format` the format of its blended picture.
        pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, format: wgpu::TextureFormat, camera: &wgpu::Buffer, lighting: &wgpu::Buffer, depth: &wgpu::TextureView) -> Self {
            use wgpu::ShaderStages as S;
            let both = S::VERTEX | S::FRAGMENT;
            let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("atmosphere"),
                entries: &[
                    uniform_entry(0, both),
                    uniform_entry(1, both),
                    uniform_entry(2, both),
                    wgpu::BindGroupLayoutEntry {
                        binding: 3,
                        visibility: S::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Depth,
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 4,
                        visibility: S::VERTEX,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Uint,
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                ],
            });
            let layer_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("atmosphere layer"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: both,
                    ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: true, min_binding_size: None },
                    count: None,
                }],
            });
            let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("atmosphere"),
                source: wgpu::ShaderSource::Wgsl(include_str!("atmosphere.wgsl").into()),
            });
            let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("atmosphere"),
                bind_group_layouts: &[Some(&layout), Some(&layer_layout)],
                immediate_size: 0,
            });
            // Premultiplied: what is laid over the picture covers it by alpha, what is added has alpha 0.
            let over = wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                operation: wgpu::BlendOperation::Add,
            };
            let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("atmosphere"),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState { module: &module, entry_point: Some("vs_main"), compilation_options: Default::default(), buffers: &[] },
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some("fs_main"),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: Some(wgpu::BlendState { color: over, alpha: over }),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                multiview_mask: None,
                cache: None,
            });

            let atmos_buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("atmosphere"),
                size: std::mem::size_of::<AtmosUniform>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            // One slot per kind, a dynamic offset apart (the offset must be a multiple of the device's alignment).
            let stride = (device.limits().min_uniform_buffer_offset_alignment as u64).max(256);
            let layer_buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("atmosphere layers"),
                size: stride * Kind::ALL.len() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let layer_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("atmosphere layer"),
                layout: &layer_layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding { buffer: &layer_buffer, offset: 0, size: wgpu::BufferSize::new(std::mem::size_of::<LayerUniform>() as u64) }),
                }],
            });
            let columns = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("atmosphere columns"),
                size: wgpu::Extent3d { width: SIZE as u32, height: SIZE as u32, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::R16Uint,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            // Until the first map arrives every column is unknown, so nothing shows.
            queue.write_texture(
                wgpu::TexelCopyTextureInfo { texture: &columns, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
                bytemuck::cast_slice(&vec![sim::COLUMN_UNKNOWN; SIZE * SIZE]),
                wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(SIZE as u32 * 2), rows_per_image: Some(SIZE as u32) },
                wgpu::Extent3d { width: SIZE as u32, height: SIZE as u32, depth_or_array_layers: 1 },
            );
            let group = Self::make_group(device, &layout, camera, lighting, &atmos_buffer, depth, &columns);
            Atmosphere { pipeline, layout, layer_group, atmos_buffer, layer_buffer, stride, columns, group, camera: camera.clone(), lighting: lighting.clone() }
        }

        fn make_group(
            device: &wgpu::Device,
            layout: &wgpu::BindGroupLayout,
            camera: &wgpu::Buffer,
            lighting: &wgpu::Buffer,
            atmos: &wgpu::Buffer,
            depth: &wgpu::TextureView,
            columns: &wgpu::Texture,
        ) -> wgpu::BindGroup {
            let view = columns.create_view(&Default::default());
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("atmosphere"),
                layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: camera.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 1, resource: lighting.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 2, resource: atmos.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(depth) },
                    wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::TextureView(&view) },
                ],
            })
        }

        /// The canvas changed size: the depth is a new texture.
        pub fn resize(&mut self, device: &wgpu::Device, depth: &wgpu::TextureView) {
            self.group = Self::make_group(device, &self.layout, &self.camera, &self.lighting, &self.atmos_buffer, depth, &self.columns);
        }

        /// A finished column map ([`Columns::update`]).
        pub fn set_columns(&self, queue: &wgpu::Queue, data: &[u16]) {
            queue.write_texture(
                wgpu::TexelCopyTextureInfo { texture: &self.columns, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
                bytemuck::cast_slice(data),
                wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(SIZE as u32 * 2), rows_per_image: Some(SIZE as u32) },
                wgpu::Extent3d { width: SIZE as u32, height: SIZE as u32, depth_or_array_layers: 1 },
            );
        }

        /// Draw `draws` over `blended` (the pass keeps what is there).
        pub fn render(&self, encoder: &mut wgpu::CommandEncoder, queue: &wgpu::Queue, blended: &wgpu::TextureView, uniform: &AtmosUniform, draws: &[Draw], timer: Option<&crate::gputime::GpuTimer>) {
            if draws.is_empty() {
                return;
            }
            queue.write_buffer(&self.atmos_buffer, 0, bytemuck::bytes_of(uniform));
            for (slot, draw) in draws.iter().enumerate() {
                queue.write_buffer(&self.layer_buffer, slot as u64 * self.stride, bytemuck::bytes_of(&draw.uniform()));
            }
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("atmosphere"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: blended,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: crate::gputime::GpuTimer::writes(timer, "weather and ambient life"),
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.group, &[]);
            for (slot, draw) in draws.iter().enumerate() {
                pass.set_bind_group(1, &self.layer_group, &[(slot as u64 * self.stride) as u32]);
                pass.draw(0..6, 0..draw.count);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wurfel_sim::block::id;
    use wurfel_sim::{AirGenerator, Block};

    fn kinds(plan: &[Draw]) -> Vec<Kind> {
        plan.iter().map(|d| d.kind).collect()
    }

    #[test]
    fn flags_and_densities_read_like_the_grass_settings() {
        assert!(parse_flag("1") && parse_flag("on") && parse_flag("true") && parse_flag("anything"));
        assert!(!parse_flag("0") && !parse_flag("off") && !parse_flag("False"));
        assert_eq!(parse_density("1.5"), Some(1.5));
        assert_eq!(parse_density("99"), Some(sim::MAX_DENSITY));
        assert_eq!(parse_density("-4"), Some(0.0));
        assert_eq!(parse_density("lots"), None);
        assert_eq!(parse_density("NaN"), None);
    }

    #[test]
    fn noon_shows_pollen_and_dusk_shows_fireflies_and_beams() {
        let s = Settings::default();
        let noon = kinds(&plan(&s, 0.9));
        assert!(noon.contains(&Kind::Pollen) && !noon.contains(&Kind::Firefly) && !noon.contains(&Kind::GodRay));
        let dusk = kinds(&plan(&s, 0.12));
        assert!(dusk.contains(&Kind::Firefly) && dusk.contains(&Kind::GodRay) && dusk.contains(&Kind::Mote));
        let night = kinds(&plan(&s, -0.5));
        assert!(night.contains(&Kind::Firefly) && !night.contains(&Kind::Pollen));
    }

    #[test]
    fn switches_and_density_remove_things() {
        let off = Settings { ambient: false, volumetrics: false, ..Settings::default() };
        assert!(plan(&off, 0.12).is_empty(), "no weather, no ambient, no fog");
        let none = Settings { ambient_density: 0.0, ..Settings::default() };
        assert!(plan(&none, 0.12).is_empty());
        let fog_only = Settings { ambient: false, ..Settings::default() };
        assert!(kinds(&plan(&fog_only, 0.12)).iter().all(|k| matches!(k, Kind::Fog | Kind::GodRay)));
    }

    #[test]
    fn rain_and_snow_draw_their_own_kinds_within_the_caps() {
        let rain = Settings { weather: Weather::Rain, weather_density: sim::MAX_DENSITY, ..Settings::default() };
        let p = plan(&rain, 0.9);
        let find = |k: Kind| p.iter().find(|d| d.kind == k).copied();
        assert_eq!(find(Kind::Rain).unwrap().count, Kind::Rain.cap());
        assert_eq!(find(Kind::Splash).unwrap().count, Kind::Rain.cap(), "one ring for every drop");
        assert!(find(Kind::Snow).is_none() && find(Kind::GodRay).is_none());
        assert!(find(Kind::Pollen).map_or(true, |d| d.amount < 0.2), "rain takes the pollen away");
        let snow = Settings { weather: Weather::Snow, ..Settings::default() };
        assert!(kinds(&plan(&snow, 0.9)).contains(&Kind::Snow));
        assert!(!kinds(&plan(&snow, 0.9)).contains(&Kind::Rain));
        let total: u32 = plan(&rain, 0.12).iter().map(|d| d.count).sum();
        assert!(total <= Kind::ALL.iter().map(|k| k.cap()).sum::<u32>());
        // At most one draw per kind: the uniform buffer has one slot each.
        for s in [&rain, &snow, &Settings::default()] {
            assert!(plan(s, 0.12).len() <= Kind::ALL.len());
        }
    }

    #[test]
    fn the_uniforms_have_the_size_the_shader_expects() {
        assert_eq!(std::mem::size_of::<AtmosUniform>(), 6 * 16);
        assert_eq!(std::mem::size_of::<LayerUniform>(), 16);
        let u = AtmosUniform::new(2.0, true, [1.0, 2.0, 3.0], Some((-5, 7)), 0.5, 1.5, sim::wind_direction());
        assert_eq!((u.clock[0], u.clock[1], u.viewer[3], u.map[2]), (2.0, 1.0, 1.0, SIZE as f32));
        assert_eq!(AtmosUniform::new(0.0, false, [0.0; 3], None, 0.0, 0.0, sim::wind_direction()).viewer[3], 0.0);
        let d = Draw { kind: Kind::Snow, count: 10, amount: 0.5 };
        assert_eq!(d.uniform().params, [7.0, 10.0, 0.5, 0.0]);
    }

    #[test]
    fn the_column_map_is_built_over_frames_and_again_after_a_move_or_an_edit() {
        let mut world = World::new(AirGenerator);
        world.set(0, 0, 0, Block::new(id::GRASS, 0));
        let mut columns = Columns::default();
        let mut finished = None;
        let mut frames = 0;
        while finished.is_none() {
            frames += 1;
            assert!(frames <= SIZE / ROWS_PER_FRAME + 2, "it finishes");
            finished = columns.update(&world, 1, (0.0, 0.0));
        }
        assert!(frames > 1, "spread over several frames");
        let (origin, data) = finished.unwrap();
        assert_eq!(data.len(), SIZE * SIZE);
        assert_eq!(columns.origin, Some(origin));
        // Nothing to do while nothing changed.
        for _ in 0..20 {
            assert!(columns.update(&world, 1, (3.0, -2.0)).is_none());
        }
        // An edit starts a new one, and so does walking far.
        let rebuilt = |columns: &mut Columns, version: u64, at: (f32, f32)| (0..SIZE).find_map(|_| columns.update(&world, version, at));
        assert!(rebuilt(&mut columns, 2, (3.0, -2.0)).is_some());
        let far = rebuilt(&mut columns, 2, (60.0, 0.0)).expect("moved");
        assert_ne!(far.0, origin);
    }

    fn validated() -> naga::Module {
        let source = include_str!("atmosphere.wgsl");
        let module = naga::front::wgsl::parse_str(source).unwrap_or_else(|e| panic!("atmosphere.wgsl does not parse:\n{}", e.emit_to_string(source)));
        naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::empty())
            .validate(&module)
            .unwrap_or_else(|e| panic!("atmosphere.wgsl is not valid:\n{}", e.emit_to_string(source)));
        module
    }

    fn struct_size(module: &naga::Module, name: &str) -> usize {
        let t = module.types.iter().find(|(_, t)| t.name.as_deref() == Some(name)).unwrap_or_else(|| panic!("struct {name}")).1;
        let naga::TypeInner::Struct { span, .. } = &t.inner else { panic!("{name} is not a struct") };
        *span as usize
    }

    #[test]
    fn the_shader_is_valid_and_its_structs_match_the_uniforms() {
        let module = validated();
        assert_eq!(struct_size(&module, "Atmos"), std::mem::size_of::<AtmosUniform>());
        assert_eq!(struct_size(&module, "Layer"), std::mem::size_of::<LayerUniform>());
        // Same camera as the scene shader (`CameraUniform` in web.rs is 64 bytes), and a prefix of its lighting.
        assert_eq!(struct_size(&module, "Camera"), 64);
        assert!(struct_size(&module, "Lighting") <= std::mem::size_of::<crate::lighting::Lighting>());
        let entries: Vec<_> = module.entry_points.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(entries, ["vs_main", "fs_main"]);
    }

    #[test]
    fn the_shader_constants_are_the_simulation_ones() {
        let source = include_str!("atmosphere.wgsl");
        let consts = [
            ("RAIN_SPEED", sim::RAIN_SPEED),
            ("RAIN_FALL", sim::RAIN_FALL),
            ("RING_LIFE", sim::RING_LIFE),
            ("RAIN_SLANT", sim::RAIN_SLANT),
            ("SNOW_SPEED", sim::SNOW_SPEED),
            ("SNOW_FALL", sim::SNOW_FALL),
            ("SNOW_DRIFT", sim::SNOW_DRIFT),
        ];
        for (name, value) in consts {
            assert!(source.contains(&format!("const {name} = {value:?};")), "{name} no longer matches wurfel_sim::atmosphere ({value:?})");
        }
        for kind in Kind::ALL {
            let name = match kind {
                Kind::Pollen => "POLLEN",
                Kind::Firefly => "FIREFLY",
                Kind::Mote => "MOTE",
                Kind::Leaf => "LEAF",
                Kind::Mist => "MIST",
                Kind::Rain => "RAIN",
                Kind::Splash => "SPLASH",
                Kind::Snow => "SNOW",
                Kind::Fog => "FOG",
                Kind::GodRay => "GOD_RAY",
            };
            assert!(source.contains(&format!("const {name} = {};", kind as u32)), "{name}");
        }
        // The depth of field is the one of tonemap.wgsl, and the depth is what the scene writes.
        let tone = include_str!("tonemap.wgsl");
        for line in ["const FOCUS_HALF = 0.14;", "const FOCUS_FALLOFF = 0.36;", "const BLUR_MAX = 14.0;"] {
            assert!(source.contains(line) && tone.contains(line), "{line}");
        }
        assert!(source.contains("0.5 - depth * 0.002"));
    }
}
