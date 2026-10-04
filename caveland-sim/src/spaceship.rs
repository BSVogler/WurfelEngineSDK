//! The spaceship of the intro (`Spaceship`): it flies in, with the players on board, and crashes
//! near the start of the map.
//!
//! The game spawns it while the map's save has not seen the crash (`IntroCutsceneCompleted`),
//! calls [`Transport::enable_crash`] and puts the players in with [`Transport::add_ship_content`].
//! Passengers are carried, hidden, until the ship hits the ground; then the explosion goes off and
//! they get out.

use glam::Vec3;
use wurfel_sim::entity::{Entities, EntityId};
use wurfel_sim::World;

use crate::game::{cell_floor, Caveland, Cell};
use crate::transport::{hor_distance, Transport, TransportEvent};

/// Flight speed towards the crash site, blocks per second.
const FLIGHT_SPEED: f32 = 11.0;
/// The ship starts to fall this close (horizontally) to the crash site, in blocks.
const CRASH_DISTANCE: f32 = 25.0;
/// The first explosion in the air: only light and noise.
const AIR_EXPLOSION_RADIUS: i32 = 3;
/// The one on the ground.
const CRASH_RADIUS: i32 = 2;
const CRASH_DAMAGE: i32 = 100;

/// The state of a `Spaceship`.
#[derive(Debug, Default)]
pub struct Spaceship {
    /// It has hit the ground. Stays true: the cutscene happens once.
    pub crashed: bool,
    /// It has started falling.
    crashing: bool,
    target: Option<Cell>,
    content: Vec<EntityId>,
}

impl Spaceship {
    pub fn is_crashing(&self) -> bool {
        self.crashing
    }

    /// Who is on board.
    pub fn content(&self) -> &[EntityId] {
        &self.content
    }
}

impl Transport {
    /// A spaceship at `position`, with mass 1000. It does nothing until [`Transport::enable_crash`].
    pub fn spawn_spaceship(&mut self, entities: &mut Entities, position: Vec3) -> EntityId {
        let mut entity = wurfel_sim::entity::Entity::new("Spaceship", 80).movable().at(position);
        entity.mass = 1000.0;
        let id = entities.spawn(entity);
        self.ships.insert(id, Spaceship::default());
        id
    }

    /// Let the ship float towards `cell` and crash there (`enableCrash`).
    pub fn enable_crash(&mut self, entities: &mut Entities, ship: EntityId, cell: Cell) {
        if let Some(s) = self.ships.get_mut(&ship) {
            s.crashed = false;
            s.crashing = false;
            s.target = Some(cell);
        }
        if let Some(body) = entities.get_mut(ship).and_then(|e| e.body.as_mut()) {
            body.floating = true;
        }
    }

    /// Take someone on board (`addContent`): they ride in the ship, hidden.
    pub fn add_ship_content(&mut self, entities: &mut Entities, ship: EntityId, passenger: EntityId) {
        let Some(at) = entities.get(ship).map(|e| e.position) else { return };
        if let Some(s) = self.ships.get_mut(&ship) {
            if !s.content.contains(&passenger) {
                s.content.push(passenger);
            }
        }
        if let Some(p) = entities.get_mut(passenger) {
            p.position = at;
        }
        self.hidden.insert(passenger);
    }

    /// The first, harmless explosion and the fall (`startCrash`).
    fn start_crash(&mut self, entities: &mut Entities, ship: EntityId) {
        let Some(position) = entities.get(ship).map(|e| e.position) else { return };
        self.game_events.push(crate::game::GameEvent::Explosion { position, radius: AIR_EXPLOSION_RADIUS, damage: 0 });
        self.sound("explosion", position);
        if let Some(body) = entities.get_mut(ship).and_then(|e| e.body.as_mut()) {
            body.floating = false;
        }
        if let Some(s) = self.ships.get_mut(&ship) {
            s.crashing = true;
        }
    }

    /// `Spaceship.update`.
    pub(crate) fn update_ships(&mut self, game: &mut Caveland, entities: &mut Entities, world: &mut World) {
        let ids: Vec<EntityId> = self.ships.keys().copied().collect();
        for id in ids {
            let Some(position) = entities.get(id).map(|e| e.position) else {
                self.ships.remove(&id);
                continue;
            };
            let (crashed, crashing, target) = {
                let s = &self.ships[&id];
                (s.crashed, s.crashing, s.target)
            };

            // The passengers are in the ship. (Java lets them keep falling inside it; they are held
            // still here, so they do not step out at the speed of a long fall.)
            if !crashed {
                for rider in self.ships[&id].content.clone() {
                    if let Some(e) = entities.get_mut(rider) {
                        e.position = position;
                        if let Some(body) = e.body.as_mut() {
                            body.movement = Vec3::ZERO;
                        }
                    }
                }
            }

            // Fly to the crash site.
            if let (false, false, Some(cell)) = (crashed, crashing, target) {
                let mut direction = cell_floor(cell) - position;
                direction.z = 0.0;
                if let Some(body) = entities.get_mut(id).and_then(|e| e.body.as_mut()) {
                    body.set_movement(direction.normalize_or_zero() * FLIGHT_SPEED);
                }
            }
            if let (false, Some(cell)) = (crashing, target) {
                if hor_distance(cell_floor(cell), position) < CRASH_DISTANCE {
                    self.start_crash(entities, id);
                }
            }

            // On the ground: the big explosion, which the ship and its passengers survive.
            let now_crashing = self.ships[&id].crashing;
            if now_crashing && !crashed && entities.get(id).is_some_and(|e| e.is_on_ground(world)) {
                self.crash_on_ground(game, entities, world, id);
            }
        }
    }

    fn crash_on_ground(&mut self, game: &mut Caveland, entities: &mut Entities, world: &mut World, id: EntityId) {
        let Some(position) = entities.get(id).map(|e| e.position) else { return };
        let content = self.ships[&id].content.clone();
        let protect = |entities: &mut Entities, on: bool| {
            for who in std::iter::once(id).chain(content.iter().copied()) {
                if let Some(e) = entities.get_mut(who) {
                    e.indestructible = on;
                }
            }
        };
        protect(entities, true);
        game.explode(world, entities, position, CRASH_RADIUS, CRASH_DAMAGE);
        protect(entities, false);

        // The passengers get out.
        for rider in &content {
            self.hidden.remove(rider);
        }
        if let Some(s) = self.ships.get_mut(&id) {
            s.crashed = true;
        }
        self.notes.push(TransportEvent::ShipCrashed { ship: id, position });
        self.notes.push(TransportEvent::IntroCutsceneCompleted);
    }
}
