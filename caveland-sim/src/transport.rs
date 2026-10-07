//! Getting around: carts on rails, lifts, portals and the intro spaceship.
//!
//! [`Transport`] holds the state of everything that moves things from one place to another, the
//! way [`Caveland`] holds the rest of the game. It lives inside [`Caveland`] and is advanced by
//! [`Caveland::tick`]; the behaviour of each kind is in its own module:
//!
//! * [`crate::minecart`]: `MineCart` and its rails (rail geometry in [`crate::rails`]),
//! * [`crate::lift`]: `LiftBasket`, `LiftLogic` and `LiftLogicGround`,
//! * [`crate::portal`]: `Portal`, `ExitPortal` and `CaveEntryBlockLogic`,
//! * [`crate::spaceship`]: `Spaceship`.
//!
//! # Logic blocks
//!
//! The Java engine attaches a logic object to a block when the block is loaded or built and
//! updates it every frame. Here the blocks that need it (`LIFT`, `LIFT_GROUND`, `ENTRY`) are found
//! by looking through every chunk once when it loads ([`Transport::scan_chunk`]) and by
//! [`Caveland::block_changed`] when a block is placed. A logic whose block is gone is dropped the
//! next time its chunk is loaded and it is looked at.

use std::collections::{BTreeMap, BTreeSet};

use glam::{Vec2, Vec3};
use wurfel_sim::entity::{Entities, EntityId};
use wurfel_sim::generator::java_random::JavaRandom;
use wurfel_sim::generator::EntitySpawn;
use wurfel_sim::grid::from_iso;
use wurfel_sim::{World, CHUNK_SIZE_X, CHUNK_SIZE_Y, CHUNK_SIZE_Z};

use crate::blocks::ids;
use crate::game::{cell_center, cell_floor, Caveland, Cell, EntityKind, GameEvent};
use crate::lift::{GroundLogic, LiftBasket, LiftLogic};
use crate::minecart::MineCart;
use crate::player::INTERACT_RADIUS;
use crate::portal::{EntryLogic, Portal};
use crate::spaceship::Spaceship;

/// Chunks looked through for logic blocks per step.
const SCANS_PER_TICK: usize = 2;

/// Something that happened to a vehicle or a portal that clients and the server have to react to
/// and that is not a sound ([`GameEvent::Sound`] covers those).
#[derive(Debug, Clone, PartialEq)]
pub enum TransportEvent {
    /// An entity was moved somewhere else (portal, lift). A client that predicts that entity must
    /// jump to `to` instead of blending.
    Teleported { entity: EntityId, to: Vec3 },
    /// `passenger` climbed into `cart`.
    Boarded { cart: EntityId, passenger: EntityId },
    /// `passenger` left `cart`.
    Left { cart: EntityId, passenger: EntityId },
    /// The sound `name` that was started on `entity` (the rolling of a cart) stops.
    SoundStopped { name: &'static str, entity: EntityId },
    /// The intro spaceship hit the ground. Clients start the fire (`ParticleType.FIRE`).
    ShipCrashed { ship: EntityId, position: Vec3 },
    /// The ship has crashed for the first time: the map's save should remember it
    /// (`IntroCutsceneCompleted`).
    IntroCutsceneCompleted,
    /// `player` used a cave entry: the client asks "construct a lift construction site?" and, on
    /// yes, calls [`Caveland::confirm_lift_site`] with `site`.
    LiftSiteOffered { player: EntityId, entry: Cell, site: Cell },
}

/// What a player can use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Interaction {
    /// Get into a cart.
    Cart(EntityId),
    /// The lift block on the surface.
    LiftTop(Cell),
    /// The lift block in the cave.
    LiftGround(Cell),
    /// A portal that leads somewhere when used (an `ExitPortal` without a lift).
    ExitPortal(EntityId),
    /// The hole to the caves: offers to build a lift.
    Entry(Cell),
}

