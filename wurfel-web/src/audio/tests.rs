use std::collections::HashMap;

use wurfel_sim::entity::Event;

use super::*;

fn plays(commands: &[SoundCommand], name: &str) -> Vec<(f32, f32, f32)> {
    commands
        .iter()
        .filter_map(|c| match c {
            SoundCommand::Play { name: n, volume, pitch, pan } if n == name => Some((*volume, *pitch, *pan)),
            _ => None,
        })
        .collect()
}

fn logic() -> AudioLogic {
    let mut l = AudioLogic::with_seed(42);
    l.take_commands(); // the initial bus setup
    l
}

fn walking(speed: f32) -> EntityInfo {
    EntityInfo { position: [5.0, 5.0, 1.0], velocity: [speed, 0.0, 0.0], mass: 60.0, on_ground: true, floating: false }
}

// -------------------------------------------------------------------------------- formulas

/// The Java formula in its own units (game units), to check the block-based version against it.
fn java_volume(distance_game_units: f32) -> f32 {
    let (decay, edge) = (6000.0f32, 141.421_36f32);
    (decay * edge / (distance_game_units * distance_game_units + decay * edge)).min(1.0)
}

#[test]
fn attenuation_matches_the_java_formula() {
    assert_eq!(attenuation(0.0), 1.0);
    for blocks in [0.5f32, 1.0, 3.0, 6.5, 10.0, 19.5, 40.0] {
        let expected = java_volume(blocks * 141.421_36);
        assert!((attenuation(blocks) - expected).abs() < 1e-5, "{blocks} blocks: {} vs {expected}", attenuation(blocks));
    }
}

#[test]
fn attenuation_is_half_at_6_5_blocks_and_crosses_the_play_threshold_at_19_5() {
    assert!((attenuation(6.514) - 0.5).abs() < 0.002, "{}", attenuation(6.514));
    assert!(attenuation(19.0) > PLAY_THRESHOLD);
    assert!(attenuation(20.0) < PLAY_THRESHOLD);
    let mut last = 1.0;
    for i in 1..100 {
        let a = attenuation(i as f32 * 0.5);
        assert!(a < last, "must fall with distance");
        last = a;
    }
}

#[test]
fn pan_follows_the_screen_position_not_the_depth() {
    let listener = [10.0, 10.0, 1.0];
    assert_eq!(pan(listener, listener), 0.0);
    // +x in this frame moves right on screen (and down); +y moves left (and down).
    assert!(pan([11.0, 10.0, 1.0], listener) > 0.0);
    assert!(pan([10.0, 11.0, 1.0], listener) < 0.0);
    // Moving along the depth axis (+x +y together) or in height does not pan.
    assert_eq!(pan([13.0, 13.0, 1.0], listener), 0.0);
    assert_eq!(pan([10.0, 10.0, 6.0], listener), 0.0);
    // 100 px per block of (gx - gy), 500 px for full pan: 2.5 blocks of x give 0.5.
    assert!((pan([12.5, 10.0, 1.0], listener) - 0.5).abs() < 1e-5);
    assert_eq!(pan([40.0, 10.0, 1.0], listener), 1.0);
    assert_eq!(pan([10.0, 40.0, 1.0], listener), -1.0);
}

// ------------------------------------------------------------------------------- settings

#[test]
fn bus_gains_are_master_times_category_and_mute_wins() {
    let s = AudioSettings { master: 0.5, music: 0.4, effects: 0.8, muted: false };
    assert!((s.effects_gain() - 0.4).abs() < 1e-6);
    assert!((s.music_gain() - 0.2).abs() < 1e-6);
    let muted = AudioSettings { muted: true, ..s };
    assert_eq!((muted.effects_gain(), muted.music_gain()), (0.0, 0.0));
}

fn settings_from(entries: &[(&str, Setting)]) -> AudioSettings {
    let map: HashMap<String, Setting> = entries.iter().map(|(k, v)| (k.to_string(), *v)).collect();
    AudioSettings::from_lookup(&|key| map.get(key).copied())
}

