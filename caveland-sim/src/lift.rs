//! The lift: a basket (`LiftBasket`) that travels between a lift block on the surface
//! (`LiftLogic`) and a lift ground block in the cave (`LiftLogicGround`).
//!
//! The lift is built on top of a cave entry (a hole with a portal). While it stands, the portal of
//! the hole is closed, the cave's exit portal is made to lead back to the lift, and the cave floor
//! below that exit becomes a lift ground block. The basket takes a passenger down by being
//! teleported through the portal and up through the exit portal. A passenger can be a player or a
//! cart (the basket takes minecarts that roll into it).

use glam::Vec3;
use wurfel_sim::block::Block;
use wurfel_sim::entity::physics::block_at;
use wurfel_sim::entity::{screen_to_iso, Entities, EntityId};
use wurfel_sim::grid::{chunk_of, row_offset};
use wurfel_sim::{World, CHUNK_SIZE_Z};

use crate::blocks::ids;
use crate::game::{cell_center, cell_floor, Caveland, Cell};
use crate::portal::ground_below;
use crate::transport::{hor_distance, Transport};

/// Blocks per second the basket travels (`setMovement(new Vector3(0, 0, 4))`).
pub const BASKET_SPEED: f32 = 4.0;
/// Speed of the push that sends a passenger off when the basket stops.
const EJECT_SPEED: f32 = 4.0;
/// Above this height a basket in a cave has reached the ceiling and looks for the exit portal.
const CAVE_TOP: f32 = 9.0;
/// How far from a basket another basket or an exit portal is still "there" for the lift logic.
const NEAR: f32 = 1.0;
/// Reach for portals and carts near a basket (two and a half blocks).
const PORTAL_REACH: f32 = 2.0;
const CART_REACH: f32 = 0.5;

/// The state of a `LiftBasket`.
#[derive(Debug, Default)]
pub struct LiftBasket {
    /// 1 up, 0 standing, -1 down.
    pub(crate) dir: i8,
    pub(crate) passenger: Option<EntityId>,
    /// The one that just got out. It does not get back in until it has walked away.
    leaving: Option<EntityId>,
}

impl LiftBasket {
    /// `1` up, `0` standing still, `-1` down (`getMovementDir`).
    pub fn movement_dir(&self) -> i8 {
        self.dir
    }

    pub fn passenger(&self) -> Option<EntityId> {
        self.passenger
    }
}

/// A lift block on the surface (`LiftLogic`): which basket belongs to it.
#[derive(Debug, Default)]
pub struct LiftLogic {
    pub(crate) basket: Option<EntityId>,
}

impl LiftLogic {
    pub fn basket(&self) -> Option<EntityId> {
        self.basket
    }
}

/// A lift ground block in the cave (`LiftLogicGround`).
#[derive(Debug, Default)]
pub struct GroundLogic {
    pub(crate) basket: Option<EntityId>,
    /// Someone called the basket from the surface: where it waits, and who called.
    pub(crate) requested: Option<(Cell, EntityId)>,
}

/// Is the basket's shaft a lift on the surface? True when the column has a lift block anywhere
/// (`LiftBasket.isOnSurface`), false in a cave.
fn is_on_surface(world: &World, (x, y, _): Cell) -> bool {
    (0..CHUNK_SIZE_Z).any(|z| world.get(x, y, z).id() == ids::LIFT)
}

impl Transport {
    /// A basket at `position` (`new LiftBasket()`): floating, only goes up and down.
    pub fn spawn_basket(&mut self, entities: &mut Entities, position: Vec3) -> EntityId {
        let mut entity = wurfel_sim::entity::Entity::new("Lift Basket", 25).movable().at(position);
        entity.mass = 150.0;
        if let Some(body) = entity.body.as_mut() {
            body.floating = true;
        }
        let id = entities.spawn(entity);
        self.baskets.insert(id, LiftBasket::default());
        id
    }

