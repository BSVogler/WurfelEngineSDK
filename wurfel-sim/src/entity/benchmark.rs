//! `BenchmarkBall`: a bouncing ball for stress-testing the physics and the renderer, started by
//! the console command `benchmark`.
//!
//! Each ball hops around at random. In Java every ball also spawned further balls while the frame
//! rate stayed above 60 and the interval shrank over time, until the machine could not keep up.
//! A component cannot spawn entities, so that part is the [`BenchmarkSpawner`], which the host
//! updates once per frame and which hands back the next ball to add.

use glam::Vec3;

use super::{Component, Entity};
use crate::particle::Rng;
use crate::World;

/// Sprite id of the ball (Java: 5).
pub const SPRITE_ID: u8 = 5;
/// The Java ball was made to bounce with a random vertical speed up to this (blocks per second).
const MAX_HOP: f32 = 2.0;
/// Spawning only continues while a frame takes less than this (over 60 FPS, Java: 0.013 s).
pub const FAST_FRAME: f32 = 0.013;
/// Seconds between balls at the start. The Java timer ran on milliseconds from 1000 and
/// shrank by `dt / 5000000` each update, which is 1 s then.
const START_INTERVAL: f32 = 1.0;
/// How much shorter the interval gets per second of running (the Java factor, in seconds).
const SHRINK_PER_SECOND: f32 = 1.0 / 5000.0;

/// A new ball falling with a random horizontal drift. Not floating, so gravity pulls it down.
pub fn benchmark_ball(position: Vec3, rng: &mut Rng) -> Entity {
    let mut ball = Entity::new("Benchmark Ball", SPRITE_ID).movable().at(position);
    let body = ball.body.as_mut().expect("movable");
    body.set_movement(Vec3::new(rng.next_f32() - 0.5, rng.next_f32() - 0.5, -1.0));
    body.floating = false;
    ball.add_component(Box::new(Bounce { rng: rng.clone() }));
    ball
}

/// Hops again each time the ball lands, in a random direction and to a random height.
struct Bounce {
    rng: Rng,
}

impl Component for Bounce {
    fn update(&mut self, parent: &mut Entity, world: &World, _dt: f32) -> bool {
        if parent.is_on_ground(world) {
            let sideways = Vec3::new(self.rng.next_f32() - 0.5, self.rng.next_f32() - 0.5, 0.0);
            let hop = self.rng.next_f32() * MAX_HOP;
            if let Some(body) = parent.body.as_mut() {
                body.set_hor_movement(sideways.truncate());
                body.jump_with(hop);
            }
        }
        true
    }
}

/// Decides when the next ball is due (the static `timer` and `timeTillBall` of the Java class).
#[derive(Debug, Clone)]
pub struct BenchmarkSpawner {
    timer: f32,
    interval: f32,
    rng: Rng,
}

impl BenchmarkSpawner {
    pub fn new(seed: u64) -> Self {
        BenchmarkSpawner { timer: 0.0, interval: START_INTERVAL, rng: Rng::new(seed) }
    }

    /// Time between balls right now, in seconds.
    pub fn interval(&self) -> f32 {
        self.interval
    }

    /// Advance by `dt` seconds. `frame_time` is how long the last rendered frame took; a ball is
    /// only added while that is under [`FAST_FRAME`], so the load grows until the frame rate
    /// drops. `at` is where balls enter (the Java map centre, two blocks under the top).
    pub fn update(&mut self, dt: f32, frame_time: f32, at: Vec3) -> Option<Entity> {
        self.timer += dt;
        let mut ball = None;
        if self.timer > self.interval {
            if frame_time < FAST_FRAME {
                ball = Some(benchmark_ball(at, &mut self.rng));
            }
            self.timer = 0.0;
        }
        self.interval -= dt * SHRINK_PER_SECOND;
        ball
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block::id;
    use crate::entity::Entities;
    use crate::{Block, Generator};

    struct Floor;
    impl Generator for Floor {
        fn generate(&self, _x: i32, _y: i32, z: i32) -> Block {
            if z == 0 { Block::new(id::STONE, 0) } else { Block::AIR }
        }
    }

    #[test]
    fn a_ball_is_a_falling_movable_with_a_random_drift() {
        let ball = benchmark_ball(Vec3::new(2.0, 2.0, 10.0), &mut Rng::new(1));
        let body = ball.body.as_ref().unwrap();
        assert_eq!((ball.name.as_str(), ball.sprite_id), ("Benchmark Ball", SPRITE_ID));
        assert_eq!(body.movement.z, -1.0);
        assert!(body.movement.x.abs() <= 0.5 && body.movement.y.abs() <= 0.5);
        assert!(!body.floating);
    }

    #[test]
    fn a_ball_keeps_hopping_instead_of_coming_to_rest() {
        let world = World::new(Floor);
        let mut entities = Entities::new();
        let id = entities.spawn(benchmark_ball(Vec3::new(3.0, 3.0, 3.0), &mut Rng::new(7)));
        let mut airborne_after_landing = 0;
        let mut landed = false;
        for _ in 0..600 {
            entities.update(&world, 1.0 / 60.0);
            let ball = entities.get(id).unwrap();
            landed |= ball.is_on_ground(&world);
            if landed && !ball.is_on_ground(&world) {
                airborne_after_landing += 1;
            }
        }
        assert!(landed);
        assert!(airborne_after_landing > 30, "it left the ground again: {airborne_after_landing}");
    }

    #[test]
    fn the_spawner_adds_a_ball_per_interval_and_the_interval_shrinks() {
        let mut spawner = BenchmarkSpawner::new(3);
        let at = Vec3::new(1.0, 1.0, 30.0);
        assert!(spawner.update(0.5, 0.005, at).is_none());
        assert!(spawner.update(0.6, 0.005, at).is_some(), "1.1 s in");
        assert!(spawner.update(0.5, 0.005, at).is_none(), "the timer started over");
        let before = spawner.interval();
        spawner.update(10.0, 0.005, at);
        assert!(spawner.interval() < before);
    }

    #[test]
    fn no_ball_is_added_while_the_frame_rate_is_low_but_the_timer_still_resets() {
        let mut spawner = BenchmarkSpawner::new(3);
        let at = Vec3::ZERO;
        assert!(spawner.update(1.5, 0.05, at).is_none(), "20 FPS");
        assert!(spawner.update(0.5, 0.005, at).is_none(), "the slow frame used up the interval");
        assert!(spawner.update(0.6, 0.005, at).is_some());
    }
}