#[test]
fn settings_are_read_under_several_key_names_as_fractions_or_percent() {
    let s = settings_from(&[("masterVolume", Setting::Number(0.5)), ("musicVolume", Setting::Number(30.0)), ("sfxVolume", Setting::Number(100.0))]);
    assert_eq!((s.master, s.music, s.effects), (0.5, 0.3, 1.0));

    let s = settings_from(&[("master", Setting::Number(80.0)), ("music", Setting::Number(0.0)), ("sound", Setting::Number(0.25)), ("mute", Setting::Bool(true))]);
    assert_eq!((s.master, s.music, s.effects, s.muted), (0.8, 0.0, 0.25, true));

    // The first key in the documented order wins.
    let s = settings_from(&[("masterVolume", Setting::Number(0.1)), ("master", Setting::Number(0.9))]);
    assert_eq!(s.master, 0.1);
}

#[test]
fn bad_settings_keep_their_defaults() {
    let s = settings_from(&[
        ("masterVolume", Setting::Number(f64::NAN)),
        ("musicVolume", Setting::Bool(true)),
        ("effectsVolume", Setting::Number(f64::INFINITY)),
        ("muted", Setting::Number(1.0)),
    ]);
    assert_eq!(s, AudioSettings::default());
    assert_eq!(normalize_volume(-5.0), Some(0.0));
    assert_eq!(normalize_volume(250.0), Some(1.0));
    assert_eq!(normalize_volume(f64::NAN), None);
}

#[test]
fn changing_settings_updates_the_buses_once_and_pauses_music_at_zero() {
    let mut l = logic();
    l.start_music();
    l.take_commands();

    let quiet = AudioSettings { master: 0.5, music: 0.0, effects: 1.0, muted: false };
    l.set_settings(quiet);
    let commands = l.take_commands();
    assert!(commands.contains(&SoundCommand::SetBuses { effects: 0.5, music: 0.0 }));
    assert!(commands.contains(&SoundCommand::Music(MusicCommand::Pause { track: "title".into() })));

    l.set_settings(quiet); // same again: nothing
    assert!(l.take_commands().is_empty());

    l.set_settings(AudioSettings { music: 1.0, ..quiet });
    assert!(l.take_commands().contains(&SoundCommand::Music(MusicCommand::Resume { track: "title".into() })));
}

// ----------------------------------------------------------------------------- the bank

#[test]
fn the_first_registration_of_a_name_wins() {
    let mut bank = SoundBank::new();
    assert!(bank.register("hit", "a.wav"));
    assert!(!bank.register("hit", "b.wav"));
    assert_eq!(bank.path("hit"), Some("a.wav"));
    assert_eq!(bank.path("missing"), None);
}

#[test]
fn every_default_sound_exists_on_disk_and_the_names_are_unique() {
    let bank = SoundBank::with_defaults();
    assert_eq!(bank.len(), DEFAULT_SOUNDS.len(), "duplicate names in DEFAULT_SOUNDS");
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    for name in bank.names() {
        let path = root.join(bank.path(name).unwrap());
        let size = std::fs::metadata(&path).unwrap_or_else(|_| panic!("{name}: missing {}", path.display())).len();
        assert!(size > 1000, "{name}: {} looks empty ({size} bytes)", path.display());
    }
    for essential in ["landing", "splash", "wind", "step", "urfJump", "menuSelect"] {
        assert!(bank.contains(essential), "{essential}");
    }
}

// ------------------------------------------------------------------------------ one-offs

#[test]
fn a_nearby_positional_sound_is_attenuated_and_panned() {
    let mut l = logic();
    l.set_listener([10.0, 10.0, 1.0]);
    assert!(l.play("collect", Some([12.5, 10.0, 1.0]), 1.0));
    let (volume, pitch, pan_value) = plays(&l.take_commands(), "collect")[0];
    assert!((volume - attenuation(2.5)).abs() < 1e-6);
    assert_eq!(pitch, 1.0);
    assert!((pan_value - 0.5).abs() < 1e-5);
}

