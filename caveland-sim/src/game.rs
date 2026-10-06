//! The Caveland ruleset: [`Caveland`] owns everything the engine's entity system does not know
//! about (who is a player, what a collectible is, what the ovens are doing) and advances it next to
//! [`Entities::update`].
//!
//! The engine's `Entities` stays generic. A game keeps per-entity data in a table keyed by
//! [`EntityId`] instead of subclassing, and calls [`Caveland::tick`] where a plain server would
//! call `entities.update`.

use std::collections::HashMap;
use std::sync::Arc;

use glam::{Vec2, Vec3};
use wurfel_sim::entity::physics::{is_on_ground, UNIT};
use wurfel_sim::entity::{Controllable, Entities, Entity, EntityId, Event};
use wurfel_sim::generator::java_random::JavaRandom;
use wurfel_sim::grid::{from_iso, to_iso};
use wurfel_sim::{Block, World};

use crate::ai::IdleAi;
use crate::barrier::ColumnBarrier;
use crate::blocks::{self, ids, CavelandBlocks};
use crate::collectible::{CollectibleType, Item};
use crate::crafting::{self, RecipeResult};
use crate::logic::{OvenEvent, OvenLogic};
use crate::player::*;
use crate::team::Team;
use crate::tuning::Tuning;
use crate::transport::{Interaction, Transport, TransportEvent};
use crate::extras::{Extras, Other};

/// A block cell `(x, y, z)`.
pub type Cell = (i32, i32, i32);

/// Seconds a freshly dropped collectible lies there before anyone, even its last holder, may take
/// it (`timeParentBlocked = 1500`). Nobody holds it at that point, so it only matters after a
/// throw.
const INITIAL_PICKUP_BLOCK: f32 = 1.5;
/// A robot can attack when it has charged this long.
const ROBOT_CHARGE_TIME: f32 = 1.0;
const ROBOT_ATTACK_TIME: f32 = 0.6;
const ROBOT_WALK_SPEED: f32 = 2.0;
const ROBOT_JUMP_SPEED: f32 = 5.0;
/// A robot looks for an enemy this far away (horizontally), in blocks: four diagonals.
const ROBOT_SIGHT: f32 = 4.0 * 1.414_213_6;
/// The robot walks towards its target until it is this close.
const ROBOT_STOP_DISTANCE: f32 = 1.5;
const ROBOT_ATTACK_RANGE: f32 = 2.0;
/// A cave is one block shorter than the world: the ceiling the player is kept below.
const CAVE_CEILING: f32 = (wurfel_sim::caveland::HEIGHT - 1) as f32;
/// Radius and damage of an exploding explosive (`new Explosion(3, 150, ...)`).
const EXPLOSIVE_RADIUS: i32 = 3;
const EXPLOSIVE_DAMAGE: i32 = 150;
/// How far above the cell's floor a thrown item starts.
const THROW_HEIGHT: f32 = 1.0;

/// What an entity is, for rendering and the network.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntityKind {
    Player { number: u8 },
    Collectible(CollectibleType),
    Money,
    Robot(Team),
    MineCart,
    LiftBasket,
    /// An invisible portal.
    Portal,
    ExitPortal,
    Spaceship,
    /// The gathering robot (`SpiderRobot`).
    SpiderRobot(Team),
    /// The flying robot (`Quadrocopter`).
    Drone(Team),
    Vanya,
    Shopkeeper,
    Bird,
    /// A flag of a team.
    Flag(Team),
    DropSpaceFlag,
}

impl EntityKind {
    /// The name the network and the client use for it: `money`, `robot`, an item's name (`Torch`)...
    pub fn name(&self) -> String {
        match self {
            EntityKind::Player { .. } => "player".to_string(),
            EntityKind::Collectible(kind) => kind.name().to_string(),
            EntityKind::Money => "money".to_string(),
            EntityKind::Robot(Team::Robots) => "robot".to_string(),
            EntityKind::Robot(_) => "friendly_robot".to_string(),
            EntityKind::MineCart => "minecart".to_string(),
            EntityKind::LiftBasket => "lift_basket".to_string(),
            EntityKind::Portal => "portal".to_string(),
            EntityKind::ExitPortal => "exit_portal".to_string(),
            EntityKind::Spaceship => "spaceship".to_string(),
            EntityKind::SpiderRobot(Team::Robots) => "spider_robot".to_string(),
            EntityKind::SpiderRobot(_) => "friendly_spider_robot".to_string(),
            EntityKind::Drone(Team::Robots) => "drone".to_string(),
            EntityKind::Drone(_) => "friendly_drone".to_string(),
            EntityKind::Vanya => "vanya".to_string(),
            EntityKind::Shopkeeper => "shopkeeper".to_string(),
            EntityKind::Bird => "bird".to_string(),
            EntityKind::Flag(Team::Neutral) => "flag".to_string(),
            EntityKind::Flag(Team::Player) => "flag_player".to_string(),
            EntityKind::Flag(Team::Robots) => "flag_robots".to_string(),
            EntityKind::DropSpaceFlag => "drop_space_flag".to_string(),
        }
    }
}

/// What a player's screen shows about themselves.
#[derive(Debug, Clone, PartialEq)]
pub struct PlayerView {
    pub health: f32,
    /// Fraction of the jetpack tank that is left, 0 to 1.
    pub jetpack: f32,
    /// What the player carries, the item in hand first.
    pub items: Vec<&'static str>,
    /// The recipes in the fixed order of [`crate::crafting::recipes`], which is what
    /// [`Action::Craft`] indexes. Clients order them for display themselves.
    pub recipes: Vec<RecipeView>,
}

/// One recipe as a player's screen shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct RecipeView {
    pub name: &'static str,
    /// Whether the pack holds the ingredients right now.
    pub can_craft: bool,
    /// The ingredient item names, in recipe order.
    pub ingredients: Vec<&'static str>,
}

