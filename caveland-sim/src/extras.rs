//! The second half of the Caveland rules: characters, machines and the story. Everything here is
//! an `impl Caveland` that works on [`Extras`], the state kept next to the core rules in `game.rs`:
//! dialogs, construction sites, the power network, turrets, robot factories, flags, spiders and the
//! tutorial.

use std::collections::{HashMap, HashSet};

use glam::{Vec2, Vec3};
use serde::{Deserialize, Serialize};
use wurfel_sim::entity::{Entities, Entity, EntityId};
use wurfel_sim::{Block, World};

use crate::blocks::{self, ids};
use crate::cells::{cell_at, find_blocks, neighbour};
use crate::collectible::{CollectibleType, Item};
use crate::construction::{
    continue_line, kit_block, line_options, site_value, toolkit_options, ConstructionSite, Flag, FLAG_HEIGHT,
};
use crate::dialog::{option, Dialog, DialogMode, OpenDialog, Source};
use crate::enemy::{
    self, drops_loot_at, factory_options, nearby_resources, variant_for_option, RobotFactory, RobotVariant, Spider, SPIDER_CHARGE, SPIDER_FLAG_SIGHT, SPIDER_REACH, SPIDER_SCAN_EVERY, SPIDER_WORK,
};
use crate::game::{cell_center, cell_floor, Caveland, Cell, EntityKind, GameEvent, Kind};
use crate::launcher::{Launcher, LauncherKind, Shell};
use crate::logic::OvenLogic;
use crate::npc::{self, Bird, Vanya, SHOP_STOCK, VANYA_DUPLICATE_RADIUS, VANYA_LOOK_RADIUS};
use crate::player::{PlayerState, INTERACT_RADIUS};
use crate::power::{self, Candidate, PowerGrid, TargetMode, Turret, TURRET_SIGHT};
use crate::team::Team;
use wurfel_sim::entity::ai::MoveToAi;
use wurfel_sim::entity::Component;

/// Seconds between two looks at the world for machines and power blocks.
const SCAN_EVERY: f32 = 0.5;
/// Where the tutorial's guide appears (`new Coordinate(-3, 8, 6)`).
pub const VANYA_SPAWN: Cell = (-3, 8, 6);
/// The place whose crossing starts the tutorial's end fight, corners of the cube.
pub const FIGHT_ZONE: (Cell, Cell) = ((28, -5, 3), (30, 7, wurfel_sim::caveland::HEIGHT));
/// Where the robots of the end fight come from: x 34 to 38, y 4 to 8, z 5.
pub const FIGHT_ROBOTS: usize = 5;
/// Where a player starts and comes back to until a flag says otherwise (`respawnX/Y/Z` defaults).
pub const DEFAULT_RESPAWN: Cell = (0, 0, 10);

/// Something that happened that clients or other systems may want, which [`GameEvent`] has no
/// place for (it is matched exhaustively by the server).
#[derive(Debug, Clone, PartialEq)]
pub enum ExtraEvent {
    /// A player opened a dialog.
    DialogOpened { player: EntityId, dialog: Dialog },
    /// A player's dialog went away.
    DialogClosed { player: EntityId },
    /// A block changed in a way that clients need the exact block for (value included).
    BlockChanged { cell: Cell, block: Block },
    /// A construction site turned into a machine.
    Built { cell: Cell, block: u8 },
    /// A player paid for something at the shop.
    Bought { player: EntityId, kind: CollectibleType, price: u32 },
    /// A flag changed teams.
    FlagCaptured { flag: EntityId, team: Team },
    /// A flag set the respawn point.
    RespawnSet { cell: Cell },
    /// A factory built a robot.
    RobotBuilt { factory: Cell, robot: EntityId, variant: RobotVariant },
    /// A torch, turret or station gained or lost power.
    PowerChanged { cell: Cell, powered: bool },
    /// A turret fired: draw a tracer from `from` to `to`.
    TurretShot { turret: Cell, from: Vec3, to: Vec3 },
    /// A player was thrown by a catapult or cannon (or told to fly by the server): where from and
    /// how fast. Clients run the same arc from this start.
    Launched { entity: EntityId, position: Vec3, velocity: Vec3 },
    /// The tutorial's guide moved on to a new step.
    TutorialStep { step: u8 },
    /// The robots of the end fight appeared.
    EndFightStarted,
}

/// Things that are not players, collectibles, money, robots or minecarts.
pub(crate) enum Other {
    Vanya(Box<Vanya>),
    Shopkeeper,
    Bird(Bird),
    Flag(Flag),
    DropSpaceFlag,
}

impl Other {
    pub(crate) fn entity_kind(&self) -> EntityKind {
        match self {
            Other::Vanya(_) => EntityKind::Vanya,
            Other::Shopkeeper => EntityKind::Shopkeeper,
            Other::Bird(_) => EntityKind::Bird,
            Other::Flag(f) => EntityKind::Flag(f.team),
            Other::DropSpaceFlag => EntityKind::DropSpaceFlag,
        }
    }
}

/// How far the story has got, and whether the rules run it (`CLGameController`).
#[derive(Debug, Default)]
pub(crate) struct Scenario {
    pub enabled: bool,
    pub end_fight_started: bool,
    pub vanya: Option<EntityId>,
}

#[derive(Default)]
pub(crate) struct Extras {
    pub dialogs: HashMap<EntityId, OpenDialog>,
    pub sites: HashMap<Cell, ConstructionSite>,
    pub grid: PowerGrid,
    pub since_scan: f32,
    pub dirty: bool,
    /// Non-cable power blocks that had power at the last look, to report changes.
    pub powered_before: HashSet<Cell>,
    pub turrets: HashMap<Cell, Turret>,
    /// Whoever pressed Build last on a turret. Not saved: player ids last one session, like friends.
    pub turret_owners: HashMap<Cell, EntityId>,
    /// Friendships between players as `(smaller id, larger id)`, told by the server.
    pub friends: HashSet<(EntityId, EntityId)>,
    pub factories: HashMap<Cell, RobotFactory>,
    /// Catapults and cannons: how they are aimed and what they hold.
    pub launchers: HashMap<Cell, Launcher>,
    /// Cannon shells in the air.
    pub shells: HashMap<EntityId, Shell>,
    pub flagpoles: HashMap<Cell, EntityId>,
    pub variants: HashMap<EntityId, RobotVariant>,
    pub spiders: HashMap<EntityId, Spider>,
    pub scenario: Scenario,
    pub respawn: Option<Cell>,
    pub events: Vec<ExtraEvent>,
    /// The teams of the flags on poles, kept when a flag entity is lost (a save keeps them).
    pub pole_teams: HashMap<Cell, Team>,
}

impl Extras {
    /// Is a robot busy with a job, so that it should not wander off on its own?
    pub(crate) fn robot_busy(&self, id: EntityId) -> bool {
        self.spiders.get(&id).is_some_and(|s| s.has_job())
    }
}

/// What a game keeps of the extras between runs (a save slot's sidecar): the parts of the world
/// that are not blocks.
#[derive(Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct SaveData {
    pub money: u32,
    pub respawn: Option<Cell>,
    pub ovens: Vec<(Cell, OvenLogic)>,
    pub sites: Vec<(Cell, ConstructionSite)>,
    pub tutorial_step: u8,
    pub end_fight_started: bool,
    /// Flag poles with the team their flag belongs to (team id).
    pub flags: Vec<(Cell, u8)>,
    /// Catapults and cannons with their aim and the gunpowder in the barrel.
    #[serde(default)]
    pub launchers: Vec<(Cell, Launcher)>,
}

