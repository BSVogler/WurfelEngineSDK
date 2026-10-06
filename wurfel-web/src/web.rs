//! Browser glue: canvas, wgpu, input, networking and the frame loop.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use glam::Vec3;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{HtmlCanvasElement, KeyboardEvent, MessageEvent, MouseEvent, WebSocket, WheelEvent};
use wurfel_sim::entity::{Entities, EntityId};
use wurfel_sim::grid::{chunk_of, from_iso, to_iso};
use wurfel_sim::player::{apply_input, new_player, PlayerInput, PLAYER_HEIGHT, TICK_DT, TICK_RATE};
use wurfel_sim::protocol::{decode_chunk, ClientMsg, PlayerInfo, PlayerState, ServerMsg, ThingState};
use wurfel_sim::{Block, IslandGenerator, World};

use crate::actors::Actors;
use crate::audio::{Audio, EntityInfo};
use crate::bindings::{parse_hex_color, Bindings};
use crate::caveland_client::{self, Happening};
use crate::editor::{self, Button, Editor, Tool};
use crate::interp::{RenderClock, Track};
use crate::locator;
use crate::mesh::{self, Vertex};
use crate::model::Model;
use crate::prediction::{classify, replay, Correction, InputHistory, VisualOffset};
use crate::minimap::{CameraView, ChunkInfo, ChunkState, EntityDot, Minimap, MinimapData, Mode};
use crate::netstats::{format_report, NetStats};
use crate::peel::gpu::{Peeling, DEPTH_FORMAT};
use crate::pick::{pick, Pick};
use crate::reconnect::{Closed, Reconnect};
use crate::render_storage::RenderStorage;
use crate::texture;

/// Seed of the island shown behind the menu, before a world has been joined.
const DEFAULT_SEED: u64 = 1;
/// Most fires placed in the world at once (each costs a light and particles).
const MAX_EMITTERS: usize = 8;
const PLAYER_COLORS: [[f32; 3]; 6] = [
    [0.90, 0.30, 0.30],
    [0.95, 0.75, 0.20],
    [0.60, 0.35, 0.85],
    [0.20, 0.75, 0.75],
    [0.95, 0.50, 0.15],
    [0.90, 0.90, 0.95],
];
const MARKER_COLOR: [f32; 3] = [1.0, 0.92, 0.25];
/// Room for the players and the hover marker.
const DYNAMIC_VERTICES: u64 = 24576;

/// How often the latency probe is sent, and how often the overlays refresh (milliseconds).
const PING_EVERY_MS: f64 = 1000.0;
const NET_OVERLAY_EVERY_MS: f64 = 250.0;
const MINIMAP_EVERY_MS: f64 = 100.0;
const STATUS_EVERY_MS: f64 = 500.0;
const MINIMAP_SIZE: (u32, u32) = (150, 300);
/// Give up on a connection that has not delivered the world after this long.
const CONNECT_TIMEOUT_MS: f64 = 8000.0;

pub fn start() {
    console_error_panic_hook::set_once();
    expose_generator_preview();
    wasm_bindgen_futures::spawn_local(async {
        if let Err(message) = run().await {
            web_sys::console::error_1(&message.clone().into());
            set_info(&format!("Failed to start: {message}"));
        }
    });
}

/// `window.wurfelGeneratorPreview(id, seed)` for the "Create map" screen (menu.js): the RGBA pixels
/// of a `wurfelGeneratorPreviewSize` square top-down image of that generator, or `undefined`.
fn expose_generator_preview() {
    use crate::preview::{render, PREVIEW_SIZE};
    let Some(window) = web_sys::window() else { return };
    let preview = Closure::<dyn Fn(String, f64) -> JsValue>::new(|id: String, seed: f64| match render(&id, seed as u64, PREVIEW_SIZE) {
        Some(pixels) => js_sys::Uint8Array::from(pixels.as_slice()).into(),
        None => JsValue::UNDEFINED,
    });
    let _ = js_sys::Reflect::set(&window, &"wurfelGeneratorPreview".into(), preview.as_ref());
    let _ = js_sys::Reflect::set(&window, &"wurfelGeneratorPreviewSize".into(), &PREVIEW_SIZE.into());
    preview.forget();
}

fn set_info(text: &str) {
    if let Some(info) = web_sys::window().and_then(|w| w.document()).and_then(|d| d.get_element_by_id("info")) {
        info.set_text_content(Some(text));
    }
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct CameraUniform {
    center: [f32; 2],
    scale: [f32; 2],
    center_depth: f32,
    _pad: [f32; 3],
}

struct Camera {
    /// Screen-space point (px at zoom 1, y down) shown in the middle of the canvas.
    center: [f32; 2],
    zoom: f32,
}

/// Another player, smoothed between server snapshots.
struct Remote {
    /// Where it is drawn now: the snapshots interpolated at the render clock (see `interp`).
    pos: Vec3,
    track: Track,
}

struct State {
    // --- graphics
    canvas: HtmlCanvasElement,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipeline: wgpu::RenderPipeline,
    camera_buffer: wgpu::Buffer,
    lighting_buffer: wgpu::Buffer,
    lighting: crate::lighting::LightingController,
    audio: Audio,
    particles: wurfel_sim::particle::Particles,
    /// Fires and the like placed in the world; their light goes to the light engine.
    emitters: Vec<wurfel_sim::particle::ParticleEmitter>,
    /// The exhaust of our own jetpack, lit while Caveland's rules say it burns.
    jetpack: crate::particles::Jetpack,
    /// The Caveland ruleset, when the world is played by it (the local player is predicted with it).
    caveland: Option<caveland_sim::Caveland>,
    /// Items, robots... of the game mode, as the server last said.
    things: Vec<ThingState>,
    /// The server moves us (a cart, the intro ship): nothing of ours is predicted meanwhile.
    riding: bool,
    /// Players that are not drawn (inside the intro ship).
    hidden: HashSet<u32>,
    /// Cells of power blocks (torches, turrets, stations) that have power.
    powered: HashSet<(i32, i32, i32)>,
    /// Catapults and cannons as the server last described them.
    launchers: Vec<caveland_client::LauncherInfo>,
    /// The aiming menu of one of them is open: the arc preview is drawn.
    aiming: bool,
    /// The preview dots and what they were computed for.
    preview: Option<(caveland_client::PreviewKey, Vec<caveland_sim::launcher::PreviewDot>)>,
    /// Sounds of the game mode that go on until the server says they stop (a cart rolling), by name.
    sound_loops: HashMap<String, crate::audio::LoopHandle>,
    /// Sprites for the players and the things, once the atlas has loaded (`?flat=1` never loads it).
    actors: Actors,
    /// The names above the other players' heads.
    name_tags: NameTags,
    /// Arrows at the screen edge towards friends who are out of view.
    friend_markers: FriendMarkers,
    bind_group: wgpu::BindGroup,
    /// The sprite atlas texture array (a 1 pixel placeholder until it has loaded).
    atlas_bind_group: wgpu::BindGroup,
    world_buffer: wgpu::Buffer,
    world_vertices: u32,
    dynamic_buffer: wgpu::Buffer,
    dynamic_vertices: u32,
    /// A glTF model asked for with `?model=` (see [`load_model`]): loaded, then placed next to the
    /// local player once there is one.
    model_loaded: Option<LoadedModel>,
    model_placed: Option<PlacedModel>,
    /// Draws the scene in layers of depth peeling (see `peel.rs`) and blends them onto the canvas.
    peeling: Peeling,
    backend: wgpu::Backend,
    camera: Camera,
    dpr: f32,

    // --- game
    world: World,
    /// The chunk the camera follows: the 3x3 chunks around it are meshed and drawn.
    view_chunk: (i32, i32),
    /// The render-side view of the chunks around the player: clipping, shading and cached meshes.
    render: RenderStorage,
    /// Rebuild the world mesh at the start of the next frame.
    remesh: bool,
    /// Bumped whenever blocks change, so the minimap knows to redraw its terrain.
    terrain_version: u64,
    socket: Option<WebSocket>,
    connected: bool,
    /// When the current connection attempt started, until the server's welcome arrives.
    connecting_since: Option<f64>,
    /// Rejoining after the connection broke (a server update): what to try next and when to stop.
    reconnect: Reconnect,
    /// The server address of this session, for reconnecting.
    server_url: String,
    /// The server build the "update available" notice was already shown for.
    update_notified: Option<String>,
    my_id: Option<u32>,
    /// Holds just our own player, simulated locally with the same fixed steps as the server so
    /// that movement feels instant (prediction).
    entities: Entities,
    local_id: Option<EntityId>,
    remotes: HashMap<u32, Remote>,
    /// Names and colours of everybody in the world.
    roster: HashMap<u32, PlayerInfo>,
    /// Everybody's ping in ms as the server last told us (the Tab player list).
    pings: HashMap<u32, u32>,
    /// Hearts in the Tab list: friends (both agreed), players we invited, players who invited us.
    friends: HashSet<u32>,
    invited: HashSet<u32>,
    invited_by: HashSet<u32>,
    map_name: String,
    /// Real time not yet consumed by fixed physics steps.
    accumulator: f32,
    /// Our inputs since the server's last acknowledgement, for reconciling the prediction.
    history: InputHistory,
    /// The part of the last server correction that is still being faded out of view.
    visual_offset: VisualOffset,
    /// Runs slightly behind the server so remote players can be interpolated.
    render_clock: RenderClock,

    // --- input
    keys: HashSet<String>,
    bindings: Bindings,
    /// Pointer position in canvas pixels.
    pointer: Option<(f32, f32)>,
    /// The map editor: the only mode in which the mouse edits blocks (see `editor.rs`).
    editor: Editor,
    /// Last toolbar state pushed to the page, to send it only when it changes.
    editor_ui: String,

    // --- debug overlays
    net: NetStats,
    net_overlay: Option<web_sys::HtmlElement>,
    show_net: bool,
    minimap: Option<Minimap>,
    next_ping_ms: f64,
    next_overlay_ms: f64,
    next_minimap_ms: f64,
    next_status_ms: f64,

    last_frame_ms: Option<f64>,
    info_text: String,
}

async fn run() -> Result<(), String> {
    let window = web_sys::window().ok_or("no window")?;
    let document = window.document().ok_or("no document")?;
    let canvas: HtmlCanvasElement = document
        .get_element_by_id("canvas")
        .ok_or("no #canvas")?
        .dyn_into()
        .map_err(|_| "#canvas is not a canvas")?;
    fit_canvas(&canvas);

    let instance = wgpu::Instance::default();
    let surface = instance
        .create_surface(wgpu::SurfaceTarget::Canvas(canvas.clone()))
        .map_err(|e| format!("create_surface: {e}"))?;
    let adapter = instance
        .request_adapter(&wgpu::RequestAdapterOptions {
            compatible_surface: Some(&surface),
            ..Default::default()
        })
        .await
        .map_err(|e| format!("no suitable GPU adapter: {e}"))?;
    let backend = adapter.get_info().backend;
    // WebGL2 cannot satisfy the default (WebGPU-sized) limits. Ask for the WebGL2 baseline and
    // let the adapter raise what it can.
    let (device, queue) = adapter
        .request_device(&wgpu::DeviceDescriptor {
            required_limits: wgpu::Limits::downlevel_webgl2_defaults().using_resolution(adapter.limits()),
            ..Default::default()
        })
        .await
        .map_err(|e| format!("request_device: {e}"))?;
    device.on_uncaptured_error(std::sync::Arc::new(|error: wgpu::Error| {
        web_sys::console::error_1(&format!("wgpu error: {error}").into());
    }));

    let mut config = surface
        .get_default_config(&adapter, canvas.width().max(1), canvas.height().max(1))
        .ok_or("surface not supported by adapter")?;
    config.present_mode = wgpu::PresentMode::AutoVsync;
    // The palette in mesh.rs is plain display colours, so render into a non-sRGB surface. The default
    // differs per backend (WebGL picks an sRGB format), which made WebGL look washed out.
    if let Some(format) = surface.get_capabilities(&adapter).formats.into_iter().find(|f| !f.is_srgb()) {
        config.format = format;
    }
    surface.configure(&device, &config);

    let mut world = World::new(IslandGenerator::new(DEFAULT_SEED));
    let mut render = RenderStorage::new();
    render.update(&mut world, (0, 0));
    let world_vertices = render.vertices();
    let world_buffer = vertex_buffer_with(&device, &world_vertices);
    let dynamic_buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("dynamic mesh"),
        size: DYNAMIC_VERTICES * std::mem::size_of::<Vertex>() as u64,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });

    let camera_buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("camera"),
        size: std::mem::size_of::<CameraUniform>() as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let lighting_buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("lighting"),
        size: std::mem::size_of::<crate::lighting::Lighting>() as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let uniform_entry = |binding: u32| wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::VERTEX,
        ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
        count: None,
    };
    let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("camera and lighting layout"),
        entries: &[uniform_entry(0), uniform_entry(1)],
    });
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("camera"),
        layout: &bind_group_layout,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: camera_buffer.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 1, resource: lighting_buffer.as_entire_binding() },
        ],
    });

    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("blocks"),
        source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
    });
    let atlas_layout = texture::bind_group_layout(&device);
    let atlas_bind_group = texture::placeholder(&device, &queue, &atlas_layout);
    let peeling = Peeling::new(&device, &queue, config.format, config.width, config.height);
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("blocks"),
        bind_group_layouts: &[Some(&bind_group_layout), Some(&atlas_layout), Some(peeling.peel_layout())],
        immediate_size: 0,
    });
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("blocks"),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            compilation_options: Default::default(),
            buffers: &[Some(Vertex::layout())],
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
            compilation_options: Default::default(),
            targets: &[Some(config.format.into())],
        }),
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: Some(wgpu::DepthStencilState {
            format: DEPTH_FORMAT,
            depth_write_enabled: Some(true),
            depth_compare: Some(wgpu::CompareFunction::Less),
            stencil: Default::default(),
            bias: Default::default(),
        }),
        multisample: wgpu::MultisampleState::default(),
        multiview_mask: None,
        cache: None,
    });

    let dpr = window.device_pixel_ratio() as f32;
    let (peak_x, peak_y) = IslandGenerator::new(DEFAULT_SEED).peak();
    let (gx, gy) = to_iso(peak_x, peak_y);
    let net_overlay = create_net_overlay(&document);
    let minimap = Minimap::new(&document, MINIMAP_SIZE.0, MINIMAP_SIZE.1, dpr.round().max(1.0) as u32).ok();
    let state = Rc::new(RefCell::new(State {
        peeling,
        camera: Camera { center: [(gx - gy) * 100.0, (gx + gy) * 50.0 - 4.0 * 122.0], zoom: 0.5 * dpr },
        dpr,
        canvas,
        surface,
        config,
        device,
        queue,
        pipeline,
        camera_buffer,
        lighting_buffer,
        lighting: crate::lighting::LightingController::new(),
        audio: Audio::new(),
        particles: wurfel_sim::particle::Particles::default(),
        emitters: Vec::new(),
        jetpack: crate::particles::Jetpack::new(),
        caveland: None,
        things: Vec::new(),
        riding: false,
        hidden: HashSet::new(),
        powered: HashSet::new(),
        launchers: Vec::new(),
        aiming: false,
        preview: None,
        sound_loops: HashMap::new(),
        actors: Actors::default(),
        name_tags: NameTags::new(&document),
        friend_markers: FriendMarkers::new(&document),
        bind_group,
        atlas_bind_group,
        world_buffer,
        world_vertices: world_vertices.len() as u32,
        dynamic_buffer,
        dynamic_vertices: 0,
        model_loaded: None,
        model_placed: None,
        backend,
        world,
        view_chunk: (0, 0),
        render,
        remesh: false,
        terrain_version: 0,
        socket: None,
        connected: false,
        connecting_since: None,
        reconnect: Reconnect::new(),
        server_url: String::new(),
        update_notified: None,
        my_id: None,
        entities: Entities::new(),
        local_id: None,
        remotes: HashMap::new(),
        roster: HashMap::new(),
        pings: HashMap::new(),
        friends: HashSet::new(),
        invited: HashSet::new(),
        invited_by: HashSet::new(),
        map_name: String::new(),
        accumulator: 0.0,
        history: InputHistory::new(),
        visual_offset: VisualOffset::default(),
        render_clock: RenderClock::default(),
        keys: HashSet::new(),
        bindings: read_bindings(),
        pointer: None,
        editor: Editor::default(),
        editor_ui: String::new(),
        net: NetStats::new(),
        net_overlay,
        show_net: false,
        minimap,
        next_ping_ms: 0.0,
        next_overlay_ms: 0.0,
        next_minimap_ms: 0.0,
        next_status_ms: 0.0,
        last_frame_ms: None,
        info_text: String::new(),
    }));

    install_input(&window, &state);
    install_menu_events(&window, &state);
    install_net_bridge(&window, &state);
    if flat_look() {
        web_sys::console::log_1(&"sprites: ?flat=1, drawing flat colours".into());
    } else {
        let (device, queue) = {
            let s = state.borrow();
            (s.device.clone(), s.queue.clone())
        };
        wasm_bindgen_futures::spawn_local(load_sprites(state.clone(), device.clone(), queue.clone(), atlas_layout.clone()));
        if let Some(url) = query_value("model") {
            wasm_bindgen_futures::spawn_local(load_model(state.clone(), device, queue, atlas_layout, url));
        }
    }
    start_frame_loop(state);
    Ok(())
}

