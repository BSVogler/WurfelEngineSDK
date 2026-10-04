//! Sound and music, ported from the Java `SoundEngine` (`WE.SOUND`).
//!
//! The module has two layers:
//!
//! * [`AudioLogic`] (platform independent, unit tested): the sound bank, the Java volume and pan
//!   formulas, the mapping from simulation [`Event`]s to sounds, and the music state machine. It
//!   never plays anything; it produces [`SoundCommand`]s.
//! * `Audio` (browser only, in `web.rs`): wraps an `AudioLogic`, executes its commands with
//!   WebAudio, loads the assets and handles the autoplay policy.
//!
//! # Calling it from the game loop
//!
//! ```ignore
//! let mut audio = Audio::new();                         // once; never fails, audio may be inert
//! // every frame:
//! audio.set_listener(camera_or_player_position);        // [gx, gy, z] in blocks
//! audio.handle_events(&events, &|id| info_for(id));     // events from Entities::update
//! audio.update_entities(dt, &[(id, EntityInfo::from_entity(entity, &world))]);
//! audio.update(dt);                                     // positional loops, fades, executes commands
//! // when the player jumps: audio.on_jump(position); one-off sounds: audio.play("collect", None)
//! // music: audio.start_music() after the player pressed Play; audio.play_music("overworld")
//! ```
//!
//! # Settings read from the page
//!
//! `window.wurfelSettings` and the `detail` of the `wurfel:settings` event (both optional). Volumes
//! are numbers; values above 1 are taken as percentages (0..100). The first key present wins:
//!
//! | meaning | keys |
//! |---|---|
//! | master volume | `masterVolume`, `master`, `volumeMaster`, `volume` |
//! | music volume | `musicVolume`, `music` |
//! | effects volume | `effectsVolume`, `sfxVolume`, `soundVolume`, `effects`, `sfx`, `sound` |
//! | mute (boolean) | `muted`, `mute` |
//!
//! # Playing sounds from page scripts
//!
//! `window.dispatchEvent(new CustomEvent('wurfel:sound', { detail: { name: 'menuSelect', volume: 1 } }))`
//! plays a registered sound without positioning (for the menu's click sounds).
//!
//! The Java engine had two volumes (the CVars `sound` and `music`, both default 1). Master and mute
//! are additions for the browser; the effective gains are `master * music` and `master * effects`.

// The public API is called from web.rs by the game loop; until then it is only used by the tests.
#![allow(dead_code)]

pub mod entity_sounds;
pub mod music;

#[cfg(target_arch = "wasm32")]
mod web;
#[cfg(target_arch = "wasm32")]
#[allow(unused_imports)] // until web.rs creates one
pub use web::{Audio, AudioStatus};

use std::collections::HashMap;

pub use entity_sounds::{EntityInfo, EntitySoundConfig};
#[allow(unused_imports)]
pub use music::{Music, MusicCommand, PlaybackMode};
use wurfel_sim::entity::EntityId;

/// A position in the isometric ground frame: `[gx, gy, z]` in blocks (see
/// `wurfel_sim::entity::physics`).
pub type Position = [f32; 3];

// --------------------------------------------------------------------------------------- settings

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AudioSettings {
    pub master: f32,
    pub music: f32,
    pub effects: f32,
    pub muted: bool,
}

impl Default for AudioSettings {
    /// The Java defaults: everything at full volume.
    fn default() -> Self {
        AudioSettings { master: 1.0, music: 1.0, effects: 1.0, muted: false }
    }
}

/// A value found on the page's settings object.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Setting {
    Number(f64),
    Bool(bool),
}

pub const MASTER_KEYS: [&str; 4] = ["masterVolume", "master", "volumeMaster", "volume"];
pub const MUSIC_KEYS: [&str; 2] = ["musicVolume", "music"];
pub const EFFECTS_KEYS: [&str; 6] = ["effectsVolume", "sfxVolume", "soundVolume", "effects", "sfx", "sound"];
pub const MUTE_KEYS: [&str; 2] = ["muted", "mute"];

