//! The minecart (`MineCart`): rolls along rails, carries one passenger and up to five items.
//!
//! The rail geometry is in [`crate::rails`]. A cart that moves at all on a rail block is brought up
//! to full speed and keeps it until something stops it; off the rails it slows down. A player gets
//! in by using it and stays in until they jump out or leave it behind.

use glam::{Vec2, Vec3};
use wurfel_sim::entity::physics::block_at;
use wurfel_sim::entity::{Entities, EntityId};
use wurfel_sim::World;

use crate::blocks::ids;
use crate::collectible::{CollectibleType, Item};
use crate::game::Caveland;
use crate::rails::{self, EDGE2};
use crate::transport::{cell_at, hor_distance, Transport, TransportEvent};

/// Top speed on rails, blocks per second (`MAXSPEED`).
pub const MAX_SPEED: f32 = 6.0;
/// Speed on powered booster rails (`BOOSTERSPEED`).
pub const BOOSTER_SPEED: f32 = 20.0;
/// How high above the cart's floor a passenger sits: the bottom plate (`GAME_EDGELENGTH / 3`).
pub const BOTTOM_HEIGHT: f32 = 1.0 / 3.0;
/// How many items a cart carries.
pub const CAPACITY: usize = 5;
/// Friction on rails and off them (per millisecond).
const RAIL_FRICTION: f32 = 0.001;
const OFFROAD_FRICTION: f32 = 0.005;
/// Items this close, falling, are loaded: 80 units.
const LOAD_REACH: f32 = 80.0 / 141.421_36;
/// The cart pushes what is this far ahead of it.
const FRONT_REACH: f32 = 80.0 / 141.421_36;
const FRONT_RADIUS: f32 = EDGE2;
/// A passenger lets go when more than this far from the cart: a block sideways, half a block up.
const LEAVE_DISTANCE: f32 = 1.0;
const LEAVE_HEIGHT: f32 = EDGE2;
/// Tiny lift so a cart standing exactly on the floor of its block is not read as the block below.
const FLOOR_EPSILON: f32 = 1e-3;

/// The state of a `MineCart`.
#[derive(Debug, Default)]
pub struct MineCart {
    pub(crate) passenger: Option<EntityId>,
    content: Vec<Item>,
    /// The rolling sound has been started.
    sound_on: bool,
    /// Standing on a rail block (the cart's lamp is lit).
    pub(crate) on_rails: bool,
    pub(crate) last_position: Vec3,
}

impl MineCart {
    pub fn passenger(&self) -> Option<EntityId> {
        self.passenger
    }

    /// The items in the cart.
    pub fn content(&self) -> &[Item] {
        &self.content
    }

    /// Standing on rails: the cart's lamp burns.
    pub fn on_rails(&self) -> bool {
        self.on_rails
    }

    /// Put an item in. `false` if the cart is full (`add`).
    pub fn add(&mut self, item: Item) -> Result<(), Item> {
        if self.content.len() < CAPACITY {
            self.content.push(item);
            Ok(())
        } else {
            Err(item)
        }
    }

    /// Empty the cart (`getContent`).
    pub fn take_content(&mut self) -> Vec<Item> {
        std::mem::take(&mut self.content)
    }
}

impl Transport {
    /// A minecart on the floor at `position`. Its mass is 40 and it starts without moving.
    pub fn spawn_minecart(&mut self, entities: &mut Entities, position: Vec3) -> EntityId {
        let mut entity = wurfel_sim::entity::Entity::new("Minecart", 42).movable().at(position);
        entity.mass = 40.0;
        let id = entities.spawn(entity);
        self.carts.insert(id, MineCart { last_position: position, ..MineCart::default() });
        id
    }

    /// Get in (`interact`). Only when nobody is in it.
    pub(crate) fn board(&mut self, entities: &mut Entities, cart: EntityId, actor: EntityId) {
        if self.carts.get(&cart).is_none_or(|c| c.passenger.is_some()) || cart == actor {
            return;
        }
        if let Some(c) = self.carts.get_mut(&cart) {
            c.passenger = Some(actor);
        }
        self.center_passenger(entities, cart, false);
        self.notes.push(TransportEvent::Boarded { cart, passenger: actor });
    }