/// Something a player can talk to or operate (see `Caveland::nearest_extra`).
#[derive(Clone, Copy)]
pub(crate) enum Target {
    Site(Cell),
    Factory(Cell),
    Turret(Cell),
    Launcher(Cell),
    Entity(EntityId),
}

impl Caveland {
    // ---- spawning -------------------------------------------------------------------------

    /// The guide (`new Vanya()`).
    pub fn spawn_vanya(&mut self, entities: &mut Entities, position: Vec3) -> EntityId {
        let mut entity = Entity::new("Vanya", 40).movable().at(position);
        entity.indestructible = true;
        let id = entities.spawn(entity);
        self.kinds.insert(id, Kind::Other(Box::new(Other::Vanya(Box::new(Vanya::new())))));
        id
    }

    /// The shopkeeper (`new Shopkeeper()`).
    pub fn spawn_shopkeeper(&mut self, entities: &mut Entities, position: Vec3) -> EntityId {
        let entity = Entity::new("Shopkeeper", 41).movable().at(position);
        let id = entities.spawn(entity);
        self.kinds.insert(id, Kind::Other(Box::new(Other::Shopkeeper)));
        id
    }

    /// A bird that drifts around (`new Bird()`).
    pub fn spawn_bird(&mut self, entities: &mut Entities, position: Vec3) -> EntityId {
        let mut entity = Entity::new("Bird", 40).movable().at(position);
        if let Some(body) = entity.body.as_mut() {
            body.floating = true;
        }
        let id = entities.spawn(entity);
        let seed = 100 + id as i64;
        self.kinds.insert(id, Kind::Other(Box::new(Other::Bird(Bird::new(seed)))));
        id
    }

    /// A flag (`new Flag()`) of a team, without a pole.
    pub fn spawn_flag(&mut self, entities: &mut Entities, team: Team, position: Vec3) -> EntityId {
        let entity = Entity::new("Flag", 21).at(position);
        let id = entities.spawn(entity);
        self.kinds.insert(id, Kind::Other(Box::new(Other::Flag(Flag { team }))));
        id
    }

    /// A drop-space flag (`new DropSpaceFlag()`): where spiders bring their loot.
    pub fn spawn_drop_space_flag(&mut self, entities: &mut Entities, position: Vec3) -> EntityId {
        let entity = Entity::new("DropSpaceFlag", 24).at(position);
        let id = entities.spawn(entity);
        self.kinds.insert(id, Kind::Other(Box::new(Other::DropSpaceFlag)));
        id
    }

    /// The ore-gathering robot (`new SpiderRobot()`).
    pub fn spawn_spider(&mut self, entities: &mut Entities, team: Team, position: Vec3) -> EntityId {
        let id = self.spawn_robot(entities, team, position);
        if let Some(entity) = entities.get_mut(id) {
            entity.sprite_id = enemy::sprite_id(RobotVariant::Spider);
            if let Some(body) = entity.body.as_mut() {
                body.floating = false;
            }
        }
        self.x.variants.insert(id, RobotVariant::Spider);
        self.x.spiders.insert(id, Spider::new());
        id
    }

    /// The flying robot (`new Quadrocopter()`).
    pub fn spawn_drone(&mut self, entities: &mut Entities, team: Team, position: Vec3) -> EntityId {
        let id = self.spawn_robot(entities, team, position);
        if let Some(entity) = entities.get_mut(id) {
            entity.sprite_id = enemy::sprite_id(RobotVariant::Drone);
            if let Some(body) = entity.body.as_mut() {
                body.floating = true;
            }
        }
        self.x.variants.insert(id, RobotVariant::Drone);
        id
    }

    /// What kind of robot an entity is.
    pub fn robot_variant(&self, id: EntityId) -> Option<RobotVariant> {
        match self.kinds.get(&id)? {
            Kind::Robot(_) => Some(self.x.variants.get(&id).copied().unwrap_or(RobotVariant::Fighter)),
            _ => None,
        }
    }

    /// `kind_of` for robots: spiders and drones have their own kinds.
    pub(crate) fn robot_kind(&self, id: EntityId, team: Team) -> EntityKind {
        match self.x.variants.get(&id) {
            Some(RobotVariant::Spider) => EntityKind::SpiderRobot(team),
            Some(RobotVariant::Drone) => EntityKind::Drone(team),
            _ => EntityKind::Robot(team),
        }
    }

    // ---- reading state --------------------------------------------------------------------

    /// The dialog a player has open.
    pub fn open_dialog(&self, player: EntityId) -> Option<&Dialog> {
        self.x.dialogs.get(&player).map(|d| &d.dialog)
    }

    /// Does the block at `cell` have power? (Only power blocks can.)
    pub fn is_powered(&self, cell: Cell) -> bool {
        self.x.grid.is_powered(cell)
    }

    /// How far a turret has come up, 0 to 1.
    pub fn turret_online(&self, cell: Cell) -> Option<f32> {
        self.x.turrets.get(&cell).map(|t| t.online())
    }

    /// The construction site at a cell, once it has been seen.
    pub fn construction_site(&self, cell: Cell) -> Option<&ConstructionSite> {
        self.x.sites.get(&cell)
    }

    /// Where a player who died, or one who joins, appears: one cell north of the respawn cell of
    /// the last flag they used (`Ejira.die`).
    pub fn respawn_position(&self) -> Vec3 {
        let (x, y, z) = self.x.respawn.unwrap_or(DEFAULT_RESPAWN);
        cell_floor((x, y + 1, z))
    }

    /// How far the tutorial has got, if its guide exists.
    pub fn tutorial_step(&self, entities: &Entities) -> Option<u8> {
        let id = self.x.scenario.vanya?;
        entities.get(id)?;
        match self.kinds.get(&id)? {
            Kind::Other(other) => match &**other {
                Other::Vanya(v) => Some(v.tutorial_step()),
                _ => None,
            },
            _ => None,
        }
    }

    /// Tell the tutorial something happened outside the rules (the intro ship crashed...): the guide
    /// moves to at least this step.
    pub fn set_tutorial_step(&mut self, entities: &Entities, step: u8) {
        let Some(id) = self.x.scenario.vanya else { return };
        if entities.get(id).is_none() {
            return;
        }
        if let Some(Kind::Other(other)) = self.kinds.get_mut(&id) {
            if let Other::Vanya(v) = &mut **other {
                let before = v.tutorial_step();
                v.set_tutorial_step(step);
                if v.tutorial_step() != before {
                    self.x.events.push(ExtraEvent::TutorialStep { step: v.tutorial_step() });
                }
            }
        }
    }

    /// Let the rules run the story: the guide appears, steps follow the player, the end fight starts
    /// (`CLGameController.update`). Off by default so other worlds stay as they are.
    pub fn set_scenario(&mut self, enabled: bool) {
        self.x.scenario.enabled = enabled;
    }

    /// Replace who is friends with whom (the server tells it whenever it changes). Turrets spare
    /// the friends of their owner.
    pub fn set_friends(&mut self, pairs: impl IntoIterator<Item = (EntityId, EntityId)>) {
        self.x.friends = pairs.into_iter().map(|(a, b)| (a.min(b), a.max(b))).collect();
    }

    /// Take the events collected since the last call.
    pub fn drain_extra_events(&mut self) -> Vec<ExtraEvent> {
        std::mem::take(&mut self.x.events)
    }