/// `?flat=1` in the page address keeps the old look: solid coloured blocks, no sprites.
fn flat_look() -> bool {
    let search = web_sys::window().and_then(|w| w.location().search().ok()).unwrap_or_default();
    search.trim_start_matches('?').split('&').any(|part| part == "flat=1" || part == "flat")
}

/// Fetch the sprite atlas in the background; the world is meshed again with sprites when it is
/// there, and the flat colours stay if it cannot be loaded.
async fn load_sprites(state: Rc<RefCell<State>>, device: wgpu::Device, queue: wgpu::Queue, layout: wgpu::BindGroupLayout) {
    let started = now_ms();
    match texture::load(&device, &queue, &layout).await {
        Ok((sprites, group)) => {
            let sprites = Rc::new(sprites);
            web_sys::console::log_1(&format!("sprites: {} sprites loaded in {:.0} ms", sprites.atlas.len(), now_ms() - started).into());
            let missing = sprites.missing_player_sheets();
            if !missing.is_empty() {
                let sheets: String = missing.iter().collect();
                web_sys::console::warn_1(&format!("sprites: the atlas has no frames for the player sheets '{sheets}'").into());
                show_banner(&format!("The sprite atlas is out of date (no player animations '{sheets}'). Reload the page without its cache."), Tone::Error);
            }
            let mut s = state.borrow_mut();
            s.atlas_bind_group = group;
            s.actors.set_sprites(Some(sprites.clone()));
            s.render.set_sprites(Some(sprites));
            s.remesh = true;
        }
        Err(message) => web_sys::console::warn_1(&format!("sprites: {message}; keeping the flat colours").into()),
    }
}

/// The value of `key` in the page address (`?key=value`), if present.
fn query_value(key: &str) -> Option<String> {
    let search = web_sys::window()?.location().search().ok()?;
    search.trim_start_matches('?').split('&').find_map(|part| part.strip_prefix(key)?.strip_prefix('=')).map(str::to_string)
}

/// A model that is parsed and has its pictures on the GPU, waiting for a place in the world.
struct LoadedModel {
    model: Model,
    /// One bind group per picture of the model, in the order of [`Model::textures`].
    pictures: Vec<wgpu::BindGroup>,
}

/// The model's triangles in a buffer, with what to bind for each run of them.
struct PlacedModel {
    buffer: wgpu::Buffer,
    draws: Vec<(Option<usize>, std::ops::Range<u32>)>,
}

/// Blocks per glTF metre when a model is placed in the world.
const MODEL_SCALE: f32 = 0.6;

/// `?model=assets/models/x.glb`: fetch a glTF binary, in the background. It is shown standing next
/// to the local player (see [`place_model`]). Animations are not played yet: the file is posed at
/// the middle of its first animation.
async fn load_model(state: Rc<RefCell<State>>, device: wgpu::Device, queue: wgpu::Queue, layout: wgpu::BindGroupLayout, url: String) {
    let started = now_ms();
    let bytes = match texture::fetch_bytes(&url).await {
        Ok(bytes) => bytes,
        Err(message) => return web_sys::console::warn_1(&format!("model: {message}").into()),
    };
    let model = match Model::from_glb(&bytes) {
        Ok(model) => model,
        Err(message) => return web_sys::console::warn_1(&format!("model {url}: {message}").into()),
    };
    for warning in &model.warnings {
        web_sys::console::warn_1(&format!("model {url}: {warning}").into());
    }
    let pictures = model.textures().iter().map(|t| texture::model_bind_group(&device, &queue, &layout, t)).collect();
    web_sys::console::log_1(
        &format!("model: {url}: {} triangles, {} pictures, {:.0} ms", model.triangle_count(), model.textures().len(), now_ms() - started).into(),
    );
    state.borrow_mut().model_loaded = Some(LoadedModel { model, pictures });
}

/// Put the loaded model on the ground beside the local player, once, and upload its triangles.
fn place_model(s: &mut State) {
    use wgpu::util::DeviceExt;
    let Some(player) = local_position(s) else { return };
    let Some(loaded) = s.model_loaded.as_ref() else { return };
    // To the right of the player on the screen, standing on its lowest point.
    let lowest = loaded.model.bounds().map_or(0.0, |(lo, _)| lo.z);
    let position = Vec3::new(player.x + 1.5, player.y - 1.5, player.z - lowest * MODEL_SCALE);
    let mut vertices = Vec::new();
    let draws = loaded.model.append(&mut vertices, position, 0.0, MODEL_SCALE);
    let buffer = s.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("model"),
        contents: bytemuck::cast_slice(&vertices),
        usage: wgpu::BufferUsages::VERTEX,
    });
    let draws = draws.into_iter().map(|d| (d.texture, d.vertices.start as u32..d.vertices.end as u32)).collect();
    s.model_placed = Some(PlacedModel { buffer, draws });
}

fn vertex_buffer_with(device: &wgpu::Device, vertices: &[Vertex]) -> wgpu::Buffer {
    use wgpu::util::DeviceExt;
    device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("world mesh"),
        // wgpu rejects zero-sized buffers; a world with no geometry still needs a valid one.
        contents: if vertices.is_empty() { &[0u8; 24] } else { bytemuck::cast_slice(vertices) },
        usage: wgpu::BufferUsages::VERTEX,
    })
}

