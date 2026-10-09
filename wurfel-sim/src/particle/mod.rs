//! Particle system, ported from the Java `Particle`, `ParticleType`, `ParticleEmitter` and
//! `DestructionParticle`.
//!
//! Particles are short-lived, purely visual bodies. They are not [`crate::entity::Entity`]s: the
//! Java particles were full `MovableEntity`s, which is too heavy for hundreds of specks, and they
//! never take part in game rules. Here they live in a flat pool ([`Particles`]) and are moved with
//! the same world queries as entity physics ([`crate::entity::physics`]).
//!
//! Differences from Java:
//! * time is in seconds (Java: milliseconds), speeds in blocks per second;
//! * collision tests the particle's centre only (entities test four footprint corners), resolved
//!   per axis so particles slide along walls and floors instead of the Java "undo the step";
//! * one pool with a capacity ([`Particles::new`]) replaces the per-emitter libGDX `Pool`; when it
//!   is full new particles are dropped;
//! * `DestructionParticle` behaviour (gravity, bounce, 6 s lifetime) is the
//!   [`ParticleSpec::debris`] preset; the sprite value cycling becomes a shade variant
//!   ([`Particle::variant`], see [`ParticleSpec::cycle_distance`]);
//! * randomness comes from an injectable [`Rng`] (Java used `Math.random` and a shared seeded
//!   `Random`), so effects can be tested deterministically.
//! * the emitter's point light is [`ParticleEmitter::light`], to be handed to the light engine.

mod emitter;
mod spec;

pub use emitter::{ParticleEmitter, LIGHT_COLOR, LIGHT_RADIUS};
pub use spec::{ParticleSpec, ParticleType};

use glam::Vec3;

use crate::entity::physics::{block_at, is_in_liquid, is_obstacle, GRAVITY};
use crate::entity::{Entities, Event};
use crate::World;

/// Default pool capacity.
pub const DEFAULT_CAPACITY: usize = 4096;

/// Small deterministic random number generator (xorshift64*), injectable for tests.
#[derive(Debug, Clone)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        // Zero is the one fixed point of xorshift.
        Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }

    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// Uniform in `[0, 1)`.
    pub fn next_f32(&mut self) -> f32 {
        (self.next_u64() >> 40) as f32 / (1u64 << 24) as f32
    }

    /// Uniform in `[-1, 1)`, the Java `(random - 0.5) * 2`.
    pub fn signed(&mut self) -> f32 {
        self.next_f32() * 2.0 - 1.0
    }

    pub fn bool(&mut self) -> bool {
        self.next_f32() >= 0.5
    }

    fn noise(&mut self, spread: Vec3) -> Vec3 {
        Vec3::new(self.signed() * spread.x, self.signed() * spread.y, self.signed() * spread.z)
    }
}

/// One particle.
#[derive(Debug, Clone, PartialEq)]
pub struct Particle {
    /// Isometric ground frame, blocks.
    pub position: Vec3,
    /// Blocks per second.
    pub velocity: Vec3,
    spec: ParticleSpec,
    /// Remaining lifetime in seconds.
    time_left: f32,
    /// Current colour (rgb) and opacity (a), after fading.
    color: [f32; 4],
    /// Degrees.
    rotation: f32,
    rotate_right: bool,
    scale: f32,
    /// Which of the [`SHADES`] the particle is drawn with (the Java sprite value).
    variant: u8,
    /// Distance travelled since the variant last changed, blocks.
    moved: f32,
}

/// Brightness factors of the three variants. In Java the sprite value picks one of three
/// differently shaded block sprites; without sprites the shade is the equivalent.
pub const SHADES: [f32; 3] = [1.0, 0.8, 0.62];

impl Particle {
    fn new(spec: &ParticleSpec, position: Vec3, velocity: Vec3, rotation: f32, rotate_right: bool) -> Self {
        Particle {
            position,
            velocity,
            spec: *spec,
            time_left: spec.ttl,
            color: spec.color,
            rotation,
            rotate_right,
            scale: spec.start_scale(),
            variant: 0,
            moved: 0.0,
        }
    }

    /// The entity sprite this particle is drawn with.
    pub fn sprite(&self) -> u8 {
        self.spec.sprite
    }