    /// Put a passenger in the basket (`setPassenger`): the basket goes down from the surface and
    /// up from a cave. Not for the one that just got out.
    pub fn basket_set_passenger(&mut self, entities: &Entities, world: &World, basket: EntityId, passenger: EntityId) {
        let Some(position) = entities.get(basket).map(|e| e.position) else { return };
        let surface = is_on_surface(world, crate::transport::cell_at(position));
        let Some(b) = self.baskets.get_mut(&basket) else { return };
        if b.leaving != Some(passenger) {
            b.passenger = Some(passenger);
            b.dir = if surface { -1 } else { 1 };
        }
    }

    /// Tell a basket which way to go: 1 up, 0 stand, -1 down.
    pub fn basket_set_dir(&mut self, basket: EntityId, dir: i8) {
        if let Some(b) = self.baskets.get_mut(&basket) {
            b.dir = dir.clamp(-1, 1);
        }
    }

    /// `LiftBasket.stop`: the passenger gets a little push out, the basket stands still.
    fn basket_stop(&mut self, entities: &mut Entities, basket: EntityId) {
        let Some(b) = self.baskets.get_mut(&basket) else { return };
        if let Some(passenger) = b.passenger.take() {
            b.leaving = Some(passenger);
            if let Some(body) = entities.get_mut(passenger).and_then(|e| e.body.as_mut()) {
                body.set_hor_movement(screen_to_iso(glam::Vec2::new(-1.0, 1.0).normalize()) * EJECT_SPEED);
            }
        }
        b.dir = 0;
        if let Some(body) = entities.get_mut(basket).and_then(|e| e.body.as_mut()) {
            body.movement = Vec3::ZERO;
        }
    }

    /// `LiftBasket.update`.
    pub(crate) fn update_baskets(&mut self, entities: &mut Entities, world: &mut World) {
        let ids: Vec<EntityId> = self.baskets.keys().copied().collect();
        for id in ids {
            let Some(entity) = entities.get(id) else {
                self.baskets.remove(&id);
                continue;
            };
            let position = entity.position;
            let cell = crate::transport::cell_at(position);
            if !world.is_loaded_at(cell.0, cell.1) {
                continue;
            }

            // A passenger that has walked away may get in again.
            if let Some(left) = self.baskets[&id].leaving {
                if entities.get(left).is_none_or(|e| hor_distance(e.position, position) > NEAR) {
                    self.baskets.get_mut(&id).expect("just found").leaving = None;
                }
            }

            // It only moves up or down.
            let dir = self.baskets[&id].dir;
            if let Some(body) = entities.get_mut(id).and_then(|e| e.body.as_mut()) {
                body.set_hor_movement(glam::Vec2::ZERO);
                body.movement.z = f32::from(dir) * BASKET_SPEED;
            }
            if dir == 0 {
                self.basket_stop(entities, id);
            }

            let obstacle = world.blocks().is_obstacle(block_at(world, position));
            if is_on_surface(world, cell) {
                let dir = self.baskets[&id].dir;
                if dir > 0 && !obstacle {
                    // Arrived at the top.
                    self.basket_stop(entities, id);
                } else if dir < 0 && !obstacle {
                    // Down the hole: the portal below takes it.
                    if let Some(portal) = self.nearest_portal(entities, position, PORTAL_REACH, false) {
                        let target = self.portals[&portal].target;
                        self.teleport(entities, id, target);
                    }
                }
            } else if position.z > CAVE_TOP {
                // At the cave's ceiling: out through the exit portal, or back down.
                match self.nearest_portal(entities, position, PORTAL_REACH, true) {
                    Some(portal) => {
                        let target = self.portals[&portal].target;
                        self.teleport(entities, id, target);
                    }
                    None => self.baskets.get_mut(&id).expect("just found").dir = -1,
                }
            } else if self.baskets[&id].dir < 0 && entities.get(id).is_some_and(|e| e.is_on_ground(world)) {
                self.basket_stop(entities, id);
            }

            self.update_basket_passenger(entities, world, id);
        }
    }