fn fit_canvas(canvas: &HtmlCanvasElement) {
    let window = web_sys::window().unwrap();
    let dpr = window.device_pixel_ratio();
    let width = window.inner_width().ok().and_then(|v| v.as_f64()).unwrap_or(800.0);
    let height = window.inner_height().ok().and_then(|v| v.as_f64()).unwrap_or(600.0);
    canvas.set_width(((width * dpr) as u32).max(1));
    canvas.set_height(((height * dpr) as u32).max(1));
}

// ------------------------------------------------------------------------------------ JS helpers

fn now_ms() -> f64 {
    web_sys::window().and_then(|w| w.performance()).map_or(0.0, |p| p.now())
}

fn js_get(target: &JsValue, key: &str) -> JsValue {
    js_sys::Reflect::get(target, &JsValue::from_str(key)).unwrap_or(JsValue::UNDEFINED)
}

fn window_flag(key: &str) -> bool {
    web_sys::window().is_some_and(|w| js_get(&w, key).as_bool().unwrap_or(false))
}

/// Menu or console in front of the game: ignore gameplay input.
fn input_blocked() -> bool {
    window_flag("wurfelMenuOpen") || window_flag("wurfelConsoleOpen") || window_flag("wurfelDialogOpen")
}

/// The player's name and colour from the menu (`window.wurfelSettings`), with defaults.
fn read_identity() -> (String, [u8; 3]) {
    let settings = web_sys::window().map(|w| js_get(&js_get(&w, "wurfelSettings"), "playerName"));
    let name = settings.and_then(|v| v.as_string()).unwrap_or_default();
    let color = web_sys::window()
        .and_then(|w| js_get(&js_get(&w, "wurfelSettings"), "playerColor").as_string())
        .and_then(|c| parse_hex_color(&c))
        .unwrap_or([230, 190, 50]);
    (name, color)
}

/// Bindings from `window.wurfelSettings.keys`, or the defaults until the menu has set them.
fn read_bindings() -> Bindings {
    let Some(window) = web_sys::window() else { return Bindings::default() };
    let keys = js_get(&js_get(&window, "wurfelSettings"), "keys");
    if !keys.is_object() {
        return Bindings::default();
    }
    let mut slots = HashMap::new();
    for action in crate::bindings::ACTIONS {
        let value = js_get(&keys, action);
        if js_sys::Array::is_array(&value) {
            let names: Vec<String> = js_sys::Array::from(&value).iter().filter_map(|v| v.as_string()).collect();
            slots.insert(action.to_string(), names);
        }
    }
    Bindings::from_settings(&slots)
}

fn create_net_overlay(document: &web_sys::Document) -> Option<web_sys::HtmlElement> {
    let element: web_sys::HtmlElement = document.create_element("pre").ok()?.dyn_into().ok()?;
    element.set_id("netinfo");
    element.set_attribute("aria-hidden", "true").ok()?;
    element
        .set_attribute(
            "style",
            "position:fixed;top:10px;right:12px;margin:0;padding:6px 8px;border-radius:6px;\
             background:rgba(10,12,18,0.72);color:#cfd6e4;font:12px/1.45 ui-monospace,Menlo,Consolas,monospace;\
             pointer-events:none;white-space:pre;display:none;z-index:5",
        )
        .ok()?;
    document.body()?.append_child(&element).ok()?;
    Some(element)
}

fn set_overlay_visible(element: &Option<web_sys::HtmlElement>, visible: bool) {
    if let Some(element) = element {
        let _ = element.style().set_property("display", if visible { "block" } else { "none" });
    }
}

/// `window.wurfelStatus`, which the menu shows.
fn export_status(s: &mut State) {
    let Some(window) = web_sys::window() else { return };
    let status = js_sys::Object::new();
    let set = |key: &str, value: JsValue| {
        let _ = js_sys::Reflect::set(&status, &JsValue::from_str(key), &value);
    };
    set("connected", s.connected.into());
    set("players", ((s.remotes.len() + usize::from(s.my_id.is_some())) as f64).into());
    set("fps", s.net.report(0.0).fps.round().into());
    set("backend", format!("{:?}", s.backend).into());
    set("map", s.map_name.as_str().into());
    let _ = js_sys::Reflect::set(&window, &JsValue::from_str("wurfelStatus"), &status);
    export_players(s, &window);
}

/// `window.wurfelPlayers` for the Tab player list (scoreboard.js): everybody in the roster with
/// their ping in ms (`null` until known). Our own ping is the one we measure ourselves.
fn export_players(s: &State, window: &web_sys::Window) {
    let mut roster: Vec<&PlayerInfo> = s.roster.values().collect();
    roster.sort_by_key(|p| p.id);
    let players = js_sys::Array::new();
    for p in roster {
        let me = Some(p.id) == s.my_id;
        let ping = if me { s.net.last_rtt().map(|rtt| rtt.round() as u32) } else { s.pings.get(&p.id).copied() };
        let entry = js_sys::Object::new();
        let set = |key: &str, value: JsValue| {
            let _ = js_sys::Reflect::set(&entry, &JsValue::from_str(key), &value);
        };
        set("id", p.id.into());
        set("name", p.name.as_str().into());
        set("color", format!("#{:02x}{:02x}{:02x}", p.color[0], p.color[1], p.color[2]).into());
        set("pingMs", ping.map_or(JsValue::NULL, JsValue::from));
        set("me", me.into());
        set("friend", s.friends.contains(&p.id).into());
        set("invited", s.invited.contains(&p.id).into());
        set("invitesMe", s.invited_by.contains(&p.id).into());
        players.push(&entry);
    }
    let _ = js_sys::Reflect::set(window, &JsValue::from_str("wurfelPlayers"), &players);
}

fn clear_friends(s: &mut State) {
    s.friends.clear();
    s.invited.clear();
    s.invited_by.clear();
}

// --------------------------------------------------------------------------------- status banner

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tone {
    Info,
    Ok,
    /// News the player has to read and act on: stays a little longer than `Ok`.
    Invite,
    Error,
}

/// A banner at the top centre of the game, so connection progress and failures are visible while
/// playing and not only inside the menu. `Ok` messages disappear by themselves.
fn show_banner(text: &str, tone: Tone) {
    let Some(window) = web_sys::window() else { return };
    let Some(document) = window.document() else { return };
    let element = match document.get_element_by_id("statusbanner") {
        Some(element) => element,
        None => {
            let Ok(element) = document.create_element("div") else { return };
            element.set_id("statusbanner");
            let _ = element.set_attribute("role", "status");
            let _ = element.set_attribute("aria-live", "polite");
            if let Some(body) = document.body() {
                let _ = body.append_child(&element);
            }
            element
        }
    };
    let (background, color) = match tone {
        Tone::Info => ("rgba(20,28,48,0.92)", "#dfe8ff"),
        Tone::Ok => ("rgba(24,74,40,0.92)", "#dcffe6"),
        Tone::Invite => ("rgba(92,28,70,0.94)", "#ffe0f3"),
        Tone::Error => ("rgba(110,24,24,0.95)", "#ffe3e3"),
    };
    let _ = element.set_attribute(
        "style",
        &format!(
            "position:fixed;top:14px;left:50%;transform:translateX(-50%);max-width:min(92vw,640px);\
             padding:10px 16px;border-radius:10px;background:{background};color:{color};\
             font:600 14px/1.4 system-ui,sans-serif;box-shadow:0 6px 24px rgba(0,0,0,0.35);\
             z-index:20;text-align:center;pointer-events:none"
        ),
    );
    element.set_text_content(Some(text));

    // A newer message must not be hidden by an older timer.
    let seq = element.get_attribute("data-seq").and_then(|v| v.parse::<u32>().ok()).unwrap_or(0) + 1;
    let _ = element.set_attribute("data-seq", &seq.to_string());
    let hide_after_ms = match tone {
        Tone::Ok => Some(2500),
        Tone::Invite => Some(7000),
        Tone::Info | Tone::Error => None,
    };
    if let Some(hide_after_ms) = hide_after_ms {
        let hide = Closure::once_into_js(move || {
            if let Some(el) = web_sys::window().and_then(|w| w.document()).and_then(|d| d.get_element_by_id("statusbanner")) {
                if el.get_attribute("data-seq").and_then(|v| v.parse::<u32>().ok()) == Some(seq) {
                    let _ = el.set_attribute("style", "display:none");
                }
            }
        });
        let _ = window.set_timeout_with_callback_and_timeout_and_arguments_0(hide.unchecked_ref(), hide_after_ms);
    }
}

fn hide_banner() {
    if let Some(el) = web_sys::window().and_then(|w| w.document()).and_then(|d| d.get_element_by_id("statusbanner")) {
        let _ = el.set_attribute("style", "display:none");
        let seq = el.get_attribute("data-seq").and_then(|v| v.parse::<u32>().ok()).unwrap_or(0) + 1;
        let _ = el.set_attribute("data-seq", &seq.to_string());
    }
}

/// Tell the player something went wrong: banner, console, and a `wurfel:error` event for the menu.
/// The event is dispatched from a timer, because the menu reacts by dispatching events back into this
/// code and callers of this function may still be holding the game state.
fn report_error(message: &str) {
    web_sys::console::error_1(&message.into());
    show_banner(message, Tone::Error);
    let Some(window) = web_sys::window() else { return };
    let message = message.to_string();
    let dispatch = Closure::once_into_js(move || {
        let Some(window) = web_sys::window() else { return };
        let init = web_sys::CustomEventInit::new();
        let detail = js_sys::Object::new();
        let _ = js_sys::Reflect::set(&detail, &JsValue::from_str("message"), &JsValue::from_str(&message));
        init.set_detail(&detail);
        if let Ok(event) = web_sys::CustomEvent::new_with_event_init_dict("wurfel:error", &init) {
            let _ = window.dispatch_event(&event);
        }
    });
    let _ = window.set_timeout_with_callback_and_timeout_and_arguments_0(dispatch.unchecked_ref(), 0);
}

// ------------------------------------------------------------------------------------ networking

/// What the menu asked for in `wurfel:play`: the server's WebSocket URL. Choosing and loading the
/// map happened in the menu's lobby connection before this.
struct Join {
    server: String,
}

/// Leave the current world (if any) and show the island preview again.
fn end_session(s: &mut State) {
    if let Some(socket) = s.socket.take() {
        // Detach the handlers first so a late event from the old connection cannot touch the new one.
        socket.set_onopen(None);
        socket.set_onclose(None);
        socket.set_onmessage(None);
        let _ = socket.close();
    }
    s.reconnect.stop(); // leaving (or giving up) is final
    s.connected = false;
    s.connecting_since = None;
    s.my_id = None;
    s.local_id = None;
    s.entities = Entities::new();
    s.remotes.clear();
    s.roster.clear();
    clear_friends(s);
    s.history = InputHistory::new();
    s.visual_offset.clear();
    s.render_clock = RenderClock::default();
    s.caveland = None;
    s.things.clear();
    reset_mode_state(s);
    hud("show", &JsValue::FALSE);
    hud("closeDialog", &JsValue::UNDEFINED);
    s.world = World::new(IslandGenerator::new(DEFAULT_SEED));
    s.view_chunk = (0, 0);
    s.remesh = true;
    s.terrain_version += 1;
}

fn begin_session(state: &Rc<RefCell<State>>, join: Join) {
    show_banner("Connecting to the server…", Tone::Info);
    {
        let mut s = state.borrow_mut();
        end_session(&mut s);
        s.reconnect.begin();
        s.server_url = join.server.clone();
    }
    if let Some(window) = web_sys::window() {
        connect(&window, state, &join.server);
    }
}