    pub fn kind(&self) -> ParticleType {
        self.spec.kind
    }

    /// `[r, g, b, a]` after fading and shading.
    pub fn color(&self) -> [f32; 4] {
        let shade = SHADES[self.variant as usize];
        [self.color[0] * shade, self.color[1] * shade, self.color[2] * shade, self.color[3]]
    }

    /// Index into [`SHADES`]; changes every [`ParticleSpec::cycle_distance`] blocks travelled for
    /// debris (Java `setSpriteValue(random * 3)`).
    pub fn variant(&self) -> u8 {
        self.variant
    }

    /// Degrees.
    pub fn rotation(&self) -> f32 {
        self.rotation
    }

    pub fn scale(&self) -> f32 {
        self.scale
    }

    /// Edge length in blocks as it should be drawn.
    pub fn size(&self) -> f32 {
        self.spec.size * self.scale
    }

    /// 1 when just born, 0 when about to die (`getPercentageOfLife`).
    pub fn life_fraction(&self) -> f32 {
        (self.time_left / self.spec.ttl).clamp(0.0, 1.0)
    }

    pub fn is_dead(&self) -> bool {
        self.time_left <= 0.0
    }

    /// Ageing and appearance, `Particle.update`.
    fn age(&mut self, dt: f32) {
        self.time_left -= dt;
        // 1/10 degree per millisecond.
        let turn = dt * 100.0;
        self.rotation += if self.rotate_right { -turn } else { turn };
        let kind = self.spec.kind;
        if kind.is_growing() {
            self.scale += dt / 0.8;
        }
        let t = self.life_fraction();
        if kind.fades() {
            self.color[3] = self.spec.color[3] * fade(t);
        }
        if kind.fades_to_black() {
            for c in 0..3 {
                self.color[c] = self.spec.color[c] * t;
            }
        }
    }

    /// Gravity, drag and block collision.
    fn step(&mut self, world: &World, dt: f32, rng: &mut Rng) {
        let spec = self.spec;
        let before = self.position;
        self.step_motion(world, dt, rng);
        // `DestructionParticle`: after every `rotateEachNMeters` of movement pick a new sprite value.
        if spec.cycle_distance > 0.0 {
            self.moved += (self.position - before).length();
            if self.moved > spec.cycle_distance {
                self.variant = (rng.next_f32() * SHADES.len() as f32) as u8 % SHADES.len() as u8;
                self.moved %= spec.cycle_distance;
            }
        }
    }

    fn step_motion(&mut self, world: &World, dt: f32, rng: &mut Rng) {
        let spec = self.spec;
        if spec.gravity != 0.0 {
            self.velocity.z -= GRAVITY * spec.gravity * dt;
        }
        if spec.drag > 0.0 {
            self.velocity *= 1.0 / (1.0 + spec.drag * dt);
        }
        if !spec.collides {
            self.position += self.velocity * dt;
            return;
        }
        // Per axis, so a particle falling onto a floor keeps its sideways motion.
        for axis in 0..3 {
            let mut next = self.position;
            next[axis] += self.velocity[axis] * dt;
            if is_obstacle(block_at(world, next)) {
                let v = self.velocity[axis];
                self.velocity[axis] = -v * spec.bounce;
                if axis == 2 && v < 0.0 {
                    // Landed: scrub speed off like friction would, and stop jittering.
                    let slow = 1.0 - spec.ground_friction;
                    self.velocity.x *= slow;
                    self.velocity.y *= slow;
                    if self.velocity.z.abs() < 0.2 {
                        self.velocity.z = 0.0;
                    }
                    // Debris tumbles on impact.
                    if spec.bounce > 0.0 {
                        self.rotate_right = rng.bool();
                    }
                }
            } else {
                self.position = next;
            }
        }
    }
}

/// Java `Interpolation.fade`: smootherstep.
pub fn fade(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}

/// All live particles of a game.
#[derive(Debug, Clone)]
pub struct Particles {
    list: Vec<Particle>,
    capacity: usize,
    rng: Rng,
}

impl Default for Particles {
    fn default() -> Self {
        Self::new(DEFAULT_CAPACITY, 1)
    }
}