    /// Hold the passenger in the basket while it moves; an empty basket takes a cart that rolls in.
    fn update_basket_passenger(&mut self, entities: &mut Entities, world: &World, id: EntityId) {
        let Some(position) = entities.get(id).map(|e| e.position) else { return };
        let basket = &self.baskets[&id];
        match basket.passenger {
            Some(passenger) => {
                if basket.dir != 0 {
                    self.move_with_riders(entities, passenger, position);
                    if let Some(body) = entities.get_mut(passenger).and_then(|e| e.body.as_mut()) {
                        body.set_hor_movement(glam::Vec2::ZERO);
                    }
                }
            }
            None => {
                let cart = self
                    .carts
                    .keys()
                    .copied()
                    .find(|&c| entities.get(c).is_some_and(|e| hor_distance(e.position, position) <= CART_REACH));
                if let Some(cart) = cart {
                    self.basket_set_passenger(entities, world, id, cart);
                }
            }
        }
    }

    /// `LiftLogic.update`, for every lift block: keep the entry's portal shut, the cave's exit
    /// portal pointing back, the cave floor a lift ground and a basket in the shaft.
    pub(crate) fn update_lifts(&mut self, entities: &mut Entities, world: &mut World) {
        let cells: Vec<Cell> = self.lifts.keys().copied().collect();
        for cell in cells {
            if !world.is_loaded_at(cell.0, cell.1) {
                continue;
            }
            if world.get(cell.0, cell.1, cell.2).id() != ids::LIFT {
                if let Some(lift) = self.lifts.remove(&cell) {
                    self.dispose_lift_basket(entities, lift);
                }
                continue;
            }
            let below = (cell.0, cell.1, cell.2 - 1);
            let Some(portal) = self.entries.get(&below).and_then(|e| e.portal).filter(|p| self.portals.contains_key(p)) else {
                continue; // a lift with no hole under it does nothing
            };
            if let Some(p) = self.portals.get_mut(&portal) {
                p.active = false; // carts and players are carried by the basket, they do not fall in
            }
            let Some(exit) = self.exit_portal_of(entities, portal) else { continue };

            let live = self.lifts[&cell].basket.filter(|&b| entities.get(b).is_some());
            let basket = live.unwrap_or_else(|| self.spawn_basket(entities, cell_floor(cell)));
            self.lifts.get_mut(&cell).expect("just found").basket = Some(basket);

            let exit_cell = self.portals[&exit].cell;
            let ground = ground_below(world, exit_cell);
            if world.get(ground.0, ground.1, ground.2).id() != ids::LIFT_GROUND
                && world.set(ground.0, ground.1, ground.2, Block::new(ids::LIFT_GROUND, 0))
            {
                self.grounds.entry(ground).or_default();
            }
        }
    }

    fn dispose_lift_basket(&mut self, entities: &mut Entities, lift: LiftLogic) {
        if let Some(basket) = lift.basket {
            self.baskets.remove(&basket);
            if let Some(e) = entities.get_mut(basket) {
                e.dispose();
            }
        }
    }

    /// `LiftLogicGround.update`, for every lift ground block.
    pub(crate) fn update_grounds(&mut self, game: &mut Caveland, entities: &mut Entities, world: &mut World) {
        let cells: Vec<Cell> = self.grounds.keys().copied().collect();
        for cell in cells {
            if !world.is_loaded_at(cell.0, cell.1) {
                continue;
            }
            if world.get(cell.0, cell.1, cell.2).id() != ids::LIFT_GROUND {
                self.grounds.remove(&cell);
                continue;
            }
            let here = cell_floor(cell);

            // It does not make a basket, it only notices one in the shaft.
            let near = self
                .baskets
                .keys()
                .copied()
                .find(|&b| entities.get(b).is_some_and(|e| hor_distance(e.position, here) <= NEAR));
            if let Some(near) = near {
                self.grounds.get_mut(&cell).expect("just found").basket = Some(near);
            }
            let basket = self.grounds[&cell].basket.filter(|b| entities.get(*b).is_some());
            self.grounds.get_mut(&cell).expect("just found").basket = basket;

            // A basket that comes down stops on the floor.
            if let Some(b) = basket {
                let arrived = self.baskets.get(&b).is_some_and(|l| l.dir < 0)
                    && entities.get(b).is_some_and(|e| e.is_on_ground(world) && hor_distance(e.position, here) < NEAR);
                if arrived {
                    self.baskets.get_mut(&b).expect("checked").dir = 0;
                }
                self.grounds.get_mut(&cell).expect("just found").requested = None;
            }

            // Someone asked for the basket and the lift is loaded: fetch it.
            if let Some((at, requestor)) = self.grounds[&cell].requested {
                if world.is_loaded_at(at.0, at.1) && world.get(at.0, at.1, at.2).id() == ids::LIFT {
                    let basket = match basket {
                        Some(b) => b,
                        None => self.spawn_basket(entities, cell_floor(at)),
                    };
                    self.grounds.get_mut(&cell).expect("just found").basket = Some(basket);
                    self.toggle_ground_basket(entities, world, cell, basket, requestor);
                    continue;
                }
            }

            // The way out must be free: dig away what blocks the cell above the neighbour.
            let odd = row_offset(cell.1);
            let beside = (cell.0 - 1 + odd, cell.1 + 1, cell.2 + 1);
            let block = world.get(beside.0, beside.1, beside.2);
            if !block.is_air() && world.blocks().is_obstacle(block) {
                game.damage_block(world, entities, beside, 100);
            }
        }
    }