    // ---- building -------------------------------------------------------------------------

    /// Put a construction site at a cell that turns into `result` (`ConstructionKit.build`).
    /// Returns false if the cell is not free.
    pub fn place_construction_site(&mut self, world: &mut World, cell: Cell, result: u8) -> bool {
        if !world.get(cell.0, cell.1, cell.2).is_air() {
            return false;
        }
        if result == ids::LIFT && world.get(cell.0, cell.1, cell.2 - 1).id() != ids::ENTRY {
            return false; // a lift only goes over a cave entry
        }
        let block = Block::new(ids::CONSTRUCTION_SITE, site_value(result));
        if !world.set(cell.0, cell.1, cell.2, block) {
            return false;
        }
        self.x.sites.insert(cell, ConstructionSite::new(result));
        self.x.events.push(ExtraEvent::BlockChanged { cell, block });
        self.x.dirty = true;
        true
    }

    fn set_block(&mut self, world: &mut World, cell: Cell, block: Block) -> bool {
        if !world.set(cell.0, cell.1, cell.2, block) {
            return false;
        }
        self.x.events.push(ExtraEvent::BlockChanged { cell, block });
        self.x.dirty = true;
        true
    }

    // ---- dialogs --------------------------------------------------------------------------

    pub(crate) fn open(&mut self, player: EntityId, dialog: Dialog, source: Source) {
        self.x.events.push(ExtraEvent::DialogOpened { player, dialog: dialog.clone() });
        self.x.dialogs.insert(player, OpenDialog { dialog, source });
    }

    fn close(&mut self, player: EntityId) {
        if self.x.dialogs.remove(&player).is_some() {
            self.x.events.push(ExtraEvent::DialogClosed { player });
        }
    }

    pub(crate) fn sound(&mut self, name: &'static str, position: Vec3) {
        self.events.push(GameEvent::Sound { name, position });
    }

    /// A player answers the dialog they have open (`Action::Choose`).
    pub(crate) fn choose(&mut self, entities: &mut Entities, world: &mut World, id: EntityId, state: &mut PlayerState, answer: u8) {
        let Some(open) = self.x.dialogs.get(&id).cloned() else { return };
        if !open.dialog.accepts(answer) {
            return;
        }
        let position = entities.get(id).map(|e| e.position).unwrap_or(Vec3::ZERO);
        self.close(id);
        match open.source {
            Source::Vanya(vanya) => self.vanya_answer(entities, id, vanya, answer == 1),
            Source::Shop(shop) => self.shop_buy(entities, id, state, shop, answer),
            Source::Site(cell) => self.site_choice(entities, world, id, state, cell, answer),
            Source::Factory(cell) => self.factory_choice(entities, id, cell, answer, &open.dialog),
            Source::Turret(cell) => self.turret_choice(cell, answer),
            Source::Launcher(cell) => self.launcher_choice(entities, world, id, state, cell, answer),
            Source::Toolkit => self.toolkit_build(entities, world, state, position, answer),
            Source::LineKit => self.line_kit_lay(entities, world, id, state, answer),
        }
    }

    /// A player closes their dialog without choosing. Java's cancel button of Vanya's chat goes on
    /// with "no"; the others just close.
    pub(crate) fn cancel(&mut self, entities: &mut Entities, world: &mut World, id: EntityId, state: &mut PlayerState) {
        let Some(open) = self.x.dialogs.get(&id).cloned() else { return };
        if let Source::Vanya(_) = open.source {
            self.choose(entities, world, id, state, 0);
            return;
        }
        self.close(id);
    }

    // ---- interaction ----------------------------------------------------------------------

    /// The characters, flags and machines of the logic blocks the player at `position` could use,
    /// the nearest first.
    pub(crate) fn nearest_extra(&self, entities: &Entities, world: &World, position: Vec3) -> Option<(f32, Target)> {
        let mut best: Option<(f32, Target)> = None;
        let mut offer = |distance: f32, target: Target| {
            if best.is_none_or(|(d, _)| distance < d) {
                best = Some((distance, target));
            }
        };
        // Machines of the logic blocks.
        let origin = cell_at(position);
        for dx in -2..=2 {
            for dy in -4..=4 {
                for dz in -2..=2 {
                    let cell = (origin.0 + dx, origin.1 + dy, origin.2 + dz);
                    let target = match world.get(cell.0, cell.1, cell.2).id() {
                        ids::CONSTRUCTION_SITE => Target::Site(cell),
                        ids::ROBOT_FACTORY => Target::Factory(cell),
                        ids::TURRET if self.x.turrets.contains_key(&cell) => Target::Turret(cell),
                        ids::CATAPULT | ids::CANNON if self.x.launchers.contains_key(&cell) => Target::Launcher(cell),
                        _ => continue,
                    };
                    let d = cell_center(cell).distance(position + Vec3::Z * 0.5);
                    if d <= INTERACT_RADIUS {
                        offer(d, target);
                    }
                }
            }
        }
        // Characters and flags within two blocks, horizontally.
        for (&eid, kind) in &self.kinds {
            let Kind::Other(other) = kind else { continue };
            let Some(entity) = entities.get(eid) else { continue };
            let usable = match &**other {
                Other::Vanya(v) => v.interactable(),
                Other::Shopkeeper | Other::Flag(_) | Other::DropSpaceFlag => true,
                Other::Bird(_) => false,
            };
            let d = Vec2::new(entity.position.x - position.x, entity.position.y - position.y).length();
            if usable && d <= INTERACT_RADIUS {
                offer(d, Target::Entity(eid));
            }
        }
        best
    }

    /// The interactable thing nearest to the player, if one is nearer than the nearest oven (which
    /// the core rules handle). Returns whether something was used.
    pub(crate) fn interact_extra(
        &mut self,
        entities: &mut Entities,
        world: &World,
        id: EntityId,
        state: &mut PlayerState,
        position: Vec3,
    ) -> bool {
        if self.x.dialogs.contains_key(&id) {
            return true; // busy talking
        }
        let Some((distance, target)) = self.nearest_extra(entities, world, position) else { return false };
        // An oven that is closer wins; the core rules use it.
        if let Some(oven) = self.machines_near(world, position).first() {
            if cell_center(*oven).distance(position + Vec3::Z * 0.5) < distance {
                return false;
            }
        }
        match target {
            Target::Site(cell) => self.open_site(id, state, cell, world),
            Target::Factory(cell) => self.open_factory_dialog(entities, id, cell),
            Target::Turret(cell) => self.open_turret_dialog(id, cell),
            Target::Launcher(cell) => self.open_launcher_dialog(id, state, cell, ""),
            Target::Entity(eid) => self.use_entity(entities, id, state, eid),
        }
        true
    }

