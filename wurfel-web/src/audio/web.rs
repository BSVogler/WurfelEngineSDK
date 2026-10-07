//! Browser side of the audio: WebAudio nodes, asset loading and the autoplay policy.
//!
//! * Effects are decoded into `AudioBuffer`s (fetched in the background) and played through
//!   `source -> gain -> stereo panner -> effects bus -> destination`.
//! * Music is streamed with an `<audio>` element routed through a gain node into the music bus, so
//!   a 4 minute track does not have to be decoded into memory.
//! * Browsers keep an `AudioContext` suspended and refuse `play()` until the user has interacted
//!   with the page. The first pointer, key, touch or `wurfel:play` event resumes the context and
//!   retries music that was blocked.
//! * Nothing here panics. If audio is unavailable or a file fails to load the problem is logged
//!   once and the affected sound stays silent.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use js_sys::{ArrayBuffer, Reflect};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::{spawn_local, JsFuture};
use web_sys::{
    AnalyserNode, AudioBuffer, AudioBufferSourceNode, AudioContext, AudioContextState, AudioNode, AudioScheduledSourceNode,
    CustomEvent, GainNode, HtmlAudioElement, MediaElementAudioSourceNode, Response, StereoPannerNode,
};
use wurfel_sim::entity::{EntityId, Event};

use super::{AudioLogic, AudioSettings, EntityInfo, LoopHandle, MusicCommand, Position, Setting, SoundCommand};

/// Events that count as "the user interacted with the page".
const GESTURE_EVENTS: [&str; 5] = ["pointerdown", "mousedown", "keydown", "touchstart", "wurfel:play"];
/// Smoothing time constant for gain changes in seconds, so volume changes do not click.
const GAIN_SMOOTHING: f64 = 0.02;
/// Loaded immediately: the menu and the player's own sounds, about 0.9 MB. The rest is loaded
/// after the first user gesture, or when a sound is first asked for.
const ESSENTIAL: [&str; 8] = ["menuSelect", "menuAbort", "menuConfirm", "landing", "splash", "step", "urfJump", "wind"];

enum Load {
    Loading,
    Ready(AudioBuffer),
    Failed,
}

/// State shared with event listeners and background tasks.
#[derive(Default)]
struct Shared {
    buffers: RefCell<HashMap<String, Load>>,
    warned: RefCell<HashSet<String>>,
    /// Set by the gesture listeners, consumed by `Backend::tick`.
    gesture: Cell<bool>,
    pending_settings: RefCell<Option<AudioSettings>>,
    /// Non-looping tracks that finished playing.
    ended_tracks: RefCell<Vec<String>>,
    /// Sounds requested by page scripts with the `wurfel:sound` event: `(name, volume)`.
    pending_sounds: RefCell<Vec<(String, f32)>>,
}

impl Shared {
    /// Log a problem once per `key`.
    fn warn_once(&self, key: &str, message: &str) {
        if self.warned.borrow_mut().insert(key.to_string()) {
            web_sys::console::warn_1(&format!("audio: {message}").into());
        }
    }
}

struct LoopNode {
    name: String,
    volume: f32,
    pan: f32,
    /// `None` until the buffer has loaded.
    playing: Option<Playing>,
}

struct Playing {
    source: AudioBufferSourceNode,
    gain: GainNode,
    panner: Option<StereoPannerNode>,
}

struct MusicNode {
    element: HtmlAudioElement,
    gain: GainNode,
    /// Keeps the graph connection alive.
    _source: MediaElementAudioSourceNode,
    wants_play: bool,
    paused: bool,
    /// `play()` was rejected (autoplay policy); retried on the next gesture.
    blocked: Rc<Cell<bool>>,
}

struct Backend {
    ctx: Option<AudioContext>,
    effects_bus: Option<GainNode>,
    /// All one-off sounds go through this node so `StopAll` can cut them off by replacing it.
    oneshot_bus: Option<GainNode>,
    music_bus: Option<GainNode>,
    /// Both buses end here; the analyser only listens, it is there to measure the output level.
    analyser: Option<AnalyserNode>,
    shared: Rc<Shared>,
    sound_paths: HashMap<String, String>,
    music_paths: HashMap<String, String>,
    loops: HashMap<LoopHandle, LoopNode>,
    music: HashMap<String, MusicNode>,
    prefetched_all: bool,
}