#[test]
fn a_far_positional_sound_is_dropped_but_a_global_one_is_not() {
    let mut l = logic();
    l.set_listener([0.0, 0.0, 1.0]);
    assert!(!l.play("collect", Some([30.0, 0.0, 1.0]), 1.0), "30 blocks away is below the threshold");
    assert!(l.take_commands().is_empty());
    assert!(l.play("collect", None, 0.05), "non-positional sounds are not culled");
    assert_eq!(plays(&l.take_commands(), "collect"), vec![(0.05, 1.0, 0.0)]);
}

#[test]
fn unknown_sounds_are_ignored() {
    let mut l = logic();
    assert!(!l.play("no-such-sound", None, 1.0));
    assert!(!l.play_with("no-such-sound", 1.0, 1.0, 0.0));
    assert_eq!(l.start_loop("no-such-sound", None, 1.0), None);
    assert!(l.take_commands().is_empty());
}

#[test]
fn explicit_pan_is_clamped() {
    let mut l = logic();
    l.play_with("step", 0.3, 1.2, 3.0);
    assert_eq!(plays(&l.take_commands(), "step"), vec![(0.3, 1.2, 1.0)]);
}

// --------------------------------------------------------------------------------- loops

#[test]
fn positional_loops_follow_the_listener() {
    let mut l = logic();
    l.set_listener([0.0, 0.0, 1.0]);
    let handle = l.start_loop("wagon", Some([2.0, 0.0, 1.0]), 1.0).unwrap();
    let start = l.take_commands();
    assert!(matches!(&start[0], SoundCommand::StartLoop { handle: h, volume, .. } if *h == handle && (*volume - attenuation(2.0)).abs() < 1e-6));

    l.update(0.016);
    assert!(l.take_commands().is_empty(), "nothing moved, nothing to send");

    l.set_listener([-10.0, 0.0, 1.0]); // walk away
    l.update(0.016);
    match &l.take_commands()[0] {
        SoundCommand::SetLoop { volume, pan, .. } => {
            assert!((volume - attenuation(12.0)).abs() < 1e-6);
            assert!(*pan > 0.0, "the sound is now to the right of the listener");
        }
        other => panic!("expected SetLoop, got {other:?}"),
    }

    l.stop_loop(handle);
    assert_eq!(l.take_commands(), vec![SoundCommand::StopLoop { handle }]);
    l.stop_loop(handle); // stopping twice is harmless
    assert!(l.take_commands().is_empty());
}

#[test]
fn global_loops_and_stop_all() {
    let mut l = logic();
    let a = l.start_loop("droneLoop", None, 0.7).unwrap();
    let b = l.start_loop("droneLoop", None, 0.7).unwrap();
    assert_ne!(a, b);
    l.set_loop_volume(a, 0.2);
    let commands = l.take_commands();
    assert!(commands.contains(&SoundCommand::SetLoop { handle: a, volume: 0.2, pan: 0.0 }));
    l.update(1.0);
    assert!(l.take_commands().is_empty(), "global loops need no per-frame updates");
    l.stop_all();
    assert_eq!(l.take_commands(), vec![SoundCommand::StopAll]);
    l.stop_loop(a);
    assert!(l.take_commands().is_empty(), "stop_all forgot the loops");
}

// ------------------------------------------------------------------ entity events and cycle

#[test]
fn landing_plays_the_landing_sound_at_the_entity_and_a_quiet_step() {
    let mut l = logic();
    l.set_listener([5.0, 5.0, 1.0]);
    l.handle_events(&[Event::Landed(7)], &|id| (id == 7).then(|| walking(0.0)));
    let commands = l.take_commands();
    let landing = plays(&commands, "landing");
    assert_eq!(landing.len(), 1);
    assert!((landing[0].0 - 1.0).abs() < 1e-6, "right at the listener: full volume");
    let step = plays(&commands, "step");
    assert_eq!(step.len(), 1);
    assert_eq!(step[0].0, 0.3);
    assert!((0.9..1.1).contains(&step[0].1), "pitch {}", step[0].1);
    assert!((-0.5..0.5).contains(&step[0].2), "pan {}", step[0].2);
}