fn connect(window: &web_sys::Window, state: &Rc<RefCell<State>>, url: &str) {
    let _ = window;
    let socket = match WebSocket::new(url) {
        Ok(socket) => socket,
        Err(_) => {
            // While reconnecting this is just another failed try.
            if state.borrow().reconnect.active() {
                handle_close(state, url);
            } else {
                state.borrow_mut().reconnect.stop();
                report_error(&format!("Cannot open {url}"));
            }
            return;
        }
    };

    let s = state.clone();
    let on_open = Closure::<dyn FnMut()>::new(move || {
        let mut s = s.borrow_mut();
        s.connected = true;
        s.net = NetStats::new();
        s.next_ping_ms = 0.0;
        show_banner("Connected, loading the world…", Tone::Info);
        // The server holds a lobby connection until it is told to join.
        let (name, color) = read_identity();
        send(&mut s, &ClientMsg::Join { name, color });
    });
    socket.set_onopen(Some(on_open.as_ref().unchecked_ref()));
    on_open.forget();

    let s = state.clone();
    let url_for_message = url.to_string();
    let on_close = Closure::<dyn FnMut()>::new(move || handle_close(&s, &url_for_message));
    socket.set_onclose(Some(on_close.as_ref().unchecked_ref()));
    on_close.forget();

    let s = state.clone();
    socket.set_binary_type(web_sys::BinaryType::Arraybuffer);
    let on_message = Closure::<dyn FnMut(MessageEvent)>::new(move |event: MessageEvent| {
        let data = event.data();
        let now = now_ms();
        let mut s = s.borrow_mut();
        // Binary frames are chunks of terrain.
        if data.is_instance_of::<js_sys::ArrayBuffer>() {
            let bytes = js_sys::Uint8Array::new(&data).to_vec();
            s.net.on_received(bytes.len(), now);
            match decode_chunk(&bytes) {
                Ok(chunk) => {
                    s.world.insert_chunk(chunk);
                    s.remesh = true;
                    s.terrain_version += 1;
                }
                Err(e) => web_sys::console::warn_1(&format!("bad chunk from the server: {e}").into()),
            }
            return;
        }
        let Some(text) = data.as_string() else { return };
        s.net.on_received(text.len(), now);
        match serde_json::from_str::<ServerMsg>(&text) {
            Ok(msg) => handle_server_message(&mut s, msg, now),
            Err(e) => web_sys::console::warn_1(&format!("bad server message: {e}").into()),
        }
    });
    socket.set_onmessage(Some(on_message.as_ref().unchecked_ref()));
    on_message.forget();

    {
        let mut s = state.borrow_mut();
        s.socket = Some(socket);
        s.connecting_since = Some(now_ms());
    }
}

/// A Caveland action of the local player: sent to the server, which decides, and played by the
/// local animation at once.
fn send_action(s: &mut State, name: &'static str, arg: i32) {
    if let Some(me) = s.my_id {
        s.actors.local_action(me, name);
    }
    send(s, &ClientMsg::Action { name: name.to_string(), arg });
}

/// The game socket closed. While playing that is most likely a server update: keep the world, the
/// camera and the HUD as they are, say so, and try again with growing pauses (`reconnect.rs`). The
/// server's fresh world replaces the old one when its welcome arrives.
fn handle_close(state: &Rc<RefCell<State>>, url: &str) {
    let mut s = state.borrow_mut();
    match s.reconnect.on_closed(now_ms()) {
        // The player left, or this is an old socket: nothing of ours.
        Closed::Ignore => {}
        Closed::Refused => {
            end_session(&mut s);
            report_error(&format!(
                "Could not join the world: the server at {url} refused or closed the connection. \
                 Does the world still exist?"
            ));
        }
        Closed::GiveUp => {
            end_session(&mut s);
            report_error("Connection to the server lost. Pick a world to join again.");
        }
        Closed::Retry { delay_ms, epoch } => {
            s.connected = false;
            s.connecting_since = None;
            s.socket = None;
            s.keys.clear(); // do not walk on when the key is released while nobody is listening
            let attempt = s.reconnect.attempt();
            show_banner(
                &if attempt > 2 {
                    format!("Server updating, reconnecting… (try {attempt})")
                } else {
                    "Server updating, reconnecting…".to_string()
                },
                Tone::Info,
            );
            let retry_state = state.clone();
            let retry = Closure::once_into_js(move || retry_connect(&retry_state, epoch));
            if let Some(window) = web_sys::window() {
                let _ = window.set_timeout_with_callback_and_timeout_and_arguments_0(retry.unchecked_ref(), delay_ms as i32);
            }
        }
    }
}

/// The pause before a retry is over: open a new socket, unless the player left in the meantime.
fn retry_connect(state: &Rc<RefCell<State>>, epoch: u32) {
    let url = {
        let mut s = state.borrow_mut();
        if !s.reconnect.fire(epoch) {
            return;
        }
        s.server_url.clone()
    };
    if let Some(window) = web_sys::window() {
        connect(&window, state, &url);
    }
}

/// Offer a reload when the server runs another build than this page (`wurfelUpdate` in `menu.js`).
/// Once per server build, so it does not nag.
fn check_build(s: &mut State, server: &str) {
    if !wurfel_sim::protocol::build_mismatch(&wurfel_sim::protocol::build_id(), server) || s.update_notified.as_deref() == Some(server) {
        return;
    }
    s.update_notified = Some(server.to_string());
    call_js("wurfelUpdate", "show", &JsValue::from_str(server));
}

fn send(s: &mut State, msg: &ClientMsg) {
    if let (true, Some(socket)) = (s.connected, &s.socket) {
        if let Ok(json) = serde_json::to_string(msg) {
            if socket.send_with_str(&json).is_ok() {
                s.net.on_sent(json.len(), now_ms());
            }
        }
    }
}

fn handle_server_message(s: &mut State, msg: ServerMsg, now: f64) {
    match msg {
        ServerMsg::Welcome { your_id, map, players, roster, gamemode, build, .. } => {
            check_build(s, &build);
            let rejoined = s.reconnect.active();
            s.reconnect.on_welcome();
            // Whatever a dialog of the old server asked is moot; the state below is the new server's.
            hud("closeDialog", &JsValue::UNDEFINED);
            s.roster = roster.into_iter().map(|p| (p.id, p)).collect();
            s.pings.clear();
            clear_friends(s);
            s.map_name = map.clone();
            // The server sends the terrain as chunks: this world has no generator of its own and
            // fills up as they arrive.
            s.world = World::remote();
            // A game mode brings its own block rules and player: use the same ones as the server.
            s.caveland = (gamemode == caveland_client::MODE).then(|| caveland_client::start(&mut s.world));
            s.things.clear();
            reset_mode_state(s);
            hud("show", &JsValue::from_bool(s.caveland.is_some()));
            s.remesh = true;
            s.terrain_version += 1;

            s.my_id = Some(your_id);
            s.remotes.clear();
            s.entities = Entities::new();
            s.local_id = None;
            s.history = InputHistory::new();
            s.visual_offset.clear();
            s.render_clock = RenderClock::default();
            for p in &players {
                let pos = Vec3::from(p.pos);
                if p.id == your_id {
                    let id = match s.caveland.as_mut() {
                        Some(caveland) => caveland.spawn_player(&mut s.entities, 0, pos),
                        None => s.entities.spawn(new_player(pos)),
                    };
                    s.entities.get_mut(id).and_then(|e| e.body.as_mut()).expect("players move").movement = Vec3::from(p.vel);
                    s.local_id = Some(id);
                    s.camera.center = screen_position((pos.x, pos.y), pos.z + 0.7);
                    let (bx, by) = from_iso(pos.x, pos.y);
                    s.view_chunk = chunk_of(bx, by);
                } else {
                    s.remotes.insert(p.id, Remote { pos, track: Track::new() });
                }
            }
            s.connecting_since = None;
            show_banner(&if rejoined { format!("Reconnected to '{map}'") } else { format!("Joined '{map}'") }, Tone::Ok);
            s.audio.play_music("overworld");
        }
        ServerMsg::Snapshot { tick, players } => {
            s.net.on_snapshot(tick, now);
            apply_snapshot(s, tick, &players);
        }
        ServerMsg::BlockSet(e) => {
            let old = s.world.get(e.x, e.y, e.z);
            if wurfel_sim::entity::physics::is_obstacle(old) && !wurfel_sim::entity::physics::is_obstacle(Block::from_raw(e.block)) {
                let (gx, gy) = to_iso(e.x, e.y);
                s.particles.block_break(Vec3::new(gx, gy, e.z as f32 + 0.5), crate::mesh::block_color(old));
            }
            s.world.set(e.x, e.y, e.z, Block::from_raw(e.block));
            s.remesh = true;
            s.terrain_version += 1;
        }
        ServerMsg::PlayerJoined { player } => {
            if Some(player.id) != s.my_id {
                show_banner(&format!("{} joined", player.name), Tone::Ok);
            }
            s.roster.insert(player.id, player);
        }
        ServerMsg::PlayerLeft { id } => {
            if let Some(player) = s.roster.remove(&id) {
                show_banner(&format!("{} left", player.name), Tone::Ok);
            }
            s.remotes.remove(&id);
        }
        ServerMsg::ChunkUnload { cx, cy } => {
            s.world.unload_chunk(cx, cy);
            s.remesh = true;
            s.terrain_version += 1;
        }
        // The lobby is the menu's business; the game socket only sees its greeting before it joins.
        ServerMsg::Lobby { build, .. } => check_build(s, &build),
        ServerMsg::Maps { .. } | ServerMsg::WorldChanged { .. } | ServerMsg::MapCreated { .. } => {}
        // The server saved and is going away; the socket closes next and the reconnect starts.
        ServerMsg::ServerRestarting => show_banner("Server updating, reconnecting…", Tone::Info),
        ServerMsg::Failed { message, .. } => report_error(&message),
        ServerMsg::Pong { client_time, .. } => s.net.on_pong(client_time, now),
        ServerMsg::Stats(stats) => s.net.on_server_stats(stats),
        ServerMsg::Pings { list } => s.pings = list.into_iter().collect(),
        ServerMsg::Friends { player, friends, sent, received } if Some(player) == s.my_id => {
            let name = |s: &State, id: u32| s.roster.get(&id).map_or_else(|| "Somebody".to_string(), |p| p.name.clone());
            for &id in received.iter().filter(|id| !s.invited_by.contains(id)) {
                show_banner(&format!("{} wants to be friends. Hold Tab and click ♥ to accept", name(s, id)), Tone::Invite);
            }
            for &id in friends.iter().filter(|id| !s.friends.contains(id)) {
                show_banner(&format!("You and {} are friends now", name(s, id)), Tone::Ok);
            }
            s.friends = friends.into_iter().collect();
            s.invited = sent.into_iter().collect();
            s.invited_by = received.into_iter().collect();
        }
        ServerMsg::Friends { .. } => {} // somebody else's
        ServerMsg::Things { things, .. } => s.things = things,
        ServerMsg::Rules { kind, data } => handle_rules(s, &kind, &data),
    }
}

/// Call a method of a page object (`wurfelHud` from `hud.js`, `wurfelConsoleHost`...), if the page
/// has one.
fn call_js(object: &str, method: &str, arg: &JsValue) {
    let Some(window) = web_sys::window() else { return };
    let target = js_get(&window, object);
    if !target.is_object() {
        return;
    }
    if let Some(function) = js_get(&target, method).dyn_ref::<js_sys::Function>() {
        let _ = function.call1(&target, arg);
    }
}

fn hud(method: &str, arg: &JsValue) {
    call_js("wurfelHud", method, arg);
}