    /// Put the passenger in the middle of the cart; `force_height` also puts them on its floor
    /// (`centerPassenger`).
    fn center_passenger(&mut self, entities: &mut Entities, cart: EntityId, force_height: bool) {
        let Some(passenger) = self.carts.get(&cart).and_then(|c| c.passenger) else { return };
        let Some(at) = entities.get(cart).map(|e| e.position) else { return };
        if let Some(p) = entities.get_mut(passenger) {
            let z = if force_height { at.z + BOTTOM_HEIGHT } else { p.position.z };
            p.position = Vec3::new(at.x, at.y, z);
        }
    }

    fn passenger_leaves(&mut self, entities: &mut Entities, cart: EntityId) {
        if let Some(passenger) = self.carts.get_mut(&cart).and_then(|c| c.passenger.take()) {
            if let Some(body) = entities.get_mut(passenger).and_then(|e| e.body.as_mut()) {
                body.floating = false;
            }
            self.notes.push(TransportEvent::Left { cart, passenger });
        }
    }

    /// `MineCart.takeDamage` at the end of its life: iron comes out.
    pub(crate) fn cart_destroyed(&mut self, game: &mut Caveland, entities: &mut Entities, id: EntityId, cart: MineCart) {
        if let Some(passenger) = cart.passenger {
            if let Some(body) = entities.get_mut(passenger).and_then(|e| e.body.as_mut()) {
                body.floating = false;
            }
            self.notes.push(TransportEvent::Left { cart: id, passenger });
        }
        if cart.sound_on {
            self.notes.push(TransportEvent::SoundStopped { name: "wagon", entity: id });
        }
        self.sound("robot1destroy", cart.last_position);
        game.spawn_sparkling(entities, Item::new(CollectibleType::Iron), cart.last_position);
    }

    /// `MineCart.update` for every cart.
    pub(crate) fn update_carts(&mut self, game: &mut Caveland, entities: &mut Entities, world: &mut World) {
        let ids: Vec<EntityId> = self.carts.keys().copied().collect();
        for id in ids {
            let Some(entity) = entities.get(id) else {
                self.carts.remove(&id);
                continue;
            };
            let position = entity.position;
            if !world.is_loaded_at(cell_at(position).0, cell_at(position).1) {
                continue;
            }
            if let Some(cart) = self.carts.get_mut(&id) {
                cart.last_position = position;
            }

            let block = block_at(world, position + Vec3::Z * FLOOR_EPSILON);
            let rails = matches!(block.id(), ids::RAILS | ids::BOOSTER_RAILS);
            if rails {
                self.roll_on_rails(entities, world, id, block.value(), block.id() == ids::BOOSTER_RAILS);
            } else {
                let sound_on = self.carts.get(&id).is_some_and(|c| c.sound_on);
                if let Some(body) = entities.get_mut(id).and_then(|e| e.body.as_mut()) {
                    body.friction = OFFROAD_FRICTION;
                }
                if sound_on {
                    self.notes.push(TransportEvent::SoundStopped { name: "wagon", entity: id });
                    self.carts.get_mut(&id).expect("just found").sound_on = false;
                }
            }
            self.carts.get_mut(&id).expect("just found").on_rails = rails;

            self.update_cart_passenger(game, entities, id);
            self.push_in_front(entities, id);
        }
    }

    /// The part of `MineCart.update` that runs on a rail block.
    fn roll_on_rails(&mut self, entities: &mut Entities, world: &World, id: EntityId, value: u8, booster: bool) {
        // The block the cart is in: a cart resting on the floor may be a hair below it.
        let cell = cell_at(entities.get(id).expect("checked by the caller").position + Vec3::Z * FLOOR_EPSILON);
        let (centre, position) = {
            let e = entities.get(id).expect("checked by the caller");
            let (gx, gy) = wurfel_sim::grid::to_iso(cell.0, cell.1);
            (Vec2::new(gx, gy), e.position)
        };
        let Some(entity) = entities.get_mut(id) else { return };
        let Some(body) = entity.body.as_mut() else { return };
        body.friction = RAIL_FRICTION;

        // Back on the line the block describes, facing the way the cart rolls.
        if let Some(step) = rails::follow(value, centre, position.truncate(), body.hor_movement()) {
            body.set_orientation(step.orientation);
            entity.position = Vec3::new(step.position.x, step.position.y, position.z);
        }
        let body = entity.body.as_mut().expect("checked above");

        // Anything that moves at all is brought to full speed.
        let mut start_sound = false;
        if body.speed_hor() > 0.0 {
            if body.speed_hor() <= MAX_SPEED {
                body.set_speed_horizontal(MAX_SPEED);
            }
            if booster {
                // Powered rails shoot the cart on, unpowered ones stop it.
                body.set_speed_horizontal(if crate::power::booster_powered(world, cell) { BOOSTER_SPEED } else { 0.0 });
            }
            start_sound = body.speed_hor() > 0.0;
        }

        // A ramp launches a cart that drives up it and lets one roll back down.
        if let Some(launched) = rails::ramp_launch(value, body.movement) {
            body.movement = launched;
        }
        if let Some(facing) = rails::ramp_roll(value, body.hor_movement()) {
            body.set_orientation(facing);
            body.set_speed_horizontal(MAX_SPEED);
        }

        if start_sound {
            let at = entity.position;
            let cart = self.carts.get_mut(&id).expect("a cart");
            if !cart.sound_on {
                cart.sound_on = true;
                self.sound("wagon", at);
            }
        }
    }

