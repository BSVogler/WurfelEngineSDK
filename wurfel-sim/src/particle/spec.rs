//! What a particle looks and behaves like.

/// Fading behaviour (`ParticleType`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParticleType {
    /// Grows, darkens, then fades and vanishes.
    Fire,
    /// Grows, fades and vanishes.
    Smoke,
    /// Keeps size and colour.
    Regular,
}

impl ParticleType {
    pub fn fades(self) -> bool {
        matches!(self, ParticleType::Fire | ParticleType::Smoke)
    }

    pub fn fades_to_black(self) -> bool {
        matches!(self, ParticleType::Fire)
    }

    pub fn is_growing(self) -> bool {
        matches!(self, ParticleType::Fire | ParticleType::Smoke)
    }
}

/// The template particles are made from (the Java emitter's `prototype` particle plus the physics
/// parameters that were mass and floating on the Java entity).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ParticleSpec {
    pub kind: ParticleType,
    /// Start colour, rgba 0..1.
    pub color: [f32; 4],
    /// Time to live in seconds (Java default 2000 ms).
    pub ttl: f32,
    /// Edge length in blocks at scale 1.
    pub size: f32,
    /// Multiplier of the world gravity; 0 floats (the Java `Particle`).
    pub gravity: f32,
    /// Velocity lost per second to the air: `v /= 1 + drag * dt`.
    pub drag: f32,
    /// Collide with blocks.
    pub collides: bool,
    /// Fraction of speed kept (reversed) on hitting a block.
    pub bounce: f32,
    /// Fraction of sideways speed lost when landing.
    pub ground_friction: f32,
    /// Initial scale is multiplied by `1 - random * scale_jitter` (Java debris: 0.5).
    pub scale_jitter: f32,
    /// Blocks of travel after which the shade variant is re-rolled; 0 never (Java
    /// `rotateEachNMeters`, 0.06 for debris).
    pub cycle_distance: f32,
}

impl ParticleSpec {
    /// The Java default `Particle`: floats, grey, lives 2 s, scale 0.3.
    pub fn regular() -> Self {
        ParticleSpec {
            kind: ParticleType::Regular,
            color: [0.5, 0.5, 0.5, 1.0],
            ttl: 2.0,
            size: 1.0,
            gravity: 0.0,
            drag: 0.0,
            collides: true,
            bounce: 0.0,
            ground_friction: 0.0,
            scale_jitter: 0.0,
            cycle_distance: 0.0,
        }
    }

    pub fn smoke() -> Self {
        ParticleSpec { kind: ParticleType::Smoke, color: [0.45, 0.45, 0.45, 0.8], ttl: 2.5, drag: 0.5, ..Self::regular() }
    }

    pub fn fire() -> Self {
        ParticleSpec { kind: ParticleType::Fire, color: [1.0, 0.55, 0.1, 1.0], ttl: 1.0, drag: 0.5, ..Self::regular() }
    }

    /// Dust kicked up when landing.
    pub fn dust() -> Self {
        ParticleSpec {
            kind: ParticleType::Smoke,
            color: [0.7, 0.65, 0.55, 0.7],
            ttl: 0.6,
            size: 0.5,
            drag: 4.0,
            collides: false,
            ..Self::regular()
        }
    }

    /// A water drop: falls, vanishes when it hits something.
    pub fn droplet() -> Self {
        ParticleSpec {
            kind: ParticleType::Regular,
            color: [0.55, 0.75, 1.0, 1.0],
            ttl: 0.9,
            size: 0.15,
            gravity: 1.0,
            ..Self::regular()
        }
    }

    /// A chip of a broken block (`DestructionParticle`): heavy, bounces, lives 6 s.
    pub fn debris(color: [f32; 3]) -> Self {
        ParticleSpec {
            kind: ParticleType::Regular,
            color: [color[0], color[1], color[2], 1.0],
            ttl: 6.0,
            size: 0.2,
            gravity: 1.0,
            bounce: 0.4,
            ground_friction: 0.3,
            scale_jitter: 0.5,
            cycle_distance: 0.06,
            ..Self::regular()
        }
    }

    /// Scale at birth: growing particles start at 0, the others at the Java 0.3.
    pub(crate) fn start_scale(&self) -> f32 {
        if self.kind.is_growing() {
            0.0
        } else {
            0.3
        }
    }
}