impl Particles {
    pub fn new(capacity: usize, seed: u64) -> Self {
        Particles { list: Vec::with_capacity(capacity.min(DEFAULT_CAPACITY)), capacity, rng: Rng::new(seed) }
    }

    pub fn len(&self) -> usize {
        self.list.len()
    }

    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = &Particle> {
        self.list.iter()
    }

    pub fn clear(&mut self) {
        self.list.clear();
    }

    /// Spawn one particle with `velocity` plus uniform noise of `±spread` per axis. Returns false
    /// when the pool is full.
    pub fn spawn(&mut self, spec: &ParticleSpec, position: Vec3, velocity: Vec3, spread: Vec3) -> bool {
        if self.list.len() >= self.capacity {
            return false;
        }
        let velocity = velocity + self.rng.noise(spread);
        let rotation = self.rng.next_f32() * 360.0;
        let rotate_right = self.rng.bool();
        let mut particle = Particle::new(spec, position, velocity, rotation, rotate_right);
        if spec.scale_jitter > 0.0 {
            particle.scale *= 1.0 - self.rng.next_f32() * spec.scale_jitter;
        }
        self.list.push(particle);
        true
    }

    /// Spawn `count` particles at once. Returns how many fit.
    pub fn burst(&mut self, spec: &ParticleSpec, position: Vec3, count: usize, velocity: Vec3, spread: Vec3) -> usize {
        (0..count).filter(|_| self.spawn(spec, position, velocity, spread)).count()
    }

    /// Advance all particles and remove the dead ones (`Particle.update`).
    pub fn update(&mut self, world: &World, dt: f32) {
        let rng = &mut self.rng;
        for p in &mut self.list {
            p.age(dt);
            if !p.is_dead() {
                p.step(world, dt, rng);
            }
        }
        self.list.retain(|p| !p.is_dead());
    }

    /// Dust where something landed, a splash where it entered water. Collisions are not turned
    /// into particles (they happen every frame while walking along a wall).
    pub fn handle_events(&mut self, events: &[Event], entities: &Entities, world: &World) {
        for event in events {
            match *event {
                Event::Landed(id) => {
                    if let Some(entity) = entities.get(id) {
                        self.landing_dust(entity.position);
                    }
                }
                Event::EnteredLiquid(id) => {
                    if let Some(entity) = entities.get(id) {
                        // The entity may already be below the surface: start the splash at the
                        // top of the water.
                        let mut at = entity.position;
                        while is_in_liquid(world, at + Vec3::Z) {
                            at.z += 1.0;
                        }
                        at.z = at.z.floor() + 1.0;
                        self.splash(at);
                    }
                }
                Event::Collided(_) | Event::Disposed(_) => {}
            }
        }
    }

    /// A small ring of dust puffs at the feet.
    pub fn landing_dust(&mut self, feet: Vec3) {
        self.burst(&ParticleSpec::dust(), feet + Vec3::Z * 0.05, 6, Vec3::new(0.0, 0.0, 0.3), Vec3::new(0.8, 0.8, 0.2));
    }

    /// Droplets thrown up from the water surface.
    pub fn splash(&mut self, surface: Vec3) {
        self.burst(&ParticleSpec::droplet(), surface, 10, Vec3::new(0.0, 0.0, 2.5), Vec3::new(1.0, 1.0, 1.0));
    }

    /// Chips flying out of a destroyed block of the given colour (`DestructionParticle`).
    pub fn block_break(&mut self, block_center: Vec3, color: [f32; 3]) {
        let spec = ParticleSpec::debris(color);
        // Java: movement (rand - 0.5, rand - 0.5, rand * 5): up and a little sideways.
        self.burst(&spec, block_center, 8, Vec3::new(0.0, 0.0, 2.5), Vec3::new(1.0, 1.0, 2.5));
    }