/// Something that happened that clients want to show or hear, or that other systems react to.
#[derive(Debug, Clone, PartialEq)]
pub enum GameEvent {
    /// Play a sound; names are the ones the Java game registers (`"collect"`, `"impact"`, ...).
    Sound { name: &'static str, position: Vec3 },
    BlockDamaged { cell: Cell, id: u8 },
    BlockDestroyed { cell: Cell, id: u8 },
    /// A hit on something that cannot be broken: dust.
    HardHit { cell: Cell },
    Explosion { position: Vec3, radius: i32, damage: i32 },
    ItemPicked { player: EntityId, kind: CollectibleType },
    MoneyPicked { player: EntityId, total: u32 },
    PlayerDamaged { player: EntityId, health: f32 },
    PlayerDied { player: EntityId },
    Crafted { player: EntityId, result: RecipeResult },
    /// An oven finished a bar of iron.
    OvenProduced { cell: Cell, kind: CollectibleType },
    ItemPlaced { cell: Cell, block: u8 },
    RobotDestroyed { robot: EntityId, position: Vec3 },
}

pub(crate) struct CollectibleState {
    item: Item,
    /// Nobody can pick it up (it is being carried or held in a container).
    pub(crate) no_pickup: bool,
    last_parent: Option<EntityId>,
    /// Seconds left of the pickup block for `last_parent`.
    blocked_for: f32,
}

impl CollectibleState {
    fn new(item: Item) -> Self {
        CollectibleState { item, no_pickup: false, last_parent: None, blocked_for: INITIAL_PICKUP_BLOCK }
    }

    /// `canBePickedByParent`.
    fn can_be_picked_by(&self, parent: EntityId) -> bool {
        !self.no_pickup && (self.blocked_for < 0.0 || self.last_parent != Some(parent))
    }
}

pub(crate) struct RobotState {
    pub(crate) team: Team,
    /// Charges to [`ROBOT_CHARGE_TIME`], then the robot can attack.
    charge: f32,
    attack_in_progress: f32,
    target: Option<EntityId>,
    idle: IdleAi,
    last_position: Vec3,
}

pub(crate) enum Kind {
    Player(Box<PlayerState>),
    Collectible(CollectibleState),
    Money,
    Robot(Box<RobotState>),
    /// Characters, flags and other things of the second half of the rules, see [`crate::extras`].
    Other(Box<Other>),
}

/// The ruleset and its state. One per world.
pub struct Caveland {
    pub tuning: Tuning,
    pub(crate) kinds: HashMap<EntityId, Kind>,
    pub(crate) ovens: HashMap<Cell, OvenLogic>,
    pub(crate) money: u32,
    pub(crate) rng: JavaRandom,
    pub(crate) events: Vec<GameEvent>,
    explosions: Vec<Vec3>,
    engine_events: Vec<Event>,
    transport: Transport,
    /// Dialogs, construction sites, power, turrets, flags, spiders and the tutorial.
    pub(crate) x: Extras,
}

fn cell_of(p: Vec3) -> Cell {
    let (x, y) = from_iso(p.x, p.y);
    (x, y, p.z.floor() as i32)
}

/// Centre of a block cell.
pub fn cell_center(cell: Cell) -> Vec3 {
    let (gx, gy) = to_iso(cell.0, cell.1);
    Vec3::new(gx, gy, cell.2 as f32 + 0.5)
}

/// The floor of a cell: where something standing in it rests.
pub fn cell_floor(cell: Cell) -> Vec3 {
    let (gx, gy) = to_iso(cell.0, cell.1);
    Vec3::new(gx, gy, cell.2 as f32)
}

impl Caveland {
    pub fn new(tuning: Tuning, seed: i64) -> Self {
        Caveland {
            tuning,
            kinds: HashMap::new(),
            ovens: HashMap::new(),
            money: 0,
            rng: JavaRandom::new(seed),
            events: Vec::new(),
            explosions: Vec::new(),
            engine_events: Vec::new(),
            transport: Transport::new(seed),
            x: Extras::default(),
        }
    }

    /// Make a world follow Caveland's block rules. Call this once, before entities move in it.
    pub fn install(world: &mut World) {
        world.set_block_config(Arc::new(CavelandBlocks));
    }

    pub fn money(&self) -> u32 {
        self.money
    }

    pub fn set_money(&mut self, money: u32) {
        self.money = money;
    }

    /// What an entity is, if Caveland made it.
    pub fn kind_of(&self, id: EntityId) -> Option<EntityKind> {
        if let Some(kind) = self.transport.kind_of(id) {
            return Some(kind);
        }
        Some(match self.kinds.get(&id)? {
            Kind::Player(p) => EntityKind::Player { number: p.number },
            Kind::Collectible(c) => EntityKind::Collectible(c.item.kind),
            Kind::Money => EntityKind::Money,
            Kind::Robot(r) => self.robot_kind(id, r.team),
            Kind::Other(o) => o.entity_kind(),
        })
    }

    pub fn player(&self, id: EntityId) -> Option<&PlayerState> {
        match self.kinds.get(&id)? {
            Kind::Player(p) => Some(p),
            _ => None,
        }
    }

    pub fn player_mut(&mut self, id: EntityId) -> Option<&mut PlayerState> {
        match self.kinds.get_mut(&id)? {
            Kind::Player(p) => Some(p),
            _ => None,
        }
    }

    pub fn team_of(&self, id: EntityId) -> Team {
        match self.kinds.get(&id) {
            Some(Kind::Player(_)) => Team::Player,
            Some(Kind::Robot(r)) => r.team,
            _ => Team::Neutral,
        }
    }

    /// The oven logic at a cell, if that oven has been used.
    pub fn oven(&self, cell: Cell) -> Option<&OvenLogic> {
        self.ovens.get(&cell)
    }

    /// What the engine's entity update reported in the last [`Caveland::tick`] (landed, splashed...),
    /// for the sounds and particles a client makes of them.
    pub fn engine_events(&self) -> &[Event] {
        &self.engine_events
    }

    /// Take the events collected since the last call.
    pub fn drain_events(&mut self) -> Vec<GameEvent> {
        std::mem::take(&mut self.events)
    }

    // ---- spawning -------------------------------------------------------------------------

    /// The Caveland character (`new Ejira(number)`): the engine's player body plus the full-height
    /// barrier.
    pub fn spawn_player(&mut self, entities: &mut Entities, number: u8, position: Vec3) -> EntityId {
        let entity = self.new_character(position);
        let id = entities.spawn(entity);
        self.add_player_state(entities, id, number);
        id
    }