/// Volumes up to 1 are fractions, larger ones percentages. Anything that is not a finite number
/// is rejected so a malformed setting cannot silence or blast the game.
pub fn normalize_volume(value: f64) -> Option<f32> {
    if !value.is_finite() {
        return None;
    }
    let fraction = if value > 1.0 { value / 100.0 } else { value };
    Some(fraction.clamp(0.0, 1.0) as f32)
}

impl AudioSettings {
    /// Effective gain of the effects bus.
    pub fn effects_gain(&self) -> f32 {
        if self.muted { 0.0 } else { self.master * self.effects }
    }

    /// Effective gain of the music bus.
    pub fn music_gain(&self) -> f32 {
        if self.muted { 0.0 } else { self.master * self.music }
    }

    /// Read settings through `lookup`, which returns the value of a key on the page's settings
    /// object. Missing or malformed entries keep their default.
    pub fn from_lookup(lookup: &dyn Fn(&str) -> Option<Setting>) -> Self {
        let number = |keys: &[&str]| {
            keys.iter().find_map(|k| match lookup(k) {
                Some(Setting::Number(n)) => normalize_volume(n),
                _ => None,
            })
        };
        let defaults = AudioSettings::default();
        AudioSettings {
            master: number(&MASTER_KEYS).unwrap_or(defaults.master),
            music: number(&MUSIC_KEYS).unwrap_or(defaults.music),
            effects: number(&EFFECTS_KEYS).unwrap_or(defaults.effects),
            muted: MUTE_KEYS
                .iter()
                .find_map(|k| match lookup(k) {
                    Some(Setting::Bool(b)) => Some(b),
                    _ => None,
                })
                .unwrap_or(defaults.muted),
        }
    }
}

// ------------------------------------------------------------------------------------ sound bank

pub const ASSET_ROOT: &str = "assets/audio/sounds/";

/// Sound name to asset path (the Java `register(name, path)`). The first registration of a name
/// wins, as in Java.
#[derive(Debug, Clone, Default)]
pub struct SoundBank {
    sounds: HashMap<String, String>,
}

impl SoundBank {
    pub fn new() -> Self {
        SoundBank::default()
    }

    /// The sounds the Java engine and Caveland register, under the same names.
    pub fn with_defaults() -> Self {
        let mut bank = SoundBank::new();
        for (name, file) in DEFAULT_SOUNDS {
            bank.register(name, &format!("{ASSET_ROOT}{file}"));
        }
        bank
    }

    /// Returns false (and keeps the existing entry) if the name is taken.
    pub fn register(&mut self, name: &str, path: &str) -> bool {
        if self.sounds.contains_key(name) {
            return false;
        }
        self.sounds.insert(name.to_string(), path.to_string());
        true
    }

    pub fn path(&self, name: &str) -> Option<&str> {
        self.sounds.get(name).map(String::as_str)
    }

    pub fn contains(&self, name: &str) -> bool {
        self.sounds.contains_key(name)
    }

    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.sounds.keys().map(String::as_str)
    }

    pub fn len(&self) -> usize {
        self.sounds.len()
    }

    pub fn is_empty(&self) -> bool {
        self.sounds.is_empty()
    }
}