impl Backend {
    fn new(logic: &AudioLogic, shared: Rc<Shared>) -> Self {
        let sound_paths = logic.bank().names().filter_map(|n| logic.bank().path(n).map(|p| (n.to_string(), p.to_string()))).collect();
        let music_paths = logic.music().track_names().filter_map(|n| logic.music().path(n).map(|p| (n.to_string(), p.to_string()))).collect();
        let mut backend = Backend {
            ctx: None,
            effects_bus: None,
            oneshot_bus: None,
            music_bus: None,
            analyser: None,
            shared,
            sound_paths,
            music_paths,
            loops: HashMap::new(),
            music: HashMap::new(),
            prefetched_all: false,
        };
        match AudioContext::new() {
            Ok(ctx) => {
                if let Err(e) = backend.build_graph(&ctx) {
                    backend.shared.warn_once("graph", &format!("cannot build the audio graph: {}", js_err(&e)));
                } else {
                    backend.ctx = Some(ctx);
                }
            }
            Err(e) => backend.shared.warn_once("context", &format!("audio is unavailable: {}", js_err(&e))),
        }
        if backend.ctx.is_some() {
            for name in ESSENTIAL {
                backend.ensure_loaded(name);
            }
        }
        backend
    }

    /// `effects bus` and `music bus` -> `master` -> destination, with an analyser tapping `master`.
    fn build_graph(&mut self, ctx: &AudioContext) -> Result<(), JsValue> {
        let destination: &AudioNode = &ctx.destination();
        let master = ctx.create_gain()?;
        master.connect_with_audio_node(destination)?;
        let effects = ctx.create_gain()?;
        let music = ctx.create_gain()?;
        effects.connect_with_audio_node(&master)?;
        music.connect_with_audio_node(&master)?;
        let oneshot = ctx.create_gain()?;
        oneshot.connect_with_audio_node(&effects)?;
        // The analyser is optional: if it cannot be made the level just reads as 0.
        self.analyser = ctx.create_analyser().ok().and_then(|a| master.connect_with_audio_node(&a).ok().map(|_| a));
        self.effects_bus = Some(effects);
        self.music_bus = Some(music);
        self.oneshot_bus = Some(oneshot);
        Ok(())
    }

    #[allow(dead_code)] // only `Audio::status` asks
    fn is_available(&self) -> bool {
        self.ctx.is_some()
    }

    // ---- loading

    fn ensure_loaded(&mut self, name: &str) {
        let (Some(ctx), Some(path)) = (self.ctx.clone(), self.sound_paths.get(name).cloned()) else { return };
        if self.shared.buffers.borrow().contains_key(name) {
            return;
        }
        self.shared.buffers.borrow_mut().insert(name.to_string(), Load::Loading);
        let shared = self.shared.clone();
        let name = name.to_string();
        spawn_local(async move {
            let outcome = match fetch_and_decode(&ctx, &path).await {
                Ok(buffer) => Load::Ready(buffer),
                Err(message) => {
                    shared.warn_once(&format!("load:{name}"), &format!("could not load '{name}' ({path}): {message}"));
                    Load::Failed
                }
            };
            shared.buffers.borrow_mut().insert(name, outcome);
        });
    }

    fn buffer(&self, name: &str) -> Option<AudioBuffer> {
        match self.shared.buffers.borrow().get(name) {
            Some(Load::Ready(buffer)) => Some(buffer.clone()),
            _ => None,
        }
    }

    // ---- commands