#[test]
fn splash_depends_on_mass() {
    let info = |mass: f32| move |_| Some(EntityInfo { mass, ..walking(0.0) });
    let volume_for = |mass: f32| {
        let mut l = logic();
        l.set_listener([5.0, 5.0, 1.0]);
        l.handle_events(&[Event::EnteredLiquid(1)], &info(mass));
        plays(&l.take_commands(), "splash").first().map(|p| p.0)
    };
    assert_eq!(volume_for(60.0), Some(1.0), "heavy: full volume");
    assert_eq!(volume_for(3.0), Some(0.5), "medium: half volume");
    assert_eq!(volume_for(0.4), None, "light entities do not splash");
}

#[test]
fn plain_collisions_and_unknown_entities_make_no_sound() {
    let mut l = logic();
    l.handle_events(&[Event::Collided(1)], &|_| Some(walking(0.0)));
    l.handle_events(&[Event::Landed(2), Event::EnteredLiquid(2)], &|_| None);
    assert!(l.take_commands().is_empty());
}

#[test]
fn a_silent_config_makes_no_entity_sounds() {
    let mut l = logic();
    l.set_listener([5.0, 5.0, 1.0]);
    l.set_entity_config(3, EntitySoundConfig::silent());
    l.handle_events(&[Event::Landed(3), Event::EnteredLiquid(3)], &|_| Some(walking(0.0)));
    l.on_jump(3, [5.0, 5.0, 1.0]);
    assert!(l.take_commands().is_empty());
}

#[test]
fn jumping_plays_the_configured_jump_sound() {
    let mut l = logic();
    l.set_listener([5.0, 5.0, 1.0]);
    l.on_jump(1, [5.0, 5.0, 1.0]);
    assert_eq!(plays(&l.take_commands(), "urfJump").len(), 1);
    l.set_default_entity_config(EntitySoundConfig::engine_default()); // no jump sound
    l.on_jump(1, [5.0, 5.0, 1.0]);
    assert!(l.take_commands().is_empty());
}

fn simulate_steps(seconds: f32, info: EntityInfo) -> usize {
    let mut l = logic();
    let mut steps = 0;
    for _ in 0..(seconds * 60.0) as usize {
        l.update_entities(1.0 / 60.0, &[(1, info)]);
        steps += plays(&l.take_commands(), "step").len();
    }
    steps
}

#[test]
fn walking_makes_two_steps_per_cycle() {
    // The Java cycle advances by dt_ms * speed per frame and wraps at 1000: at 4 blocks/s one cycle
    // takes 0.25 s, so two steps per 0.25 s, i.e. 8 steps per second.
    let steps = simulate_steps(2.0, walking(4.0));
    assert!((15..=17).contains(&steps), "{steps} steps in 2 s");
    // Half the speed, half the steps.
    let slow = simulate_steps(2.0, walking(2.0));
    assert!((7..=9).contains(&slow), "{slow} steps in 2 s");
}

#[test]
fn no_steps_when_standing_still_or_in_the_air() {
    assert_eq!(simulate_steps(5.0, walking(0.0)), 0, "standing still, also not on spawn");
    let flying = EntityInfo { on_ground: false, ..walking(4.0) };
    assert_eq!(simulate_steps(2.0, flying), 0);
}

#[test]
fn steps_stop_when_the_entity_stops_and_resume_when_it_walks() {
    let mut l = logic();
    let count = |l: &mut AudioLogic, info: EntityInfo, frames: usize| {
        let mut n = 0;
        for _ in 0..frames {
            l.update_entities(1.0 / 60.0, &[(1, info)]);
            n += plays(&l.take_commands(), "step").len();
        }
        n
    };
    assert!(count(&mut l, walking(4.0), 60) > 4);
    assert_eq!(count(&mut l, walking(0.0), 120), 0);
    assert!(count(&mut l, walking(4.0), 60) > 4);
}

