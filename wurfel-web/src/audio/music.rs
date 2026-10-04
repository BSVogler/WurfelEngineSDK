//! Music: one track at a time with equal-power crossfades, and an optional playlist.
//!
//! The Java engine (`SoundEngine.setMusic`) plays a single looping track and pauses it while the
//! music volume is 0. This keeps that behaviour and adds crossfading between tracks (for example
//! from the title music to the overworld music) and a playlist mode.
//!
//! The state machine only decides *what should be playing at which gain*; the browser glue turns
//! [`MusicCommand`]s into audio. Gains here are per track (0..1); the master and music volumes are
//! applied once on the music bus.

use std::collections::HashMap;

/// Default length of a crossfade in seconds.
pub const DEFAULT_FADE_SECONDS: f32 = 2.0;

#[derive(Debug, Clone, PartialEq)]
pub enum MusicCommand {
    /// Begin playing a track from the start. `looping` tracks repeat until stopped.
    Start { track: String, gain: f32, looping: bool },
    SetGain { track: String, gain: f32 },
    /// Stop and rewind.
    Stop { track: String },
    /// Keep the position, e.g. while the music volume is 0.
    Pause { track: String },
    Resume { track: String },
}

/// How tracks follow each other.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaybackMode {
    /// The current track repeats forever (the Java behaviour).
    LoopOne,
    /// When a track ends, crossfade into the next one in the playlist, wrapping around.
    Sequence,
}

#[derive(Debug)]
struct Fade {
    /// 0 at the start of the fade, 1 when finished.
    progress: f32,
}

pub struct Music {
    /// Track name to asset path.
    tracks: HashMap<String, String>,
    playlist: Vec<String>,
    mode: PlaybackMode,
    fade_seconds: f32,
    current: Option<String>,
    /// Fade-in progress of `current`.
    fade_in: Fade,
    /// Tracks that are fading out, with their progress.
    outgoing: Vec<(String, Fade)>,
    audible: bool,
    commands: Vec<MusicCommand>,
}

/// Equal-power fade curves: `gain_in² + gain_out² == 1` throughout, so loudness stays constant.
pub fn fade_in_gain(progress: f32) -> f32 {
    (progress.clamp(0.0, 1.0) * std::f32::consts::FRAC_PI_2).sin()
}

pub fn fade_out_gain(progress: f32) -> f32 {
    (progress.clamp(0.0, 1.0) * std::f32::consts::FRAC_PI_2).cos()
}

impl Music {
    pub fn new() -> Self {
        Music {
            tracks: HashMap::new(),
            playlist: Vec::new(),
            mode: PlaybackMode::LoopOne,
            fade_seconds: DEFAULT_FADE_SECONDS,
            current: None,
            fade_in: Fade { progress: 1.0 },
            outgoing: Vec::new(),
            audible: true,
            commands: Vec::new(),
        }
    }

    /// The tracks that ship with the game: the title music and the overworld music.
    pub fn with_default_tracks() -> Self {
        let mut music = Music::new();
        music.register("title", "assets/audio/music/title.m4a");
        music.register("overworld", "assets/audio/music/overworld.m4a");
        music.set_playlist(vec!["title".into(), "overworld".into()]);
        music
    }

    pub fn register(&mut self, name: &str, path: &str) {
        self.tracks.entry(name.to_string()).or_insert_with(|| path.to_string());
    }

    pub fn path(&self, name: &str) -> Option<&str> {
        self.tracks.get(name).map(String::as_str)
    }

    pub fn track_names(&self) -> impl Iterator<Item = &str> {
        self.tracks.keys().map(String::as_str)
    }

    pub fn set_playlist(&mut self, playlist: Vec<String>) {
        self.playlist = playlist;
    }

    pub fn set_mode(&mut self, mode: PlaybackMode) {
        self.mode = mode;
    }

    pub fn mode(&self) -> PlaybackMode {
        self.mode
    }

    pub fn set_fade_seconds(&mut self, seconds: f32) {
        self.fade_seconds = seconds.max(0.0);
    }

    pub fn current(&self) -> Option<&str> {
        self.current.as_deref()
    }

    pub fn is_fading(&self) -> bool {
        self.fade_in.progress < 1.0 || !self.outgoing.is_empty()
    }

    /// Crossfade to `track`. Does nothing for an unknown track or if it is already the current one.
    pub fn play(&mut self, track: &str) -> bool {
        if !self.tracks.contains_key(track) {
            return false;
        }
        if self.current.as_deref() == Some(track) {
            return true;
        }
        // A track that was fading out and is wanted again would start twice; restart it cleanly.
        if let Some(i) = self.outgoing.iter().position(|(name, _)| name == track) {
            self.outgoing.remove(i);
            self.commands.push(MusicCommand::Stop { track: track.to_string() });
        }
        if let Some(previous) = self.current.take() {
            // Resume the fade-out from where the fade-in had got to, so the level does not jump.
            let progress = 1.0 - self.fade_in.progress;
            self.outgoing.push((previous, Fade { progress: progress.min(1.0) }));
        }
        let instant = self.fade_seconds == 0.0;
        self.fade_in = Fade { progress: if instant { 1.0 } else { 0.0 } };
        self.current = Some(track.to_string());
        let looping = self.mode == PlaybackMode::LoopOne;
        self.commands.push(MusicCommand::Start { track: track.to_string(), gain: fade_in_gain(self.fade_in.progress), looping });
        if !self.audible {
            self.commands.push(MusicCommand::Pause { track: track.to_string() });
        }
        if instant {
            self.finish_fades();
        }
        true
    }