    /// What a destroyed block leaves: a shower of pebbles of its colour thrown up and out, and a puff of
    /// dust. Fewer when the pool is getting full, so a big explosion (hundreds of blocks at once) stays
    /// affordable.
    pub fn rubble(&mut self, block_center: Vec3, color: [f32; 3]) {
        // Rubble only ever takes the lower half of the pool: the rest stays free for the effects that must
        // show (sparks, smoke, flames), which are dropped when the pool is full.
        let used = self.list.len();
        let (pebbles, dust) = if used >= self.capacity / 2 {
            return;
        } else if used >= self.capacity / 4 {
            (4, 0)
        } else {
            (14, 2)
        };
        self.burst(&ParticleSpec::pebble(color), block_center, pebbles, Vec3::new(0.0, 0.0, 3.0), Vec3::new(2.5, 2.5, 2.0));
        self.burst(&ParticleSpec::dust(), block_center, dust, Vec3::new(0.0, 0.0, 0.4), Vec3::new(0.6, 0.6, 0.2));
    }

    /// An explosion of the given radius in blocks (`Explosion.spawn`): Java put one fire-dust particle in
    /// every cell of the 2r x 4r x 2r volume, flying outwards from the centre at 4 blocks per second.
    /// The count is capped so a big blast does not empty the pool.
    pub fn explosion(&mut self, center: Vec3, radius: i32) {
        let r = radius.max(1) as f32;
        let count = ((2.0 * r * 4.0 * r * 2.0 * r) as usize).min(160);
        let spec = ParticleSpec::blast();
        for _ in 0..count {
            let offset = Vec3::new(self.rng.next_f32() - 0.5, self.rng.next_f32() - 0.5, self.rng.next_f32() - 0.5) * Vec3::new(2.0 * r, 2.0 * r, 2.0 * r);
            let direction = (offset + Vec3::Z * 0.01).normalize();
            self.spawn(&spec, center + offset * 0.2, direction * 4.0, Vec3::ZERO);
        }
    }

    /// A speck of dirt where a hit block did not give way (`Ejira`).
    pub fn dirt_kick(&mut self, at: Vec3) {
        self.burst(&ParticleSpec::dirt(), at, 3, Vec3::new(0.0, 0.0, 1.5), Vec3::new(2.5, 2.5, 1.0));
    }

    /// A puff of dust at a walking player's feet, drifting back against `movement` (`Ejira.step`).
    pub fn footstep(&mut self, feet: Vec3, movement: glam::Vec2) {
        let back = -movement * 0.1;
        let at = feet + Vec3::new(self.rng.next_f32() - 0.5, self.rng.next_f32() - 0.5, 0.0) * 0.4;
        self.spawn(&ParticleSpec::footstep(), at, Vec3::new(back.x, back.y, 0.2), Vec3::ZERO);
    }

    /// A flash at the muzzle and a trail of sparks along the shot from `from` to `to`.
    pub fn shot(&mut self, from: Vec3, to: Vec3) {
        self.burst(&ParticleSpec::muzzle(), from, 3, Vec3::ZERO, Vec3::splat(0.3));
        let length = from.distance(to);
        let steps = (length * 3.0).clamp(1.0, 60.0) as usize;
        for i in 1..=steps {
            self.spawn(&ParticleSpec::tracer(), from.lerp(to, i as f32 / steps as f32), Vec3::ZERO, Vec3::splat(0.05));
        }
        self.burst(&ParticleSpec::dirt(), to, 3, Vec3::new(0.0, 0.0, 1.0), Vec3::new(1.5, 1.5, 1.0));
    }

    /// The pieces of a destroyed robot (`Robot`: three `DestructionParticle`s).
    pub fn robot_break(&mut self, center: Vec3) {
        for sprite in [34, 35, 36] {
            self.burst(&ParticleSpec::robot_piece(sprite), center, 4, Vec3::new(0.0, 0.0, 2.5), Vec3::new(1.0, 1.0, 2.5));
        }
    }

    /// Smoke rising from a burning oven (`OvenLogic`).
    pub fn oven_smoke(&mut self, chimney: Vec3) {
        self.spawn(&ParticleSpec::oven_smoke(), chimney, Vec3::new(0.0, 0.0, 0.5), Vec3::new(0.1, 0.1, 0.1));
    }

    /// Spawn from an emitter: see [`ParticleEmitter::update`].
    pub fn rng(&mut self) -> &mut Rng {
        &mut self.rng
    }
}

#[cfg(test)]
mod tests;
