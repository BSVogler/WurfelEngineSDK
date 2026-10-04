//! Browser glue: canvas, wgpu, input, networking and the frame loop.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use glam::Vec3;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{HtmlCanvasElement, KeyboardEvent, MessageEvent, MouseEvent, WebSocket, WheelEvent};
use wurfel_sim::block::id;
use wurfel_sim::entity::{Entities, EntityId};
use wurfel_sim::grid::{chunk_of, from_iso, to_iso};
use wurfel_sim::player::{apply_input, new_player, PlayerInput, PLAYER_HEIGHT, TICK_DT};
use wurfel_sim::protocol::{decode_chunk, ClientMsg, PlayerInfo, PlayerState, ServerMsg};
use wurfel_sim::{Block, IslandGenerator, World};

use crate::audio::{Audio, EntityInfo};
use crate::bindings::{parse_hex_color, Bindings};
use crate::mesh::{self, Vertex};
use crate::minimap::{CameraView, ChunkInfo, ChunkState, EntityDot, Minimap, MinimapData, Mode};
use crate::netstats::{format_report, NetStats};
use crate::pick::{pick, Pick};
use crate::render_storage::RenderStorage;

/// Seed of the island shown behind the menu, before a world has been joined.
const DEFAULT_SEED: u64 = 1;
/// Blocks selectable with the number keys.
const HOTBAR: [(u8, &str); 4] =
    [(id::STONE, "stone"), (id::DIRT, "dirt"), (id::GRASS, "grass"), (id::SAND, "sand")];
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
const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

/// How often the latency probe is sent, and how often the overlays refresh (milliseconds).
const PING_EVERY_MS: f64 = 1000.0;
const NET_OVERLAY_EVERY_MS: f64 = 250.0;
const MINIMAP_EVERY_MS: f64 = 100.0;
const STATUS_EVERY_MS: f64 = 500.0;
/// Our predicted position is only corrected by the server when it differs by more than this.
const SNAP_DISTANCE: f32 = 1.5;
const MINIMAP_SIZE: (u32, u32) = (150, 300);
/// Give up on a connection that has not delivered the world after this long.
const CONNECT_TIMEOUT_MS: f64 = 8000.0;

pub fn start() {
    console_error_panic_hook::set_once();
    wasm_bindgen_futures::spawn_local(async {
        if let Err(message) = run().await {
            web_sys::console::error_1(&message.clone().into());
            set_info(&format!("Failed to start: {message}"));
        }
    });
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
    pos: Vec3,
    target: Vec3,
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
    bind_group: wgpu::BindGroup,
    world_buffer: wgpu::Buffer,
    world_vertices: u32,
    dynamic_buffer: wgpu::Buffer,
    dynamic_vertices: u32,
    depth_view: wgpu::TextureView,
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
    my_id: Option<u32>,
    /// Holds just our own player, simulated locally with the same fixed steps as the server so
    /// that movement feels instant (prediction).
    entities: Entities,
    local_id: Option<EntityId>,
    remotes: HashMap<u32, Remote>,
    /// Names and colours of everybody in the world.
    roster: HashMap<u32, PlayerInfo>,
    /// Real time not yet consumed by fixed physics steps.
    accumulator: f32,

    // --- input
    keys: HashSet<String>,
    bindings: Bindings,
    input: PlayerInput,
    /// Pointer position in canvas pixels.
    pointer: Option<(f32, f32)>,
    selected: usize,

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
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("blocks"),
        bind_group_layouts: &[Some(&bind_group_layout)],
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
        depth_view: create_depth_view(&device, config.width, config.height),
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
        bind_group,
        world_buffer,
        world_vertices: world_vertices.len() as u32,
        dynamic_buffer,
        dynamic_vertices: 0,
        backend,
        world,
        view_chunk: (0, 0),
        render,
        remesh: false,
        terrain_version: 0,
        socket: None,
        connected: false,
        connecting_since: None,
        my_id: None,
        entities: Entities::new(),
        local_id: None,
        remotes: HashMap::new(),
        roster: HashMap::new(),
        accumulator: 0.0,
        keys: HashSet::new(),
        bindings: read_bindings(),
        input: PlayerInput::default(),
        pointer: None,
        selected: 0,
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
    start_frame_loop(state);
    Ok(())
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

fn create_depth_view(device: &wgpu::Device, width: u32, height: u32) -> wgpu::TextureView {
    device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some("depth"),
            size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: DEPTH_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        })
        .create_view(&Default::default())
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
    window_flag("wurfelMenuOpen") || window_flag("wurfelConsoleOpen")
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
    let _ = js_sys::Reflect::set(&window, &JsValue::from_str("wurfelStatus"), &status);
}