    /// `LiftLogicGround.toggleBasket`: send the standing basket up with `actor`, or call it down.
    fn toggle_ground_basket(&mut self, entities: &mut Entities, world: &World, cell: Cell, basket: EntityId, actor: EntityId) {
        let here = cell_floor(cell);
        let near = entities.get(basket).is_some_and(|e| hor_distance(e.position, here) < NEAR);
        let dir = self.baskets.get(&basket).map_or(0, |b| b.dir);
        if dir == 0 && near {
            self.basket_set_dir(basket, 1);
            self.basket_set_passenger(entities, world, basket, actor);
        } else if dir >= 0 {
            self.basket_set_dir(basket, -1);
        }
    }

    /// Use the lift block on the surface (`LiftLogic.interact`).
    pub(crate) fn use_lift_top(&mut self, entities: &mut Entities, world: &World, cell: Cell, actor: EntityId) {
        let Some(basket) = self.lifts.get(&cell).and_then(|l| l.basket).filter(|b| entities.get(*b).is_some()) else {
            return;
        };
        let here = cell_floor(cell);
        let dir = self.baskets.get(&basket).map_or(0, |b| b.dir);
        if dir == 0 && entities.get(basket).is_some_and(|e| hor_distance(e.position, here) < NEAR) {
            self.basket_set_dir(basket, -1);
            self.basket_set_passenger(entities, world, basket, actor);
        } else if dir <= 0 {
            self.basket_set_dir(basket, 1);
        }
    }

    /// Use the lift ground block in a cave (`LiftLogicGround.interact`): ride an existing basket, or
    /// ask the surface for one.
    pub(crate) fn use_lift_ground(&mut self, entities: &mut Entities, world: &mut World, cell: Cell, actor: EntityId) {
        let here = cell_floor(cell);
        let basket = self.grounds.get(&cell).and_then(|g| g.basket).filter(|b| entities.get(*b).is_some());
        if let Some(basket) = basket {
            self.toggle_ground_basket(entities, world, cell, basket, actor);
            return;
        }
        if let Some(exit) = self.nearest_portal(entities, here, PORTAL_REACH, true) {
            let target = self.portals[&exit].target;
            if !world.is_loaded_at(target.0, target.1) {
                let (cx, cy) = chunk_of(target.0, target.1);
                world.load_chunk(cx, cy);
            }
            if let Some(ground) = self.grounds.get_mut(&cell) {
                ground.requested = Some((target, actor));
            }
            return;
        }
        self.sound("interactionFail", cell_center(cell));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wurfel_sim::AirGenerator;

    #[test]
    fn the_surface_is_where_a_lift_block_is_in_the_column() {
        let mut world = World::new(AirGenerator);
        assert!(!is_on_surface(&world, (3, 4, 5)));
        world.set(3, 4, 7, Block::new(ids::LIFT, 0));
        assert!(is_on_surface(&world, (3, 4, 0)), "any height in the column");
        assert!(!is_on_surface(&world, (3, 6, 7)), "another column is not");
    }

    #[test]
    fn a_new_basket_stands_and_has_nobody_in_it() {
        let basket = LiftBasket::default();
        assert_eq!(basket.movement_dir(), 0);
        assert_eq!(basket.passenger(), None);
    }
}
