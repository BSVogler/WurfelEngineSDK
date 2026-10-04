//! `MoveToAi`: walks an entity to a goal, jumping when it keeps running into a wall.

use glam::{Vec2, Vec3};

use super::{Component, Entity};
use crate::World;

/// Walks the parent to `goal` and removes itself on arrival.
pub struct MoveToAi {
    goal: Vec3,
    /// Never walk slower than this (blocks per second), so the entity does not stall.
    pub min_speed: f32,
    /// Seconds spent pushing against something without making progress.
    stuck_for: f32,
    last_position: Option<Vec3>,
}

/// After this long without progress the entity tries to jump.
const JUMP_AFTER_SECONDS: f32 = 0.5;

impl MoveToAi {
    pub fn new(goal: Vec3) -> Self {
        MoveToAi { goal, min_speed: 2.0, stuck_for: 0.0, last_position: None }
    }

    pub fn goal(&self) -> Vec3 {
        self.goal
    }

    /// Close enough to stop. Walkers only consider the horizontal distance, floaters all three.
    fn at_goal(&self, parent: &Entity, dt: f32) -> bool {
        let Some(body) = &parent.body else { return true };
        let distance_squared = if body.floating {
            parent.position.distance_squared(self.goal)
        } else {
            Vec2::new(parent.position.x, parent.position.y).distance_squared(Vec2::new(self.goal.x, self.goal.y))
        };
        // 20 game units squared, or less than one step away at the current speed.
        let step = body.speed() * dt;
        distance_squared < 20.0 * super::physics::UNIT * super::physics::UNIT || distance_squared < step * step
    }
}

impl Component for MoveToAi {
    fn update(&mut self, parent: &mut Entity, world: &World, dt: f32) -> bool {
        if parent.body.is_none() {
            return false;
        }
        if self.at_goal(parent, dt) {
            if let Some(body) = parent.body.as_mut() {
                body.set_speed_horizontal(0.0);
            }
            return false;
        }

        let position = parent.position;
        let body = parent.body.as_mut().expect("checked above");
        let mut direction = self.goal - position;
        let speed = if body.floating {
            body.speed()
        } else {
            direction.z = 0.0;
            body.speed_hor()
        };
        let mut movement = direction.normalize_or_zero() * speed.max(self.min_speed);
        if !body.floating {
            movement.z = body.movement.z; // keep falling or rising as before
        }
        body.set_movement(movement);

        // Not floating and not getting anywhere: probably a step in the way, so jump.
        if !body.floating {
            if Some(position) == self.last_position && body.speed() > 0.0 {
                self.stuck_for += dt;
            } else {
                self.stuck_for = 0.0;
                self.last_position = Some(position);
            }
            if self.stuck_for > JUMP_AFTER_SECONDS {
                self.stuck_for = 0.0;
                parent.jump(world);
            }
        }
        true
    }
}