    /// The same, under a chosen id: a player who died comes back as themselves. `None` if the id is
    /// in use.
    pub fn spawn_player_as(&mut self, entities: &mut Entities, id: EntityId, number: u8, position: Vec3) -> Option<EntityId> {
        let entity = self.new_character(position);
        if !entities.spawn_as(id, entity) {
            return None;
        }
        self.add_player_state(entities, id, number);
        Some(id)
    }

    fn add_player_state(&mut self, entities: &Entities, id: EntityId, number: u8) {
        let facing = entities.get(id).and_then(|e| e.body.as_ref()).map_or(Vec2::X, |b| b.orientation());
        self.kinds.insert(id, Kind::Player(Box::new(PlayerState::new(id, number, self.tuning.jetpack_max_time, facing))));
    }

    fn new_character(&self, position: Vec3) -> Entity {
        let mut entity = wurfel_sim::player::new_player(position);
        entity.name = "Ejira".to_string();
        if let Some(body) = entity.body.as_mut() {
            body.friction = self.tuning.player_friction;
        }
        entity.add_component(Box::new(ColumnBarrier));
        entity
    }

    /// Everything Caveland made that is not a player, for the clients to draw.
    pub fn things(&self) -> Vec<(EntityId, EntityKind)> {
        let mut things: Vec<_> = self
            .kinds
            .keys()
            .filter_map(|&id| match self.kind_of(id) {
                Some(EntityKind::Player { .. }) | None => None,
                Some(kind) => Some((id, kind)),
            })
            .collect();
        things.extend(self.transport.things());
        things.sort_unstable_by_key(|&(id, _)| id);
        things
    }

    /// What the player's screen shows.
    pub fn player_view(&self, entities: &Entities, id: EntityId) -> Option<PlayerView> {
        let state = self.player(id)?;
        let entity = entities.get(id)?;
        Some(PlayerView {
            health: entity.health(),
            jetpack: (state.jetpack_time / self.tuning.jetpack_max_time.max(f32::EPSILON)).clamp(0.0, 1.0),
            items: state.inventory.items().iter().map(|item| item.kind.name()).collect(),
            recipes: crafting::recipes()
                .iter()
                .map(|r| RecipeView {
                    name: r.name,
                    can_craft: crafting::can_craft(r, &state.inventory),
                    ingredients: r.ingredients.iter().map(|i| i.name()).collect(),
                })
                .collect(),
        })
    }

    pub fn spawn_collectible(&mut self, entities: &mut Entities, item: Item, position: Vec3) -> EntityId {
        let mut entity = Entity::new(&format!("Collectible {}", item.kind.name()), item.kind.sprite_id())
            .movable()
            .at(position);
        entity.mass = 1.0;
        entity.indestructible = true;
        entity.add_component(Box::new(ColumnBarrier));
        let id = entities.spawn(entity);
        self.kinds.insert(id, Kind::Collectible(CollectibleState::new(item)));
        id
    }

    /// A collectible that jumps out in a random direction, like the iron from an oven (`sparkle`).
    pub fn spawn_sparkling(&mut self, entities: &mut Entities, item: Item, position: Vec3) -> EntityId {
        let id = self.spawn_collectible(entities, item, position);
        let v = Vec3::new(self.rng.next_float() - 0.5, self.rng.next_float() - 0.5, self.rng.next_float());
        if let Some(body) = entities.get_mut(id).and_then(|e| e.body.as_mut()) {
            body.set_movement(v);
        }
        id
    }

    pub fn spawn_money(&mut self, entities: &mut Entities, position: Vec3) -> EntityId {
        let mut entity = Entity::new("money", 20).movable().at(position);
        entity.mass = 0.1;
        entity.add_component(Box::new(ColumnBarrier));
        let id = entities.spawn(entity);
        self.kinds.insert(id, Kind::Money);
        id
    }

    /// A robot (`Robot`): friendly (`Team::Player`), evil (`Team::Robots`) or neutral.
    pub fn spawn_robot(&mut self, entities: &mut Entities, team: Team, position: Vec3) -> EntityId {
        let mut entity = Entity::new(if team == Team::Robots { "Evil Robot" } else { "Friendly Robot" }, 45)
            .movable()
            .at(position);
        entity.mass = 40.0;
        if let Some(body) = entity.body.as_mut() {
            body.jump_speed = Some(ROBOT_JUMP_SPEED);
        }
        let id = entities.spawn(entity);
        let state = RobotState {
            team,
            charge: 0.0,
            attack_in_progress: 0.0,
            target: None,
            idle: IdleAi::new(1 + id as i64),
            last_position: position,
        };
        self.kinds.insert(id, Kind::Robot(Box::new(state)));
        id
    }

    /// A minecart (`MineCart`): see [`crate::minecart`].
    pub fn spawn_minecart(&mut self, entities: &mut Entities, position: Vec3) -> EntityId {
        self.transport.spawn_minecart(entities, position)
    }

    // ---- damage ---------------------------------------------------------------------------

    /// Hurt an entity. A player in `godmode` takes nothing, and being hurt delays regeneration.
    pub fn damage_entity(&mut self, entities: &mut Entities, id: EntityId, amount: f32) {
        let godmode = self.tuning.godmode;
        let position = entities.get(id).map(|e| e.position).unwrap_or(Vec3::ZERO);
        let Some(entity) = entities.get_mut(id) else { return };
        match self.kinds.get_mut(&id) {
            Some(Kind::Player(p)) => {
                if godmode {
                    return;
                }
                entity.take_damage(amount);
                p.time_since_damage = 0.0;
                let health = entity.health();
                self.events.push(GameEvent::PlayerDamaged { player: id, health });
                self.events.push(GameEvent::Sound { name: "urfHurt", position });
            }
            _ => entity.take_damage(amount),
        }
    }

    /// Damage a block (`Coordinate.damage`). Health is stored in the chunk's health byte, where 0
    /// stands for untouched (full, 100). Returns whether anything was hit.
    pub fn damage_block(&mut self, world: &mut World, entities: &mut Entities, cell: Cell, amount: u8) -> bool {
        let block = world.get(cell.0, cell.1, cell.2);
        if block.is_air() || amount == 0 {
            return false;
        }
        if world.blocks().is_indestructible(block) {
            return true; // the Java code counts the hit and keeps the block
        }
        let stored = world.block_health(cell.0, cell.1, cell.2);
        let health = if stored == 0 { 100 } else { stored };
        let left = health.saturating_sub(amount);
        if left == 0 {
            self.destroy_block(world, entities, cell);
        } else {
            world.set_block_health(cell.0, cell.1, cell.2, left);
            self.events.push(GameEvent::BlockDamaged { cell, id: block.id() });
        }
        true
    }

