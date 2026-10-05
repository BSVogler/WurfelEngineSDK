use glam::Vec3;

use crate::light::PointLight;

use super::{ParticleSpec, Particles};

/// Spawns particles at a steady rate (`ParticleEmitter`).
#[derive(Debug, Clone)]
pub struct ParticleEmitter {
    pub position: Vec3,
    pub active: bool,
    /// What is spawned.
    pub spec: ParticleSpec,
    /// Seconds between spawns (Java `timeEachSpawn`, default 100 ms).
    pub interval: f32,
    /// Base velocity of new particles, blocks per second.
    pub velocity: Vec3,
    /// Random noise added to the velocity, `±spread` per axis.
    pub spread: Vec3,
    /// The glow, if any (Java `lightsource`); see [`ParticleEmitter::set_brightness`].
    light: Option<PointLight>,
    timer: f32,
}

/// The Java emitter's light: `new PointLightSource(Color.YELLOW, 5, 11, view)`.
pub const LIGHT_COLOR: Vec3 = Vec3::new(1.0, 1.0, 0.0);
pub const LIGHT_RADIUS: f32 = 5.0;

impl ParticleEmitter {
    /// Active, one fire particle every 100 ms going up. (The Java emitter started with random
    /// vectors; a fixed default is easier to reason about.)
    pub fn new(position: Vec3) -> Self {
        ParticleEmitter {
            position,
            active: true,
            spec: ParticleSpec::fire(),
            interval: 0.1,
            velocity: Vec3::new(0.0, 0.0, 1.0),
            spread: Vec3::new(0.2, 0.2, 0.2),
            light: None,
            timer: 0.0,
        }
    }

    /// The jetpack exhaust: starts switched off, sprays flame down and a little sideways every
    /// 30 ms. Move it to the player's feet and set `active` while the jetpack burns.
    pub fn jetpack() -> Self {
        ParticleEmitter {
            active: false,
            spec: ParticleSpec::jetpack(),
            interval: 0.03,
            velocity: Vec3::new(0.0, 0.0, -2.0),
            spread: Vec3::new(0.1, 0.1, 0.5),
            ..Self::new(Vec3::ZERO)
        }
    }

    /// Give the emitter a yellow point light of the given brightness, or remove it with a negative
    /// value (`ParticleEmitter.setBrightness`).
    pub fn set_brightness(&mut self, brightness: f32) {
        self.light = (brightness >= 0.0).then(|| PointLight::new(self.position, LIGHT_COLOR, LIGHT_RADIUS, brightness));
    }

    /// The light as it is right now: at the emitter, and only while the emitter is active (the Java
    /// emitter only moved and updated its light source when active).
    pub fn light(&self) -> Option<PointLight> {
        self.light.filter(|_| self.active).map(|light| PointLight { position: self.position, ..light })
    }

    pub fn toggle(&mut self) {
        self.active = !self.active;
    }

    /// Spawn what is due in `dt` seconds; may spawn several (the Java `while (timer >= ...)`).
    /// Returns how many were spawned.
    pub fn update(&mut self, dt: f32, particles: &mut Particles) -> usize {
        if !self.active || self.interval <= 0.0 {
            return 0;
        }
        self.timer += dt;
        let mut spawned = 0;
        while self.timer >= self.interval {
            self.timer -= self.interval;
            if particles.spawn(&self.spec, self.position, self.velocity, self.spread) {
                spawned += 1;
            }
        }
        spawned
    }
}