/// What a game mode told us about the world and the people in it is only valid for one session.
fn reset_mode_state(s: &mut State) {
    // The server forgets the editor with the connection, so a new session starts outside it.
    s.editor.set_active(false);
    s.riding = false;
    s.hidden.clear();
    s.powered.clear();
    s.launchers.clear();
    s.aiming = false;
    s.preview = None;
    s.emitters.clear();
    for (_, handle) in s.sound_loops.drain() {
        s.audio.logic_mut().stop_loop(handle);
    }
}

/// News of the game mode: our health and pack for the HUD, and what happened around us.
fn handle_rules(s: &mut State, kind: &str, data: &serde_json::Value) {
    let Some(me) = s.my_id else { return };
    match kind {
        "state" => {
            if let Some(state) = caveland_client::parse_state(data, me) {
                hud("update", &JsValue::from_str(&caveland_client::hud_json(&state)));
            }
            let flags = caveland_client::parse_flags(data);
            s.hidden = flags.iter().filter(|(_, f)| f.hidden).map(|(&id, _)| id).collect();
            s.riding = flags.get(&me).is_some_and(|f| f.riding);
        }
        // Private news (the broadcast reaches everybody; `to` names who it is for).
        "dialog" if caveland_client::addressed_to(data, me) => {
            s.aiming = data.get("title").and_then(|t| t.as_str()).is_some_and(caveland_client::is_aiming_dialog);
            hud("dialog", &JsValue::from_str(&data.to_string()))
        }
        "dialog_closed" if caveland_client::addressed_to(data, me) => {
            s.aiming = false;
            hud("closeDialog", &JsValue::UNDEFINED)
        }
        "launchers" => s.launchers = caveland_client::parse_launchers(data),
        "lift_offer" if caveland_client::addressed_to(data, me) => hud("liftOffer", &JsValue::from_str(&data.to_string())),
        "console" if caveland_client::addressed_to(data, me) => call_js("wurfelConsoleHost", "reply", &JsValue::from_str(&data.to_string())),
        "power" => s.powered = caveland_client::parse_power(data),
        "events" => {
            let ours = local_position(s).unwrap_or(Vec3::ZERO);
            for happening in caveland_client::parse_events(data, me) {
                match happening {
                    // Our own jump already made its sound when we pressed the key.
                    Happening::Sound { name, pos } if name == "urfJump" && pos.distance(ours) < 2.0 => {}
                    // The cart's rolling goes on until the server says it stops.
                    Happening::Sound { name, pos } if name == "wagon" => match s.sound_loops.get(&name).copied() {
                        Some(handle) => s.audio.logic_mut().set_loop_position(handle, pos.to_array()),
                        None => {
                            if let Some(handle) = s.audio.logic_mut().start_loop(&name, Some(pos.to_array()), 1.0) {
                                s.sound_loops.insert(name, handle);
                            }
                        }
                    },
                    Happening::Sound { name, pos } => s.audio.play(&name, Some(pos.to_array())),
                    Happening::SoundStopped { name } => {
                        if let Some(handle) = s.sound_loops.remove(&name) {
                            s.audio.logic_mut().stop_loop(handle);
                        }
                    }
                    Happening::Teleported { entity, pos } if entity == me => {
                        if let Some(entity) = s.local_id.and_then(|id| s.entities.get_mut(id)) {
                            entity.position = pos;
                            if let Some(body) = entity.body.as_mut() {
                                body.movement = Vec3::ZERO;
                            }
                        }
                        s.visual_offset.clear();
                    }
                    // Thrown by a catapult or cannon: only a position and a velocity, then the same
                    // movement rules as the server (control by speed, bounces) run the flight.
                    Happening::Launched { player, pos, vel } if player == me => {
                        if let Some(entity) = s.local_id.and_then(|id| s.entities.get_mut(id)) {
                            entity.position = pos;
                            if let Some(body) = entity.body.as_mut() {
                                body.set_movement(vel);
                            }
                        }
                        s.visual_offset.clear();
                    }
                    Happening::Launched { .. } => {} // others are interpolated from the snapshots
                    Happening::Teleported { .. } => {} // others are interpolated, a big jump counts as a teleport there
                    Happening::ShipCrashed { pos } => {
                        // The wreck burns (`ParticleType.FIRE`) and lights its surroundings.
                        let mut fire = wurfel_sim::particle::ParticleEmitter::new(pos + Vec3::Z * 0.5);
                        fire.set_brightness(2.0);
                        s.emitters.push(fire);
                        if s.emitters.len() > MAX_EMITTERS {
                            s.emitters.remove(0);
                        }
                        s.particles.block_break(pos, [1.0, 0.55, 0.1]);
                    }
                    Happening::Dust { pos } => s.particles.block_break(pos, [0.6, 0.55, 0.5]),
                    Happening::Explosion { pos } => {
                        s.particles.block_break(pos, [1.0, 0.55, 0.1]);
                        s.particles.block_break(pos + Vec3::Z * 0.5, [0.3, 0.3, 0.3]);
                    }
                    Happening::Toast(text) => hud("toast", &JsValue::from_str(&text)),
                    Happening::Action { player, name, ok } => s.actors.announced(player, player == me, &name, ok),
                    Happening::Died => {
                        show_banner("You died. Back at the start.", Tone::Error);
                        s.visual_offset.clear();
                    }
                }
            }
        }
        _ => {}
    }
}

fn apply_snapshot(s: &mut State, tick: u64, players: &[PlayerState]) {
    let server_ms = tick as f64 * 1000.0 / TICK_RATE as f64;
    s.render_clock.on_snapshot(server_ms);
    let present: HashSet<u32> = players.iter().map(|p| p.id).collect();
    s.remotes.retain(|id, _| present.contains(id));
    for p in players {
        let pos = Vec3::from(p.pos);
        if Some(p.id) == s.my_id {
            reconcile(s, p);
        } else {
            s.remotes.entry(p.id).or_insert_with(|| Remote { pos, track: Track::new() }).track.push(server_ms, pos, Vec3::from(p.vel));
        }
    }
}

/// Our own player runs ahead of the server, so the two never agree at the moment a snapshot
/// arrives. The snapshot says where the server was after some number of ticks under one of our
/// inputs; replay our inputs from there up to the present and compare with where we are. What is
/// left is the real prediction error: it is corrected in the simulation at once and faded out of
/// the picture, and only a big one (we were blocked, pushed...) makes the player jump.
fn reconcile(s: &mut State, server: &PlayerState) {
    if s.riding {
        // A vehicle or the ship moves us: take the server's word, there is nothing to predict.
        if let Some(entity) = s.local_id.and_then(|id| s.entities.get_mut(id)) {
            entity.position = Vec3::from(server.pos);
            if let Some(body) = entity.body.as_mut() {
                body.movement = Vec3::from(server.vel);
            }
        }
        s.visual_offset.clear();
        return;
    }
    let Some(plan) = s.history.replay_plan(server.input_seq, server.input_ticks) else { return };
    let Some(id) = s.local_id else { return };
    let Some(entity) = s.entities.get_mut(id) else { return };
    let before = entity.position;
    let before_velocity = entity.body.as_ref().expect("players move").movement;
    // Only our player lives in `entities`, so stepping it steps nothing else. The events of the
    // replayed steps already happened (and made their sounds) when we first ran them.
    let replayed = if s.caveland.is_some() {
        // Caveland's rules replay on a scratch player, so the live jetpack is not burned twice.
        let result = caveland_client::replay(&mut s.world, Vec3::from(server.pos), Vec3::from(server.vel), &plan);
        result.and_then(|(position, velocity)| {
            let entity = s.entities.get_mut(id)?;
            entity.position = position;
            entity.body.as_mut()?.movement = velocity;
            Some(())
        })
    } else {
        replay(&mut s.entities, id, &s.world, Vec3::from(server.pos), Vec3::from(server.vel), &plan).map(|_| ())
    };
    if replayed.is_none() {
        return;
    }
    let Some(entity) = s.entities.get_mut(id) else { return };
    let error = entity.position.distance(before);
    s.net.on_prediction_error(error);
    match classify(error) {
        Correction::Ignore => {
            entity.position = before;
            entity.body.as_mut().expect("players move").movement = before_velocity;
        }
        Correction::Blend => s.visual_offset.absorb(before, entity.position),
        Correction::Snap => s.visual_offset.clear(),
    }
}

// ----------------------------------------------------------------------------------------- input

fn listen<E: JsCast + 'static>(window: &web_sys::Window, event: &str, mut handler: impl FnMut(E) + 'static) {
    let closure = Closure::<dyn FnMut(web_sys::Event)>::new(move |e: web_sys::Event| handler(e.unchecked_into()));
    window.add_event_listener_with_callback(event, closure.as_ref().unchecked_ref()).unwrap();
    closure.forget();
}

/// Events from the HTML menu (see the contract at the top of `menu.js`).
fn install_menu_events(window: &web_sys::Window, state: &Rc<RefCell<State>>) {
    let s = state.clone();
    listen(window, "wurfel:settings", move |_: web_sys::Event| {
        let mut s = s.borrow_mut();
        s.bindings = read_bindings();
        s.lighting.apply_settings();
    });

    let s = state.clone();
    listen(window, "wurfel:play", move |e: web_sys::CustomEvent| start_from_menu(&s, &e.detail()));
    // The menu may have been used before this code finished loading.
    let pending = js_get(window, "wurfelPlayRequest");
    if pending.is_object() {
        start_from_menu(state, &pending);
    }

    let s = state.clone();
    listen(window, "wurfel:leave", move |_: web_sys::Event| end_session(&mut s.borrow_mut()));
    let s = state.clone();
    listen(window, "wurfel:pause", move |_: web_sys::Event| s.borrow_mut().keys.clear());
}

/// What the page's HUD and console send to the server: `wurfelNet.action(name, arg)` answers a
/// dialog or offer, `wurfelNet.command(line)` is a console line of the game mode,
/// `wurfelNet.heart(playerId, on)` is the heart in the Tab player list.
fn install_net_bridge(window: &web_sys::Window, state: &Rc<RefCell<State>>) {
    let net = js_sys::Object::new();
    let s = state.clone();
    let action = Closure::<dyn FnMut(String, i32)>::new(move |name: String, arg: i32| {
        send(&mut s.borrow_mut(), &ClientMsg::Action { name, arg });
    });
    let _ = js_sys::Reflect::set(&net, &"action".into(), action.as_ref());
    action.forget();
    let s = state.clone();
    let command = Closure::<dyn FnMut(String)>::new(move |line: String| {
        send(&mut s.borrow_mut(), &ClientMsg::Command { line });
    });
    let _ = js_sys::Reflect::set(&net, &"command".into(), command.as_ref());
    command.forget();
    // The editor toolbar and the `editor` console command (editor.js, console-host.js).
    let s = state.clone();
    let editor_set = Closure::<dyn FnMut(String) -> String>::new(move |mode: String| {
        let mut s = s.borrow_mut();
        let on = match mode.as_str() {
            "on" => true,
            "off" => false,
            _ => !s.editor.active(),
        };
        match set_editor(&mut s, on) {
            Ok(()) => String::new(),
            Err(message) => message.to_string(),
        }
    });
    let _ = js_sys::Reflect::set(&net, &"editor".into(), editor_set.as_ref());
    editor_set.forget();
    let s = state.clone();
    let editor_tool = Closure::<dyn FnMut(String)>::new(move |name: String| {
        if let Some(tool) = Tool::parse(&name) {
            s.borrow_mut().editor.select_tool(tool);
        }
    });
    let _ = js_sys::Reflect::set(&net, &"editorTool".into(), editor_tool.as_ref());
    editor_tool.forget();
    let s = state.clone();
    let editor_block = Closure::<dyn FnMut(u32)>::new(move |index: u32| {
        s.borrow_mut().editor.select_block(index as usize);
    });
    let _ = js_sys::Reflect::set(&net, &"editorBlock".into(), editor_block.as_ref());
    editor_block.forget();
    let s = state.clone();
    let heart = Closure::<dyn FnMut(u32, bool)>::new(move |to: u32, on: bool| {
        send(&mut s.borrow_mut(), &ClientMsg::Heart { to, on });
    });
    let _ = js_sys::Reflect::set(&net, &"heart".into(), heart.as_ref());
    heart.forget();
    let _ = js_sys::Reflect::set(window, &"wurfelNet".into(), &net);
}