    /// Remove a block, drop its loot and, for a tree, its other half.
    fn destroy_block(&mut self, world: &mut World, entities: &mut Entities, cell: Cell) {
        let block = world.get(cell.0, cell.1, cell.2);
        if block.is_air() || !world.set(cell.0, cell.1, cell.2, Block::AIR) {
            return;
        }
        world.set_block_health(cell.0, cell.1, cell.2, 0);
        self.events.push(GameEvent::BlockDestroyed { cell, id: block.id() });
        self.events.push(GameEvent::Sound { name: "blockDestroy", position: cell_center(cell) });
        if let Some(kind) = blocks::loot(block.id()) {
            self.spawn_collectible(entities, Item::new(kind), cell_floor(cell));
        }
        if block.id() == ids::TREE {
            // The top half has value 8 (`CustomTree.TREETOPVALUE`).
            let other = if block.value() == 8 { (cell.0, cell.1, cell.2 - 1) } else { (cell.0, cell.1, cell.2 + 1) };
            if world.get(other.0, other.1, other.2).id() == ids::TREE {
                self.destroy_block(world, entities, other);
            }
        }
    }

    /// An explosion (`Explosion`): damages blocks and entities around `center`, falling off with the
    /// square of the distance. Radius in blocks.
    pub fn explode(&mut self, world: &mut World, entities: &mut Entities, center: Vec3, radius: i32, damage: i32) {
        self.events.push(GameEvent::Explosion { position: center, radius, damage });
        self.events.push(GameEvent::Sound { name: "explosion", position: center });
        let r2 = (radius * radius) as f32;
        let origin = cell_of(center);
        // Rows are half a block apart in y, so twice the radius covers the same distance.
        for dx in -radius..=radius {
            for dy in -2 * radius..=2 * radius {
                for dz in -radius..=radius {
                    let cell = (origin.0 + dx, origin.1 + dy, origin.2 + dz);
                    let falloff = 1.0 - center.distance_squared(cell_center(cell)) / r2;
                    let amount = (damage as f32 * falloff) as i32;
                    if amount > 0 {
                        self.damage_block(world, entities, cell, amount.min(100) as u8);
                    }
                }
            }
        }
        let hit: Vec<(EntityId, f32)> = entities
            .iter()
            .filter(|e| e.body.is_some())
            .map(|e| (e.id(), e.position.distance_squared(center)))
            .filter(|&(_, d2)| d2 < r2)
            .collect();
        for (id, d2) in hit {
            // Entities break a little easier than blocks.
            let amount = (damage as f32 * (1.0 - d2 / r2) * 1.2).clamp(0.0, 100.0);
            self.damage_entity(entities, id, amount);
        }
    }

    // ---- player input ---------------------------------------------------------------------

    /// Set what a player is holding down for the next step: walking, jumping and the jetpack.
    pub fn set_controls(&mut self, entities: &mut Entities, world: &World, id: EntityId, controls: Controls) {
        let walking_speed = self.tuning.walking_speed;
        let Some(Kind::Player(state)) = self.kinds.get_mut(&id) else { return };
        let Some(entity) = entities.get_mut(id) else { return };
        let last = state.last_controls;
        state.last_controls = controls;

        let any_direction = controls.up || controls.down || controls.left || controls.right;
        if any_direction && !state.movement_locked() {
            entity.walk(controls.up, controls.down, controls.left, controls.right, walking_speed);
        }

        if controls.jump && !last.jump {
            let (position, dimension_z) = (entity.position, entity.dimension_z);
            // A few centimetres above the ground still count as standing on it.
            let can_jump = state.bunny_hop_forced
                || is_on_ground(world, position - Vec3::Z * (20.0 * UNIT), dimension_z);
            state.bunny_hop_forced = false;
            if can_jump {
                state.prepare_throw = false; // `playAnimation('j')` cancels the pose
                if let Some(body) = entity.body.as_mut() {
                    body.movement.z = 0.0;
                    body.jump_with(wurfel_sim::player::JUMP_SPEED);
                }
                self.events.push(GameEvent::Sound { name: "urfJump", position });
            } else if state.jetpack_time > 0.0 {
                if !state.jetpack_on {
                    self.events.push(GameEvent::Sound { name: "jetpack", position });
                }
                state.jetpack_on = true;
            }
        }
        if !controls.jump && last.jump {
            state.jetpack_on = false;
        }
    }

    /// Take `controls` as already held before the next [`Caveland::set_controls`], so a key that is
    /// down is not a fresh press (no jump, no jetpack start). For a client that starts predicting
    /// in the middle of an input.
    pub fn assume_held(&mut self, id: EntityId, controls: Controls) {
        if let Some(p) = self.player_mut(id) {
            p.last_controls = controls;
        }
    }

    /// Make the next jump work in mid-air (`forceBunnyHop`).
    pub fn force_bunny_hop(&mut self, id: EntityId) {
        if let Some(p) = self.player_mut(id) {
            p.bunny_hop_forced = true;
        }
    }

    /// A one-off action of a player.
    pub fn act(&mut self, entities: &mut Entities, world: &mut World, id: EntityId, action: Action) {
        let Some(Kind::Player(mut state)) = self.kinds.remove(&id) else { return };
        if entities.get(id).is_some() {
            self.act_inner(entities, world, id, &mut state, action);
        }
        self.kinds.insert(id, Kind::Player(state));
    }