/// All vehicles, portals and lifts of a world.
pub struct Transport {
    pub(crate) carts: BTreeMap<EntityId, MineCart>,
    pub(crate) baskets: BTreeMap<EntityId, LiftBasket>,
    pub(crate) portals: BTreeMap<EntityId, Portal>,
    pub(crate) ships: BTreeMap<EntityId, Spaceship>,
    pub(crate) entries: BTreeMap<Cell, EntryLogic>,
    pub(crate) lifts: BTreeMap<Cell, LiftLogic>,
    pub(crate) grounds: BTreeMap<Cell, GroundLogic>,
    /// Entities that are inside something (a passenger in the spaceship): not drawn.
    pub(crate) hidden: BTreeSet<EntityId>,
    /// What a construction site at a cell is going to become (set by [`Caveland::confirm_lift_site`]).
    pub(crate) site_results: BTreeMap<Cell, u8>,
    pub(crate) rng: JavaRandom,
    pub(crate) game_events: Vec<GameEvent>,
    pub(crate) notes: Vec<TransportEvent>,
    scanned: BTreeSet<(i32, i32)>,
}

impl Default for Transport {
    fn default() -> Self {
        Transport::new(0)
    }
}

/// The block cell an entity at `position` is in.
pub(crate) fn cell_at(position: Vec3) -> Cell {
    let (x, y) = from_iso(position.x, position.y);
    (x, y, position.z.floor() as i32)
}

/// Horizontal distance in blocks (`distanceToHorizontal`); the same in both frames.
pub(crate) fn hor_distance(a: Vec3, b: Vec3) -> f32 {
    Vec2::new(a.x - b.x, a.y - b.y).length()
}

impl Transport {
    pub fn new(seed: i64) -> Self {
        Transport {
            carts: BTreeMap::new(),
            baskets: BTreeMap::new(),
            portals: BTreeMap::new(),
            ships: BTreeMap::new(),
            entries: BTreeMap::new(),
            lifts: BTreeMap::new(),
            grounds: BTreeMap::new(),
            hidden: BTreeSet::new(),
            site_results: BTreeMap::new(),
            rng: JavaRandom::new(seed),
            game_events: Vec::new(),
            notes: Vec::new(),
            scanned: BTreeSet::new(),
        }
    }

    /// What an entity is, if it is a vehicle or a portal.
    pub fn kind_of(&self, id: EntityId) -> Option<EntityKind> {
        if self.carts.contains_key(&id) {
            Some(EntityKind::MineCart)
        } else if self.baskets.contains_key(&id) {
            Some(EntityKind::LiftBasket)
        } else if let Some(portal) = self.portals.get(&id) {
            Some(if portal.exit { EntityKind::ExitPortal } else { EntityKind::Portal })
        } else if self.ships.contains_key(&id) {
            Some(EntityKind::Spaceship)
        } else {
            None
        }
    }

    /// Everything to draw: not the plain portals, which are invisible.
    pub fn things(&self) -> Vec<(EntityId, EntityKind)> {
        let mut things: Vec<_> = self
            .carts
            .keys()
            .map(|&id| (id, EntityKind::MineCart))
            .chain(self.baskets.keys().map(|&id| (id, EntityKind::LiftBasket)))
            .chain(self.portals.iter().filter(|(_, p)| p.exit).map(|(&id, _)| (id, EntityKind::ExitPortal)))
            .chain(self.ships.keys().map(|&id| (id, EntityKind::Spaceship)))
            .collect();
        things.sort_unstable_by_key(|&(id, _)| id);
        things
    }

    /// Is the entity somewhere it cannot be seen (a passenger of the spaceship)?
    pub fn is_hidden(&self, id: EntityId) -> bool {
        self.hidden.contains(&id)
    }

    pub fn cart(&self, id: EntityId) -> Option<&MineCart> {
        self.carts.get(&id)
    }

    pub fn basket(&self, id: EntityId) -> Option<&LiftBasket> {
        self.baskets.get(&id)
    }

    pub fn portal(&self, id: EntityId) -> Option<&Portal> {
        self.portals.get(&id)
    }