/// Join the world described by the menu's `wurfel:play` detail.
fn start_from_menu(state: &Rc<RefCell<State>>, detail: &JsValue) {
    let text = |key: &str| js_get(detail, key).as_string().unwrap_or_default();
    let join = Join { server: text("server") };
    {
        let mut s = state.borrow_mut();
        s.bindings = read_bindings();
        // The menu's default zoom (1 is the engine's native 100 px per block).
        if let Some(window) = web_sys::window() {
            if let Some(zoom) = js_get(&js_get(&window, "wurfelSettings"), "zoom").as_f64() {
                s.camera.zoom = (zoom as f32 * s.dpr).clamp(0.1 * s.dpr, 4.0 * s.dpr);
            }
        }
    }
    begin_session(state, join);
}

/// A mouse button went down in the game view. Only the editor turns this into a block edit.
fn editor_click(s: &mut State, button: i16) {
    let Some(button) = Button::from_dom(button) else { return };
    let target = hovered(s);
    let world = &s.world;
    let edit = s.editor.click(button, target, |(x, y, z)| world.get(x, y, z).id());
    if let Some(edit) = edit {
        send(s, &ClientMsg::SetBlock { x: edit.x, y: edit.y, z: edit.z, block: edit.block });
    }
}

/// Enter or leave the editor (F2, the `editor` console command, the toolbar). Not available
/// offline (the preview is read-only) or in a game mode (it has its own rules for blocks).
fn set_editor(s: &mut State, on: bool) -> Result<(), &'static str> {
    if on && !s.connected {
        return Err("The editor needs a world: join one from the menu first.");
    }
    if on && s.caveland.is_some() {
        return Err("The editor is not available in Caveland maps.");
    }
    if s.editor.active() != on {
        s.editor.set_active(on);
        send(s, &ClientMsg::Editor { on });
    }
    Ok(())
}

fn toggle_editor(s: &mut State) {
    let on = !s.editor.active();
    if let Err(message) = set_editor(s, on) {
        show_banner(message, Tone::Info);
    }
}

/// Push the toolbar state (and the cursor line) to `editor.js` when it changed.
fn update_editor_ui(s: &mut State, target: Option<Pick>) {
    let cursor = match target {
        Some(pick) => editor::cursor_text(Some(pick), s.world.get(pick.hit.0, pick.hit.1, pick.hit.2).id()),
        None => String::new(),
    };
    let json = s.editor.ui_json(&cursor);
    if json != s.editor_ui {
        call_js("wurfelEditor", "update", &JsValue::from_str(&json));
        s.editor_ui = json;
    }
}

fn install_input(window: &web_sys::Window, state: &Rc<RefCell<State>>) {
    let s = state.clone();
    listen(window, "resize", move |_: web_sys::Event| {
        let mut s = s.borrow_mut();
        fit_canvas(&s.canvas);
        s.config.width = s.canvas.width().max(1);
        s.config.height = s.canvas.height().max(1);
        s.surface.configure(&s.device, &s.config);
        let s = &mut *s;
        s.peeling.resize(&s.device, s.config.width, s.config.height);
    });

    let s = state.clone();
    listen(window, "mousemove", move |e: MouseEvent| {
        let mut s = s.borrow_mut();
        s.pointer = Some((e.client_x() as f32 * s.dpr, e.client_y() as f32 * s.dpr));
    });
    let s = state.clone();
    listen(window, "mousedown", move |e: MouseEvent| {
        let mut s = s.borrow_mut();
        s.pointer = Some((e.client_x() as f32 * s.dpr, e.client_y() as f32 * s.dpr));
        // Clicking a heart in the Tab list must not also place a block behind it.
        if input_blocked() || window_flag("wurfelScoreboardOpen") {
            return;
        }
        s.keys.insert(format!("mouse{}", e.button()));
        if s.caveland.is_some() {
            if let Some((name, arg)) = caveland_client::mouse_action(e.button(), true) {
                send_action(&mut s, name, arg);
            }
        }
        editor_click(&mut s, e.button());
    });
    let s = state.clone();
    listen(window, "mouseup", move |e: MouseEvent| {
        let mut s = s.borrow_mut();
        s.keys.remove(&format!("mouse{}", e.button()));
        if s.caveland.is_some() {
            if let Some((name, arg)) = caveland_client::mouse_action(e.button(), false) {
                send_action(&mut s, name, arg);
            }
        }
    });
    listen(window, "contextmenu", move |e: MouseEvent| {
        if !input_blocked() {
            e.prevent_default();
        }
    });

    let s = state.clone();
    listen(window, "wheel", move |e: WheelEvent| {
        if input_blocked() {
            return;
        }
        e.prevent_default();
        let mut s = s.borrow_mut();
        let dpr = s.dpr;
        s.camera.zoom = (s.camera.zoom * (-e.delta_y() as f32 * 0.001).exp()).clamp(0.1 * dpr, 4.0 * dpr);
    });

    let s = state.clone();
    listen(window, "keydown", move |e: KeyboardEvent| {
        let key = e.key().to_lowercase();
        let mut s = s.borrow_mut();
        match key.as_str() {
            "f3" if !window_flag("wurfelMenuOpen") => {
                e.prevent_default();
                s.show_net = !s.show_net;
                if s.show_net {
                    s.net.reset_prediction_peak();
                }
                set_overlay_visible(&s.net_overlay, s.show_net);
                return;
            }
            "f2" if !window_flag("wurfelMenuOpen") => {
                e.prevent_default();
                if !e.repeat() {
                    toggle_editor(&mut s);
                }
                return;
            }
            "f4" if !window_flag("wurfelMenuOpen") => {
                e.prevent_default();
                if let Some(minimap) = s.minimap.as_mut() {
                    minimap.toggle();
                }
                return;
            }
            _ => {}
        }
        if input_blocked() {
            return;
        }
        if s.caveland.is_some() {
            // Caveland keys: swing, throw, use, drop, craft...
            if !e.repeat() {
                if let Some((name, arg)) = caveland_client::key_action(&key, true) {
                    send_action(&mut s, name, arg);
                }
            }
        } else if let Some(index) = Editor::index_for_key(&key) {
            // Number keys choose the block to build with, but only inside the editor.
            s.editor.select_block(index);
        }
        s.keys.insert(key);
    });
    let s = state.clone();
    listen(window, "keyup", move |e: KeyboardEvent| {
        let key = e.key().to_lowercase();
        let mut s = s.borrow_mut();
        if s.caveland.is_some() && s.keys.contains(&key) {
            if let Some((name, arg)) = caveland_client::key_action(&key, false) {
                send_action(&mut s, name, arg);
            }
        }
        s.keys.remove(&key);
    });
    // Letting go of the window with keys held would otherwise leave the player running.
    let s = state.clone();
    listen(window, "blur", move |_: web_sys::Event| s.borrow_mut().keys.clear());
}

fn read_input(s: &State) -> PlayerInput {
    let held = |action: &str| s.bindings.held(action, &s.keys);
    PlayerInput { up: held("up"), down: held("down"), left: held("left"), right: held("right"), jump: held("jump") }
}

// ------------------------------------------------------------------------------------- game loop

/// Screen-space position (px at zoom 1, y down) of an isometric point.
fn screen_position((gx, gy): (f32, f32), z: f32) -> [f32; 2] {
    [(gx - gy) * 100.0, (gx + gy) * 50.0 - z * 122.0]
}

/// Where our player is drawn: the simulation plus the correction that is still fading out.
fn local_position(s: &State) -> Option<Vec3> {
    s.local_id.and_then(|id| s.entities.get(id)).map(|e| e.position + s.visual_offset.value())
}

/// The block under the pointer, if any.
fn hovered(s: &State) -> Option<Pick> {
    let (px, py) = s.pointer?;
    let sx = s.camera.center[0] + (px - s.config.width as f32 / 2.0) / s.camera.zoom;
    let sy = s.camera.center[1] + (py - s.config.height as f32 / 2.0) / s.camera.zoom;
    pick(&s.world, sx, sy)
}

fn start_frame_loop(state: Rc<RefCell<State>>) {
    let callback: Rc<RefCell<Option<Closure<dyn FnMut(f64)>>>> = Rc::new(RefCell::new(None));
    let next = callback.clone();
    let mut last_frame = f64::NEG_INFINITY;
    *next.borrow_mut() = Some(Closure::new(move |now_ms: f64| {
        // `fpsLimit` from the menu: 0 is unlimited. Skip callbacks that arrive too early; the 1 ms slack
        // keeps a 60 FPS cap from dropping to 30 on a 60 Hz display whose callbacks jitter.
        let limit = web_sys::window()
            .and_then(|w| js_get(&js_get(&w, "wurfelSettings"), "fpsLimit").as_f64())
            .unwrap_or(60.0);
        if limit < 1.0 || now_ms - last_frame >= 1000.0 / limit - 1.0 {
            last_frame = now_ms;
            frame(&mut state.borrow_mut(), now_ms);
        }
        request_animation_frame(callback.borrow().as_ref().unwrap());
    }));
    request_animation_frame(next.borrow().as_ref().unwrap());
}

fn request_animation_frame(closure: &Closure<dyn FnMut(f64)>) {
    web_sys::window().unwrap().request_animation_frame(closure.as_ref().unchecked_ref()).unwrap();
}