    fn use_entity(&mut self, entities: &mut Entities, player: EntityId, _state: &mut PlayerState, target: EntityId) {
        let position = entities.get(target).map(|e| e.position).unwrap_or(Vec3::ZERO);
        let actor_position = entities.get(player).map(|e| e.position).unwrap_or(Vec3::ZERO);
        enum Next {
            Chat(Option<npc::Chat>),
            Shop,
            Flag,
            DropFlag,
        }
        let next = match self.kinds.get_mut(&target) {
            Some(Kind::Other(other)) => match &mut **other {
                Other::Vanya(v) => Next::Chat(v.next_chat(true)),
                Other::Shopkeeper => Next::Shop,
                Other::Flag(_) => Next::Flag,
                Other::DropSpaceFlag => Next::DropFlag,
                Other::Bird(_) => return,
            },
            _ => return,
        };
        match next {
            Next::Chat(chat) => {
                if let Some(chat) = chat {
                    self.sound("huhu", position);
                    let dialog = if chat.choice { Dialog::boolean("Vanya", chat.text) } else { Dialog::simple("Vanya", chat.text) };
                    self.open(player, dialog, Source::Vanya(target));
                }
                self.report_tutorial_step(target);
            }
            Next::Shop => {
                self.sound("merchantWelcome", actor_position);
                let options = SHOP_STOCK
                    .iter()
                    .enumerate()
                    .map(|(i, (kind, price))| option(i as u8, format!("Buy {} ({price})", kind.name())))
                    .collect();
                self.open(player, Dialog::selection("Shopkeeper", "Hello!", options), Source::Shop(target));
            }
            Next::Flag => self.capture_flag(entities, player, target),
            Next::DropFlag => {
                self.spawn_collectible(entities, Item::new(CollectibleType::DropSpaceFlagConstructionKit), actor_position);
                if let Some(e) = entities.get_mut(target) {
                    e.dispose();
                }
                self.kinds.remove(&target);
            }
        }
    }

    /// Tell the events about the tutorial step Vanya has reached, if it moved on.
    fn report_tutorial_step(&mut self, vanya: EntityId) {
        if let Some(Kind::Other(other)) = self.kinds.get(&vanya) {
            if let Other::Vanya(v) = &**other {
                let step = v.tutorial_step();
                if step > 0 && !self.x.events.iter().any(|e| matches!(e, ExtraEvent::TutorialStep { step: s } if *s == step)) {
                    self.x.events.push(ExtraEvent::TutorialStep { step });
                }
            }
        }
    }

    fn vanya_answer(&mut self, entities: &mut Entities, player: EntityId, vanya: EntityId, confirm: bool) {
        let position = entities.get(vanya).map(|e| e.position).unwrap_or(Vec3::ZERO);
        let chat = match self.kinds.get_mut(&vanya) {
            Some(Kind::Other(other)) => match &mut **other {
                Other::Vanya(v) => v.next_chat(confirm),
                _ => return,
            },
            _ => return,
        };
        if let Some(chat) = chat {
            self.sound("huhu", position);
            let dialog = if chat.choice { Dialog::boolean("Vanya", chat.text) } else { Dialog::simple("Vanya", chat.text) };
            self.open(player, dialog, Source::Vanya(vanya));
        }
        self.report_tutorial_step(vanya);
    }

    fn shop_buy(&mut self, entities: &mut Entities, player: EntityId, state: &mut PlayerState, shop: EntityId, answer: u8) {
        let position = entities.get(shop).map(|e| e.position).unwrap_or(Vec3::ZERO);
        let Some(&(kind, price)) = SHOP_STOCK.get(answer as usize) else { return };
        if self.money < price || state.inventory.is_full() {
            self.sound("interactionFail", position);
            return;
        }
        self.money -= price;
        state.inventory.add(Item::new(kind));
        self.sound("merchantAha", position);
        self.x.events.push(ExtraEvent::Bought { player, kind, price });
    }

    fn capture_flag(&mut self, entities: &mut Entities, player: EntityId, flag: EntityId) {
        let team = Team::Player;
        let Some(position) = entities.get(flag).map(|e| e.position) else { return };
        if let Some(Kind::Other(other)) = self.kinds.get_mut(&flag) {
            if let Other::Flag(f) = &mut **other {
                if f.team != team {
                    f.team = team;
                    self.x.events.push(ExtraEvent::FlagCaptured { flag, team });
                }
            }
        }
        // The respawn point is the cell the flag hangs over, one lower (`getZ() - 1`).
        let (x, y, z) = cell_at(position);
        let cell = (x, y, z - 1);
        self.x.respawn = Some(cell);
        self.x.events.push(ExtraEvent::RespawnSet { cell });
        let _ = player;
    }

    // ---- construction sites and the toolkit -----------------------------------------------

    fn open_site(&mut self, player: EntityId, state: &PlayerState, cell: Cell, world: &World) {
        let value = world.get(cell.0, cell.1, cell.2).value();
        let site = self.x.sites.entry(cell).or_insert_with(|| ConstructionSite::from_value(value));
        let front = state.inventory.front().map(|i| i.kind);
        let add = match front {
            Some(kind) if site.accepts(kind) => format!("Add: {}", kind.name()),
            _ => "Add: You have nothing to add".to_string(),
        };
        let take = match site.contents().items().last() {
            Some(item) => format!("Take: {}", item.kind.name()),
            None => "Take: Empty".to_string(),
        };
        let options = vec![option(0, add), option(1, take), option(2, format!("Build: {}", site.status()))];
        let name = blocks::ClBlock::from_id(site.result).name();
        self.open(player, Dialog::selection(&format!("Build {name}"), "", options), Source::Site(cell));
    }

    fn site_choice(&mut self, entities: &mut Entities, world: &mut World, player: EntityId, state: &mut PlayerState, cell: Cell, answer: u8) {
        let position = cell_center(cell);
        match answer {
            0 => {
                let Some(site) = self.x.sites.get_mut(&cell) else { return };
                let Some(kind) = state.inventory.front().map(|i| i.kind) else { return };
                if site.accepts(kind) {
                    if let Some(item) = state.inventory.retrieve(0) {
                        site.add(item);
                    }
                }
            }
            1 => {
                let taken = self.x.sites.get_mut(&cell).and_then(|s| s.take_last());
                if let Some(item) = taken {
                    self.spawn_collectible(entities, item, cell_floor(cell));
                }
            }
            2 => {
                self.build_site(entities, world, cell, player);
            }
            _ => {}
        }
        let _ = position;
    }

    /// Turn a site into its machine if everything is in (`ConstructionSite.build`).
    /// The player who does it becomes the owner of a turret.
    fn build_site(&mut self, _entities: &mut Entities, world: &mut World, cell: Cell, player: EntityId) -> bool {
        let Some(site) = self.x.sites.get_mut(&cell) else { return false };
        if !site.can_build() {
            return false;
        }
        let result = site.result;
        site.consume();
        self.x.sites.remove(&cell);
        self.set_block(world, cell, Block::new(result, 0));
        if result == ids::TURRET {
            self.x.turret_owners.insert(cell, player);
        }
        self.x.events.push(ExtraEvent::Built { cell, block: result });
        self.sound("construct", cell_center(cell));
        true
    }

    fn toolkit_build(&mut self, entities: &mut Entities, world: &mut World, state: &mut PlayerState, position: Vec3, result: u8) {
        let cell = cell_at(position);
        let has_kit = state.inventory.front().is_some_and(|i| i.kind == CollectibleType::Toolkit);
        if !has_kit || !toolkit_options().iter().any(|o| o.id == result) {
            return;
        }
        if self.place_construction_site(world, cell, result) {
            state.inventory.retrieve(0); // the kit is used up
            self.sound("metallic", cell_center(cell));
        } else {
            self.sound("interactionFail", position);
        }
        let _ = entities;
    }

    // ---- rails and cable kits -------------------------------------------------------------

