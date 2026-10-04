//! The full-height barrier of `CLMovableEntity`: an indestructible obstacle stops movers at every
//! height, not only at the height they stand at. Caveland uses it to wall in the playable area.

use glam::Vec3;
use wurfel_sim::entity::{Component, Entity};
use wurfel_sim::{World, CHUNK_SIZE_Z};
use wurfel_sim::grid::from_iso;

use crate::blocks::ids;

/// Half the side of an entity's footprint (the engine probes the same corners).
const PROBE: f32 = 0.25;

/// Is there an indestructible obstacle anywhere in the block column `(x, y)`?
pub fn column_blocked(world: &World, x: i32, y: i32) -> bool {
    (0..CHUNK_SIZE_Z).any(|z| world.get(x, y, z).id() == ids::INDESTRUCTIBLE_OBSTACLE)
}

/// Does a footprint centred at `pos` touch a barrier column?
pub fn footprint_blocked(world: &World, pos: Vec3) -> bool {
    [(-PROBE, -PROBE), (PROBE, PROBE), (-PROBE, PROBE), (PROBE, -PROBE)].iter().any(|&(dx, dy)| {
        let (x, y) = from_iso(pos.x + dx, pos.y + dy);
        column_blocked(world, x, y)
    })
}

/// Component that gives an entity the barrier behaviour. Attach it to everything that extends
/// `CLMovableEntity` in Java: the player, collectibles, money.
pub struct ColumnBarrier;

impl Component for ColumnBarrier {
    fn update(&mut self, parent: &mut Entity, world: &World, dt: f32) -> bool {
        let position = parent.position;
        let Some(body) = parent.body.as_mut() else { return false };
        if !body.collider {
            return true;
        }
        let step = body.hor_movement() * dt;
        if step.length_squared() > 0.0 && footprint_blocked(world, position + step.extend(0.0)) {
            body.set_hor_movement(glam::Vec2::ZERO);
        }
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

    fn world() -> World {
        let mut world = World::new(AirGenerator);
        for x in 0..20 {
            for y in 0..80 {
                world.set(x, y, 0, Block::new(8, 0));
            }
        }
        world
    }

    #[test]
    fn a_barrier_high_up_still_stops_a_walker_on_the_floor() {
        let mut world = world();
        // Wall of barrier blocks only at height 6, nothing at the walker's height.
        for y in 36..44 {
            world.set(10, y, 6, Block::new(ids::INDESTRUCTIBLE_OBSTACLE, 0));
        }
        assert!(column_blocked(&world, 10, 40));
        assert!(!column_blocked(&world, 9, 40));

        let (gx, gy) = to_iso(5, 40);
        let mut entities = Entities::new();
        let mut e = Entity::new("walker", 1).movable().at(Vec3::new(gx, gy, 1.0));
        e.add_component(Box::new(ColumnBarrier));
        let id = entities.spawn(e);
        let (bx, by) = to_iso(10, 40);
        for _ in 0..240 {
            let body = entities.get_mut(id).unwrap().body.as_mut().unwrap();
            // Steady push towards the barrier column.
            body.set_hor_movement(glam::Vec2::new(bx - gx, by - gy).normalize() * 3.0);
            entities.update(&world, 1.0 / 60.0);
        }
        let p = entities.get(id).unwrap().position;
        let (x, _) = from_iso(p.x, p.y);
        assert!(x < 10, "got through the barrier column: {p:?}");
        assert!(p.distance(Vec3::new(bx, by, 1.0)) > 0.4, "{p:?}");
    }

    #[test]
    fn without_a_barrier_the_same_walk_gets_through() {
        let world = world();
        let (gx, gy) = to_iso(5, 40);
        let mut entities = Entities::new();
        let id = entities.spawn(Entity::new("walker", 1).movable().at(Vec3::new(gx, gy, 1.0)));
        let (bx, by) = to_iso(10, 40);
        for _ in 0..240 {
            let body = entities.get_mut(id).unwrap().body.as_mut().unwrap();
            body.set_hor_movement(glam::Vec2::new(bx - gx, by - gy).normalize() * 3.0);
            entities.update(&world, 1.0 / 60.0);
        }
        let (x, _) = from_iso(entities.get(id).unwrap().position.x, entities.get(id).unwrap().position.y);
        assert!(x >= 10);
    }
}
