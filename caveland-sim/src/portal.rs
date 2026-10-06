//! Portals (`Portal`, `ExitPortal`) and the hole to the caves (`CaveEntryBlockLogic`).
//!
//! A [`Portal`] moves everything that falls into its block to its target cell. An exit portal is
//! the one at the other end: it is closed (it does not teleport by itself, so nobody bounces back
//! and forth) and a player can use it to go back. The caves' exit portals can also spawn robots.

use glam::Vec3;
use wurfel_sim::block::Block;
use wurfel_sim::entity::{Entities, EntityId};
use wurfel_sim::World;

use crate::blocks::ids;
use crate::game::{cell_floor, Caveland, Cell};
use crate::team::Team;
use crate::transport::{cell_at, hor_distance, Transport};

/// Only things in the lowest part of the portal's block are teleported (10 units).
const TELEPORT_HEIGHT: f32 = 10.0 / 141.421_36;
/// Seconds between two robot waves of an enemy spawner (`spawnCooldown += 5000`).
const SPAWN_INTERVAL: f32 = 5.0;
/// Robots an enemy spawner keeps alive.
const MAX_SPAWNED: usize = 3;
/// No robots appear while a player is this close, in blocks (`GAME_EDGELENGTH * 13`).
const SPAWN_SAFE_DISTANCE: f32 = 13.0;

struct Spawner {
    robots: Vec<EntityId>,
    cooldown: f32,
}

/// A `Portal` or `ExitPortal`.
pub struct Portal {
    /// The block it sits in.
    pub cell: Cell,
    /// Where it leads: the entity ends up on the floor of this cell.
    pub target: Cell,
    /// Is it open? A closed portal teleports nothing.
    pub active: bool,
    /// An exit portal (`ExitPortal`): visible, closed by default, and usable by hand.
    pub exit: bool,
    spawner: Option<Spawner>,
}

impl Portal {
    /// Can a player use it? An exit portal that has a lift behind it is used through the lift
    /// (the sprite shows the lift then).
    pub fn interactable(&self, world: &World) -> bool {
        self.exit && world.get(self.target.0, self.target.1, self.target.2).id() != ids::LIFT
    }

    /// Does it send out robots (`enableEnemySpawner`)?
    pub fn is_spawner(&self) -> bool {
        self.spawner.is_some()
    }
}

/// The logic of a `CaveEntryBlockLogic`: the portal that sits in the hole.
#[derive(Default)]
pub struct EntryLogic {
    pub(crate) portal: Option<EntityId>,
}

impl EntryLogic {
    /// The hole can be used while nothing is built above it (`interactable`).
    pub fn interactable(&self, world: &World, cell: Cell) -> bool {
        world.get(cell.0, cell.1, cell.2 + 1).is_air()
    }

    /// The portal in the hole, once it exists.
    pub fn portal(&self) -> Option<EntityId> {
        self.portal
    }
}

/// The cell at the bottom of the shaft: `getGround`, the first obstacle at or below `cell`.
pub fn ground_below(world: &World, cell: Cell) -> Cell {
    let mut z = cell.2;
    while z > 0 && !world.blocks().is_obstacle(world.get(cell.0, cell.1, z)) {
        z -= 1;
    }
    (cell.0, cell.1, z)
}

impl Transport {
    /// Put a portal in a block. A plain portal is invisible and open; an exit portal is visible
    /// and closed.
    pub fn spawn_portal(&mut self, entities: &mut Entities, cell: Cell, target: Cell, exit: bool) -> EntityId {
        let mut entity = wurfel_sim::entity::Entity::new(if exit { "Exit Portal" } else { "Portal" }, if exit { 15 } else { 0 })
            .at(cell_floor(cell));
        entity.indestructible = true;
        if exit {
            entity.sprite_value = 1;
        }
        let id = entities.spawn(entity);
        self.portals.insert(id, Portal { cell, target, active: !exit, exit, spawner: None });
        id
    }

    /// Let an exit portal send robots into its cave (`enableEnemySpawner`).
    pub fn enable_enemy_spawner(&mut self, portal: EntityId) {
        if let Some(p) = self.portals.get_mut(&portal) {
            p.spawner.get_or_insert(Spawner { robots: Vec::new(), cooldown: 0.0 });
        }
    }

    /// The exit portal at the other end of `portal`, made if missing, and set to lead back to the
    /// block above this portal (`Portal.getExitPortal`).
    pub fn exit_portal_of(&mut self, entities: &mut Entities, portal: EntityId) -> Option<EntityId> {
        let (cell, target) = self.portals.get(&portal).map(|p| (p.cell, p.target))?;
        let back = (cell.0, cell.1, cell.2 + 1);
        let existing = self.portals.iter().find(|(_, p)| p.exit && p.cell == target).map(|(&id, _)| id);
        let exit = match existing {
            Some(id) => id,
            None => self.spawn_portal(entities, target, (0, 0, wurfel_sim::caveland::HEIGHT - 1), true),
        };
        if let Some(p) = self.portals.get_mut(&exit) {
            if p.target != back {
                p.target = back;
            }
        }
        Some(exit)
    }