    fn act_inner(&mut self, entities: &mut Entities, world: &mut World, id: EntityId, state: &mut PlayerState, action: Action) {
        let (position, movement) = {
            let e = entities.get(id).expect("checked by the caller");
            (e.position, e.body.as_ref().map_or(Vec3::ZERO, |b| b.movement))
        };
        match action {
            Action::Attack => self.start_attack(entities, world, id, state, ATTACK_DAMAGE),
            Action::ReleaseAttack => self.release_attack(entities, world, id, state),
            Action::PrepareThrow => {
                // The hold is timed even with nothing to throw: it ends in a drop (`throwDown`).
                state.throw_held = Some(0.0);
                if !state.inventory.is_empty() {
                    state.prepare_throw = true;
                }
            }
            Action::Throw => {
                state.throw_held = None;
                let item = if state.prepare_throw { state.inventory.retrieve(0) } else { None };
                match item {
                    Some(item) => {
                        state.prepare_throw = false;
                        let eid = self.spawn_collectible(entities, item, position + Vec3::Z * THROW_HEIGHT);
                        if let Some(body) = entities.get_mut(eid).and_then(|e| e.body.as_mut()) {
                            body.set_movement(movement + state.aim.extend(0.0) * THROW_SPEED);
                        }
                        self.block_pickup(eid, id, THROW_PICKUP_BLOCK);
                    }
                    None => self.events.push(GameEvent::Sound { name: "interactionFail", position }),
                }
            }
            Action::Drop => self.drop_item(entities, id, state, position),
            Action::UseItem => self.use_item(entities, world, id, state, position),
            Action::Interact => self.interact(entities, world, state, position),
            Action::SwitchItems { left } => state.inventory.switch_items(left),
            Action::Choose(answer) => self.choose(entities, world, id, state, answer),
            Action::Cancel => self.cancel(entities, world, id, state),
            Action::Craft(index) => {
                // The fixed list, not the display order: the pack may change between the client
                // drawing the menu and the key arriving.
                let recipes = crafting::recipes();
                let Some(recipe) = recipes.get(index) else { return };
                match crafting::craft(recipe, &mut state.inventory) {
                    Some(result) => {
                        if result == RecipeResult::MineCart {
                            self.spawn_minecart(entities, position + Vec3::Z * 0.2);
                        }
                        self.events.push(GameEvent::Crafted { player: id, result });
                    }
                    None => self.events.push(GameEvent::Sound { name: "interactionFail", position }),
                }
            }
        }
    }

    /// Lay the item in hand down (`Ejira.dropItem`).
    fn drop_item(&mut self, entities: &mut Entities, id: EntityId, state: &mut PlayerState, position: Vec3) {
        state.prepare_throw = false;
        state.throw_held = None;
        if let Some(item) = state.inventory.retrieve(0) {
            let eid = self.spawn_collectible(entities, item, position + Vec3::Z * (0.1));
            self.block_pickup(eid, id, DROP_PICKUP_BLOCK);
        }
        state.performing_power_attack = false;
    }

    fn block_pickup(&mut self, collectible: EntityId, parent: EntityId, seconds: f32) {
        if let Some(Kind::Collectible(c)) = self.kinds.get_mut(&collectible) {
            c.last_parent = Some(parent);
            c.blocked_for = seconds;
        }
    }

    /// Use the item in hand (`Inventory.action`): a torch is placed where the player stands, an
    /// explosive gets its fuse lit and stays in the pack. Other items do nothing.
    fn use_item(&mut self, entities: &mut Entities, world: &mut World, id: EntityId, state: &mut PlayerState, position: Vec3) {
        let Some(mut item) = state.inventory.retrieve(0) else { return };
        match item.kind {
            CollectibleType::Torch => {
                let cell = cell_of(position);
                if world.get(cell.0, cell.1, cell.2).is_air() && world.set(cell.0, cell.1, cell.2, Block::new(ids::TORCH, 0)) {
                    self.events.push(GameEvent::ItemPlaced { cell, block: ids::TORCH });
                    return; // used up
                }
                self.events.push(GameEvent::Sound { name: "interactionFail", position });
            }
            CollectibleType::Explosives => {
                item.ignite();
                self.events.push(GameEvent::Sound { name: "hiss", position });
            }
            _ => {
                if self.use_kit(entities, world, id, &item, position) {
                    return; // used up
                }
            }
        }
        state.inventory.add_front(item);
    }

    /// Machines within reach, nearest first.
    pub(crate) fn machines_near(&self, world: &World, position: Vec3) -> Vec<Cell> {
        let origin = cell_of(position);
        let mut found = Vec::new();
        for dx in -2..=2 {
            for dy in -4..=4 {
                for dz in -2..=2 {
                    let cell = (origin.0 + dx, origin.1 + dy, origin.2 + dz);
                    if world.get(cell.0, cell.1, cell.2).id() == ids::OVEN {
                        let d = cell_center(cell).distance(position + Vec3::Z * 0.5);
                        if d <= INTERACT_RADIUS {
                            found.push((d, cell));
                        }
                    }
                }
            }
        }
        found.sort_by(|a, b| a.0.total_cmp(&b.0));
        found.into_iter().map(|(_, c)| c).collect()
    }

    /// The nearest machine the player could use right now.
    pub fn nearest_interactable(&self, world: &World, position: Vec3) -> Option<Cell> {
        self.machines_near(world, position).into_iter().next()
    }

    fn interact(&mut self, entities: &mut Entities, world: &mut World, state: &mut PlayerState, position: Vec3) {
        // Dialog characters, construction sites, factories and flags first: see `interact_extra`.
        if self.interact_extra(entities, world, state.entity(), state, position) {
            return;
        }
        // Carts, lifts and portals count with the machines: the nearest of them is used.
        let oven = self.nearest_interactable(world, position);
        let vehicle = self.transport.nearest_interactable(entities, world, position);
        if let Some((distance, what)) = vehicle {
            let oven_distance = oven.map(|cell| cell_center(cell).distance(position + Vec3::Z * 0.5));
            if oven_distance.is_none_or(|d| distance < d) {
                self.interact_transport(entities, world, state.entity(), what);
                return;
            }
        }
        let Some(cell) = oven else { return };
        let oven = self.ovens.entry(cell).or_default();
        if oven.interact(&mut state.inventory) {
            self.events.push(GameEvent::Sound { name: "metallic", position: cell_center(cell) });
        } else {
            self.events.push(GameEvent::Sound { name: "interactionFail", position: cell_center(cell) });
        }
    }

    /// Use a cart, lift or portal (`Interactable.interact`).
    fn interact_transport(&mut self, entities: &mut Entities, world: &mut World, actor: EntityId, what: Interaction) {
        // Only players act (the player's own state is out of the table while they do).
        let mut transport = std::mem::take(&mut self.transport);
        transport.interact(entities, world, actor, true, what);
        self.transport = transport;
    }

    // ---- vehicles, lifts and portals ------------------------------------------------------

