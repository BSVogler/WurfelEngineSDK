//! The player entity: the Caveland character's physical properties and how input drives it.
//!
//! Values come from Caveland's `Ejira` and the engine's CVars: 1.4 blocks tall, heavy (mass 60),
//! `playerfriction` 0.03, `playerWalkingSpeed` 4.0 and a jump speed of 4.7 blocks per second.

use glam::{Vec2, Vec3};
use serde::{Deserialize, Serialize};

use crate::entity::physics::ground_height;
use crate::entity::{Controllable, Entity};
use crate::grid::to_iso;
use crate::World;

pub const WALKING_SPEED: f32 = 4.0;
pub const JUMP_SPEED: f32 = 4.7;
pub const PLAYER_HEIGHT: f32 = 1.4;
pub const PLAYER_MASS: f32 = 60.0;
pub const PLAYER_FRICTION: f32 = 0.03;
/// Sprite id of the Caveland character.
pub const PLAYER_SPRITE: u8 = 30;

/// Physics runs at a fixed rate on the server and in the browser so that both compute the same
/// movement from the same input.
pub const TICK_RATE: u32 = 60;
pub const TICK_DT: f32 = 1.0 / TICK_RATE as f32;

/// What a player is asking for right now. Directions are screen directions.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlayerInput {
    pub up: bool,
    pub down: bool,
    pub left: bool,
    pub right: bool,
    pub jump: bool,
    /// Free camera: the exact walking direction on the screen (see [`heading_units`]), which
    /// replaces the eight directions of the keys while any of them is held. The keys stay set to the
    /// nearest of the eight, for everything that only knows keys (animation, Caveland).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub heading: Option<u16>,
}

/// A full turn in the units of [`PlayerInput::heading`] (0.35 degrees each).
pub const HEADING_STEPS: u16 = 1024;

/// A screen direction (x right, y towards the viewer) as a heading.
pub fn heading_units(screen: Vec2) -> u16 {
    let turns = screen.y.atan2(screen.x) / std::f32::consts::TAU;
    ((turns * HEADING_STEPS as f32).round() as i32).rem_euclid(HEADING_STEPS as i32) as u16
}

/// The screen direction of a heading.
pub fn heading_direction(units: u16) -> Vec2 {
    let angle = (units % HEADING_STEPS) as f32 / HEADING_STEPS as f32 * std::f32::consts::TAU;
    Vec2::new(angle.cos(), angle.sin())
}

/// A new player entity standing at `position` (feet).
pub fn new_player(position: Vec3) -> Entity {
    let mut entity = Entity::new("player", PLAYER_SPRITE).movable().at(position);
    entity.dimension_z = PLAYER_HEIGHT;
    entity.mass = PLAYER_MASS;
    let body = entity.body.as_mut().expect("movable");
    body.friction = PLAYER_FRICTION;
    body.jump_speed = Some(JUMP_SPEED);
    entity
}

/// Steer the entity for the next physics step.
pub fn apply_input(entity: &mut Entity, input: PlayerInput, world: &World) {
    let any_key = input.up || input.down || input.left || input.right;
    match input.heading {
        Some(units) if any_key => entity.walk_toward(heading_direction(units), WALKING_SPEED),
        _ => entity.walk(input.up, input.down, input.left, input.right, WALKING_SPEED),
    }
    if input.jump {
        entity.jump(world);
    }
}

/// Places to spawn players: the columns around the mountain peak, standing on the ground.
pub fn spawn_points(world: &World, peak: (i32, i32)) -> Vec<Vec3> {
    let mut points = Vec::new();
    for dx in -3..=3 {
        for dy in -3..=3 {
            let (x, y) = (peak.0 + dx, peak.1 + dy);
            let (gx, gy) = to_iso(x, y);
            points.push(Vec3::new(gx, gy, ground_height(world, x, y)));
        }
    }
    points
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::Entities;
    use crate::grid::from_iso;
    use crate::{IslandGenerator, World};

    fn world() -> World {
        World::new(IslandGenerator::new(1))
    }

    #[test]
    fn a_heading_survives_the_round_trip_and_replaces_the_key_directions() {
        for units in [0, 1, 100, 256, 777, HEADING_STEPS - 1] {
            assert_eq!(heading_units(heading_direction(units)), units);
        }
        let mut e = new_player(Vec3::ZERO);
        let heading = Some(heading_units(Vec2::new(1.0, 0.2)));
        apply_input(&mut e, PlayerInput { right: true, heading, ..Default::default() }, &world());
        let screen = crate::entity::iso_to_screen(e.body.as_ref().unwrap().hor_movement()).normalize();
        assert!((screen - Vec2::new(1.0, 0.2).normalize()).length() < 0.01, "{screen:?}");
        // Without a key the heading does nothing.
        let mut idle = new_player(Vec3::ZERO);
        apply_input(&mut idle, PlayerInput { heading, ..Default::default() }, &world());
        assert_eq!(idle.body.as_ref().unwrap().speed_hor(), 0.0);
    }

    #[test]
    fn spawn_points_are_inside_the_world_and_on_the_ground() {
        let world = world();
        let points = spawn_points(&world, IslandGenerator::new(1).peak());
        assert!(points.len() > 20);
        for p in points {
            let (x, y) = from_iso(p.x, p.y);
            assert_eq!(p.z, ground_height(&world, x, y));
        }
    }

    #[test]
    fn a_new_player_settles_on_the_ground_where_it_spawned() {
        let world = world();
        let spawn = spawn_points(&world, IslandGenerator::new(1).peak())[10];
        let mut entities = Entities::new();
        let id = entities.spawn(new_player(spawn));
        for _ in 0..60 {
            entities.update(&world, TICK_DT);
        }
        let p = entities.get(id).unwrap().position;
        assert_eq!(p.z, spawn.z, "standing still");
        assert!((p.x - spawn.x).abs() < 1e-4 && (p.y - spawn.y).abs() < 1e-4);
    }

    #[test]
    fn holding_a_key_walks_at_walking_speed_and_releasing_stops() {
        let world = World::new(crate::AirGenerator);
        let mut world = world;
        for x in -5..25 {
            for y in 0..60 {
                world.set(x, y, 0, crate::Block::new(8, 0));
            }
        }
        let (gx, gy) = to_iso(5, 20);
        let mut entities = Entities::new();
        let id = entities.spawn(new_player(Vec3::new(gx, gy, 1.0)));
        let start = entities.get(id).unwrap().position;

        let held = PlayerInput { right: true, ..Default::default() };
        for _ in 0..30 {
            apply_input(entities.get_mut(id).unwrap(), held, &world);
            entities.update(&world, TICK_DT);
        }
        let moved = (entities.get(id).unwrap().position - start).length();
        // Half a second at 4 blocks/s, minus the first step's friction.
        assert!((moved - 2.0).abs() < 0.15, "moved {moved}");

        for _ in 0..30 {
            entities.update(&world, TICK_DT);
        }
        assert_eq!(entities.get(id).unwrap().body.as_ref().unwrap().speed_hor(), 0.0);
    }

    #[test]
    fn jump_input_only_works_from_the_ground() {
        let world = World::new(crate::AirGenerator); // nothing to stand on at z > 0 except bounds floor
        let mut entities = Entities::new();
        let id = entities.spawn(new_player(Vec3::new(5.0, 5.0, 6.0))); // in the air
        let jump = PlayerInput { jump: true, ..Default::default() };
        apply_input(entities.get_mut(id).unwrap(), jump, &world);
        assert_eq!(entities.get(id).unwrap().body.as_ref().unwrap().movement.z, 0.0, "no mid-air jump");
    }
}