    pub fn ship(&self, id: EntityId) -> Option<&Spaceship> {
        self.ships.get(&id)
    }

    /// What the construction site at `cell` is going to turn into, if a lift was asked for.
    pub fn site_result(&self, cell: Cell) -> Option<u8> {
        self.site_results.get(&cell).copied()
    }

    /// Has the intro spaceship crashed?
    pub fn intro_crashed(&self) -> bool {
        self.ships.values().any(|s| s.crashed)
    }

    pub(crate) fn take_game_events(&mut self) -> Vec<GameEvent> {
        std::mem::take(&mut self.game_events)
    }

    pub(crate) fn take_notes(&mut self) -> Vec<TransportEvent> {
        std::mem::take(&mut self.notes)
    }

    pub(crate) fn sound(&mut self, name: &'static str, position: Vec3) {
        self.game_events.push(GameEvent::Sound { name, position });
    }

    // ---- finding logic blocks -------------------------------------------------------------

    /// Remember the logic block at `cell`, if it is one that needs updating.
    pub fn register(&mut self, world: &World, cell: Cell) {
        match world.get(cell.0, cell.1, cell.2).id() {
            ids::LIFT => {
                self.lifts.entry(cell).or_default();
            }
            ids::LIFT_GROUND => {
                self.grounds.entry(cell).or_default();
            }
            ids::ENTRY => {
                self.entries.entry(cell).or_default();
            }
            _ => {}
        }
    }

    /// Look through a loaded chunk for logic blocks.
    pub fn scan_chunk(&mut self, world: &World, chunk: (i32, i32)) {
        self.scanned.insert(chunk);
        let (ox, oy) = (chunk.0 * CHUNK_SIZE_X, chunk.1 * CHUNK_SIZE_Y);
        for x in ox..ox + CHUNK_SIZE_X {
            for y in oy..oy + CHUNK_SIZE_Y {
                for z in 0..CHUNK_SIZE_Z {
                    self.register(world, (x, y, z));
                }
            }
        }
    }

    /// Scan the chunks that are loaded and not yet looked at, a couple per step.
    fn scan_new_chunks(&mut self, world: &World) {
        let fresh: Vec<(i32, i32)> = world
            .loaded_chunks()
            .map(|c| c.pos())
            .filter(|pos| !self.scanned.contains(pos))
            .take(SCANS_PER_TICK)
            .collect();
        for chunk in fresh {
            self.scan_chunk(world, chunk);
        }
    }

    // ---- the step -------------------------------------------------------------------------

    /// Advance everything by `dt` seconds. Runs after the engine's entity update.
    pub(crate) fn update(&mut self, game: &mut Caveland, entities: &mut Entities, world: &mut World, dt: f32) {
        self.scan_new_chunks(world);
        self.update_entries(entities, world);
        self.update_portals(game, entities, world, dt);
        self.update_lifts(entities, world);
        self.update_grounds(game, entities, world);
        self.update_baskets(entities, world);
        self.update_carts(game, entities, world);
        self.update_ships(game, entities, world);
    }

    /// An entity that was part of a vehicle was disposed (health ran out).
    pub(crate) fn on_disposed(&mut self, game: &mut Caveland, entities: &mut Entities, id: EntityId) {
        self.hidden.remove(&id);
        if let Some(cart) = self.carts.remove(&id) {
            self.cart_destroyed(game, entities, id, cart);
        }
        self.baskets.remove(&id);
        self.portals.remove(&id);
        self.ships.remove(&id);
        for cart in self.carts.values_mut() {
            if cart.passenger == Some(id) {
                cart.passenger = None;
            }
        }
        for basket in self.baskets.values_mut() {
            if basket.passenger == Some(id) {
                basket.passenger = None;
            }
        }
    }

    // ---- teleporting ----------------------------------------------------------------------

    /// Put an entity and what it carries on the floor of `target` (`Events.teleport`).
    pub(crate) fn teleport(&mut self, entities: &mut Entities, id: EntityId, target: Cell) {
        let to = cell_floor(target);
        self.move_with_riders(entities, id, to);
        self.notes.push(TransportEvent::Teleported { entity: id, to });
    }