    /// `updatePassenger`: the passenger rides at the cart's speed, or, with nobody in, the cart
    /// loads items that fall into it.
    fn update_cart_passenger(&mut self, game: &mut Caveland, entities: &mut Entities, id: EntityId) {
        let Some((cart_pos, cart_hor)) = entities.get(id).map(|e| (e.position, e.body.as_ref().map_or(Vec2::ZERO, |b| b.hor_movement()))) else {
            return;
        };
        let passenger = self.carts.get(&id).and_then(|c| c.passenger).filter(|&p| entities.get(p).is_some());
        let Some(passenger) = passenger else {
            self.carts.get_mut(&id).expect("a cart").passenger = None;
            // Load what falls in.
            for item_id in game.collectibles_for(entities, id, cart_pos, LOAD_REACH) {
                if self.carts[&id].content.len() >= CAPACITY {
                    break;
                }
                if let Some(item) = game.take_collectible(entities, item_id) {
                    self.carts.get_mut(&id).expect("a cart").content.push(item);
                }
            }
            return;
        };

        // Same speed as the cart.
        let (passenger_z, is_player) = {
            let p = entities.get_mut(passenger).expect("filtered above");
            if let Some(body) = p.body.as_mut() {
                body.set_hor_movement(cart_hor);
            }
            (p.position.z, game.player(passenger).is_some())
        };
        // Standing in the cart: held in it, and every jump out of it is a bunny hop.
        if passenger_z <= cart_pos.z + BOTTOM_HEIGHT {
            if let Some(body) = entities.get_mut(passenger).and_then(|e| e.body.as_mut()) {
                body.floating = true;
            }
            self.center_passenger(entities, id, true);
            if is_player {
                game.force_bunny_hop(passenger);
            }
        } else if let Some(body) = entities.get_mut(passenger).and_then(|e| e.body.as_mut()) {
            body.floating = false;
        }

        // Out of the cart: jumped high or left behind.
        let p = entities.get(passenger).expect("filtered above").position;
        if p.z - cart_pos.z > LEAVE_HEIGHT || hor_distance(cart_pos, p) > LEAVE_DISTANCE {
            self.passenger_leaves(entities, id);
        }
    }

    /// `checkCollisionInFront`: whatever is in the way is thrown aside.
    fn push_in_front(&mut self, entities: &mut Entities, id: EntityId) {
        let Some((position, movement, orientation)) =
            entities.get(id).and_then(|e| e.body.as_ref().map(|b| (e.position, b.movement, b.orientation())))
        else {
            return;
        };
        let speed = movement.length();
        if speed <= 0.0 {
            return;
        }
        let ahead = position + (orientation * FRONT_REACH).extend(0.0);
        let hit: Vec<EntityId> = entities
            .iter()
            .filter(|e| e.id() != id && e.body.is_some() && e.position.distance(ahead) < FRONT_RADIUS)
            .map(|e| e.id())
            .collect();
        for other in hit {
            let jitter = Vec3::new(self.rng.next_float() * 0.5 - 0.25, self.rng.next_float() * 0.5 - 0.25, self.rng.next_float());
            if let Some(body) = entities.get_mut(other).and_then(|e| e.body.as_mut()) {
                body.set_movement(Vec3::new(movement.x + jitter.x, movement.y + jitter.y, jitter.z));
                let along = body.speed_hor() + speed;
                body.set_speed_horizontal(along);
            }
        }
    }
}