    fn line_kit_lay(&mut self, entities: &mut Entities, world: &mut World, player: EntityId, state: &mut PlayerState, value: u8) {
        let Some(mut item) = state.inventory.retrieve(0) else { return };
        let Some(block_id) = kit_block(item.kind) else {
            state.inventory.add_front(item);
            return;
        };
        let Some(entity) = entities.get(player) else {
            state.inventory.add_front(item);
            return;
        };
        let (here, before) = (cell_at(entity.position), entity.position);
        if !world.get(here.0, here.1, here.2).is_air() || !self.set_block(world, here, Block::new(block_id, value)) {
            state.inventory.add_front(item);
            self.sound("interactionFail", before);
            return;
        }
        self.sound("metallic", before);
        item.charges = item.charges.saturating_sub(1);

        // Move the player on to the next cell of the line, facing along it.
        let piece = if block_id == ids::POWER_CABLE { value / 2 } else { value };
        let facing = entities
            .get(player)
            .and_then(|e| e.body.as_ref())
            .map(|b| wurfel_sim::entity::iso_to_screen(b.orientation()))
            .unwrap_or(Vec2::X);
        let continued = continue_line(piece, facing);
        if let Some((side, _)) = continued {
            item.last_dir = side;
        }
        let up = continued.is_some_and(|(_, up)| up);
        let mut next = neighbour(here, item.last_dir);
        if up {
            next.2 += 1;
        }
        if let Some(entity) = entities.get_mut(player) {
            let target = cell_floor(next);
            let direction = (target - before).truncate().normalize_or_zero();
            entity.position = target;
            if let Some(body) = entity.body.as_mut() {
                if direction != Vec2::ZERO {
                    body.set_orientation(direction);
                }
            }
        }
        if item.charges > 0 {
            state.inventory.add_front(item);
        }
    }

    // ---- robot factories ------------------------------------------------------------------

    fn robot_alive(&self, entities: &Entities, factory: &RobotFactory) -> Option<EntityId> {
        factory.linked.filter(|&id| entities.get(id).is_some_and(|e| !e.is_disposed()))
    }

    /// The factory's dialog depends on whether its robot still lives.
    fn open_factory_dialog(&mut self, entities: &Entities, player: EntityId, cell: Cell) {
        let factory = self.x.factories.entry(cell).or_default().clone();
        let dialog = if self.robot_alive(entities, &factory).is_none() {
            Dialog::selection("What do you want to build?", "Drones can only be build at the surface.", factory_options())
        } else {
            Dialog::boolean("Robot in use", "The robot is already in use. Destroy it?")
        };
        self.open(player, dialog, Source::Factory(cell));
    }

    /// A turret's menu: who it shoots at. Like everything built, anybody can change it.
    fn open_turret_dialog(&mut self, player: EntityId, cell: Cell) {
        let Some(turret) = self.x.turrets.get(&cell) else { return };
        let options = TargetMode::ALL.iter().enumerate().map(|(i, mode)| option(i as u8, mode.label())).collect();
        let text = format!("Shoots at: {}", turret.mode.label());
        self.open(player, Dialog::selection("Turret", &text, options), Source::Turret(cell));
    }

    fn turret_choice(&mut self, cell: Cell, answer: u8) {
        let Some(mode) = TargetMode::ALL.get(answer as usize).copied() else { return };
        if let Some(turret) = self.x.turrets.get_mut(&cell) {
            turret.mode = mode;
            self.sound("metallic", cell_center(cell));
        }
    }

    /// Who the turret at `cell` shoots at.
    pub fn turret_mode(&self, cell: Cell) -> Option<TargetMode> {
        self.x.turrets.get(&cell).map(|t| t.mode)
    }

    fn factory_choice(&mut self, entities: &mut Entities, player: EntityId, cell: Cell, answer: u8, dialog: &Dialog) {
        let factory = self.x.factories.entry(cell).or_default().clone();
        let alive = self.robot_alive(entities, &factory);
        if dialog.mode == DialogMode::Boolean {
            if answer == 1 {
                if let Some(robot) = alive {
                    self.damage_entity(entities, robot, 100.0);
                }
            }
            return;
        }
        if alive.is_some() {
            return;
        }
        let Some(variant) = variant_for_option(answer) else { return };
        // The robot stands on top of the factory, not inside it.
        let position = cell_floor((cell.0, cell.1, cell.2 + 1));
        let robot = match variant {
            RobotVariant::Fighter => self.spawn_robot(entities, Team::Player, position),
            RobotVariant::Spider => self.spawn_spider(entities, Team::Player, position),
            RobotVariant::Drone => self.spawn_drone(entities, Team::Player, position),
        };
        if let Some(f) = self.x.factories.get_mut(&cell) {
            f.linked = Some(robot);
        }
        self.sound("construct", position);
        self.x.events.push(ExtraEvent::RobotBuilt { factory: cell, robot, variant });
        let _ = player;
    }

    // ---- the item in hand -----------------------------------------------------------------

    /// Use a kit in hand (`Interactable` collectibles that only work when carried): the toolkit and
    /// the rails and cable kits open a dialog, the drop-space flag kit puts a flag down. Returns true
    /// when the item is used up.
    pub(crate) fn use_kit(&mut self, entities: &mut Entities, world: &World, id: EntityId, item: &Item, position: Vec3) -> bool {
        let cell = cell_at(position);
        let free = world.get(cell.0, cell.1, cell.2).is_air();
        match item.kind {
            CollectibleType::Toolkit => {
                if free {
                    self.open(id, Dialog::selection("Choose construction", "", toolkit_options()), Source::Toolkit);
                } else {
                    self.sound("interactionFail", position);
                }
                false
            }
            CollectibleType::Rails | CollectibleType::Powercable => {
                let Some(block_id) = kit_block(item.kind) else { return false };
                if free {
                    self.open(id, Dialog::selection("Choose direction", "", line_options(block_id, item.last_dir)), Source::LineKit);
                } else {
                    self.sound("interactionFail", position);
                }
                false
            }
            CollectibleType::DropSpaceFlagConstructionKit => {
                self.spawn_drop_space_flag(entities, position);
                true
            }
            _ => false,
        }
    }

    // ---- the step -------------------------------------------------------------------------

    /// Run the machines, characters and the story for `dt` seconds. Called from [`Caveland::tick`].
    pub(crate) fn update_extras(&mut self, entities: &mut Entities, world: &mut World, dt: f32) {
        self.x.since_scan += dt;
        if self.x.dirty || self.x.since_scan >= SCAN_EVERY {
            self.x.since_scan = 0.0;
            self.x.dirty = false;
            self.scan_machines(entities, world);
        }
        self.update_turrets(entities, world, dt);
        self.update_launchers(entities, dt);
        self.update_others(entities, world, dt);
        self.update_spiders(entities, world, dt);
        self.update_scenario(entities, world);
        self.prune_extras(entities);
    }