    fn execute(&mut self, command: SoundCommand) {
        match command {
            SoundCommand::SetBuses { effects, music } => {
                self.set_gain(self.effects_bus.as_ref(), effects);
                self.set_gain(self.music_bus.as_ref(), music);
            }
            SoundCommand::Play { name, volume, pitch, pan } => self.play_once(&name, volume, pitch, pan),
            SoundCommand::StartLoop { handle, name, volume, pan } => {
                self.ensure_loaded(&name);
                let mut node = LoopNode { name, volume, pan, playing: None };
                self.try_start_loop(&mut node);
                self.loops.insert(handle, node);
            }
            SoundCommand::SetLoop { handle, volume, pan } => {
                if let Some(node) = self.loops.get_mut(&handle) {
                    node.volume = volume;
                    node.pan = pan;
                    if let Some(playing) = &node.playing {
                        set_gain_node(self.ctx.as_ref(), Some(&playing.gain), volume);
                        if let Some(panner) = &playing.panner {
                            panner.pan().set_value(pan);
                        }
                    }
                }
            }
            SoundCommand::StopLoop { handle } => {
                if let Some(node) = self.loops.remove(&handle) {
                    stop_loop_node(node);
                }
            }
            SoundCommand::StopAll => {
                for (_, node) in self.loops.drain() {
                    stop_loop_node(node);
                }
                self.replace_oneshot_bus();
            }
            SoundCommand::Music(command) => self.execute_music(command),
        }
    }

    fn set_gain(&self, node: Option<&GainNode>, value: f32) {
        set_gain_node(self.ctx.as_ref(), node, value);
    }

    /// Cut off every sound that is currently playing through the one-shot bus.
    fn replace_oneshot_bus(&mut self) {
        let (Some(ctx), Some(effects)) = (&self.ctx, &self.effects_bus) else { return };
        if let Some(old) = self.oneshot_bus.take() {
            let _ = old.disconnect();
        }
        if let Ok(new) = ctx.create_gain() {
            if new.connect_with_audio_node(effects).is_ok() {
                self.oneshot_bus = Some(new);
            }
        }
    }

    fn play_once(&mut self, name: &str, volume: f32, pitch: f32, pan: f32) {
        let Some(ctx) = self.ctx.clone() else { return };
        let Some(buffer) = self.buffer(name) else {
            // Not loaded yet (or failed): start loading and skip this one.
            self.ensure_loaded(name);
            return;
        };
        let Some(bus) = self.oneshot_bus.clone() else { return };
        match build_chain(&ctx, &buffer, volume, pan, false, pitch, &bus) {
            Ok(_) => {}
            Err(e) => self.shared.warn_once(&format!("play:{name}"), &format!("cannot play '{name}': {}", js_err(&e))),
        }
    }

    fn try_start_loop(&self, node: &mut LoopNode) {
        if node.playing.is_some() {
            return;
        }
        let (Some(ctx), Some(buffer), Some(bus)) = (&self.ctx, self.buffer(&node.name), &self.oneshot_bus) else { return };
        match build_chain(ctx, &buffer, node.volume, node.pan, true, 1.0, bus) {
            Ok(playing) => node.playing = Some(playing),
            Err(e) => self.shared.warn_once(&format!("loop:{}", node.name), &format!("cannot loop '{}': {}", node.name, js_err(&e))),
        }
    }

    fn execute_music(&mut self, command: MusicCommand) {
        match command {
            MusicCommand::Start { track, gain, looping } => self.start_track(&track, gain, looping),
            MusicCommand::SetGain { track, gain } => {
                if let Some(node) = self.music.get(&track) {
                    set_gain_node(self.ctx.as_ref(), Some(&node.gain), gain);
                }
            }
            MusicCommand::Stop { track } => {
                if let Some(node) = self.music.get_mut(&track) {
                    node.wants_play = false;
                    node.paused = false;
                    let _ = node.element.pause();
                    node.element.set_current_time(0.0);
                }
            }
            MusicCommand::Pause { track } => {
                if let Some(node) = self.music.get_mut(&track) {
                    node.paused = true;
                    let _ = node.element.pause();
                }
            }
            MusicCommand::Resume { track } => {
                if let Some(node) = self.music.get_mut(&track) {
                    node.paused = false;
                    if node.wants_play {
                        play_element(node, &self.shared, &track);
                    }
                }
            }
        }
    }