/// `(name, file)` pairs from `SoundEngine.loadRegisterIGSounds`, `Caveland.java` and `CLGameView.java`.
pub const DEFAULT_SOUNDS: [(&str, &str); 38] = [
    // engine
    ("landing", "landing.wav"),
    ("splash", "splash.wav"),
    ("wind", "wind.ogg"),
    ("explosion", "explosion2.wav"),
    // main menu
    ("menuSelect", "menusound.wav"),
    ("menuAbort", "menusoundAbort.wav"),
    ("menuConfirm", "bong.wav"),
    // Caveland
    ("turret", "turret.ogg"),
    ("jetpack", "jetpack.wav"),
    ("step", "step.wav"),
    ("urfJump", "urf_jump.wav"),
    ("urfHurt", "urfHurt.wav"),
    ("loadAttack", "loadAttack.wav"),
    ("ha", "ha.wav"),
    ("release", "release.wav"),
    ("impact", "impact.wav"),
    ("robot1destroy", "robot1destroy.wav"),
    ("robot1Wobble", "robot1Wobble.mp3"),
    ("robotHit", "robotHit.wav"),
    ("blockDestroy", "poch.wav"),
    ("vanya_jump", "vanya_jump.wav"),
    ("wagon", "wagon.mp3"),
    ("collect", "collect.wav"),
    ("sword", "sword.wav"),
    ("hiss", "hiss.wav"),
    ("treehit", "treehit.wav"),
    ("metallic", "metallic.wav"),
    ("construct", "construct.wav"),
    ("huhu", "huhu.wav"),
    ("interactionFail", "throwFail.wav"),
    ("droneLoop", "droneLoop.mp3"),
    ("robot2walk", "robot2walk.mp3"),
    ("robotScream", "robotScream.mp3"),
    ("robotWeep", "robotWeep.wav"),
    ("craft", "craft.wav"),
    ("moneyPickup", "moneyPickup.wav"),
    ("merchantAha", "merchantAha.mp3"),
    ("merchantWelcome", "merchantWelcome.mp3"),
];

// ----------------------------------------------------------------------------------- positional

/// CVar `soundDecay`.
pub const SOUND_DECAY: f32 = 6000.0;
/// Java game units per block edge (`GAME_EDGELENGTH`).
pub const GAME_UNITS_PER_BLOCK: f32 = 141.421_36;
/// One-off positional sounds quieter than this are not played at all (`SoundEngine.play`).
pub const PLAY_THRESHOLD: f32 = 0.1;
/// Screen pixels from the listener at which a sound is panned fully to one side.
pub const PAN_RANGE_PX: f32 = 500.0;
/// Screen pixels per block of `gx - gy` (a block is 200 px wide in the Java engine).
const PX_PER_ISO_UNIT: f32 = 100.0;

/// Volume factor for a sound `distance` blocks (horizontally) from the listener: the energy
/// spreads radially, `decay * edge / (d^2 + decay * edge)` in game units. It is 1 at the listener,
/// 0.5 at about 6.5 blocks and falls under [`PLAY_THRESHOLD`] at about 19.5 blocks.
pub fn attenuation(distance: f32) -> f32 {
    let d = distance * GAME_UNITS_PER_BLOCK;
    let k = SOUND_DECAY * GAME_UNITS_PER_BLOCK;
    (k / (d * d + k)).min(1.0)
}

/// Horizontal distance between two positions in blocks (`distanceToHorizontal`).
pub fn horizontal_distance(a: Position, b: Position) -> f32 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

/// Stereo pan, -1 (left) to 1 (right): how far the source is to the side of the listener *on the
/// screen*, which in this frame depends on `gx - gy` only, over 500 px, clamped.
pub fn pan(source: Position, listener: Position) -> f32 {
    let screen_dx = ((source[0] - source[1]) - (listener[0] - listener[1])) * PX_PER_ISO_UNIT;
    (screen_dx / PAN_RANGE_PX).clamp(-1.0, 1.0)
}

// ------------------------------------------------------------------------------------- commands

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct LoopHandle(pub u32);