    /// Look at the world's blocks: find the machines, keep their state in step, work out the power.
    fn scan_machines(&mut self, entities: &mut Entities, world: &mut World) {
        const WATCHED: [u8; 10] = [
            ids::CONSTRUCTION_SITE,
            ids::ROBOT_FACTORY,
            ids::FLAG_POLE,
            ids::TURRET,
            ids::POWER_CABLE,
            ids::POWER_STATION,
            ids::TORCH,
            ids::OVEN,
            ids::CATAPULT,
            ids::CANNON,
        ];
        let found = find_blocks(world, &WATCHED);
        let mut sites = HashSet::new();
        let mut factories = HashSet::new();
        let mut poles = HashSet::new();
        let mut turrets = HashSet::new();
        let mut launchers = HashSet::new();
        for &(cell, id) in &found {
            match id {
                ids::CONSTRUCTION_SITE => {
                    sites.insert(cell);
                    let value = world.get(cell.0, cell.1, cell.2).value();
                    // A site the lift code asked for knows its result already; others carry it in
                    // their block value.
                    let result = self.transport().site_result(cell);
                    self.x.sites.entry(cell).or_insert_with(|| match result {
                        Some(result) => ConstructionSite::new(result),
                        None => ConstructionSite::from_value(value),
                    });
                }
                ids::ROBOT_FACTORY => {
                    factories.insert(cell);
                    self.x.factories.entry(cell).or_default();
                }
                ids::FLAG_POLE => {
                    poles.insert(cell);
                }
                ids::TURRET => {
                    turrets.insert(cell);
                    self.x.turrets.entry(cell).or_insert_with(Turret::new);
                }
                ids::CATAPULT | ids::CANNON => {
                    launchers.insert(cell);
                    if let Some(kind) = LauncherKind::from_block(id) {
                        self.x.launchers.entry(cell).or_insert_with(|| Launcher::new(kind));
                    }
                }
                _ => {}
            }
        }
        // Machines that are gone: a site gives back what was in it.
        let gone: Vec<Cell> = self.x.sites.keys().filter(|c| !sites.contains(*c)).copied().collect();
        for cell in gone {
            if let Some(mut site) = self.x.sites.remove(&cell) {
                for item in site.consume() {
                    self.spawn_sparkling(entities, item, cell_floor(cell));
                }
            }
        }
        // A launcher that was broken gives back its gunpowder. Cells in chunks that are not in
        // memory are not gone, just not seen.
        let broken: Vec<Cell> =
            self.x.launchers.keys().filter(|c| !launchers.contains(*c) && world.is_loaded_at(c.0, c.1)).copied().collect();
        for cell in broken {
            if let Some(launcher) = self.x.launchers.remove(&cell) {
                for _ in 0..launcher.loaded {
                    self.spawn_sparkling(entities, Item::new(CollectibleType::Gunpowder), cell_floor(cell));
                }
            }
        }
        self.x.factories.retain(|c, _| factories.contains(c));
        self.x.turrets.retain(|c, _| turrets.contains(c));
        self.x.turret_owners.retain(|c, _| turrets.contains(c));

        // Flag poles carry a flag above them.
        let lost: Vec<Cell> = self.x.flagpoles.keys().filter(|c| !poles.contains(*c)).copied().collect();
        for cell in lost {
            if let Some(flag) = self.x.flagpoles.remove(&cell) {
                if let Some(e) = entities.get_mut(flag) {
                    e.dispose();
                }
                self.kinds.remove(&flag);
            }
        }
        for &cell in &poles {
            let alive = self.x.flagpoles.get(&cell).is_some_and(|&f| entities.get(f).is_some_and(|e| !e.is_disposed()));
            if !alive {
                let team = self.x.pole_teams.get(&cell).copied().unwrap_or_default();
                let position = cell_floor(cell) + Vec3::Z * FLAG_HEIGHT;
                let flag = self.spawn_flag(entities, team, position);
                self.x.flagpoles.insert(cell, flag);
            }
        }

        // Power.
        let nodes: Vec<(Cell, u8)> = found.iter().copied().filter(|(_, id)| power::NODE_IDS.contains(id)).collect();
        let updates = self.x.grid.rescan_with(world, &nodes);
        for u in updates {
            self.set_block(world, u.cell, Block::new(ids::POWER_CABLE, u.value));
        }
        let now: HashSet<Cell> = nodes
            .iter()
            .filter(|(cell, id)| *id != ids::POWER_CABLE && self.x.grid.is_powered(*cell))
            .map(|(cell, _)| *cell)
            .collect();
        let mut changes: Vec<(Cell, bool)> = now.difference(&self.x.powered_before).map(|&c| (c, true)).collect();
        changes.extend(self.x.powered_before.difference(&now).map(|&c| (c, false)));
        changes.sort_unstable();
        for (cell, powered) in changes {
            self.x.events.push(ExtraEvent::PowerChanged { cell, powered });
        }
        self.x.powered_before = now;
    }

    fn update_turrets(&mut self, entities: &mut Entities, world: &World, dt: f32) {
        let mut cells: Vec<Cell> = self.x.turrets.keys().copied().collect();
        cells.sort_unstable();
        for cell in cells {
            let powered = self.x.grid.is_powered(cell);
            let Some(turret) = self.x.turrets.get_mut(&cell) else { continue };
            turret.update(powered, dt);
            if !turret.is_online() {
                continue;
            }
            let gun = turret.gun_position(cell_floor(cell));
            let mode = turret.mode;
            let owner = self.x.turret_owners.get(&cell).copied();
            let origin = cell_center(cell);
            let friends = &self.x.friends;
            let are_friends = |a: EntityId, b: EntityId| friends.contains(&(a.min(b), a.max(b)));
            // What the turret may shoot at within sight, in the order of the ids.
            let mut candidates = Vec::new();
            for e in entities.iter() {
                if e.is_disposed() {
                    continue;
                }
                let (team, player) = match self.kinds.get(&e.id()) {
                    Some(Kind::Robot(r)) => (r.team, false),
                    Some(Kind::Player(_)) => (Team::Player, true),
                    _ => continue,
                };
                let horizontal = Vec2::new(e.position.x - origin.x, e.position.y - origin.y).length();
                if horizontal >= TURRET_SIGHT {
                    continue;
                }
                let candidate = Candidate { id: e.id(), position: e.position + Vec3::Z * 0.5, team, player };
                if power::should_target(mode, owner, &candidate, &are_friends) {
                    candidates.push(candidate);
                }
            }
            let Some(target) = power::pick_target(world, gun, &candidates) else { continue };
            let fired = self.x.turrets.get_mut(&cell).is_some_and(|t| t.fire());
            if fired {
                self.damage_entity(entities, target.id, power::GUN_DAMAGE);
                self.sound("turret", gun);
                self.x.events.push(ExtraEvent::TurretShot { turret: cell, from: gun, to: target.position });
            }
        }
    }

    fn update_others(&mut self, entities: &mut Entities, world: &mut World, dt: f32) {
        let mut ids_: Vec<EntityId> =
            self.kinds.iter().filter(|(_, k)| matches!(k, Kind::Other(_))).map(|(&id, _)| id).collect();
        ids_.sort_unstable();
        let players: Vec<(EntityId, Vec3)> = {
            let mut v: Vec<_> = self
                .kinds
                .iter()
                .filter(|(_, k)| matches!(k, Kind::Player(_)))
                .filter_map(|(&id, _)| entities.get(id).map(|e| (id, e.position)))
                .collect();
            v.sort_unstable_by_key(|&(id, _)| id);
            v
        };
        for id in ids_ {
            if entities.get(id).is_none_or(|e| e.is_disposed()) {
                continue;
            }
            let Some(Kind::Other(mut other)) = self.kinds.remove(&id) else { continue };
            let position = entities.get(id).map(|e| e.position).unwrap_or(Vec3::ZERO);
            match &mut *other {
                Other::Vanya(vanya) => {
                    // There is only one guide: the one with the lower id stays.
                    let twins: Vec<EntityId> = self
                        .kinds
                        .iter()
                        .filter(|(_, k)| matches!(k, Kind::Other(o) if matches!(**o, Other::Vanya(_))))
                        .map(|(&other_id, _)| other_id)
                        .filter(|&other_id| {
                            entities.get(other_id).is_some_and(|e| !e.is_disposed() && e.position.distance(position) <= VANYA_DUPLICATE_RADIUS)
                        })
                        .collect();
                    for twin in twins {
                        if let Some(e) = entities.get_mut(twin) {
                            e.dispose();
                        }
                    }
                    let look_at = players
                        .iter()
                        .map(|&(_, p)| p)
                        .filter(|p| Vec2::new(p.x - position.x, p.y - position.y).length() <= VANYA_LOOK_RADIUS)
                        .map(|p| Vec2::new(p.x - position.x, p.y - position.y))
                        .next();
                    let before = vanya.tutorial_step();
                    let hopped = entities.get_mut(id).is_some_and(|e| vanya.update(e, world, look_at));
                    if hopped {
                        self.events.push(GameEvent::Sound { name: "vanya_jump", position });
                    }
                    if vanya.tutorial_step() != before {
                        self.x.events.push(ExtraEvent::TutorialStep { step: vanya.tutorial_step() });
                    }
                }
                Other::Bird(bird) => {
                    if let Some(e) = entities.get_mut(id) {
                        bird.update(e, dt);
                    }
                }
                Other::Flag(_) | Other::Shopkeeper | Other::DropSpaceFlag => {}
            }
            self.kinds.insert(id, Kind::Other(other));
        }
        let _ = world;
    }

