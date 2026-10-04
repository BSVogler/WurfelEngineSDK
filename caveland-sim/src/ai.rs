//! `IdleAI`: wanders around its home position when there is nothing to do.

use glam::{Vec2, Vec3};
use wurfel_sim::entity::ai::MoveToAi;
use wurfel_sim::entity::physics::block_at;
use wurfel_sim::entity::{Component, Entity};
use wurfel_sim::generator::java_random::JavaRandom;
use wurfel_sim::World;

/// Seconds between picking new places to walk to.
const PAUSE: f32 = 1.5;
/// Tries to find a free spot before giving up until the next pause.
const ATTEMPTS: u32 = 100;
/// The radius around home: two blocks (`GAME_EDGELENGTH * 2`).
pub const DEFAULT_IDLE_RADIUS: f32 = 2.0;

pub struct IdleAi {
    home: Option<Vec3>,
    time_till_move: f32,
    pub idle_radius: f32,
    walking: Option<MoveToAi>,
    random: JavaRandom,
}

impl IdleAi {
    /// The Java class shares one `Random(1)` between all instances; each instance here has its own
    /// so the result does not depend on the order entities are updated in.
    pub fn new(seed: i64) -> Self {
        IdleAi { home: None, time_till_move: 0.0, idle_radius: DEFAULT_IDLE_RADIUS, walking: None, random: JavaRandom::new(seed) }
    }

    pub fn home(&self) -> Option<Vec3> {
        self.home
    }

    pub fn is_walking(&self) -> bool {
        self.walking.is_some()
    }

    fn pick_target(&mut self, parent: &Entity, world: &World, home: Vec3) -> Option<Vec3> {
        let floating = parent.body.as_ref().is_some_and(|b| b.floating);
        for _ in 0..ATTEMPTS {
            let (rx, ry) = (self.random.next_float() - 0.5, self.random.next_float() - 0.5);
            let target = if floating {
                let rz = self.random.next_float() - 0.5;
                home + Vec3::new(rx, ry, rz).normalize_or_zero() * self.idle_radius
            } else {
                home + Vec2::new(rx, ry).normalize_or_zero().extend(0.0) * self.idle_radius
            };
            if !world.blocks().is_obstacle(block_at(world, target)) {
                return Some(target);
            }
        }
        None
    }

    /// Advance by `dt`; call this every step while the entity has nothing better to do.
    pub fn step(&mut self, parent: &mut Entity, world: &World, dt: f32) {
        if parent.body.is_none() {
            return;
        }
        let home = *self.home.get_or_insert(parent.position);
        // Keep walking to the current goal first.
        if let Some(walk) = self.walking.as_mut() {
            if !walk.update(parent, world, dt) {
                self.walking = None;
            }
            self.time_till_move = (self.time_till_move - dt).max(0.0);
            return;
        }
        if self.time_till_move > 0.0 {
            self.time_till_move -= dt;
        }
        if self.time_till_move <= 0.0 {
            self.time_till_move = PAUSE;
            if let Some(target) = self.pick_target(parent, world, home) {
                self.walking = Some(MoveToAi::new(target));
            }
        }
    }
}

impl Component for IdleAi {
    fn update(&mut self, parent: &mut Entity, world: &World, dt: f32) -> bool {
        self.step(parent, world, dt);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wurfel_sim::block::Block;
    use wurfel_sim::entity::Entities;
    use wurfel_sim::grid::to_iso;
    use wurfel_sim::AirGenerator;

    fn floor_world() -> World {
        let mut world = World::new(AirGenerator);
        for x in -10..30 {
            for y in -10..90 {
                world.set(x, y, 0, Block::new(8, 0));
            }
        }
        world
    }

    fn walker(world: &World, seed: i64) -> (Entities, u32) {
        let (gx, gy) = to_iso(10, 40);
        let mut entities = Entities::new();
        let mut e = Entity::new("walker", 45).movable().at(Vec3::new(gx, gy, 1.0));
        e.add_component(Box::new(IdleAi::new(seed)));
        let id = entities.spawn(e);
        let _ = world;
        (entities, id)
    }

    #[test]
    fn an_idle_entity_wanders_but_stays_near_home() {
        let world = floor_world();
        let (mut entities, id) = walker(&world, 1);
        let home = entities.get(id).unwrap().position;
        let mut farthest = 0.0f32;
        let mut moved = false;
        for _ in 0..60 * 20 {
            entities.update(&world, 1.0 / 60.0);
            let p = entities.get(id).unwrap().position;
            farthest = farthest.max(p.distance(home));
            moved |= p.distance(home) > 0.5;
        }
        assert!(moved, "never left home");
        assert!(farthest < 3.5, "wandered {farthest} blocks away");
    }

    #[test]
    fn the_same_seed_gives_the_same_walk() {
        let world = floor_world();
        let run = |seed| {
            let (mut entities, id) = walker(&world, seed);
            for _ in 0..600 {
                entities.update(&world, 1.0 / 60.0);
            }
            entities.get(id).unwrap().position
        };
        assert_eq!(run(7), run(7));
        assert_ne!(run(7), run(8));
    }

    #[test]
    fn it_never_picks_a_target_inside_a_wall() {
        let mut world = floor_world();
        // A ring of stone one block around home would be hit often; fill everything but a corridor.
        for x in -10..30 {
            for y in -10..90 {
                world.set(x, y, 1, Block::new(3, 0));
                world.set(x, y, 2, Block::new(3, 0));
            }
        }
        let (gx, gy) = to_iso(10, 40);
        let mut ai = IdleAi::new(3);
        let e = Entity::new("w", 45).movable().at(Vec3::new(gx, gy, 3.0));
        for _ in 0..50 {
            if let Some(t) = ai.pick_target(&e, &world, e.position) {
                assert!(!world.blocks().is_obstacle(block_at(&world, t)));
            }
        }
    }
}