    fn start_track(&mut self, track: &str, gain: f32, looping: bool) {
        let (Some(ctx), Some(bus)) = (self.ctx.clone(), self.music_bus.clone()) else { return };
        if !self.music.contains_key(track) {
            let Some(path) = self.music_paths.get(track).cloned() else { return };
            match create_music_node(&ctx, &bus, &path, track, &self.shared) {
                Ok(node) => {
                    self.music.insert(track.to_string(), node);
                }
                Err(e) => {
                    self.shared.warn_once(&format!("music:{track}"), &format!("cannot play music '{track}': {}", js_err(&e)));
                    return;
                }
            }
        }
        if let Some(node) = self.music.get_mut(track) {
            node.element.set_loop(looping);
            node.element.set_current_time(0.0);
            node.gain.gain().set_value(gain);
            node.wants_play = true;
            node.paused = false;
            play_element(node, &self.shared, track);
        }
    }

    // ---- per frame

    /// Housekeeping: resume the context after a gesture, start loops whose assets arrived, retry
    /// blocked music.
    fn tick(&mut self) {
        let gesture = self.shared.gesture.replace(false);
        if gesture {
            if let Some(ctx) = &self.ctx {
                if ctx.state() == AudioContextState::Suspended {
                    if let Ok(promise) = ctx.resume() {
                        spawn_local(async move {
                            let _ = JsFuture::from(promise).await;
                        });
                    }
                }
            }
            if !self.prefetched_all {
                self.prefetched_all = true;
                let names: Vec<String> = self.sound_paths.keys().cloned().collect();
                for name in names {
                    self.ensure_loaded(&name);
                }
            }
            let shared = self.shared.clone();
            for (track, node) in self.music.iter_mut() {
                if node.blocked.get() && node.wants_play && !node.paused {
                    play_element(node, &shared, track);
                }
            }
        }

        let pending: Vec<LoopHandle> = self.loops.iter().filter(|(_, n)| n.playing.is_none()).map(|(h, _)| *h).collect();
        for handle in pending {
            if let Some(mut node) = self.loops.remove(&handle) {
                self.try_start_loop(&mut node);
                self.loops.insert(handle, node);
            }
        }
    }
}

fn stop_loop_node(node: LoopNode) {
    if let Some(playing) = node.playing {
        let _ = AudioScheduledSourceNode::stop(&playing.source);
        let _ = playing.source.disconnect();
        let _ = playing.gain.disconnect();
        if let Some(panner) = playing.panner {
            let _ = panner.disconnect();
        }
    }
}

fn set_gain_node(ctx: Option<&AudioContext>, node: Option<&GainNode>, value: f32) {
    let Some(node) = node else { return };
    let value = value.max(0.0);
    match ctx {
        Some(ctx) => {
            if node.gain().set_target_at_time(value, ctx.current_time(), GAIN_SMOOTHING).is_err() {
                node.gain().set_value(value);
            }
        }
        None => node.gain().set_value(value),
    }
}

/// `source -> gain -> panner -> destination_node`, started immediately.
fn build_chain(
    ctx: &AudioContext,
    buffer: &AudioBuffer,
    volume: f32,
    pan: f32,
    looping: bool,
    pitch: f32,
    destination: &GainNode,
) -> Result<Playing, JsValue> {
    let source = ctx.create_buffer_source()?;
    source.set_buffer(Some(buffer));
    source.set_loop(looping);
    source.playback_rate().set_value(pitch);
    let gain = ctx.create_gain()?;
    gain.gain().set_value(volume.max(0.0));
    source.connect_with_audio_node(&gain)?;
    // Stereo panning is optional: without it the sound is simply centred.
    let panner = ctx.create_stereo_panner().ok();
    match &panner {
        Some(panner) => {
            panner.pan().set_value(pan.clamp(-1.0, 1.0));
            gain.connect_with_audio_node(panner)?;
            panner.connect_with_audio_node(destination)?;
        }
        None => {
            gain.connect_with_audio_node(destination)?;
        }
    }
    source.start()?;
    Ok(Playing { source, gain, panner })
}

