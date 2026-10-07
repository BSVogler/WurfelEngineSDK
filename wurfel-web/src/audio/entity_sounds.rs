//! Entity sounds: footsteps, landing, splashing and the falling wind, ported from the sound code
//! in the Java `MovableEntity`.
//!
//! The simulation reports things as [`Event`]s (the Java engine sent telegrams); continuous
//! behaviour such as walking cycles needs the entity's state every frame, which the caller passes
//! as [`EntityInfo`].

use std::collections::HashSet;

use wurfel_sim::entity::{Entity, EntityId, Event};
use wurfel_sim::World;

use super::{AudioLogic, LoopHandle, Position};

/// CVar `walkingAnimationSpeedCorrection`.
pub const WALKING_ANIMATION_SPEED_CORRECTION: f32 = 1.0;
/// Length of one walking cycle in the Java engine's arbitrary units; two steps are played per cycle.
const CYCLE_LENGTH: f32 = 1000.0;
/// Footsteps play at 30% volume with a little random pitch and pan (`MovableEntity.step`).
const STEP_VOLUME: f32 = 0.3;
/// Entities lighter than this make no wind while falling.
const FALLING_SOUND_MIN_MASS: f32 = 0.3;
/// Entities lighter than this make no splash.
const SPLASH_MIN_MASS: f32 = 1.0;
/// Above this mass the splash is at full volume, below it at half.
const SPLASH_LOUD_MASS: f32 = 5.0;
/// A running sound plays while the entity moves faster than this (blocks per second).
const RUNNING_MIN_SPEED: f32 = 0.5;

/// Which sounds an entity makes. `None` disables a sound, as in Java where each is optional.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntitySoundConfig {
    pub landing: Option<String>,
    pub step: Option<String>,
    pub jump: Option<String>,
    pub falling: Option<String>,
    pub running: Option<String>,
    pub splash: Option<String>,
}

impl EntitySoundConfig {
    /// What the Java engine does for an entity with no configuration: landing, splash and falling
    /// wind, no footsteps or jump sound.
    pub fn engine_default() -> Self {
        EntitySoundConfig {
            landing: Some("landing".into()),
            step: None,
            jump: None,
            falling: Some("wind".into()),
            running: None,
            splash: Some("splash".into()),
        }
    }

    /// The Caveland character (`Ejira`): engine defaults plus footsteps and a jump sound.
    pub fn player() -> Self {
        EntitySoundConfig { step: Some("step".into()), jump: Some("urfJump".into()), ..Self::engine_default() }
    }

    #[cfg(test)]
    pub fn silent() -> Self {
        EntitySoundConfig { landing: None, step: None, jump: None, falling: None, running: None, splash: None }
    }
}

/// What the audio needs to know about an entity this frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EntityInfo {
    pub position: Position,
    /// Velocity in blocks per second.
    pub velocity: [f32; 3],
    pub mass: f32,
    pub on_ground: bool,
    pub floating: bool,
}

impl EntityInfo {
    pub fn from_entity(entity: &Entity, world: &World) -> Self {
        let body = entity.body.as_ref();
        EntityInfo {
            position: entity.position.to_array(),
            velocity: body.map_or([0.0; 3], |b| b.movement.to_array()),
            mass: entity.mass,
            on_ground: entity.is_on_ground(world),
            floating: body.is_some_and(|b| b.floating),
        }
    }

    /// 3D speed in blocks per second (`getSpeed`).
    pub fn speed(&self) -> f32 {
        let [x, y, z] = self.velocity;
        (x * x + y * y + z * z).sqrt()
    }
}

/// Per-entity sound state.
#[derive(Debug)]
pub struct EntityState {
    /// Position in the walking cycle, 0..1000.
    walking_cycle: f32,
    step_played_in_phase: bool,
    wind: Option<LoopHandle>,
    running: Option<LoopHandle>,
}

impl EntityState {
    pub fn new() -> Self {
        // Starting with the step "already played" keeps a freshly spawned, standing entity quiet.
        // (The Java field starts false, so its first frame on the ground makes a step sound.)
        EntityState { walking_cycle: 0.0, step_played_in_phase: true, wind: None, running: None }
    }
}

impl Default for EntityState {
    fn default() -> Self {
        Self::new()
    }
}

impl AudioLogic {
    /// Override the sounds of one entity.
    #[cfg(test)]
    pub fn set_entity_config(&mut self, id: EntityId, config: EntitySoundConfig) {
        self.entity_configs.insert(id, config);
    }

    /// The sounds used for entities without their own configuration.
    #[cfg(test)]
    pub fn set_default_entity_config(&mut self, config: EntitySoundConfig) {
        self.entity_default = config;
    }

    fn config_for(&self, id: EntityId) -> EntitySoundConfig {
        self.entity_configs.get(&id).unwrap_or(&self.entity_default).clone()
    }