    /// Set the position of an entity, and carry its passenger along: a cart's rider sits a little
    /// above the cart's floor, a basket's rider in the basket.
    pub(crate) fn move_with_riders(&mut self, entities: &mut Entities, id: EntityId, to: Vec3) {
        if let Some(entity) = entities.get_mut(id) {
            entity.position = to;
        }
        let rider = self
            .carts
            .get(&id)
            .and_then(|c| c.passenger.map(|p| (p, to + Vec3::Z * crate::minecart::BOTTOM_HEIGHT)))
            .or_else(|| self.baskets.get(&id).and_then(|b| b.passenger.map(|p| (p, to))));
        if let Some((passenger, at)) = rider {
            if passenger != id {
                self.move_with_riders(entities, passenger, at);
            }
        }
    }

    /// How fast the platform under `id` moves (the cart or lift basket that carries them); zero when
    /// they stand on the ground.
    pub(crate) fn platform_velocity(&self, entities: &Entities, id: EntityId) -> Vec3 {
        let carrier = self
            .carts
            .iter()
            .find(|(_, c)| c.passenger == Some(id))
            .map(|(&cart, _)| cart)
            .or_else(|| self.baskets.iter().find(|(_, b)| b.passenger == Some(id)).map(|(&basket, _)| basket));
        carrier.and_then(|c| entities.get(c)).and_then(|e| e.body.as_ref()).map_or(Vec3::ZERO, |b| b.movement)
    }

    /// Is the entity currently carried by a cart or a basket?
    pub(crate) fn is_carried(&self, id: EntityId) -> bool {
        self.carts.values().any(|c| c.passenger == Some(id)) || self.baskets.values().any(|b| b.passenger == Some(id))
    }

    // ---- using things ---------------------------------------------------------------------

    /// The nearest thing the player at `position` can use, with its distance. Entities count by
    /// horizontal distance within the interaction radius, blocks by the distance to their middle
    /// (the nearest of [`Ejira.update`]'s "interactable focus").
    pub fn nearest_interactable(&self, entities: &Entities, world: &World, position: Vec3) -> Option<(f32, Interaction)> {
        let mut found: Vec<(f32, Interaction)> = Vec::new();
        for (&id, cart) in &self.carts {
            if cart.passenger.is_some() {
                continue;
            }
            if let Some(e) = entities.get(id) {
                let d = hor_distance(e.position, position);
                if d <= INTERACT_RADIUS {
                    found.push((d, Interaction::Cart(id)));
                }
            }
        }
        for (&id, portal) in &self.portals {
            if !portal.exit || !portal.interactable(world) {
                continue;
            }
            if let Some(e) = entities.get(id) {
                let d = hor_distance(e.position, position);
                if d <= INTERACT_RADIUS {
                    found.push((d, Interaction::ExitPortal(id)));
                }
            }
        }
        let reach = position + Vec3::Z * 0.5;
        let near_block = |cell: Cell| {
            let d = cell_center(cell).distance(reach);
            (d <= INTERACT_RADIUS).then_some(d)
        };
        for &cell in self.lifts.keys() {
            if let Some(d) = near_block(cell) {
                found.push((d, Interaction::LiftTop(cell)));
            }
        }
        for &cell in self.grounds.keys() {
            if let Some(d) = near_block(cell) {
                found.push((d, Interaction::LiftGround(cell)));
            }
        }
        for (&cell, entry) in &self.entries {
            if entry.interactable(world, cell) {
                if let Some(d) = near_block(cell) {
                    found.push((d, Interaction::Entry(cell)));
                }
            }
        }
        found.into_iter().min_by(|a, b| a.0.total_cmp(&b.0))
    }