    /// The nearest portal within `radius` blocks horizontally of `position`.
    pub(crate) fn nearest_portal(&self, entities: &Entities, position: Vec3, radius: f32, exit_only: bool) -> Option<EntityId> {
        self.portals
            .iter()
            .filter(|(_, p)| !exit_only || p.exit)
            .filter_map(|(&id, _)| entities.get(id).map(|e| (id, hor_distance(e.position, position))))
            .filter(|&(_, d)| d <= radius)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(id, _)| id)
    }

    /// `CaveEntryBlockLogic.update`: keep a portal in every hole, open while nothing covers it.
    pub(crate) fn update_entries(&mut self, entities: &mut Entities, world: &mut World) {
        let cells: Vec<Cell> = self.entries.keys().copied().collect();
        for cell in cells {
            if !world.is_loaded_at(cell.0, cell.1) {
                continue;
            }
            let block = world.get(cell.0, cell.1, cell.2);
            if block.id() != ids::ENTRY {
                if let Some(entry) = self.entries.remove(&cell) {
                    self.drop_entry_portal(entities, entry);
                }
                continue;
            }

            // Find or make the portal inside the block.
            let known = self.entries.get(&cell).and_then(|e| e.portal).filter(|&p| entities.get(p).is_some());
            let portal = known
                .or_else(|| self.portals.iter().find(|(_, p)| !p.exit && p.cell == cell).map(|(&id, _)| id))
                .unwrap_or_else(|| {
                    // The way down: cave 0 from outside, otherwise the next cave.
                    let number = wurfel_sim::caveland::cave_number(cell.0, cell.1);
                    let target = wurfel_sim::caveland::cave_up(if number < 0 { 0 } else { number + 1 });
                    self.spawn_portal(entities, cell, target, false)
                });
            let open = self.entries.get(&cell).is_some_and(|e| e.interactable(world, cell));
            if let Some(entry) = self.entries.get_mut(&cell) {
                entry.portal = Some(portal);
            }
            if let Some(p) = self.portals.get_mut(&portal) {
                p.active = open;
            }
            // Covered, the hole is shut for falling into it (block value 1).
            let value = if open { 0 } else { 1 };
            if block.value() != value {
                world.set(cell.0, cell.1, cell.2, Block::new(ids::ENTRY, value));
            }
        }
    }

    fn drop_entry_portal(&mut self, entities: &mut Entities, entry: EntryLogic) {
        if let Some(portal) = entry.portal {
            self.portals.remove(&portal);
            if let Some(e) = entities.get_mut(portal) {
                e.dispose();
            }
        }
    }

    /// `Portal.update` and `ExitPortal.update`.
    pub(crate) fn update_portals(&mut self, game: &mut Caveland, entities: &mut Entities, world: &mut World, dt: f32) {
        let ids: Vec<EntityId> = self.portals.keys().copied().collect();
        for id in ids {
            if entities.get(id).is_none() {
                self.portals.remove(&id);
                continue;
            }
            let (cell, target, active, exit) = {
                let p = &self.portals[&id];
                (p.cell, p.target, p.active, p.exit)
            };
            if !world.is_loaded_at(cell.0, cell.1) {
                continue;
            }

            // Everything solid in the lowest part of the block goes through.
            if active {
                let floor = cell_floor(cell).z;
                let movers: Vec<EntityId> = entities
                    .iter()
                    .filter(|e| e.id() != id && e.body.as_ref().is_some_and(|b| b.collider))
                    .filter(|e| cell_at(e.position) == cell && e.position.z <= floor + TELEPORT_HEIGHT)
                    .map(|e| e.id())
                    .filter(|&e| !self.is_carried(e))
                    .collect();
                for mover in movers {
                    self.teleport(entities, mover, target);
                }
            }

            if exit {
                // The sprite shows a lift when one is behind the portal.
                let value = u8::from(world.get(target.0, target.1, target.2).id() != ids::LIFT);
                if let Some(e) = entities.get_mut(id) {
                    e.sprite_value = value;
                }
                self.update_spawner(game, entities, id, dt);
            }
        }
    }

    /// `ExitPortal.update`'s enemy spawner: three robots in the cave, a new wave at most every five
    /// seconds, and never while a player is close.
    fn update_spawner(&mut self, game: &mut Caveland, entities: &mut Entities, id: EntityId, dt: f32) {
        let Some(position) = entities.get(id).map(|e| e.position) else { return };
        let player_near = entities
            .iter()
            .any(|e| game.player(e.id()).is_some() && e.position.distance(position) <= SPAWN_SAFE_DISTANCE);
        let Some(portal) = self.portals.get_mut(&id) else { return };
        let Some(spawner) = portal.spawner.as_mut() else { return };
        spawner.robots.retain(|&r| entities.get(r).is_some());
        if spawner.cooldown > 0.0 {
            spawner.cooldown -= dt;
        }
        if player_near || spawner.cooldown > 0.0 {
            return;
        }
        spawner.cooldown += SPAWN_INTERVAL;
        let cave = wurfel_sim::caveland::cave_number(portal.cell.0, portal.cell.1);
        if cave < 0 {
            return;
        }
        let (cx, cy, cz) = wurfel_sim::caveland::cave_center(cave);
        let missing = MAX_SPAWNED.saturating_sub(spawner.robots.len());
        let mut spawned = Vec::new();
        for _ in 0..missing {
            // `(int) (Math.random() * 4 - 2)` truncates towards zero. (The Java game makes every
            // second one a `SpiderRobot`, which is not ported; they are all plain robots here.)
            let dx = (self.rng.next_float() * 4.0 - 2.0) as i32;
            let dy = (self.rng.next_float() * 4.0 - 2.0) as i32;
            spawned.push(game.spawn_robot(entities, Team::Robots, cell_floor((cx + dx, cy + dy, cz + 2))));
        }
        if let Some(spawner) = self.portals.get_mut(&id).and_then(|p| p.spawner.as_mut()) {
            spawner.robots.extend(spawned);
        }
    }
}