fn create_music_node(ctx: &AudioContext, bus: &GainNode, path: &str, track: &str, shared: &Rc<Shared>) -> Result<MusicNode, JsValue> {
    let element = HtmlAudioElement::new_with_src(path)?;
    element.set_preload("auto");
    let source = ctx.create_media_element_source(&element)?;
    let gain = ctx.create_gain()?;
    gain.gain().set_value(0.0);
    source.connect_with_audio_node(&gain)?;
    gain.connect_with_audio_node(bus)?;

    // Tell the logic when a non-looping track finishes so a playlist can move on.
    let ended = {
        let shared = shared.clone();
        let track = track.to_string();
        Closure::<dyn FnMut()>::new(move || shared.ended_tracks.borrow_mut().push(track.clone()))
    };
    element.set_onended(Some(ended.as_ref().unchecked_ref()));
    ended.forget();

    // A failing file (404, undecodable) is reported once.
    let failed = {
        let shared = shared.clone();
        let track = track.to_string();
        let path = path.to_string();
        Closure::<dyn FnMut()>::new(move || shared.warn_once(&format!("music-file:{track}"), &format!("could not load music '{track}' from {path}")))
    };
    element.set_onerror(Some(failed.as_ref().unchecked_ref()));
    failed.forget();

    Ok(MusicNode { element, gain, _source: source, wants_play: false, paused: false, blocked: Rc::new(Cell::new(false)) })
}

/// `play()` returns a promise that is rejected while the autoplay policy blocks it.
fn play_element(node: &mut MusicNode, shared: &Rc<Shared>, track: &str) {
    node.blocked.set(false);
    match node.element.play() {
        Ok(promise) => {
            let blocked = node.blocked.clone();
            let shared = shared.clone();
            let track = track.to_string();
            spawn_local(async move {
                if let Err(e) = JsFuture::from(promise).await {
                    // Most likely NotAllowedError: wait for a user gesture and retry.
                    blocked.set(true);
                    shared.warn_once(&format!("music-blocked:{track}"), &format!("music '{track}' waits for a user gesture ({})", js_err(&e)));
                }
            });
        }
        Err(e) => {
            node.blocked.set(true);
            shared.warn_once(&format!("music-play:{track}"), &format!("cannot start music '{track}': {}", js_err(&e)));
        }
    }
}

async fn fetch_and_decode(ctx: &AudioContext, url: &str) -> Result<AudioBuffer, String> {
    let window = web_sys::window().ok_or("no window")?;
    let response: Response =
        JsFuture::from(window.fetch_with_str(url)).await.map_err(|e| js_err(&e))?.dyn_into().map_err(|_| "not a response")?;
    if !response.ok() {
        return Err(format!("HTTP {}", response.status()));
    }
    let bytes: ArrayBuffer = JsFuture::from(response.array_buffer().map_err(|e| js_err(&e))?)
        .await
        .map_err(|e| js_err(&e))?
        .dyn_into()
        .map_err(|_| "not an array buffer")?;
    let decoded = JsFuture::from(ctx.decode_audio_data(&bytes).map_err(|e| js_err(&e))?).await.map_err(|e| js_err(&e))?;
    decoded.dyn_into::<AudioBuffer>().map_err(|_| "decoding did not produce an AudioBuffer".to_string())
}

fn js_err(error: &JsValue) -> String {
    error
        .as_string()
        .or_else(|| Reflect::get(error, &"message".into()).ok().and_then(|m| m.as_string()))
        .unwrap_or_else(|| format!("{error:?}"))
}

/// Read the audio settings from a JS object (`window.wurfelSettings` or an event's `detail`).
fn read_settings(object: &JsValue) -> Option<AudioSettings> {
    if !object.is_object() {
        return None;
    }
    Some(AudioSettings::from_lookup(&|key| {
        let value = Reflect::get(object, &JsValue::from_str(key)).ok()?;
        match (value.as_f64(), value.as_bool()) {
            (Some(n), _) => Some(Setting::Number(n)),
            (_, Some(b)) => Some(Setting::Bool(b)),
            _ => None,
        }
    }))
}