    /// Where an [`Interaction`] is: the entity's position or the middle of the block.
    pub fn interaction_position(&self, entities: &Entities, what: Interaction) -> Option<Vec3> {
        match what {
            Interaction::Cart(id) | Interaction::ExitPortal(id) => entities.get(id).map(|e| e.position),
            Interaction::LiftTop(cell) | Interaction::LiftGround(cell) | Interaction::Entry(cell) => Some(cell_center(cell)),
        }
    }

    /// Use something (`Interactable.interact`).
    pub(crate) fn interact(&mut self, entities: &mut Entities, world: &mut World, actor: EntityId, actor_is_player: bool, what: Interaction) {
        match what {
            Interaction::Cart(id) => self.board(entities, id, actor),
            Interaction::ExitPortal(id) => {
                if let Some(target) = self.portals.get(&id).map(|p| p.target) {
                    self.teleport(entities, actor, target);
                }
            }
            Interaction::LiftTop(cell) => self.use_lift_top(entities, world, cell, actor),
            Interaction::LiftGround(cell) => self.use_lift_ground(entities, world, cell, actor),
            Interaction::Entry(cell) => {
                if actor_is_player {
                    self.notes.push(TransportEvent::LiftSiteOffered { player: actor, entry: cell, site: (cell.0, cell.1, cell.2 + 1) });
                }
            }
        }
    }

    // ---- spawning from map data -----------------------------------------------------------

    /// Create what the map generator asks for (`Generator.spawnEntities`). Returns `None` for kinds
    /// that are not transport.
    pub fn spawn_from_generator(&mut self, entities: &mut Entities, spawn: &EntitySpawn) -> Option<EntityId> {
        match spawn.kind {
            "ExitPortal" | "Portal" => {
                let exit = spawn.kind == "ExitPortal";
                let target = spawn.target.unwrap_or((0, 0, wurfel_sim::caveland::HEIGHT - 1));
                let id = self.spawn_portal(entities, spawn.cell, target, exit);
                if spawn.extras.iter().any(|(k, v)| *k == "enemy_spawner" && v == "true") {
                    self.enable_enemy_spawner(id);
                }
                Some(id)
            }
            "MineCart" => Some(self.spawn_minecart(entities, cell_floor(spawn.cell))),
            "Spaceship" => Some(self.spawn_spaceship(entities, cell_floor(spawn.cell))),
            _ => None,
        }
    }

    /// Where the selected portal leads (`PortalTargetCommand`; parsing the command is
    /// [`crate::commands`]'s). Only the first portal of the selection is changed, as in Java; `false`
    /// if none of `selected` is a portal.
    pub fn set_portal_target(&mut self, selected: &[EntityId], target: Cell) -> bool {
        for id in selected {
            if let Some(portal) = self.portals.get_mut(id) {
                portal.target = target;
                return true;
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_command_sets_the_first_selected_portal() {
        let mut transport = Transport::new(1);
        let mut entities = Entities::new();
        let portal = transport.spawn_portal(&mut entities, (1, 2, 3), (0, 0, 9), false);
        let other = transport.spawn_portal(&mut entities, (5, 5, 5), (0, 0, 9), false);
        assert!(transport.set_portal_target(&[7777, portal, other], (10, 20, 5)), "7777 is no portal and is skipped");
        assert_eq!(transport.portal(portal).unwrap().target, (10, 20, 5));
        assert_eq!(transport.portal(other).unwrap().target, (0, 0, 9), "only one portal changes");
    }

    #[test]
    fn nothing_changes_when_no_portal_is_selected() {
        let mut transport = Transport::new(1);
        let mut entities = Entities::new();
        let portal = transport.spawn_portal(&mut entities, (1, 2, 3), (0, 0, 9), true);
        assert!(!transport.set_portal_target(&[], (1, 1, 1)));
        assert!(!transport.set_portal_target(&[999], (1, 1, 1)));
        assert_eq!(transport.portal(portal).unwrap().target, (0, 0, 9));
        assert!(transport.set_portal_target(&[portal], (-4, 0, -1)), "exit portals count too");
        assert_eq!(transport.portal(portal).unwrap().target, (-4, 0, -1));
    }
}