#[test]
fn falling_starts_a_wind_loop_that_scales_with_speed_and_stops_on_landing() {
    let mut l = logic();
    let falling = |vz: f32| EntityInfo { velocity: [0.0, 0.0, vz], on_ground: false, ..walking(0.0) };

    l.update_entities(0.016, &[(1, falling(-3.0))]);
    let start = l.take_commands();
    let handle = match &start[0] {
        SoundCommand::StartLoop { handle, name, volume, pan } => {
            assert_eq!(name, "wind");
            assert!((volume - 0.3).abs() < 1e-6, "speed 3 -> volume 0.3");
            assert_eq!(*pan, 0.0, "wind is not positional");
            *handle
        }
        other => panic!("expected StartLoop, got {other:?}"),
    };

    l.update_entities(0.016, &[(1, falling(-9.0))]);
    assert_eq!(l.take_commands(), vec![SoundCommand::SetLoop { handle, volume: 0.9, pan: 0.0 }]);
    l.update_entities(0.016, &[(1, falling(-30.0))]);
    assert_eq!(l.take_commands(), vec![SoundCommand::SetLoop { handle, volume: 1.0, pan: 0.0 }], "volume is capped");

    l.update_entities(0.016, &[(1, walking(0.0))]); // landed
    assert_eq!(l.take_commands(), vec![SoundCommand::StopLoop { handle }]);
}

#[test]
fn light_and_floating_entities_make_no_wind() {
    let mut l = logic();
    let feather = EntityInfo { velocity: [0.0, 0.0, -5.0], mass: 0.2, on_ground: false, floating: false, position: [0.0; 3] };
    let balloon = EntityInfo { floating: true, mass: 5.0, ..feather };
    l.update_entities(0.016, &[(1, feather), (2, balloon)]);
    assert!(l.take_commands().is_empty());
}

#[test]
fn entities_that_vanish_lose_their_loops() {
    let mut l = logic();
    let falling = EntityInfo { velocity: [0.0, 0.0, -5.0], on_ground: false, ..walking(0.0) };
    l.update_entities(0.016, &[(1, falling)]);
    let handle = match l.take_commands().remove(0) {
        SoundCommand::StartLoop { handle, .. } => handle,
        other => panic!("{other:?}"),
    };
    l.update_entities(0.016, &[]); // gone without a Disposed event
    assert_eq!(l.take_commands(), vec![SoundCommand::StopLoop { handle }]);

    // The Disposed event does the same.
    l.update_entities(0.016, &[(2, falling)]);
    let handle = match l.take_commands().remove(0) {
        SoundCommand::StartLoop { handle, .. } => handle,
        other => panic!("{other:?}"),
    };
    l.handle_events(&[Event::Disposed(2)], &|_| None);
    assert_eq!(l.take_commands(), vec![SoundCommand::StopLoop { handle }]);
}

#[test]
fn a_running_sound_loops_while_the_entity_moves() {
    let mut l = logic();
    l.set_listener([5.0, 5.0, 1.0]);
    l.set_entity_config(1, EntitySoundConfig { running: Some("robot2walk".into()), ..EntitySoundConfig::silent() });
    l.update_entities(0.016, &[(1, walking(2.0))]);
    let start = l.take_commands();
    assert!(matches!(&start[0], SoundCommand::StartLoop { name, .. } if name == "robot2walk"));
    l.update_entities(0.016, &[(1, walking(0.1))]);
    assert!(matches!(l.take_commands()[0], SoundCommand::StopLoop { .. }));
}

#[test]
fn music_commands_come_out_of_the_same_queue() {
    let mut l = logic();
    l.start_music();
    let commands = l.take_commands();
    assert_eq!(commands, vec![SoundCommand::Music(MusicCommand::Start { track: "title".into(), gain: 0.0, looping: true })]);
    assert!(l.play_music("overworld"));
    l.update(1.0);
    assert!(l.take_commands().iter().any(|c| matches!(c, SoundCommand::Music(MusicCommand::SetGain { track, .. }) if track == "overworld")));
}

#[test]
fn the_random_numbers_are_in_range_and_not_constant() {
    let mut l = logic();
    let values: Vec<f32> = (0..1000).map(|_| l.random()).collect();
    assert!(values.iter().all(|v| (0.0..1.0).contains(v)));
    let mean = values.iter().sum::<f32>() / values.len() as f32;
    assert!((mean - 0.5).abs() < 0.05, "mean {mean}");
}