// --------------------------------------------------------------------------------- status banner

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tone {
    Info,
    Ok,
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
    if tone == Tone::Ok {
        let hide = Closure::once_into_js(move || {
            if let Some(el) = web_sys::window().and_then(|w| w.document()).and_then(|d| d.get_element_by_id("statusbanner")) {
                if el.get_attribute("data-seq").and_then(|v| v.parse::<u32>().ok()) == Some(seq) {
                    let _ = el.set_attribute("style", "display:none");
                }
            }
        });
        let _ = window.set_timeout_with_callback_and_timeout_and_arguments_0(hide.unchecked_ref(), 2500);
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
    s.connected = false;
    s.connecting_since = None;
    s.my_id = None;
    s.local_id = None;
    s.entities = Entities::new();
    s.remotes.clear();
    s.roster.clear();
    s.input = PlayerInput::default();
    s.world = World::new(IslandGenerator::new(DEFAULT_SEED));
    s.view_chunk = (0, 0);
    s.remesh = true;
    s.terrain_version += 1;
}

fn begin_session(state: &Rc<RefCell<State>>, join: Join) {
    show_banner("Connecting to the server…", Tone::Info);
    end_session(&mut state.borrow_mut());
    if let Some(window) = web_sys::window() {
        connect(&window, state, &join.server);
    }
}

fn connect(window: &web_sys::Window, state: &Rc<RefCell<State>>, url: &str) {
    let _ = window;
    let socket = match WebSocket::new(url) {
        Ok(socket) => socket,
        Err(_) => {
            report_error(&format!("Cannot open {url}"));
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
    let on_close = Closure::<dyn FnMut()>::new(move || {
        let mut s = s.borrow_mut();
        let was_in_world = s.my_id.is_some();
        s.connected = false;
        s.connecting_since = None;
        s.my_id = None;
        s.local_id = None;
        s.entities = Entities::new();
        s.remotes.clear();
        s.socket = None;
        if was_in_world {
            report_error("Connection to the server lost. Pick a world to join again.");
        } else {
            report_error(&format!(
                "Could not join the world: the server at {url_for_message} refused or closed the connection. \
                 Does the world still exist?"
            ));
        }
    });
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
        ServerMsg::Welcome { your_id, map, players, roster, .. } => {
            s.roster = roster.into_iter().map(|p| (p.id, p)).collect();
            // The server sends the terrain as chunks: this world has no generator of its own and
            // fills up as they arrive.
            s.world = World::remote();
            s.remesh = true;
            s.terrain_version += 1;

            s.my_id = Some(your_id);
            s.remotes.clear();
            s.entities = Entities::new();
            s.local_id = None;
            for p in &players {
                let pos = Vec3::from(p.pos);
                if p.id == your_id {
                    let mut entity = new_player(pos);
                    entity.body.as_mut().expect("players move").movement = Vec3::from(p.vel);
                    s.local_id = Some(s.entities.spawn(entity));
                    s.camera.center = screen_position((pos.x, pos.y), pos.z + 0.7);
                    let (bx, by) = from_iso(pos.x, pos.y);
                    s.view_chunk = chunk_of(bx, by);
                } else {
                    s.remotes.insert(p.id, Remote { pos, target: pos });
                }
            }
            s.connecting_since = None;
            show_banner(&format!("Joined '{map}'"), Tone::Ok);
            s.audio.play_music("overworld");
        }
        ServerMsg::Snapshot { tick, players } => {
            s.net.on_snapshot(tick, now);
            apply_snapshot(s, &players);
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
        ServerMsg::Lobby { .. } | ServerMsg::Maps { .. } | ServerMsg::WorldChanged { .. } | ServerMsg::MapCreated { .. } => {}
        ServerMsg::Failed { message, .. } => report_error(&message),
        ServerMsg::Pong { client_time, .. } => s.net.on_pong(client_time, now),
        ServerMsg::Stats(stats) => s.net.on_server_stats(stats),
    }
}

fn apply_snapshot(s: &mut State, players: &[PlayerState]) {
    let present: HashSet<u32> = players.iter().map(|p| p.id).collect();
    s.remotes.retain(|id, _| present.contains(id));
    for p in players {
        let pos = Vec3::from(p.pos);
        if Some(p.id) == s.my_id {
            reconcile(s, pos, Vec3::from(p.vel));
        } else {
            s.remotes.entry(p.id).or_insert(Remote { pos, target: pos }).target = pos;
        }
    }
}

/// Our own player runs ahead of the server by about one round trip, so the two never agree
/// exactly. A big difference means the server saw something else (we were blocked, pushed or fell
/// differently) and we jump to its state; while standing still any small drift is eased away.
fn reconcile(s: &mut State, server_pos: Vec3, server_vel: Vec3) {
    let Some(entity) = s.local_id.and_then(|id| s.entities.get_mut(id)) else { return };
    let error = entity.position.distance(server_pos);
    s.net.on_prediction_error(error);
    let body = entity.body.as_mut().expect("players move");
    if error > SNAP_DISTANCE {
        entity.position = server_pos;
        body.movement = server_vel;
    } else if s.input == PlayerInput::default() && body.speed() < 0.05 && server_vel.length() < 0.05 {
        entity.position += (server_pos - entity.position) * 0.1;
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

fn place_block(s: &mut State) {
    if let Some(Pick { place: (x, y, z), .. }) = hovered(s) {
        let block = Block::new(HOTBAR[s.selected].0, 0).raw();
        send(s, &ClientMsg::SetBlock { x, y, z, block });
    }
}

fn break_block(s: &mut State) {
    if let Some(Pick { hit: (x, y, z), .. }) = hovered(s) {
        send(s, &ClientMsg::SetBlock { x, y, z, block: 0 });
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
        s.depth_view = create_depth_view(&s.device, s.config.width, s.config.height);
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
        if input_blocked() {
            return;
        }
        s.keys.insert(format!("mouse{}", e.button()));
        if s.bindings.matches_button("place", e.button()) {
            place_block(&mut s);
        } else if s.bindings.matches_button("break", e.button()) {
            break_block(&mut s);
        }
    });
    let s = state.clone();
    listen(window, "mouseup", move |e: MouseEvent| {
        s.borrow_mut().keys.remove(&format!("mouse{}", e.button()));
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
        if let Some(n) = key.parse::<usize>().ok().filter(|n| (1..=HOTBAR.len()).contains(n)) {
            s.selected = n - 1;
        }
        if !e.repeat() {
            if s.bindings.matches_key("place", &key) {
                place_block(&mut s);
            } else if s.bindings.matches_key("break", &key) {
                break_block(&mut s);
            }
        }
        s.keys.insert(key);
    });
    let s = state.clone();
    listen(window, "keyup", move |e: KeyboardEvent| {
        s.borrow_mut().keys.remove(&e.key().to_lowercase());
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

fn local_position(s: &State) -> Option<Vec3> {
    s.local_id.and_then(|id| s.entities.get(id)).map(|e| e.position)
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
    *next.borrow_mut() = Some(Closure::new(move |now_ms: f64| {
        frame(&mut state.borrow_mut(), now_ms);
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
    let blocked = input_blocked();

    // Tell the server when the pressed keys change; the server keeps applying them.
    let input = if blocked { PlayerInput::default() } else { read_input(s) };
    if input != s.input {
        s.input = input;
        send(s, &ClientMsg::Input(input));
    }

    // Our own player: the same fixed physics steps as the server, so movement is instant.
    s.accumulator = (s.accumulator + dt).min(0.25);
    let mut events = Vec::new();
    while s.accumulator >= TICK_DT {
        s.accumulator -= TICK_DT;
        let mut jumped_from = None;
        if let Some(entity) = s.local_id.and_then(|id| s.entities.get_mut(id)) {
            let was_on_ground = entity.is_on_ground(&s.world);
            apply_input(entity, s.input, &s.world);
            if was_on_ground && entity.body.as_ref().is_some_and(|b| b.movement.z > 1.0) {
                jumped_from = Some(entity.position.to_array());
            }
        }
        if let (Some(id), Some(position)) = (s.local_id, jumped_from) {
            s.audio.on_jump(id, position);
        }
        events.extend(s.entities.update(&s.world, TICK_DT)); // landed, collided, splashed...
    }
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
    let focus = local_position(s).unwrap_or(Vec3::ZERO);
    s.lighting.set_dynamic_lights(s.emitters.iter().filter_map(|e| e.light()), focus);

    // Everyone else glides towards their last known position.
    let k = ease(15.0);
    for remote in s.remotes.values_mut() {
        remote.pos += (remote.target - remote.pos) * k;
    }

    // The camera follows us (or stays on the island while offline).
    if let Some(p) = local_position(s) {
        let target = screen_position((p.x, p.y), p.z + 0.7);
        let k = ease(8.0);
        s.camera.center[0] += (target[0] - s.camera.center[0]) * k;
        s.camera.center[1] += (target[1] - s.camera.center[1]) * k;
    }
    if !blocked {
        if s.bindings.held("zoomOut", &s.keys) {
            s.camera.zoom *= 1.0 - 1.5 * dt;
        }
        if s.bindings.held("zoomIn", &s.keys) {
            s.camera.zoom *= 1.0 + 1.5 * dt;
        }
    }
    s.camera.zoom = s.camera.zoom.clamp(0.1 * s.dpr, 4.0 * s.dpr);

    // The server accepted the connection (or not) but never sent the world.
    if s.connecting_since.is_some_and(|since| now_ms - since > CONNECT_TIMEOUT_MS) {
        end_session(s);
        hide_banner();
        report_error("The server did not answer in time. Check the address and try again.");
    }

    s.lighting.update(dt * 1000.0);

    // Latency probe.
    if s.connected && now_ms >= s.next_ping_ms {
        s.next_ping_ms = now_ms + PING_EVERY_MS;
        send(s, &ClientMsg::Ping { client_time: now_ms });
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

    let target = hovered(s);
    upload_dynamic_mesh(s, target);
    update_overlays(s, now_ms);
    update_info(s);
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

fn push_player(vertices: &mut Vec<Vertex>, color: [f32; 3], pos: Vec3) {
    mesh::cuboid(vertices, color, [pos.x - 0.22, pos.x + 0.22, pos.y - 0.22, pos.y + 0.22], [pos.z, pos.z + PLAYER_HEIGHT]);
}

/// Players and the hover marker change every frame, so they live in a small separate buffer.
fn upload_dynamic_mesh(s: &mut State, target: Option<Pick>) {
    let mut vertices: Vec<Vertex> = Vec::new();
    if let (Some(id), Some(pos)) = (s.my_id, local_position(s)) {
        push_player(&mut vertices, player_color(&s.roster, id), pos);
    }
    for (&id, remote) in &s.remotes {
        push_player(&mut vertices, player_color(&s.roster, id), remote.pos);
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
        (false, _) => "offline: showing a preview. Open the menu (Esc) to join a world".to_string(),
    };
    let text = format!(
        "Wurfel Engine · {:?} · {status}\nWASD walk · Space jump · left click place {} · right click break · 1-4 block · F3 network · F4 map",
        s.backend, HOTBAR[s.selected].1,
    );
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
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("world"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color { r: 0.063, g: 0.075, b: 0.102, a: 1.0 }),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &s.depth_view,
                depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Discard }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&s.pipeline);
        pass.set_bind_group(0, &s.bind_group, &[]);
        if s.world_vertices > 0 {
            pass.set_vertex_buffer(0, s.world_buffer.slice(..));
            pass.draw(0..s.world_vertices, 0..1);
        }
        if s.dynamic_vertices > 0 {
            pass.set_vertex_buffer(0, s.dynamic_buffer.slice(..));
            pass.draw(0..s.dynamic_vertices, 0..1);
        }
    }
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