    /// The ore gatherers: look for ore, walk to it, hit it, carry the loot to a drop-space flag.
    fn update_spiders(&mut self, entities: &mut Entities, world: &mut World, dt: f32) {
        let mut ids_: Vec<EntityId> = self.x.spiders.keys().copied().collect();
        ids_.sort_unstable();
        for id in ids_ {
            let Some(position) = entities.get(id).filter(|e| !e.is_disposed()).map(|e| e.position) else { continue };
            let Some(mut spider) = self.x.spiders.remove(&id) else { continue };
            self.spider_step(entities, world, id, position, &mut spider, dt);
            self.x.spiders.insert(id, spider);
        }
    }

    fn spider_step(&mut self, entities: &mut Entities, world: &mut World, id: EntityId, position: Vec3, spider: &mut Spider, dt: f32) {
        // A walk in progress.
        if let Some(walk) = spider.walking.as_mut() {
            let arrived = entities.get_mut(id).is_none_or(|e| !walk.update(e, world, dt));
            if arrived {
                spider.walking = None;
            }
        }
        // Forget ore that is gone or not ore any more.
        if let Some(cell) = spider.working_block {
            if !enemy::is_resource(world.get(cell.0, cell.1, cell.2).id()) {
                spider.working_block = None;
            }
        }
        // The carried piece rides along.
        if let Some(carry) = spider.carry {
            if entities.get(carry).is_none_or(|e| e.is_disposed()) {
                spider.carry = None;
            } else if let Some(e) = entities.get_mut(carry) {
                e.position = position + Vec3::Z * 0.5;
            }
        }
        spider.charge += dt;
        if spider.working_for > 0.0 {
            spider.working_for = (spider.working_for - dt).max(0.0);
            if let Some(body) = entities.get_mut(id).and_then(|e| e.body.as_mut()) {
                body.set_hor_movement(Vec2::ZERO);
            }
            return;
        }

        if spider.carry.is_some() {
            self.spider_deliver(entities, world, id, position, spider);
        } else {
            self.spider_mine(entities, world, id, position, spider, dt);
        }
    }

    /// Find a drop-space flag, walk to it and put the loot down.
    fn spider_deliver(&mut self, entities: &mut Entities, _world: &mut World, id: EntityId, position: Vec3, spider: &mut Spider) {
        if spider.storage.is_none() {
            spider.storage = self.nearest_drop_flag(entities, position);
            if spider.storage.is_some() {
                self.sound("robotWeep", position);
            }
        }
        let Some(storage) = spider.storage else { return };
        let goal = cell_floor(storage);
        if cell_at(position) == storage || Vec2::new(position.x - goal.x, position.y - goal.y).length() < 0.5 {
            // The flag must still be there.
            if self.drop_flag_at(entities, storage).is_none() {
                spider.storage = None;
                return;
            }
            if let Some(carry) = spider.carry.take() {
                self.set_pickup_allowed(carry, true);
                if let Some(e) = entities.get_mut(carry) {
                    e.position = goal;
                }
            }
            spider.walking = None;
            if let Some(body) = entities.get_mut(id).and_then(|e| e.body.as_mut()) {
                body.set_hor_movement(Vec2::ZERO);
            }
        } else if spider.walking.is_none() {
            spider.walking = Some(MoveToAi::new(goal));
        }
    }

    fn spider_mine(&mut self, entities: &mut Entities, world: &mut World, id: EntityId, position: Vec3, spider: &mut Spider, dt: f32) {
        if spider.working_block.is_none() {
            spider.scan_in -= dt;
            if spider.scan_in <= 0.0 {
                spider.scan_in = SPIDER_SCAN_EVERY;
                let found = nearby_resources(world, position);
                if !found.is_empty() {
                    let pick = (self.rng.next_float() * found.len() as f32) as usize;
                    spider.working_block = Some(found[pick.min(found.len() - 1)]);
                    self.sound("robotWeep", position);
                }
            }
        }
        // Know where to bring things while looking around.
        if spider.storage.is_none() {
            spider.storage = self.nearest_drop_flag(entities, position);
        }
        let Some(cell) = spider.working_block else { return };
        let target = cell_center(cell);
        if position.distance(target) < SPIDER_REACH {
            spider.walking = None;
            if let Some(body) = entities.get_mut(id).and_then(|e| e.body.as_mut()) {
                body.set_hor_movement(Vec2::ZERO);
            }
            if spider.charge >= SPIDER_CHARGE {
                spider.charge = 0.0;
                spider.working_for = SPIDER_WORK;
                let block = world.get(cell.0, cell.1, cell.2);
                self.damage_block(world, entities, cell, 1);
                self.sound("impact", position);
                let stored = world.block_health(cell.0, cell.1, cell.2);
                let health = if stored == 0 { 100 } else { stored };
                if world.get(cell.0, cell.1, cell.2).id() == block.id() && drops_loot_at(health) {
                    if let Some(kind) = blocks::loot(block.id()) {
                        let piece = self.spawn_collectible(entities, Item::new(kind), position + Vec3::Z * 0.5);
                        self.set_pickup_allowed(piece, false);
                        spider.carry = Some(piece);
                    }
                }
            }
        } else if spider.walking.is_none() {
            // An ore below the spider is approached from the side.
            let mut goal_cell = cell;
            if cell.2 < cell_at(position).2 {
                goal_cell = (cell.0 + 1, cell.1, cell.2);
            }
            spider.walking = Some(MoveToAi::new(cell_center(goal_cell)));
        }
    }

    pub(crate) fn set_pickup_allowed(&mut self, collectible: EntityId, allowed: bool) {
        if let Some(Kind::Collectible(c)) = self.kinds.get_mut(&collectible) {
            c.no_pickup = !allowed;
        }
    }

    fn drop_flag_at(&self, entities: &Entities, cell: Cell) -> Option<EntityId> {
        self.kinds.iter().find_map(|(&id, k)| match k {
            Kind::Other(o) if matches!(**o, Other::DropSpaceFlag) => {
                entities.get(id).filter(|e| !e.is_disposed() && cell_at(e.position) == cell).map(|_| id)
            }
            _ => None,
        })
    }