    /// Start the first track of the playlist if nothing is playing yet.
    pub fn start(&mut self) {
        if self.current.is_none() {
            if let Some(first) = self.playlist.first().cloned() {
                self.play(&first);
            }
        }
    }

    /// Fade the current track out and stop it.
    pub fn stop(&mut self) {
        if let Some(previous) = self.current.take() {
            let progress = 1.0 - self.fade_in.progress;
            self.outgoing.push((previous, Fade { progress: progress.min(1.0) }));
            self.fade_in = Fade { progress: 1.0 };
        }
        if self.fade_seconds == 0.0 {
            self.finish_fades();
        }
    }

    /// While the music volume is 0 nothing needs to play: pause, like the Java engine does.
    pub fn set_audible(&mut self, audible: bool) {
        if audible == self.audible {
            return;
        }
        self.audible = audible;
        let mut names: Vec<String> = self.current.iter().cloned().collect();
        names.extend(self.outgoing.iter().map(|(n, _)| n.clone()));
        for track in names {
            self.commands.push(if audible { MusicCommand::Resume { track } } else { MusicCommand::Pause { track } });
        }
    }

    /// A non-looping track finished by itself. In sequence mode the next one fades in.
    pub fn track_ended(&mut self, track: &str) {
        if self.mode != PlaybackMode::Sequence || self.current.as_deref() != Some(track) || self.playlist.is_empty() {
            return;
        }
        let index = self.playlist.iter().position(|t| t == track).map_or(0, |i| i + 1);
        let next = self.playlist[index % self.playlist.len()].clone();
        if next == track {
            // A one-track playlist repeats itself.
            self.current = None;
            self.commands.push(MusicCommand::Stop { track: track.to_string() });
        }
        self.play(&next);
    }

    pub fn update(&mut self, dt: f32) {
        if !self.is_fading() || self.fade_seconds == 0.0 {
            return;
        }
        let step = dt / self.fade_seconds;
        if let Some(track) = &self.current {
            if self.fade_in.progress < 1.0 {
                self.fade_in.progress = (self.fade_in.progress + step).min(1.0);
                self.commands.push(MusicCommand::SetGain { track: track.clone(), gain: fade_in_gain(self.fade_in.progress) });
            }
        }
        for (track, fade) in &mut self.outgoing {
            fade.progress = (fade.progress + step).min(1.0);
            self.commands.push(MusicCommand::SetGain { track: track.clone(), gain: fade_out_gain(fade.progress) });
        }
        let finished: Vec<String> =
            self.outgoing.iter().filter(|(_, f)| f.progress >= 1.0).map(|(t, _)| t.clone()).collect();
        self.outgoing.retain(|(_, f)| f.progress < 1.0);
        for track in finished {
            self.commands.push(MusicCommand::Stop { track });
        }
    }

    fn finish_fades(&mut self) {
        for (track, _) in self.outgoing.drain(..) {
            self.commands.push(MusicCommand::Stop { track });
        }
    }

    pub fn take_commands(&mut self) -> Vec<MusicCommand> {
        std::mem::take(&mut self.commands)
    }
}