    /// Carts, baskets, portals and the spaceship: the state behind [`EntityKind::MineCart`],
    /// [`EntityKind::LiftBasket`], [`EntityKind::ExitPortal`] and [`EntityKind::Spaceship`].
    pub fn transport(&self) -> &Transport {
        &self.transport
    }

    /// Spawn vehicles and portals, or board and launch them.
    pub fn transport_mut(&mut self) -> &mut Transport {
        &mut self.transport
    }

    /// Take what vehicles and portals reported since the last call (teleports, boarding, the crash).
    pub fn drain_transport_events(&mut self) -> Vec<TransportEvent> {
        self.transport.take_notes()
    }

    /// Is the entity somewhere it cannot be seen (a passenger of the spaceship)?
    pub fn is_hidden(&self, id: EntityId) -> bool {
        self.transport.is_hidden(id)
    }

    /// A block was placed at `cell`: if it needs updating (a lift, a cave entry), start doing that.
    /// Whatever sets blocks (a finished construction site) calls this.
    pub fn block_changed(&mut self, world: &World, cell: Cell) {
        self.transport.register(world, cell);
    }

    /// Create what the map generator asks for; `None` for entities that are not vehicles or portals.
    pub fn spawn_from_generator(&mut self, entities: &mut Entities, spawn: &wurfel_sim::generator::EntitySpawn) -> Option<EntityId> {
        self.transport.spawn_from_generator(entities, spawn)
    }

    fn update_transport(&mut self, entities: &mut Entities, world: &mut World, dt: f32) {
        let mut transport = std::mem::take(&mut self.transport);
        transport.update(self, entities, world, dt);
        self.events.extend(transport.take_game_events());
        self.transport = transport;
    }

    /// Collectibles within `radius` of `at` that are falling and that `parent` may take: what a
    /// cart loads (`canBePickedByParent`).
    pub(crate) fn collectibles_for(&self, entities: &Entities, parent: EntityId, at: Vec3, radius: f32) -> Vec<EntityId> {
        let mut found: Vec<EntityId> = self
            .kinds
            .iter()
            .filter_map(|(&id, kind)| match kind {
                Kind::Collectible(c) if c.can_be_picked_by(parent) => Some(id),
                _ => None,
            })
            .filter(|&id| {
                entities
                    .get(id)
                    .is_some_and(|e| e.position.distance(at) < radius && e.body.as_ref().is_some_and(|b| b.movement.z < 0.0))
            })
            .collect();
        found.sort_unstable();
        found
    }

    /// Take a collectible out of the world, for a container (`dispose` after `add`).
    pub(crate) fn take_collectible(&mut self, entities: &mut Entities, id: EntityId) -> Option<Item> {
        let Some(Kind::Collectible(c)) = self.kinds.remove(&id) else { return None };
        if let Some(e) = entities.get_mut(id) {
            e.dispose();
        }
        Some(c.item)
    }

    // ---- attacks --------------------------------------------------------------------------

    /// Swing (`Ejira.attack`). Ignored while a swing is under way.
    fn start_attack(&mut self, entities: &mut Entities, world: &World, id: EntityId, state: &mut PlayerState, damage: u8) {
        if state.time_till_impact.is_some() {
            return;
        }
        state.performing_power_attack = false;
        state.prepare_throw = false; // a swing cancels a prepared throw (`playAnimation('h')`)
        let position = entities.get(id).map(|e| e.position).unwrap_or(Vec3::ZERO);
        self.events.push(GameEvent::Sound { name: "sword", position });
        if let Some(entity) = entities.get_mut(id) {
            let on_ground = is_on_ground(world, entity.position, entity.dimension_z);
            if let Some(body) = entity.body.as_mut() {
                if on_ground {
                    // A lunge forwards (`addToHor(13)`).
                    let o = body.orientation();
                    body.add_movement((o * ATTACK_LUNGE).extend(0.0));
                }
            }
        }
        state.time_till_impact = Some(self.tuning.time_till_impact);
        state.attack_damage = damage;
        if !state.used_load_attack_in_air {
            state.load_attack = Some(0.0);
        }
    }

    /// Let go of the attack key (`attackLoadingStopped`): a long enough charge becomes a power attack.
    fn release_attack(&mut self, entities: &mut Entities, world: &World, id: EntityId, state: &mut PlayerState) {
        if state.load_attack.is_some_and(|t| t >= LOAD_ATTACK_TIME) {
            let friction = self.tuning.player_friction;
            if let Some(entity) = entities.get_mut(id) {
                let on_ground = is_on_ground(world, entity.position, entity.dimension_z);
                if let Some(body) = entity.body.as_mut() {
                    let o = body.orientation();
                    if on_ground {
                        body.add_movement((o * POWER_LUNGE).extend(0.0));
                    } else {
                        body.friction = friction / 3.0;
                        body.add_movement((o * 30.0).extend(-3.0));
                    }
                }
            }
            if let Some(position) = entities.get(id).map(|e| e.position) {
                self.events.push(GameEvent::Sound { name: "release", position });
            }
            state.time_till_impact = None; // a charged release always swings
            self.start_attack(entities, world, id, state, POWER_ATTACK_DAMAGE);
            state.performing_power_attack = true;
            state.used_load_attack_in_air = true;
        }
        state.load_attack = None;
    }

    /// The swing connects (`attackImpact`): damages and shoves entities in front of the player, and
    /// digs the block in front.
    fn attack_impact(&mut self, entities: &mut Entities, world: &mut World, id: EntityId, state: &mut PlayerState) {
        let Some(position) = entities.get(id).map(|e| e.position) else { return };
        let aim = state.aim.extend(0.0);
        let front = position + aim * HIT_REACH;
        let below = position + Vec3::Z * 0.5;
        let victims: Vec<EntityId> = entities
            .iter()
            .filter(|e| {
                e.id() != id
                    && (e.position.distance(front) < HIT_RADIUS || e.position.distance(below) < 39.0 * UNIT)
            })
            .map(|e| e.id())
            .collect();
        for victim in victims {
            self.damage_entity(entities, victim, state.attack_damage as f32);
            let shove = Vec3::new(
                aim.x + self.rng.next_float() * 0.5 - 0.25,
                aim.y + self.rng.next_float() * 0.5 - 0.25,
                self.rng.next_float(),
            );
            if let Some(body) = entities.get_mut(victim).and_then(|e| e.body.as_mut()) {
                body.set_movement(shove);
                body.set_speed_horizontal(2.0);
            }
        }

        let aim_cell = cell_of(position + Vec3::Z * 0.5 + aim * DIG_REACH);
        let block = world.get(aim_cell.0, aim_cell.1, aim_cell.2);
        if !world.blocks().is_liquid(block) && !block.is_air() {
            if blocks::hard_material(block.id()) {
                self.events.push(GameEvent::Sound { name: "impact", position: cell_center(aim_cell) });
                self.events.push(GameEvent::HardHit { cell: aim_cell });
            } else if self.damage_block(world, entities, aim_cell, state.attack_damage) {
                let name = if block.id() == ids::TREE { "treehit" } else { "impact" };
                self.events.push(GameEvent::Sound { name, position: cell_center(aim_cell) });
            }
        }
    }