    fn nearest_drop_flag(&self, entities: &Entities, position: Vec3) -> Option<Cell> {
        let mut best: Option<(f32, Cell)> = None;
        for (&id, k) in &self.kinds {
            if !matches!(k, Kind::Other(o) if matches!(**o, Other::DropSpaceFlag)) {
                continue;
            }
            let Some(e) = entities.get(id).filter(|e| !e.is_disposed()) else { continue };
            let d = e.position.distance(position);
            if d <= SPIDER_FLAG_SIGHT && best.is_none_or(|(bd, _)| d < bd) {
                best = Some((d, cell_at(e.position)));
            }
        }
        best.map(|(_, c)| c)
    }

    /// The tutorial and the fight at its end (`CLGameController.update`).
    fn update_scenario(&mut self, entities: &mut Entities, world: &mut World) {
        if !self.x.scenario.enabled {
            return;
        }
        let mut players: Vec<(u8, Vec3)> = self
            .kinds
            .iter()
            .filter_map(|(&id, k)| match k {
                Kind::Player(p) => entities.get(id).map(|e| (p.number, e.position)),
                _ => None,
            })
            .collect();
        players.sort_by_key(|&(n, _)| n);
        let Some(&(_, first)) = players.first() else { return };

        // The guide: the one near the first player, or a new one at the start.
        let known = self.x.scenario.vanya.filter(|&v| entities.get(v).is_some_and(|e| !e.is_disposed()));
        if known.is_none() {
            let near = self
                .kinds
                .iter()
                .filter(|(_, k)| matches!(k, Kind::Other(o) if matches!(**o, Other::Vanya(_))))
                .filter_map(|(&id, _)| entities.get(id).filter(|e| !e.is_disposed()).map(|e| (id, e.position)))
                .find(|(_, p)| p.distance(first) <= VANYA_DUPLICATE_RADIUS)
                .map(|(id, _)| id);
            self.x.scenario.vanya = Some(match near {
                Some(id) => id,
                None => self.spawn_vanya(entities, cell_floor(VANYA_SPAWN)),
            });
        }
        let vanya = self.x.scenario.vanya.expect("set above");

        // Down in the caves the tutorial is over.
        if players.iter().any(|&(_, p)| cell_at(p).1 > wurfel_sim::caveland::CAVES_BORDER) {
            self.set_tutorial_step(entities, 4);
        }

        // Crossing into the end zone starts the fight.
        if !self.x.scenario.end_fight_started {
            let (min, max) = FIGHT_ZONE;
            let inside = |c: Cell| (min.0..=max.0).contains(&c.0) && (min.1..=max.1).contains(&c.1) && (min.2..=max.2).contains(&c.2);
            if players.iter().any(|&(_, p)| inside(cell_at(p))) {
                self.x.scenario.end_fight_started = true;
                self.set_tutorial_step(entities, 5);
                for _ in 0..FIGHT_ROBOTS {
                    let x = (34.0 + self.rng.next_float() * 5.0) as i32;
                    let y = (4.0 + self.rng.next_float() * 5.0) as i32;
                    // `new Robot()` has no team: these robots stand around, they do not attack.
                    self.spawn_robot(entities, Team::Neutral, cell_floor((x, y, 5)));
                }
                self.x.events.push(ExtraEvent::EndFightStarted);
            }
        }
        let _ = (vanya, world);
    }

    /// Forget what belongs to things that are gone, and close dialogs nobody can answer.
    fn prune_extras(&mut self, entities: &mut Entities) {
        let gone = |entities: &Entities, id: EntityId| entities.get(id).is_none_or(|e| e.is_disposed());
        let dead_kinds: Vec<EntityId> = self
            .kinds
            .iter()
            .filter(|(_, k)| matches!(k, Kind::Other(_)))
            .map(|(&id, _)| id)
            .filter(|&id| gone(entities, id))
            .collect();
        for id in dead_kinds {
            self.kinds.remove(&id);
        }
        // A turret whose builder has left has no owner any more.
        self.x.turret_owners.retain(|_, owner| !gone(entities, *owner));
        let dead_spiders: Vec<EntityId> = self.x.spiders.keys().copied().filter(|&id| gone(entities, id)).collect();
        for id in dead_spiders {
            if let Some(spider) = self.x.spiders.remove(&id) {
                if let Some(carry) = spider.carry {
                    self.set_pickup_allowed(carry, true); // a lost robot drops what it carried
                }
            }
        }
        self.x.variants.retain(|&id, _| !gone(entities, id));
        let stale: Vec<EntityId> = self
            .x
            .dialogs
            .iter()
            .filter(|(&player, open)| {
                gone(entities, player)
                    || match open.source {
                        Source::Vanya(v) | Source::Shop(v) => gone(entities, v),
                        _ => false,
                    }
            })
            .map(|(&player, _)| player)
            .collect();
        for player in stale {
            self.close(player);
        }
    }

    // ---- saving ---------------------------------------------------------------------------

    /// What has to be kept between runs, as JSON. The map's blocks are saved by the engine; this is
    /// the rest (`money`, `respawnX/Y/Z`, machine contents, how far the story has got).
    pub fn save_state(&self, entities: &Entities) -> String {
        let mut ovens: Vec<(Cell, OvenLogic)> = self.ovens.iter().map(|(&c, o)| (c, o.clone())).collect();
        ovens.sort_by_key(|(c, _)| *c);
        let mut sites: Vec<(Cell, ConstructionSite)> = self.x.sites.iter().map(|(&c, s)| (c, s.clone())).collect();
        sites.sort_by_key(|(c, _)| *c);
        let mut flags: Vec<(Cell, u8)> = self
            .x
            .flagpoles
            .iter()
            .map(|(&pole, &flag)| {
                let team = match self.kinds.get(&flag) {
                    Some(Kind::Other(o)) => match &**o {
                        Other::Flag(f) => f.team,
                        _ => Team::Neutral,
                    },
                    _ => self.x.pole_teams.get(&pole).copied().unwrap_or_default(),
                };
                (pole, team.id())
            })
            .collect();
        flags.sort_by_key(|(c, _)| *c);
        let mut launchers: Vec<(Cell, Launcher)> = self.x.launchers.iter().map(|(&c, l)| (c, *l)).collect();
        launchers.sort_by_key(|(c, _)| *c);
        let data = SaveData {
            money: self.money,
            respawn: self.x.respawn,
            ovens,
            sites,
            tutorial_step: self.tutorial_step(entities).unwrap_or(0),
            end_fight_started: self.x.scenario.end_fight_started,
            flags,
            launchers,
        };
        serde_json::to_string(&data).expect("plain data")
    }

    /// Restore what [`Caveland::save_state`] wrote. The guide's step is applied when it exists.
    pub fn load_state(&mut self, entities: &Entities, json: &str) -> Result<(), String> {
        let data: SaveData = serde_json::from_str(json).map_err(|e| format!("unreadable caveland save: {e}"))?;
        self.money = data.money;
        self.x.respawn = data.respawn;
        self.ovens = data.ovens.into_iter().collect();
        self.x.sites = data.sites.into_iter().collect();
        self.x.scenario.end_fight_started = data.end_fight_started;
        self.x.launchers = data.launchers.into_iter().collect();
        self.x.pole_teams = data.flags.into_iter().map(|(c, t)| (c, Team::from_id(t))).collect();
        if data.tutorial_step > 0 {
            self.set_tutorial_step(entities, data.tutorial_step);
        }
        Ok(())
    }
}