fn install_listeners(shared: &Rc<Shared>) {
    let Some(window) = web_sys::window() else { return };
    for name in GESTURE_EVENTS {
        let shared = shared.clone();
        let closure = Closure::<dyn FnMut(web_sys::Event)>::new(move |_| shared.gesture.set(true));
        let _ = window.add_event_listener_with_callback(name, closure.as_ref().unchecked_ref());
        closure.forget();
    }

    let settings_shared = shared.clone();
    let on_settings = Closure::<dyn FnMut(web_sys::Event)>::new(move |event: web_sys::Event| {
        if let Some(custom) = event.dyn_ref::<CustomEvent>() {
            if let Some(settings) = read_settings(&custom.detail()) {
                *settings_shared.pending_settings.borrow_mut() = Some(settings);
            }
        }
    });
    let _ = window.add_event_listener_with_callback("wurfel:settings", on_settings.as_ref().unchecked_ref());
    on_settings.forget();

    // `window.dispatchEvent(new CustomEvent('wurfel:sound', { detail: { name: 'menuSelect', volume: 1 } }))`
    // lets page scripts such as the menu play a registered sound.
    let sound_shared = shared.clone();
    let on_sound = Closure::<dyn FnMut(web_sys::Event)>::new(move |event: web_sys::Event| {
        let Some(custom) = event.dyn_ref::<CustomEvent>() else { return };
        let detail = custom.detail();
        let name = Reflect::get(&detail, &"name".into()).ok().and_then(|n| n.as_string());
        let volume = Reflect::get(&detail, &"volume".into()).ok().and_then(|v| v.as_f64()).unwrap_or(1.0);
        if let (Some(name), true) = (name, volume.is_finite()) {
            let mut pending = sound_shared.pending_sounds.borrow_mut();
            if pending.len() < 32 {
                pending.push((name, volume.clamp(0.0, 1.0) as f32));
            }
        }
    });
    let _ = window.add_event_listener_with_callback("wurfel:sound", on_sound.as_ref().unchecked_ref());
    on_sound.forget();

    // Settings that were already there when we started.
    if let Ok(initial) = Reflect::get(&window, &"wurfelSettings".into()) {
        if let Some(settings) = read_settings(&initial) {
            *shared.pending_settings.borrow_mut() = Some(settings);
        }
    }
}

// ----------------------------------------------------------------------------------- the facade

/// What the audio system is doing, for a debug overlay and for tests.
#[allow(dead_code)] // no debug overlay shows it yet
#[derive(Debug, Clone, PartialEq)]
pub struct AudioStatus {
    /// A WebAudio context could be created.
    pub available: bool,
    /// `suspended` until the first user gesture, then `running`.
    pub context_state: String,
    pub sounds_loaded: usize,
    pub sounds_loading: usize,
    pub sounds_failed: Vec<String>,
    pub loops_playing: usize,
    pub music_playing: Vec<String>,
    /// Current gain of the effects and music buses (master volume and category volume).
    pub effects_gain: f32,
    pub music_gain: f32,
    /// Largest absolute sample at the output over the last ~45 ms, 0..1. Above 0 means sound is
    /// actually coming out.
    pub output_peak: f32,
}

/// Sound and music for the browser client. See the module docs of `audio` for how to call it.
pub struct Audio {
    logic: AudioLogic,
    backend: Backend,
    shared: Rc<Shared>,
}

impl Audio {
    /// Create the audio system. Never fails: without WebAudio every call is a no-op.
    pub fn new() -> Self {
        let shared = Rc::new(Shared::default());
        install_listeners(&shared);
        let seed = (js_sys::Date::now() as u64 & 0xFFFF_FFFF) as u32;
        let logic = AudioLogic::with_seed(seed);
        let backend = Backend::new(&logic, shared.clone());
        Audio { logic, backend, shared }
    }

