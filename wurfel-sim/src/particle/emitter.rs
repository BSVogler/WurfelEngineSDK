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

    /// One nozzle of Ejira's jetpack: starts switched off. The Java emitter holds 80 particles of
    /// 1.8 s, so it spawns about one every 22 ms; the particles spread wide sideways and little
    /// vertically. Set `velocity.z` to the exhaust speed and move it to the nozzle while it burns.
    pub fn jetpack() -> Self {
        ParticleEmitter {
            active: false,
            spec: ParticleSpec::jetpack(),
            interval: 1.8 / 80.0,
            velocity: Vec3::new(0.0, 0.0, -0.1),
            spread: Vec3::new(0.8, 0.8, 0.2),
            ..Self::new(Vec3::ZERO)
        }
    }

    /// The sparks of lit dynamite (`TFlint.sparksGenerator`): a few pale sparks spread wide sideways,
    /// hardly moving up. Starts switched off.
    pub fn sparks() -> Self {
        ParticleEmitter {
            active: false,
            spec: ParticleSpec::sparkle(),
            interval: 0.04,
            velocity: Vec3::new(0.0, 0.0, 0.6),
            spread: Vec3::new(0.9, 0.9, 0.4),
            ..Self::new(Vec3::ZERO)
        }
    }

    /// The smoke over a burning fuse; starts switched off.
    pub fn fuse_smoke() -> Self {
        ParticleEmitter {
            active: false,
            spec: ParticleSpec::fuse_smoke(),
            interval: 0.12,
            velocity: Vec3::new(0.0, 0.0, 1.0),
            spread: Vec3::new(0.15, 0.15, 0.1),
            ..Self::new(Vec3::ZERO)
        }
    }

    /// The fire at the wreck of the intro ship (`Spaceship`): long-lived fire going up at 3 blocks per second.
    pub fn wreck(position: Vec3) -> Self {
        ParticleEmitter { spec: ParticleSpec::wreck(), velocity: Vec3::new(0.0, 0.0, 3.0), ..Self::new(position) }
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
