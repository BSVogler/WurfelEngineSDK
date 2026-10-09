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
    /// The entity sprite the particle wears (Java `new Particle((byte) 22)`: a soft blob).
    pub sprite: u8,
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
            sprite: 22,
        }
    }

    pub fn smoke() -> Self {
        ParticleSpec { kind: ParticleType::Smoke, color: [0.45, 0.45, 0.45, 0.8], ttl: 2.5, drag: 0.5, ..Self::regular() }
    }

    pub fn fire() -> Self {
        ParticleSpec { kind: ParticleType::Fire, color: [1.0, 0.55, 0.1, 1.0], ttl: 1.0, drag: 0.5, ..Self::regular() }
    }

    /// The jetpack's flame (`Ejira`'s prototype particle): a yellow-orange fire particle, 70 % opaque,
    /// that lives 1.8 s.
    pub fn jetpack() -> Self {
        ParticleSpec { color: [1.0, 0.8, 0.2, 0.7], ttl: 1.8, collides: false, ..Self::fire() }
    }

    /// The sparks of a burning fuse (`TFlint`): bright pale yellow specks that live half a second. Not a
    /// fire particle: those start at size zero and fade to black, which made them too small and dim to see.
    pub fn sparkle() -> Self {
        ParticleSpec { kind: ParticleType::Regular, color: [8.0, 6.0, 2.5, 1.0], ttl: 0.5, size: 0.4, gravity: 1.0, collides: false, ..Self::regular() }
    }

    /// The smoke curling up from a burning fuse: grey, grows and fades over 2.5 s.
    pub fn fuse_smoke() -> Self {
        ParticleSpec { kind: ParticleType::Smoke, color: [0.6, 0.6, 0.6, 0.8], ttl: 2.5, size: 0.8, collides: false, ..Self::regular() }
    }

    /// What an explosion throws out (`Explosion`): a fire particle the colour of dust that lives 1.7 s.
    pub fn blast() -> Self {
        ParticleSpec { color: [0.6, 0.55, 0.4, 1.0], ttl: 1.7, size: 0.8, drag: 0.8, collides: false, ..Self::fire() }
    }

    /// A speck of dirt knocked off a block that cannot be broken by hand (`Ejira`): lives 0.4 s.
    pub fn dirt() -> Self {
        ParticleSpec { color: [0.45, 0.35, 0.25, 1.0], ttl: 0.4, size: 0.4, gravity: 1.0, ..Self::regular() }
    }

    /// The dust a walking player stirs up (`Ejira.step`): dull yellow-brown smoke, 0.7 s.
    pub fn footstep() -> Self {
        ParticleSpec { kind: ParticleType::Smoke, color: [0.4, 0.4, 0.05, 0.5], ttl: 0.7, size: 0.5, collides: false, ..Self::regular() }
    }

    /// The smoke over a burning oven (`OvenLogic`): grey-brown, 1 s.
    pub fn oven_smoke() -> Self {
        ParticleSpec { kind: ParticleType::Smoke, color: [0.5, 0.4, 0.3, 0.5], ttl: 1.0, size: 0.6, collides: false, ..Self::regular() }
    }

    /// The flash at a gun's muzzle: bright yellow, very short.
    pub fn muzzle() -> Self {
        ParticleSpec { color: [1.0, 0.9, 0.5, 1.0], ttl: 0.12, size: 0.5, collides: false, ..Self::fire() }
    }

    /// One dot of a bullet's trail: a pale yellow spark that fades fast.
    pub fn tracer() -> Self {
        ParticleSpec { color: [1.0, 0.95, 0.6, 1.0], ttl: 0.18, size: 0.25, collides: false, drag: 0.0, ..Self::fire() }
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

    /// A pebble of a broken block: bigger than the chips of a hit, bounces and rolls, stays 5 s.
    pub fn pebble(color: [f32; 3]) -> Self {
        ParticleSpec { ttl: 5.0, size: 0.32, bounce: 0.5, ground_friction: 0.4, ..Self::debris(color) }
    }

    /// A piece of a robot (`Robot` spawns `DestructionParticle`s of the sprites 34, 35 and 36): debris that
    /// shows its own picture instead of the soft blob.
    pub fn robot_piece(sprite: u8) -> Self {
        ParticleSpec { sprite, color: [1.0, 1.0, 1.0, 1.0], cycle_distance: 0.0, size: 0.35, ..Self::debris([1.0; 3]) }
    }

    /// The fire that lives on at the wreck of the intro ship (`Spaceship`): a fire emitter whose
    /// particles live 4 s.
    pub fn wreck() -> Self {
        ParticleSpec { ttl: 4.0, collides: false, ..Self::fire() }
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