    // ---- the step -------------------------------------------------------------------------

    /// Advance the world by `dt` seconds: the engine's entity update, then the Caveland rules.
    pub fn tick(&mut self, entities: &mut Entities, world: &mut World, dt: f32) -> Vec<GameEvent> {
        // Where robots are now, in case they die in this step and drop money.
        for (id, kind) in self.kinds.iter_mut() {
            if let (Kind::Robot(r), Some(e)) = (kind, entities.get(*id)) {
                r.last_position = e.position;
            }
        }

        let engine_events = entities.update(world, dt);
        self.handle_engine_events(entities, &engine_events);
        self.engine_events = engine_events;

        let mut ids: Vec<EntityId> = self.kinds.keys().copied().collect();
        ids.sort_unstable(); // deterministic: the server and a replay agree
        for id in ids {
            match self.kinds.remove(&id) {
                Some(Kind::Player(mut state)) => {
                    if entities.get(id).is_some() {
                        self.update_player(entities, world, id, &mut state, dt);
                        self.kinds.insert(id, Kind::Player(state));
                    }
                }
                Some(Kind::Collectible(mut c)) => {
                    if let Some(entity) = entities.get_mut(id) {
                        c.blocked_for -= dt;
                        if c.item.tick(dt) {
                            self.explosions.push(entity.position);
                            entity.dispose();
                        } else {
                            self.kinds.insert(id, Kind::Collectible(c));
                        }
                    }
                }
                Some(Kind::Robot(mut r)) => {
                    if entities.get(id).is_some() {
                        self.update_robot(entities, world, id, &mut r, dt);
                        self.kinds.insert(id, Kind::Robot(r));
                    }
                }
                Some(other) => {
                    if entities.get(id).is_some() {
                        self.kinds.insert(id, other);
                    }
                }
                None => {}
            }
        }

        self.update_ovens(entities, world, dt);
        self.update_transport(entities, world, dt);
        self.update_extras(entities, world, dt);

        for position in std::mem::take(&mut self.explosions) {
            self.explode(world, entities, position, EXPLOSIVE_RADIUS, EXPLOSIVE_DAMAGE);
        }
        self.drain_events()
    }

    fn handle_engine_events(&mut self, entities: &mut Entities, events: &[Event]) {
        for event in events {
            let Event::Disposed(id) = *event else { continue };
            let mut transport = std::mem::take(&mut self.transport);
            transport.on_disposed(self, entities, id);
            self.transport = transport;
            match self.kinds.remove(&id) {
                Some(Kind::Player(_)) => self.events.push(GameEvent::PlayerDied { player: id }),
                Some(Kind::Robot(robot)) => {
                    self.events.push(GameEvent::RobotDestroyed { robot: id, position: robot.last_position });
                    // Everything but the player's own robots drops money.
                    if robot.team != Team::Player {
                        self.spawn_money(entities, robot.last_position);
                    }
                }
                _ => {}
            }
        }
    }

    fn update_player(&mut self, entities: &mut Entities, world: &mut World, id: EntityId, state: &mut PlayerState, dt: f32) {
        let tuning = self.tuning.clone();
        let (position, dimension_z) = {
            let e = entities.get(id).expect("checked by the caller");
            (e.position, e.dimension_z)
        };
        let on_ground = is_on_ground(world, position, dimension_z);

        {
            let entity = entities.get_mut(id).expect("checked by the caller");
            let body = entity.body.as_mut().expect("players move");

            // In the air the engine applies no friction; the player slows down anyway.
            if !on_ground {
                if body.speed_hor() > 0.1 {
                    let factor = 1.0 / (dt * 1000.0 * body.friction + 1.0);
                    let hor = body.hor_movement() * factor;
                    body.set_hor_movement(hor);
                } else {
                    body.set_hor_movement(Vec2::ZERO);
                }
            } else {
                if state.used_load_attack_in_air {
                    body.friction = tuning.player_friction;
                }
                state.used_load_attack_in_air = false;
            }

            // Caves have a ceiling one block below the top of the world.
            if from_iso(entity.position.x, entity.position.y).1 > wurfel_sim::caveland::CAVES_BORDER
                && entity.position.z > CAVE_CEILING
            {
                entity.position.z = CAVE_CEILING;
            }
        }

        // The character turns smoothly towards where it walks.
        {
            let body = entities.get(id).and_then(|e| e.body.as_ref()).expect("players move");
            let c = state.last_controls;
            let pressing = c.up || c.down || c.left || c.right;
            let target = body.orientation();
            let turn = if pressing && body.speed_hor() < 0.1 {
                let t = dt * 8.0;
                if (state.aim - target).length() < 1.5 { t * 4.0 } else { t }
            } else {
                body.speed_hor() * dt * 2.0
            };
            state.aim = slerp(state.aim, target, turn.min(1.0));
        }

        // The throw button's hold is timed.
        if let Some(held) = state.throw_held.as_mut() {
            *held += dt;
            if *held >= tuning.item_drop_time {
                // Held that long it is a drop; the release then finds nothing prepared.
                self.drop_item(entities, id, state, position);
            }
        }

        // Swing and charge.
        if let Some(left) = state.time_till_impact.as_mut() {
            *left -= dt;
            if *left <= 0.0 {
                state.time_till_impact = None;
                self.attack_impact(entities, world, id, state);
            }
        }
        if state.performing_power_attack
            && entities.get(id).and_then(|e| e.body.as_ref()).is_some_and(|b| b.speed_hor() < 0.1)
        {
            state.performing_power_attack = false;
        }
        if let Some(charge) = state.load_attack.as_mut() {
            *charge += dt;
            let charge = *charge;
            if charge > LOAD_THRESHOLD {
                // Charging slows falling to a quarter per step.
                if let Some(body) = entities.get_mut(id).and_then(|e| e.body.as_mut()) {
                    body.movement.z /= 4.0;
                }
            }
            if charge >= LOAD_ATTACK_TIME {
                self.release_attack(entities, world, id, state);
            }
        }

        // Pick things up.
        self.pick_up(entities, id, state);

        // Health comes back when left alone.
        if state.time_since_damage > REGEN_DELAY {
            if let Some(e) = entities.get_mut(id) {
                e.heal(REGEN_PER_SECOND * dt);
            }
        } else {
            state.time_since_damage += dt;
        }

        // Jetpack.
        {
            let on_ground_now = on_ground;
            if state.jetpack_on {
                state.jetpack_time -= dt;
                if state.jetpack_time <= 0.0 {
                    state.jetpack_on = false;
                }
            }
            if on_ground_now && state.jetpack_time <= 0.0 {
                state.jetpack_time = tuning.jetpack_max_time;
            }
            if state.jetpack_on {
                if let Some(body) = entities.get_mut(id).and_then(|e| e.body.as_mut()) {
                    if body.movement.z < tuning.jetpack_max_speed {
                        body.movement.z += dt * tuning.jetpack_power;
                    }
                }
            }
        }

        // A lit explosive in the pack goes off where the player is.
        let blown = state.inventory.tick(dt);
        if blown > 0 {
            if let Some(e) = entities.get(id) {
                for _ in 0..blown {
                    self.explosions.push(e.position);
                }
            }
        }
    }