impl Default for Music {
    fn default() -> Self {
        Music::with_default_tracks()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn music() -> Music {
        let mut m = Music::with_default_tracks();
        m.set_fade_seconds(2.0);
        m
    }

    fn gains(commands: &[MusicCommand], track: &str) -> Vec<f32> {
        commands
            .iter()
            .filter_map(|c| match c {
                MusicCommand::SetGain { track: t, gain } if t == track => Some(*gain),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn fade_curves_keep_constant_power() {
        for i in 0..=20 {
            let p = i as f32 / 20.0;
            let power = fade_in_gain(p).powi(2) + fade_out_gain(p).powi(2);
            assert!((power - 1.0).abs() < 1e-5, "power {power} at {p}");
        }
        assert_eq!(fade_in_gain(0.0), 0.0);
        assert!((fade_in_gain(1.0) - 1.0).abs() < 1e-6);
        assert!((fade_out_gain(0.5) - std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-6);
    }

    #[test]
    fn the_first_track_fades_in_from_silence() {
        let mut m = music();
        m.start();
        let start = m.take_commands();
        assert_eq!(start, vec![MusicCommand::Start { track: "title".into(), gain: 0.0, looping: true }]);

        m.update(1.0); // half way
        let g = gains(&m.take_commands(), "title");
        assert_eq!(g.len(), 1);
        assert!((g[0] - std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-5, "half way gain {}", g[0]);

        m.update(1.5); // past the end: clamps to full volume
        let g = gains(&m.take_commands(), "title");
        assert!((g[0] - 1.0).abs() < 1e-6);
        assert!(!m.is_fading());
        m.update(1.0);
        assert!(m.take_commands().is_empty(), "nothing to do once the fade is over");
    }

    #[test]
    fn switching_tracks_crossfades_and_stops_the_old_one() {
        let mut m = music();
        m.play("title");
        m.update(3.0);
        m.take_commands();

        assert!(m.play("overworld"));
        let start = m.take_commands();
        assert_eq!(start, vec![MusicCommand::Start { track: "overworld".into(), gain: 0.0, looping: true }]);

        m.update(1.0);
        let commands = m.take_commands();
        let (up, down) = (gains(&commands, "overworld")[0], gains(&commands, "title")[0]);
        assert!((up * up + down * down - 1.0).abs() < 1e-5, "constant power during the crossfade");
        assert!(!commands.contains(&MusicCommand::Stop { track: "title".into() }), "still audible");

        m.update(1.0);
        let commands = m.take_commands();
        assert!(commands.contains(&MusicCommand::Stop { track: "title".into() }));
        assert_eq!(m.current(), Some("overworld"));
        assert!(!m.is_fading());
    }

    #[test]
    fn asking_for_the_current_or_an_unknown_track_changes_nothing() {
        let mut m = music();
        m.play("title");
        m.take_commands();
        assert!(m.play("title"));
        assert!(!m.play("does-not-exist"));
        assert!(m.take_commands().is_empty());
        assert_eq!(m.current(), Some("title"));
    }

    #[test]
    fn interrupting_a_fade_does_not_make_the_level_jump() {
        let mut m = music();
        m.play("title");
        m.update(0.5); // title is 25% faded in
        m.take_commands();
        m.play("overworld");
        m.update(0.0001);
        let commands = m.take_commands();
        let title_gain = gains(&commands, "title")[0];
        // The outgoing track continues from where the fade-in had reached (sin(pi/8) ~ 0.38) at
        // cos(progress * pi/2) with progress = 1 - 0.25: it must not jump back up to 1.0.
        assert!(title_gain < 0.5, "title gain {title_gain}");
    }

    #[test]
    fn zero_fade_time_switches_instantly() {
        let mut m = music();
        m.set_fade_seconds(0.0);
        m.play("title");
        m.take_commands();
        m.play("overworld");
        let commands = m.take_commands();
        assert_eq!(
            commands,
            vec![
                MusicCommand::Start { track: "overworld".into(), gain: 1.0, looping: true },
                MusicCommand::Stop { track: "title".into() },
            ]
        );
    }

    #[test]
    fn music_pauses_while_inaudible_and_resumes() {
        let mut m = music();
        m.play("title");
        m.take_commands();
        m.set_audible(false);
        assert_eq!(m.take_commands(), vec![MusicCommand::Pause { track: "title".into() }]);
        m.set_audible(false);
        assert!(m.take_commands().is_empty(), "no repeat");
        m.set_audible(true);
        assert_eq!(m.take_commands(), vec![MusicCommand::Resume { track: "title".into() }]);

        // A track started while inaudible is paused straight away.
        m.set_audible(false);
        m.take_commands();
        m.play("overworld");
        let commands = m.take_commands();
        assert!(commands.contains(&MusicCommand::Pause { track: "overworld".into() }));
    }

    #[test]
    fn sequence_mode_moves_on_when_a_track_ends_and_wraps_around() {
        let mut m = music();
        m.set_mode(PlaybackMode::Sequence);
        m.play("title");
        let start = m.take_commands();
        assert!(matches!(start[0], MusicCommand::Start { looping: false, .. }), "must not loop in sequence mode");

        m.track_ended("title");
        assert_eq!(m.current(), Some("overworld"));
        m.track_ended("overworld");
        assert_eq!(m.current(), Some("title"), "wraps to the start of the playlist");
        m.track_ended("not-playing");
        assert_eq!(m.current(), Some("title"));

        // In loop mode an ended notification is ignored.
        m.set_mode(PlaybackMode::LoopOne);
        m.track_ended("title");
        assert_eq!(m.current(), Some("title"));
    }

    #[test]
    fn stopping_fades_the_track_out_and_stops_it() {
        let mut m = music();
        m.play("title");
        m.update(5.0);
        m.take_commands();
        m.stop();
        assert_eq!(m.current(), None);
        m.update(1.0);
        let commands = m.take_commands();
        assert!(gains(&commands, "title")[0] < 0.75);
        m.update(1.5);
        assert!(m.take_commands().contains(&MusicCommand::Stop { track: "title".into() }));
    }

    #[test]
    fn the_shipped_tracks_exist_on_disk() {
        let music = Music::with_default_tracks();
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let names: Vec<&str> = music.track_names().collect();
        assert_eq!(names.len(), 2);
        for name in names {
            let path = root.join(music.path(name).unwrap());
            assert!(path.is_file(), "missing music file {}", path.display());
            assert!(std::fs::metadata(&path).unwrap().len() > 100_000, "{name} looks empty");
        }
    }
}