fn frame(s: &mut State, now_ms: f64) {
    let dt = s.last_frame_ms.map_or(0.0, |last| (((now_ms - last) / 1000.0) as f32).min(0.1));
    s.last_frame_ms = Some(now_ms);
    s.net.on_frame(dt as f64 * 1000.0);
    let ease = |rate: f32| 1.0 - (-rate * dt).exp();
    // While rejoining the world is frozen as it was: nobody is listening to our inputs.
    let blocked = input_blocked() || s.reconnect.active();

    let wanted = if blocked { PlayerInput::default() } else { read_input(s) };

    // Our own player: the same fixed physics steps as the server, so movement is instant.
    s.accumulator = (s.accumulator + dt).min(0.25);
    let mut events = Vec::new();
    while s.accumulator >= TICK_DT {
        s.accumulator -= TICK_DT;
        let mut jumped_from = None;
        let playing = s.local_id.is_some_and(|id| s.entities.get(id).is_some());
        if playing {
            // A change of keys starts a new numbered input at this step; the server applies the
            // same input from the tick it arrives in and echoes the number back.
            if let Some((seq, input)) = s.history.begin_step(wanted) {
                send(s, &ClientMsg::Input { seq, input });
            }
        }
        let input = s.history.current();
        if s.riding {
            // Carried by the server: our player does not walk, fall or jump by itself.
        } else if let (Some(caveland), Some(id)) = (s.caveland.as_mut(), s.local_id) {
            // Caveland's own walking, jump and jetpack rules, the same as the server runs.
            let was_on_ground = s.entities.get(id).is_some_and(|e| e.is_on_ground(&s.world));
            caveland.set_controls(&mut s.entities, &s.world, id, caveland_client::controls(input));
            if let Some(entity) = s.entities.get(id) {
                if was_on_ground && entity.body.as_ref().is_some_and(|b| b.movement.z > 1.0) {
                    jumped_from = Some(entity.position.to_array());
                }
            }
        } else if let Some(entity) = s.local_id.and_then(|id| s.entities.get_mut(id)) {
            let was_on_ground = entity.is_on_ground(&s.world);
            apply_input(entity, input, &s.world);
            if was_on_ground && entity.body.as_ref().is_some_and(|b| b.movement.z > 1.0) {
                jumped_from = Some(entity.position.to_array());
            }
        }
        if let (Some(id), Some(position)) = (s.local_id, jumped_from) {
            s.audio.on_jump(id, position);
        }
        match s.caveland.as_mut() {
            Some(_) if s.riding => {}
            Some(caveland) => {
                // Only our own player lives here, so the rules have nothing to hit or pick up; the
                // server decides all of that and tells us.
                caveland.tick(&mut s.entities, &mut s.world, TICK_DT);
                events.extend_from_slice(caveland.engine_events());
            }
            None => events.extend(s.entities.update(&s.world, TICK_DT)), // landed, collided, splashed...
        }
        if playing {
            s.history.end_step();
        }
    }
    s.visual_offset.decay(dt);
    // Sounds: what happened this frame, and the continuous ones (footsteps, wind) for our player.
    if let Some(id) = s.local_id {
        if let Some(entity) = s.entities.get(id) {
            let info = EntityInfo::from_entity(entity, &s.world);
            s.audio.set_listener(info.position);
            s.audio.update_entities(dt, &[(id, info)]);
        }
    }
    s.audio.handle_events(&events, &|id| s.entities.get(id).map(|e| EntityInfo::from_entity(e, &s.world)));
    s.audio.update(dt);
    s.particles.handle_events(&events, &s.entities, &s.world);
    s.particles.update(&s.world, dt);
    for emitter in &mut s.emitters {
        emitter.update(dt, &mut s.particles);
    }
    let burning = s.local_id.zip(s.caveland.as_ref()).and_then(|(id, c)| c.player(id)).is_some_and(|p| p.jetpack_on);
    let flame_at = if burning {
        local_position(s).map(|feet| (feet, s.local_id.map_or([0.0, 1.0], |id| s.actors.facing(id))))
    } else {
        None
    };
    s.jetpack.update(dt, &mut s.particles, flame_at);
    let focus = local_position(s).unwrap_or(Vec3::ZERO);
    let lamps = caveland_client::lamps(&s.world, &s.powered, &s.things);
    s.lighting.set_dynamic_lights(s.emitters.iter().filter_map(|e| e.light()).chain(s.jetpack.lights()).chain(lamps), focus);

    // Everyone else is drawn slightly in the past, between two snapshots.
    s.render_clock.advance(dt as f64 * 1000.0);
    let render_ms = s.render_clock.now_ms();
    for remote in s.remotes.values_mut() {
        if let Some(pos) = remote.track.sample(render_ms) {
            remote.pos = pos;
        }
    }
    let drawn: Vec<(u32, Vec3)> = s
        .my_id
        .zip(local_position(s))
        .into_iter()
        .chain(s.remotes.iter().map(|(&id, remote)| (id, remote.pos)))
        .collect();
    s.actors.update(dt, drawn, &s.things);

    // The camera follows us (or stays on the island while offline).
    if let Some(p) = local_position(s) {
        let target = screen_position((p.x, p.y), p.z + 0.7);
        let k = ease(8.0);
        s.camera.center[0] += (target[0] - s.camera.center[0]) * k;
        s.camera.center[1] += (target[1] - s.camera.center[1]) * k;
    }
    s.camera.zoom = s.camera.zoom.clamp(0.1 * s.dpr, 4.0 * s.dpr);

    // The server accepted the connection (or not) but never sent the world.
    if s.connecting_since.is_some_and(|since| now_ms - since > CONNECT_TIMEOUT_MS) {
        if s.reconnect.active() {
            // This try hangs: close it, which schedules the next one.
            s.connecting_since = None;
            if let Some(socket) = &s.socket {
                let _ = socket.close();
            }
        } else {
            end_session(s);
            hide_banner();
            report_error("The server did not answer in time. Check the address and try again.");
        }
    }

    s.lighting.update(dt * 1000.0);

    // Latency probe.
    if s.connected && now_ms >= s.next_ping_ms {
        s.next_ping_ms = now_ms + PING_EVERY_MS;
        let rtt_ms = s.net.last_rtt().map(|rtt| rtt as f32);
        send(s, &ClientMsg::Ping { client_time: now_ms, rtt_ms });
    }

    // Keep the drawn window centred on the player; crossing a chunk border moves it.
    if let Some(p) = local_position(s) {
        let (x, y) = from_iso(p.x, p.y);
        let chunk = chunk_of(x, y);
        if chunk != s.view_chunk {
            s.view_chunk = chunk;
            s.remesh = true;
        }
    }
    if s.remesh {
        s.remesh = false;
        s.render.update(&mut s.world, s.view_chunk);
        let vertices = s.render.vertices();
        s.world_buffer = vertex_buffer_with(&s.device, &vertices);
        s.world_vertices = vertices.len() as u32;
    }

    // The hover marker and the cursor info belong to the editor.
    let target = if s.editor.active() { hovered(s) } else { None };
    if s.model_placed.is_none() {
        place_model(s);
    }
    upload_dynamic_mesh(s, target);
    update_editor_ui(s, target);
    update_overlays(s, now_ms);
    update_info(s);
    update_name_tags(s);
    update_friend_markers(s);
    render(s);
}

/// The colour a player chose, or a palette colour until their details have arrived.
fn player_color(roster: &HashMap<u32, PlayerInfo>, id: u32) -> [f32; 3] {
    match roster.get(&id) {
        Some(info) => info.color.map(|c| c as f32 / 255.0),
        None => PLAYER_COLORS[id as usize % PLAYER_COLORS.len()],
    }
}

/// The smallest rectangle of chunks that holds everything loaded (the minimap's frame).
fn loaded_area(world: &World) -> ((i32, i32), (i32, i32)) {
    let mut area: Option<((i32, i32), (i32, i32))> = None;
    for chunk in world.loaded_chunks() {
        let (x, y) = chunk.pos();
        area = Some(match area {
            None => ((x, y), (x, y)),
            Some((min, max)) => ((min.0.min(x), min.1.min(y)), (max.0.max(x), max.1.max(y))),
        });
    }
    area.unwrap_or(((0, 0), (0, 0)))
}

/// The names above the heads of the other players: small DOM labels that follow the players.
struct NameTags {
    layer: Option<web_sys::HtmlElement>,
    tags: HashMap<u32, web_sys::HtmlElement>,
}

impl NameTags {
    fn new(document: &web_sys::Document) -> Self {
        let layer = document.create_element("div").ok().and_then(|e| e.dyn_into::<web_sys::HtmlElement>().ok());
        if let Some(layer) = &layer {
            layer.set_id("nametags");
            let _ = layer.set_attribute("aria-hidden", "true");
            // Under the HUD (8), the console and the menu, over the canvas.
            let _ = layer.set_attribute("style", "position:fixed;inset:0;overflow:hidden;pointer-events:none;z-index:4");
            if let Some(body) = document.body() {
                let _ = body.append_child(layer);
            }
        }
        NameTags { layer, tags: HashMap::new() }
    }
}

/// Place one label per other player at the top of their head, and drop the labels of those gone.
fn update_name_tags(s: &mut State) {
    let Some(layer) = s.name_tags.layer.clone() else { return };
    let Some(document) = layer.owner_document() else { return };
    let (width, height) = (s.config.width as f32, s.config.height as f32);
    let mut wanted: Vec<(u32, String, f32, f32)> = Vec::new();
    for (&id, remote) in s.remotes.iter().filter(|(id, _)| !s.hidden.contains(id)) {
        let Some(info) = s.roster.get(&id) else { continue };
        let head = (remote.pos.x, remote.pos.y, remote.pos.z + PLAYER_HEIGHT + 0.1);
        let screen = screen_position((head.0, head.1), head.2);
        let x = ((screen[0] - s.camera.center[0]) * s.camera.zoom + width / 2.0) / s.dpr;
        let y = ((screen[1] - s.camera.center[1]) * s.camera.zoom + height / 2.0) / s.dpr;
        if x > -100.0 && y > -50.0 && x < width / s.dpr + 100.0 && y < height / s.dpr + 50.0 {
            let name = if s.friends.contains(&id) { format!("♥ {}", info.name) } else { info.name.clone() };
            wanted.push((id, name, x, y));
        }
    }
    s.name_tags.tags.retain(|id, element| {
        let keep = wanted.iter().any(|(wanted_id, ..)| wanted_id == id);
        if !keep {
            element.remove();
        }
        keep
    });
    for (id, name, x, y) in wanted {
        let element = s.name_tags.tags.entry(id).or_insert_with(|| {
            let element: web_sys::HtmlElement = document.create_element("div").unwrap().unchecked_into();
            let _ = element.set_attribute(
                "style",
                "position:absolute;left:0;top:0;padding:1px 6px;border-radius:4px;white-space:nowrap;\
                 font:600 12px/1.3 system-ui,sans-serif;color:#fff;background:rgba(10,12,18,0.55);\
                 text-shadow:0 1px 2px #000;will-change:transform",
            );
            element.set_text_content(Some(&name));
            let _ = layer.append_child(&element);
            element
        });
        if element.text_content().as_deref() != Some(name.as_str()) {
            element.set_text_content(Some(&name));
        }
        let _ = element.style().set_property("transform", &format!("translate({x:.1}px, {y:.1}px) translate(-50%, -100%)"));
    }
}

/// One arrow with the friend's name and distance.
struct FriendMarker {
    root: web_sys::HtmlElement,
    arrow: web_sys::HtmlElement,
    label: web_sys::HtmlElement,
}

/// Arrows at the edge of the screen pointing to friends who are out of view, with their name and
/// distance in blocks, and a hint when they are well above or below.
struct FriendMarkers {
    layer: Option<web_sys::HtmlElement>,
    markers: HashMap<u32, FriendMarker>,
}

impl FriendMarkers {
    fn new(document: &web_sys::Document) -> Self {
        let layer = document.create_element("div").ok().and_then(|e| e.dyn_into::<web_sys::HtmlElement>().ok());
        if let Some(layer) = &layer {
            layer.set_id("friendmarkers");
            let _ = layer.set_attribute("aria-hidden", "true");
            let _ = layer.set_attribute("style", "position:fixed;inset:0;overflow:hidden;pointer-events:none;z-index:4");
            if let Some(body) = document.body() {
                let _ = body.append_child(layer);
            }
        }
        FriendMarkers { layer, markers: HashMap::new() }
    }
}