/// Everything the browser backend has to do. Volumes are relative to the bus: master, music and
/// effects volume are applied by [`SoundCommand::SetBuses`], not repeated in every command.
#[derive(Debug, Clone, PartialEq)]
pub enum SoundCommand {
    /// A one-off sound.
    Play { name: String, volume: f32, pitch: f32, pan: f32 },
    StartLoop { handle: LoopHandle, name: String, volume: f32, pan: f32 },
    SetLoop { handle: LoopHandle, volume: f32, pan: f32 },
    StopLoop { handle: LoopHandle },
    /// Stop every effect (not the music), `stopEverySound`.
    StopAll,
    Music(MusicCommand),
    /// Set the gain of the effects and music buses.
    SetBuses { effects: f32, music: f32 },
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct LoopInfo {
    pub position: Option<Position>,
    /// The volume asked for, before distance attenuation.
    pub volume: f32,
    /// What was last sent to the backend, to avoid sending the same values every frame.
    pub last_sent: (f32, f32),
}

// ---------------------------------------------------------------------------------- the engine

pub struct AudioLogic {
    pub(crate) bank: SoundBank,
    pub(crate) listener: Position,
    pub(crate) settings: AudioSettings,
    pub(crate) commands: Vec<SoundCommand>,
    pub(crate) next_loop: u32,
    pub(crate) loops: HashMap<LoopHandle, LoopInfo>,
    pub(crate) entities: HashMap<EntityId, entity_sounds::EntityState>,
    pub(crate) entity_configs: HashMap<EntityId, EntitySoundConfig>,
    pub(crate) entity_default: EntitySoundConfig,
    pub(crate) music: Music,
    rng: u32,
}

impl AudioLogic {
    pub fn new() -> Self {
        Self::with_seed(0x9E37_79B9)
    }

    /// `seed` drives the random pitch and pan of footsteps.
    pub fn with_seed(seed: u32) -> Self {
        let mut logic = AudioLogic {
            bank: SoundBank::with_defaults(),
            listener: [0.0; 3],
            settings: AudioSettings::default(),
            commands: Vec::new(),
            next_loop: 1,
            loops: HashMap::new(),
            entities: HashMap::new(),
            entity_configs: HashMap::new(),
            entity_default: EntitySoundConfig::player(),
            music: Music::with_default_tracks(),
            rng: seed.max(1),
        };
        logic.push_buses();
        logic
    }

    pub fn bank(&self) -> &SoundBank {
        &self.bank
    }

    pub fn bank_mut(&mut self) -> &mut SoundBank {
        &mut self.bank
    }

    pub fn music(&self) -> &Music {
        &self.music
    }

    pub fn settings(&self) -> AudioSettings {
        self.settings
    }

    pub fn set_listener(&mut self, position: Position) {
        self.listener = position;
    }

    /// Apply new settings. Emits the bus gains when they changed, and pauses the music while its
    /// volume is 0 (as the Java engine does).
    pub fn set_settings(&mut self, settings: AudioSettings) {
        if settings == self.settings {
            return;
        }
        self.settings = settings;
        self.push_buses();
        self.music.set_audible(settings.music_gain() > 0.0);
    }

    fn push_buses(&mut self) {
        self.commands.push(SoundCommand::SetBuses {
            effects: self.settings.effects_gain(),
            music: self.settings.music_gain(),
        });
    }

    /// A uniformly distributed number in `[0, 1)` (xorshift32).
    pub(crate) fn random(&mut self) -> f32 {
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.rng = x;
        (x >> 8) as f32 / (1u32 << 24) as f32
    }

    // ---- one-off sounds

    /// Play a registered sound once (`SoundEngine.play(name, pos, volume)`). With a position the
    /// volume falls off with the distance to the listener and the sound is panned; a positional
    /// sound that would be quieter than [`PLAY_THRESHOLD`] is not played. Returns whether a
    /// command was produced; unknown names are silently ignored, like in Java.
    pub fn play(&mut self, name: &str, position: Option<Position>, volume: f32) -> bool {
        if !self.bank.contains(name) {
            return false;
        }
        let (volume, pan_value) = match position {
            Some(p) => (volume * attenuation(horizontal_distance(p, self.listener)), pan(p, self.listener)),
            None => (volume, 0.0),
        };
        if position.is_some() && volume < PLAY_THRESHOLD {
            return false;
        }
        self.commands.push(SoundCommand::Play { name: name.to_string(), volume, pitch: 1.0, pan: pan_value });
        true
    }

    /// Play with explicit volume, pitch and pan, no positioning (`play(name, volume, pitch, pan)`).
    pub fn play_with(&mut self, name: &str, volume: f32, pitch: f32, pan: f32) -> bool {
        if !self.bank.contains(name) {
            return false;
        }
        self.commands.push(SoundCommand::Play { name: name.to_string(), volume, pitch, pan: pan.clamp(-1.0, 1.0) });
        true
    }