    fn pick_up(&mut self, entities: &mut Entities, id: EntityId, state: &mut PlayerState) {
        let position = entities.get(id).map(|e| e.position).unwrap_or(Vec3::ZERO);
        let mut picked_item = false;
        let mut coins = 0;
        for other in entities.colliding(id) {
            match self.kinds.get(&other) {
                Some(Kind::Collectible(c)) if c.can_be_picked_by(id) && !state.inventory.is_full() => {
                    let item = c.item;
                    state.inventory.add(item);
                    self.kinds.remove(&other);
                    if let Some(e) = entities.get_mut(other) {
                        e.dispose();
                    }
                    self.events.push(GameEvent::ItemPicked { player: id, kind: item.kind });
                    picked_item = true;
                }
                Some(Kind::Money) => {
                    self.kinds.remove(&other);
                    if let Some(e) = entities.get_mut(other) {
                        e.dispose();
                    }
                    coins += 1;
                }
                _ => {}
            }
        }
        if picked_item {
            self.events.push(GameEvent::Sound { name: "collect", position });
        }
        if coins > 0 {
            self.money += coins;
            self.events.push(GameEvent::MoneyPicked { player: id, total: self.money });
            self.events.push(GameEvent::Sound { name: "moneyPickup", position });
        }
    }

    fn update_robot(&mut self, entities: &mut Entities, world: &World, id: EntityId, robot: &mut RobotState, dt: f32) {
        let speed = if robot.attack_in_progress > 0.0 {
            robot.attack_in_progress -= dt;
            0.0
        } else {
            robot.attack_in_progress = 0.0;
            ROBOT_WALK_SPEED
        };
        let position = entities.get(id).map(|e| e.position).unwrap_or(Vec3::ZERO);

        // A target that is gone (or dead) is forgotten.
        if robot.target.is_some_and(|t| entities.get(t).is_none_or(|e| e.is_disposed())) {
            robot.target = None;
        }

        if let Some(target) = robot.target {
            let target_position = entities.get(target).map(|e| e.position).unwrap_or(position);
            let distance = position.distance(target_position);
            if let Some(body) = entities.get_mut(id).and_then(|e| e.body.as_mut()) {
                if distance > ROBOT_STOP_DISTANCE {
                    let dir = (target_position - position).truncate().normalize_or_zero();
                    body.set_hor_movement(dir * speed);
                } else {
                    body.set_hor_movement(Vec2::ZERO);
                }
            }
            if robot.attack_in_progress == 0.0 && robot.charge >= ROBOT_CHARGE_TIME {
                robot.charge = 0.0;
                if distance < ROBOT_ATTACK_RANGE {
                    self.damage_entity(entities, target, 1.0);
                    self.events.push(GameEvent::Sound { name: "robotHit", position: target_position });
                }
                robot.attack_in_progress = ROBOT_ATTACK_TIME;
            }
        }
        robot.charge += dt;

        if robot.target.is_none() && robot.team != Team::Neutral {
            // The nearest member of an enemy team within sight.
            let mut best: Option<(f32, EntityId)> = None;
            for other in entities.iter() {
                if other.id() == id {
                    continue;
                }
                let team = self.team_of(other.id());
                if !robot.team.is_hostile_to(team) || team == Team::Neutral {
                    continue;
                }
                let d = (other.position - position).truncate().length();
                if d < ROBOT_SIGHT && best.is_none_or(|(bd, _)| d < bd) {
                    best = Some((d, other.id()));
                }
            }
            robot.target = best.map(|(_, id)| id);
        }
        if robot.target.is_none() && !self.x.robot_busy(id) {
            if let Some(entity) = entities.get_mut(id) {
                robot.idle.step(entity, world, dt);
            }
        }
    }

    fn update_ovens(&mut self, entities: &mut Entities, world: &mut World, dt: f32) {
        let mut cells: Vec<Cell> = self.ovens.keys().copied().collect();
        cells.sort_unstable();
        for cell in cells {
            // An oven that was broken gives back what was inside.
            if world.get(cell.0, cell.1, cell.2).id() != ids::OVEN {
                if let Some(mut oven) = self.ovens.remove(&cell) {
                    for item in oven.release() {
                        self.spawn_sparkling(entities, item, cell_floor(cell));
                    }
                }
                continue;
            }
            let events = self.ovens.get_mut(&cell).map(|o| o.update(dt)).unwrap_or_default();
            for event in events {
                if let OvenEvent::Produced(kind) = event {
                    self.spawn_sparkling(entities, Item::new(kind), cell_floor((cell.0, cell.1, cell.2 + 1)));
                    self.events.push(GameEvent::OvenProduced { cell, kind });
                }
            }
        }
    }
}