    /// The platform-independent part, for settings, the bank, per-entity sound configs...
    pub fn logic_mut(&mut self) -> &mut AudioLogic {
        &mut self.logic
    }

    /// Where the listener is: the player or the camera, `[gx, gy, z]` in blocks.
    pub fn set_listener(&mut self, position: Position) {
        self.logic.set_listener(position);
    }

    /// Play a registered sound once, optionally at a position.
    pub fn play(&mut self, name: &str, position: Option<Position>) {
        self.logic.play(name, position, 1.0);
    }

    /// React to the events of `Entities::update`.
    pub fn handle_events(&mut self, events: &[Event], info: &dyn Fn(EntityId) -> Option<EntityInfo>) {
        self.logic.handle_events(events, info);
    }

    /// Per-frame sounds of entities (footsteps, falling wind). Call once per rendered frame.
    pub fn update_entities(&mut self, dt: f32, entities: &[(EntityId, EntityInfo)]) {
        self.logic.update_entities(dt, entities);
    }

    pub fn on_jump(&mut self, id: EntityId, position: Position) {
        self.logic.on_jump(id, position);
    }

    #[allow(dead_code)] // music is ported but the game loop never starts it yet
    /// Start the music (title first). Works before a user gesture: it begins at the first one.
    pub fn start_music(&mut self) {
        self.logic.start_music();
    }

    pub fn play_music(&mut self, track: &str) -> bool {
        self.logic.play_music(track)
    }

    #[allow(dead_code)] // music is ported but the game loop never starts it yet
    pub fn stop_music(&mut self) {
        self.logic.stop_music();
    }

    /// Call once per frame with the frame time in seconds: applies new settings, moves positional
    /// loops, advances fades and executes everything the logic produced.
    pub fn update(&mut self, dt: f32) {
        if let Some(settings) = self.shared.pending_settings.borrow_mut().take() {
            self.logic.set_settings(settings);
        }
        let requested: Vec<(String, f32)> = self.shared.pending_sounds.borrow_mut().drain(..).collect();
        for (name, volume) in requested {
            self.logic.play(&name, None, volume);
        }
        let ended: Vec<String> = self.shared.ended_tracks.borrow_mut().drain(..).collect();
        for track in ended {
            self.logic.music_ended(&track);
        }
        self.logic.update(dt);
        for command in self.logic.take_commands() {
            self.backend.execute(command);
        }
        self.backend.tick();
    }

    /// Snapshot of the audio state. Reads the output level, so call it for a HUD or a test, not
    /// every frame.
    #[allow(dead_code)] // no debug overlay shows it yet
    pub fn status(&self) -> AudioStatus {
        let buffers = self.shared.buffers.borrow();
        let mut sounds_failed: Vec<String> =
            buffers.iter().filter(|(_, l)| matches!(l, Load::Failed)).map(|(n, _)| n.clone()).collect();
        sounds_failed.sort();
        let output_peak = self.backend.analyser.as_ref().map_or(0.0, |analyser| {
            let mut samples = vec![0.0f32; analyser.fft_size() as usize];
            analyser.get_float_time_domain_data(&mut samples);
            samples.iter().fold(0.0f32, |peak, s| peak.max(s.abs()))
        });
        let settings = self.logic.settings();
        AudioStatus {
            available: self.backend.is_available(),
            context_state: self.backend.ctx.as_ref().map_or_else(|| "unavailable".to_string(), |c| format!("{:?}", c.state()).to_lowercase()),
            sounds_loaded: buffers.values().filter(|l| matches!(l, Load::Ready(_))).count(),
            sounds_loading: buffers.values().filter(|l| matches!(l, Load::Loading)).count(),
            sounds_failed,
            loops_playing: self.backend.loops.values().filter(|l| l.playing.is_some()).count(),
            music_playing: {
                let mut playing: Vec<String> =
                    self.backend.music.iter().filter(|(_, n)| !n.element.paused()).map(|(t, _)| t.clone()).collect();
                playing.sort();
                playing
            },
            effects_gain: settings.effects_gain(),
            music_gain: settings.music_gain(),
            output_peak,
        }
    }
}