/// Keep one arrow per friend who is on the server but not on the screen.
fn update_friend_markers(s: &mut State) {
    let Some(layer) = s.friend_markers.layer.clone() else { return };
    let Some(document) = layer.owner_document() else { return };
    let (width, height) = (s.config.width as f32, s.config.height as f32);
    let (css_width, css_height) = (width / s.dpr, height / s.dpr);
    let mut wanted: Vec<(u32, String, [u8; 3], locator::Marker)> = Vec::new();
    if let Some(me) = local_position(s) {
        for &id in &s.friends {
            let Some(remote) = s.remotes.get(&id).filter(|_| !s.hidden.contains(&id)) else { continue };
            let Some(info) = s.roster.get(&id) else { continue };
            let middle = remote.pos + Vec3::Z * (PLAYER_HEIGHT / 2.0);
            let screen = screen_position((middle.x, middle.y), middle.z);
            let x = ((screen[0] - s.camera.center[0]) * s.camera.zoom + width / 2.0) / s.dpr;
            let y = ((screen[1] - s.camera.center[1]) * s.camera.zoom + height / 2.0) / s.dpr;
            let Some(marker) = locator::edge_marker((x, y), (css_width, css_height), (70.0, 44.0)) else { continue };
            let delta = remote.pos - me;
            let mut label = format!("{} {}", info.name, locator::distance_label(delta.length()));
            if let Some(hint) = locator::height_hint(delta.z) {
                label.push(' ');
                label.push(hint);
            }
            wanted.push((id, label, info.color, marker));
        }
    }
    s.friend_markers.markers.retain(|id, m| {
        let keep = wanted.iter().any(|(wanted_id, ..)| wanted_id == id);
        if !keep {
            m.root.remove();
        }
        keep
    });
    for (id, label, color, marker) in wanted {
        let m = s.friend_markers.markers.entry(id).or_insert_with(|| {
            let make = |style: &str| -> web_sys::HtmlElement {
                let element: web_sys::HtmlElement = document.create_element("div").unwrap().unchecked_into();
                let _ = element.set_attribute("style", style);
                element
            };
            let root = make("position:absolute;left:0;top:0;display:flex;flex-direction:column;align-items:center;gap:1px;will-change:transform");
            let arrow = make("font:700 22px/1 system-ui,sans-serif;text-shadow:0 1px 3px #000;will-change:transform");
            arrow.set_text_content(Some("➤"));
            let label = make(
                "padding:1px 6px;border-radius:4px;white-space:nowrap;font:600 12px/1.3 system-ui,sans-serif;\
                 color:#fff;background:rgba(10,12,18,0.6);text-shadow:0 1px 2px #000",
            );
            let _ = root.append_child(&arrow);
            let _ = root.append_child(&label);
            let _ = layer.append_child(&root);
            FriendMarker { root, arrow, label }
        });
        if m.label.text_content().as_deref() != Some(label.as_str()) {
            m.label.set_text_content(Some(&label));
        }
        let _ = m.arrow.style().set_property("color", &format!("rgb({},{},{})", color[0], color[1], color[2]));
        let _ = m.arrow.style().set_property("transform", &format!("rotate({:.1}deg)", marker.angle_deg));
        let _ = m.root.style().set_property("transform", &format!("translate({:.1}px, {:.1}px) translate(-50%, -50%)", marker.x, marker.y));
    }
}

fn push_player(vertices: &mut Vec<Vertex>, color: [f32; 3], pos: Vec3) {
    mesh::cuboid(vertices, color, [pos.x - 0.22, pos.x + 0.22, pos.y - 0.22, pos.y + 0.22], [pos.z, pos.z + PLAYER_HEIGHT]);
}

/// Players and the hover marker change every frame, so they live in a small separate buffer.
fn upload_dynamic_mesh(s: &mut State, target: Option<Pick>) {
    let mut vertices: Vec<Vertex> = Vec::new();
    if let Some((id, pos)) = s.my_id.zip(local_position(s)).filter(|(id, _)| !s.hidden.contains(id)) {
        let color = player_color(&s.roster, id);
        if !s.actors.push_player(&mut vertices, id, pos, color) {
            push_player(&mut vertices, color, pos);
        }
    }
    for (&id, remote) in s.remotes.iter().filter(|(id, _)| !s.hidden.contains(id)) {
        let color = player_color(&s.roster, id);
        if !s.actors.push_player(&mut vertices, id, remote.pos, color) {
            push_player(&mut vertices, color, remote.pos);
        }
    }
    for thing in s.things.iter().filter(|t| !crate::sprites::is_invisible(&t.kind)) {
        if crate::shadow::casts_shadow(&thing.kind) {
            crate::shadow::push_under(&mut vertices, &s.world, Vec3::from(thing.pos));
        }
        if !s.actors.push_thing(&mut vertices, thing) {
            caveland_client::push_thing(&mut vertices, thing);
        }
    }
    // The dotted arc of the machine being aimed: only the start of the flight.
    if s.aiming {
        let aimed = local_position(s).and_then(|at| caveland_client::aimed_launcher(&s.launchers, at)).copied();
        match aimed {
            Some(info) => {
                let key = caveland_client::preview_key(&info);
                if s.preview.as_ref().is_none_or(|(k, _)| *k != key) {
                    s.preview = Some((key, caveland_client::preview_dots(&s.world, &info)));
                }
                if let Some((_, dots)) = &s.preview {
                    caveland_client::push_preview(&mut vertices, dots);
                }
            }
            None => s.preview = None,
        }
    }
    if let Some(Pick { hit: (x, y, z), .. }) = target {
        let (gx, gy) = to_iso(x, y);
        // Just above the targeted block's top face.
        mesh::top_face_unlit(&mut vertices, MARKER_COLOR, [gx - 0.5, gx + 0.5, gy - 0.5, gy + 0.5], z as f32 + 1.03);
    }
    crate::particles::append(&s.particles, &mut vertices);
    vertices.truncate(DYNAMIC_VERTICES as usize);
    s.dynamic_vertices = vertices.len() as u32;
    if !vertices.is_empty() {
        s.queue.write_buffer(&s.dynamic_buffer, 0, bytemuck::cast_slice(&vertices));
    }
}

/// The network overlay (F3), the minimap (F4) and the status the menu reads.
fn update_overlays(s: &mut State, now_ms: f64) {
    if s.show_net && now_ms >= s.next_overlay_ms {
        s.next_overlay_ms = now_ms + NET_OVERLAY_EVERY_MS;
        let report = s.net.report(now_ms);
        if let Some(element) = &s.net_overlay {
            element.set_text_content(Some(&format_report(&report, s.connected)));
        }
    }

    if now_ms >= s.next_status_ms {
        s.next_status_ms = now_ms + STATUS_EVERY_MS;
        export_status(s);
    }

    if now_ms >= s.next_minimap_ms && s.minimap.as_ref().is_some_and(|m| m.is_visible()) {
        s.next_minimap_ms = now_ms + MINIMAP_EVERY_MS;
        let chunks: Vec<ChunkInfo> = s
            .world
            .loaded_chunks()
            .map(|c| ChunkInfo { pos: c.pos(), state: ChunkState::Loaded, rendered: s.render.chunk(c.pos().0, c.pos().1).is_some() })
            .collect();
        let roster = &s.roster;
        let dot = |id: u32, pos: Vec3, local: bool| {
            let color = player_color(roster, id);
            EntityDot { id, pos: pos.to_array(), color: color.map(|c| (c * 255.0) as u8), local, label: local }
        };
        let mut entities: Vec<EntityDot> = s.remotes.iter().map(|(&id, r)| dot(id, r.pos, false)).collect();
        if let (Some(id), Some(pos)) = (s.my_id, local_position(s)) {
            entities.push(dot(id, pos, true));
        }
        let (width, height) = (s.config.width as f32, s.config.height as f32);
        if let Some(minimap) = s.minimap.as_mut() {
            let data = MinimapData {
                mode: Mode::Map,
                world: &s.world,
                area: loaded_area(&s.world),
                chunks,
                entities,
                camera: Some(CameraView { center: s.camera.center, size: [width / s.camera.zoom, height / s.camera.zoom] }),
                terrain_version: s.terrain_version,
                pixel_ratio: minimap.pixel_ratio(),
            };
            minimap.update(&data);
        }
    }
}

fn update_info(s: &mut State) {
    let status = match (s.connected, s.my_id) {
        (true, Some(id)) => format!("online as player {id} · {} other(s)", s.remotes.len()),
        (true, None) => "connecting…".to_string(),
        (false, _) if s.reconnect.active() => "server updating, reconnecting…".to_string(),
        (false, _) => "offline: showing a preview. Open the menu (Esc) to join a world".to_string(),
    };
    let text = format!("Wurfel Engine · {:?} · {status}", s.backend);
    if text != s.info_text {
        set_info(&text);
        s.info_text = text;
    }
}

fn render(s: &mut State) {
    let uniform = CameraUniform {
        center: s.camera.center,
        scale: [2.0 * s.camera.zoom / s.config.width as f32, 2.0 * s.camera.zoom / s.config.height as f32],
        // At ground level the screen row is (gx + gy) * 50.
        center_depth: s.camera.center[1] / 50.0,
        _pad: [0.0; 3],
    };
    s.queue.write_buffer(&s.camera_buffer, 0, bytemuck::bytes_of(&uniform));
    s.queue.write_buffer(&s.lighting_buffer, 0, bytemuck::bytes_of(&s.lighting.uniform()));

    use wgpu::CurrentSurfaceTexture as Frame;
    let output = match s.surface.get_current_texture() {
        Frame::Success(texture) | Frame::Suboptimal(texture) => texture,
        // Lost, outdated, timeout...: reconfigure and try again next frame.
        other => {
            web_sys::console::warn_1(&format!("surface not ready: {}", surface_status(&other)).into());
            s.surface.configure(&s.device, &s.config);
            return;
        }
    };
    let view = output.texture.create_view(&Default::default());
    let mut encoder = s.device.create_command_encoder(&Default::default());
    // The scene is drawn once per depth peeling layer, in any order, and the layers are blended
    // onto the canvas (see `peel.rs`).
    let background = wgpu::Color { r: 0.063, g: 0.075, b: 0.102, a: 1.0 };
    s.peeling.render(&mut encoder, &view, background, |pass| {
        pass.set_pipeline(&s.pipeline);
        pass.set_bind_group(0, &s.bind_group, &[]);
        pass.set_bind_group(1, &s.atlas_bind_group, &[]);
        if s.world_vertices > 0 {
            pass.set_vertex_buffer(0, s.world_buffer.slice(..));
            pass.draw(0..s.world_vertices, 0..1);
        }
        if s.dynamic_vertices > 0 {
            pass.set_vertex_buffer(0, s.dynamic_buffer.slice(..));
            pass.draw(0..s.dynamic_vertices, 0..1);
        }
        if let (Some(placed), Some(loaded)) = (&s.model_placed, &s.model_loaded) {
            pass.set_vertex_buffer(0, placed.buffer.slice(..));
            for (picture, range) in &placed.draws {
                let group = picture.and_then(|i| loaded.pictures.get(i)).unwrap_or(&s.atlas_bind_group);
                pass.set_bind_group(1, group, &[]);
                pass.draw(range.clone(), 0..1);
            }
        }
    });
    s.queue.submit(Some(encoder.finish()));
    s.queue.present(output);
}

fn surface_status(frame: &wgpu::CurrentSurfaceTexture) -> &'static str {
    use wgpu::CurrentSurfaceTexture as Frame;
    match frame {
        Frame::Success(_) => "success",
        Frame::Suboptimal(_) => "suboptimal",
        Frame::Timeout => "timeout",
        Frame::Occluded => "occluded",
        Frame::Outdated => "outdated",
        Frame::Lost => "lost",
        Frame::Validation => "validation error",
    }
}