impl Caveland {
    /// The player confirmed the "construct a lift?" question of [`TransportEvent::LiftSiteOffered`]:
    /// put a construction site above the hole that becomes a lift. The site's logic (building
    /// material, progress) is the construction site's own, which reads
    /// [`Transport::site_result`] for what to turn into.
    ///
    /// `false` if the cell above the hole is not free any more.
    ///
    /// [`TransportEvent::LiftSiteOffered`]: crate::transport::TransportEvent::LiftSiteOffered
    pub fn confirm_lift_site(&mut self, world: &mut World, site: Cell) -> bool {
        if !world.get(site.0, site.1, site.2).is_air() || world.get(site.0, site.1, site.2 - 1).id() != ids::ENTRY {
            return false;
        }
        if !world.set(site.0, site.1, site.2, Block::new(ids::CONSTRUCTION_SITE, 0)) {
            return false;
        }
        self.transport_mut().site_results.insert(site, ids::LIFT);
        let center = crate::game::cell_center(site);
        self.transport_mut().sound("metallic", center);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wurfel_sim::AirGenerator;

    fn floor_world() -> World {
        let mut world = World::new(AirGenerator);
        Caveland::install(&mut world);
        for x in -5..20 {
            for y in -5..40 {
                world.set(x, y, 0, Block::new(ids::STONE, 0));
            }
        }
        world
    }

    #[test]
    fn the_ground_is_the_first_obstacle_downwards() {
        let mut world = floor_world();
        assert_eq!(ground_below(&world, (3, 4, 6)), (3, 4, 0));
        world.set(3, 4, 2, Block::new(ids::STONE, 0));
        assert_eq!(ground_below(&world, (3, 4, 6)), (3, 4, 2));
        assert_eq!(ground_below(&world, (3, 4, 2)), (3, 4, 2), "a solid cell is its own ground");
        // Rails are not an obstacle.
        world.set(3, 4, 2, Block::new(ids::RAILS, 0));
        assert_eq!(ground_below(&world, (3, 4, 6)), (3, 4, 0));
    }

    #[test]
    fn a_portal_with_a_lift_behind_it_cannot_be_used_by_hand() {
        let mut world = floor_world();
        let exit = Portal { cell: (1, 2, 1), target: (5, 6, 1), active: false, exit: true, spawner: None };
        assert!(exit.interactable(&world));
        world.set(5, 6, 1, Block::new(ids::LIFT, 0));
        assert!(!exit.interactable(&world));
        let entry = Portal { exit: false, ..exit };
        assert!(!entry.interactable(&world), "only exit portals are used by hand");
    }

    #[test]
    fn a_hole_is_usable_while_nothing_covers_it() {
        let mut world = floor_world();
        let entry = EntryLogic::default();
        assert!(entry.interactable(&world, (4, 4, 1)));
        world.set(4, 4, 2, Block::new(ids::STONE, 0));
        assert!(!entry.interactable(&world, (4, 4, 1)));
    }
}