    // ---- loops

    /// Start a looping sound (`SoundEngine.loop`). A positional loop follows the listener: its
    /// volume and pan are updated every [`AudioLogic::update`].
    pub fn start_loop(&mut self, name: &str, position: Option<Position>, volume: f32) -> Option<LoopHandle> {
        if !self.bank.contains(name) {
            return None;
        }
        let handle = LoopHandle(self.next_loop);
        self.next_loop += 1;
        let (vol, pan_value) = self.loop_levels(position, volume);
        self.loops.insert(handle, LoopInfo { position, volume, last_sent: (vol, pan_value) });
        self.commands.push(SoundCommand::StartLoop { handle, name: name.to_string(), volume: vol, pan: pan_value });
        Some(handle)
    }

    fn loop_levels(&self, position: Option<Position>, volume: f32) -> (f32, f32) {
        match position {
            Some(p) => (volume * attenuation(horizontal_distance(p, self.listener)), pan(p, self.listener)),
            None => (volume, 0.0),
        }
    }

    /// Change the volume of a running loop (`setVolume`).
    pub fn set_loop_volume(&mut self, handle: LoopHandle, volume: f32) {
        let Some(info) = self.loops.get(&handle).copied() else { return };
        let (vol, pan_value) = self.loop_levels(info.position, volume);
        if let Some(info) = self.loops.get_mut(&handle) {
            info.volume = volume;
            info.last_sent = (vol, pan_value);
        }
        self.commands.push(SoundCommand::SetLoop { handle, volume: vol, pan: pan_value });
    }

    /// Move a positional loop, e.g. a robot that walks while its sound plays.
    pub fn set_loop_position(&mut self, handle: LoopHandle, position: Position) {
        if let Some(info) = self.loops.get_mut(&handle) {
            info.position = Some(position);
        }
    }

    pub fn stop_loop(&mut self, handle: LoopHandle) {
        if self.loops.remove(&handle).is_some() {
            self.commands.push(SoundCommand::StopLoop { handle });
        }
    }

    pub fn stop_all(&mut self) {
        self.loops.clear();
        self.entities.clear();
        self.commands.push(SoundCommand::StopAll);
    }

    // ---- music

    /// Start the playlist (the title music, then the overworld).
    pub fn start_music(&mut self) {
        self.music.start();
    }

    /// Crossfade to a track by name (`title` or `overworld`).
    pub fn play_music(&mut self, track: &str) -> bool {
        self.music.play(track)
    }

    pub fn stop_music(&mut self) {
        self.music.stop();
    }

    pub fn music_ended(&mut self, track: &str) {
        self.music.track_ended(track);
    }

    // ---- per frame

    /// Advance loops and fades by `dt` seconds.
    pub fn update(&mut self, dt: f32) {
        // Positional loops follow the listener (`SoundInstance.update`).
        let handles: Vec<LoopHandle> = self.loops.keys().copied().collect();
        for handle in handles {
            let info = self.loops[&handle];
            if info.position.is_none() {
                continue;
            }
            let (volume, pan_value) = self.loop_levels(info.position, info.volume);
            let (last_volume, last_pan) = info.last_sent;
            if (volume - last_volume).abs() > 0.005 || (pan_value - last_pan).abs() > 0.005 {
                self.loops.get_mut(&handle).expect("just read").last_sent = (volume, pan_value);
                self.commands.push(SoundCommand::SetLoop { handle, volume, pan: pan_value });
            }
        }

        self.music.update(dt);
    }

    /// The commands produced since the last call, in order, including the music commands.
    pub fn take_commands(&mut self) -> Vec<SoundCommand> {
        let mut commands = std::mem::take(&mut self.commands);
        commands.extend(self.music.take_commands().into_iter().map(SoundCommand::Music));
        commands
    }
}

impl Default for AudioLogic {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests;