    /// React to the events of one simulation update. `info` returns the current state of an
    /// entity (or `None` if it is gone).
    pub fn handle_events(&mut self, events: &[Event], info: &dyn Fn(EntityId) -> Option<EntityInfo>) {
        for event in events {
            match *event {
                Event::Landed(id) => {
                    let Some(entity) = info(id) else { continue };
                    let config = self.config_for(id);
                    if let Some(landing) = &config.landing {
                        self.play(landing, Some(entity.position), 1.0);
                    }
                    if let Some(step) = &config.step {
                        self.step_sound(step);
                        self.entities.entry(id).or_default().step_played_in_phase = true;
                    }
                }
                Event::EnteredLiquid(id) => {
                    let Some(entity) = info(id) else { continue };
                    if entity.mass >= SPLASH_MIN_MASS {
                        if let Some(splash) = self.config_for(id).splash {
                            let volume = if entity.mass > SPLASH_LOUD_MASS { 1.0 } else { 0.5 };
                            self.play(&splash, Some(entity.position), volume);
                        }
                    }
                }
                Event::Disposed(id) => self.forget_entity(id),
                // The Java engine plays nothing for a plain collision.
                Event::Collided(_) => {}
            }
        }
    }

    /// The entity jumped (the Java `jump(velo, playSound)`).
    pub fn on_jump(&mut self, id: EntityId, position: Position) {
        if let Some(jump) = self.config_for(id).jump {
            self.play(&jump, Some(position), 1.0);
        }
    }

    /// Per-frame sounds for every entity: the walking cycle's footsteps, the wind while falling
    /// and running sounds. Entities that are no longer listed lose their looping sounds.
    pub fn update_entities(&mut self, dt: f32, entities: &[(EntityId, EntityInfo)]) {
        let present: HashSet<EntityId> = entities.iter().map(|(id, _)| *id).collect();
        let gone: Vec<EntityId> = self.entities.keys().filter(|id| !present.contains(id)).copied().collect();
        for id in gone {
            self.forget_entity(id);
        }

        for (id, info) in entities {
            let config = self.config_for(*id);
            let mut state = self.entities.remove(id).unwrap_or_default();
            self.update_walking(dt, &config, info, &mut state);
            self.update_falling(&config, info, &mut state);
            self.update_running(&config, info, &mut state);
            self.entities.insert(*id, state);
        }
    }

    /// `MovableEntity.update`, walking-cycle part.
    fn update_walking(&mut self, dt: f32, config: &EntitySoundConfig, info: &EntityInfo, state: &mut EntityState) {
        if info.floating || info.on_ground {
            state.walking_cycle += dt * 1000.0 * info.speed() * WALKING_ANIMATION_SPEED_CORRECTION;
        }
        if state.walking_cycle >= CYCLE_LENGTH {
            state.walking_cycle %= CYCLE_LENGTH;
            state.step_played_in_phase = false;
        }
        if !(info.floating || info.on_ground) {
            return;
        }
        let Some(step) = &config.step else { return };
        // Two steps per cycle: one in the first quarter, one in the second half.
        if state.walking_cycle < 250.0 {
            if !state.step_played_in_phase && info.on_ground {
                self.step_sound(step);
                state.step_played_in_phase = true;
            }
        } else if state.walking_cycle < 500.0 {
            state.step_played_in_phase = false;
        } else if state.walking_cycle > 500.0 && !state.step_played_in_phase && info.on_ground {
            self.step_sound(step);
            state.step_played_in_phase = true;
        }
    }

    /// The wind loop while falling, louder the faster the entity falls.
    fn update_falling(&mut self, config: &EntitySoundConfig, info: &EntityInfo, state: &mut EntityState) {
        let falling = !info.floating && info.velocity[2] < 0.0 && info.mass > FALLING_SOUND_MIN_MASS;
        match (&config.falling, falling) {
            (Some(name), true) => {
                let volume = (info.speed() / 10.0).min(1.0);
                match state.wind {
                    Some(handle) => self.set_loop_volume(handle, volume),
                    None => state.wind = self.start_loop(name, None, volume),
                }
            }
            _ => {
                if let Some(handle) = state.wind.take() {
                    self.stop_loop(handle);
                }
            }
        }
    }

    fn update_running(&mut self, config: &EntitySoundConfig, info: &EntityInfo, state: &mut EntityState) {
        let running = info.speed() >= RUNNING_MIN_SPEED && (info.on_ground || info.floating);
        match (&config.running, running) {
            (Some(name), true) => match state.running {
                Some(handle) => self.set_loop_position(handle, info.position),
                None => state.running = self.start_loop(name, Some(info.position), 1.0),
            },
            _ => {
                if let Some(handle) = state.running.take() {
                    self.stop_loop(handle);
                }
            }
        }
    }

    /// `MovableEntity.step`: a quiet step with a random pitch of 0.9..1.1 and pan of -0.5..0.5.
    fn step_sound(&mut self, name: &str) {
        let pitch = 0.9 + self.random() / 5.0;
        let pan = self.random() - 0.5;
        self.play_with(name, STEP_VOLUME, pitch, pan);
    }

    fn forget_entity(&mut self, id: EntityId) {
        self.entity_configs.remove(&id);
        if let Some(state) = self.entities.remove(&id) {
            for handle in [state.wind, state.running].into_iter().flatten() {
                self.stop_loop(handle);
            }
        }
    }
}
