//! The Caveland game mode of the server: the glue between [`caveland_sim::Caveland`] and the
//! network. The rules live in `caveland-sim`; this file turns what they report into messages and
//! turns client messages into rule calls.
//!
//! Everything a mode says to the clients travels in the engine's generic messages: block changes as
//! `BlockSet`, the other entities as `Things`, and the rest as `Rules` with a `kind` of `state`
//! (health and pack of every player, sent when it changes) or `events` (sounds and happenings).

use std::collections::{HashMap, HashSet};
use std::io;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use caveland_sim::collectible::{CollectibleType, Item};
use caveland_sim::commands::{CommandOutcome, COMMANDS};
use caveland_sim::crafting::RecipeResult;
use caveland_sim::{Action, Caveland, Controls, DialogMode, EntityKind, ExtraEvent, GameEvent, Team, TransportEvent, Tuning};
use glam::Vec3;
use serde_json::{json, Value};
use wurfel_sim::entity::physics::ground_height;
use wurfel_sim::entity::{Entities, EntityId};
use wurfel_sim::generator::{create_generator, Generator};
use wurfel_sim::grid::{from_iso, to_iso};
use wurfel_sim::player::PlayerInput;
use wurfel_sim::protocol::{Edit, ServerMsg, ThingState};
use wurfel_sim::{Block, World, CHUNK_SIZE_X, CHUNK_SIZE_Y, CHUNK_SIZE_Z};

/// Set by `--skip-intro`: a new game starts on the ground instead of in the crashing spaceship.
pub static SKIP_INTRO: AtomicBool = AtomicBool::new(false);

/// The file next to a save slot's chunks that keeps what the blocks do not: money, machines,
/// the respawn point and whether the intro has been seen.
const SIDECAR: &str = "caveland.json";
/// Things further than this (in blocks) from every player are not sent.
const THING_RANGE: f32 = 80.0;
/// The set of powered cells is repeated this often (in ticks) for players who joined later.
const POWER_EVERY: u64 = 180;
/// Where the intro spaceship starts relative to the first player, and how high. It flies in along
/// the diagonal towards `SHIP_TARGET` (relative to the player): the ship starts to fall 25 blocks
/// short of its target and glides on, which puts the crash close to where the players start.
const SHIP_OFFSET: Vec3 = Vec3::new(-42.0, -42.0, 12.0);
const SHIP_TARGET: Vec3 = Vec3::ZERO;

/// Things go out with every n-th tick, like the player snapshots.
const THINGS_EVERY: u64 = 2;
/// The players' state is checked this often (ten times a second) and only sent when it changed.
const STATE_EVERY: u64 = 6;
/// Most things sent at once. A cave full of items would otherwise swamp the connection.
const MAX_THINGS: usize = 256;

pub struct CavelandMode {
    caveland: Caveland,
    /// Messages for everybody, collected during a tick and sent by the server loop.
    outbox: Vec<ServerMsg>,
    /// What the last `state` message said, to send only changes.
    last_state: String,
    /// Where players (re)start.
    spawn: Option<Vec3>,
    /// The first player finds a few things lying around.
    seeded: bool,
    /// Players that are alive; a death is only answered with a respawn for these.
    numbers: HashMap<EntityId, u8>,
    /// The story of the Caveland map (guide, tutorial, end fight, intro) runs.
    scenario: bool,
    /// Makes what the generator wants to spawn (portals, carts) for every chunk that comes into
    /// memory; none of it is saved, the map says where it goes.
    spawner: Option<Box<dyn Generator>>,
    scanned: HashSet<(i32, i32)>,
    /// The intro (the spaceship crash) has been seen on this save.
    intro_done: bool,
    ship: Option<EntityId>,
    /// Players in a vehicle: the server moves them, so the client must not predict.
    riding: HashSet<EntityId>,
    /// A lift site offered to a player, waiting for yes or no.
    pending_lift: HashMap<EntityId, (i32, i32, i32)>,
    /// Where the last used flag sends players back to.
    flag_respawn: Option<Vec3>,
    /// Cells of torches, turrets and stations that have power.
    powered: HashSet<(i32, i32, i32)>,
    powered_dirty: bool,
}

impl CavelandMode {
    /// Make `world` follow Caveland's block rules and start the ruleset.
    pub fn new(world: &mut World, seed: u64) -> Self {
        Caveland::install(world);
        CavelandMode {
            caveland: Caveland::new(Tuning::default(), seed as i64),
            outbox: Vec::new(),
            last_state: String::new(),
            spawn: None,
            seeded: false,
            numbers: HashMap::new(),
            scenario: false,
            spawner: None,
            scanned: HashSet::new(),
            intro_done: false,
            ship: None,
            riding: HashSet::new(),
            pending_lift: HashMap::new(),
            flag_respawn: None,
            powered: HashSet::new(),
            powered_dirty: false,
        }
    }

    /// Tell the mode which map it plays on: only the Caveland generator has the story and the caves
    /// that the generator fills with portals.
    pub fn configure(&mut self, generator: &str, seed: u64) {
        self.scenario = generator == "caveland";
        self.caveland.set_scenario(self.scenario);
        self.spawner = create_generator(generator, seed);
    }

    /// Who is friends with whom: turrets spare the friends of their owner.
    pub fn set_friends(&mut self, pairs: &[(u32, u32)]) {
        self.caveland.set_friends(pairs.iter().copied());
    }

    #[cfg(test)]
    pub fn caveland(&self) -> &Caveland {
        &self.caveland
    }

    /// Add a player at `spot`. The first one also finds some things to pick up and an enemy to
    /// fight, so there is something to do.
    pub fn spawn_player(&mut self, entities: &mut Entities, world: &World, spot: Vec3) -> EntityId {
        let number = self.numbers.len().min(u8::MAX as usize) as u8;
        let id = self.caveland.spawn_player(entities, number, spot);
        self.numbers.insert(id, number);
        self.spawn.get_or_insert(spot);
        if !self.seeded {
            self.seeded = true;
            self.scatter(entities, world, spot);
        }
        if self.intro_wanted() {
            self.board_ship(entities, id, spot);
        }
        id
    }

    pub fn remove_player(&mut self, id: EntityId) {
        self.numbers.remove(&id);
        self.riding.remove(&id);
        self.pending_lift.remove(&id);
    }

    /// A new game starts with the crash of the spaceship, until it has happened once on this save.
    fn intro_wanted(&self) -> bool {
        let crashed = self.ship.is_some_and(|s| self.caveland.transport().ship(s).is_some_and(|ship| ship.crashed));
        self.scenario && !self.intro_done && !SKIP_INTRO.load(Ordering::Relaxed) && !crashed
    }

    /// The ship comes in from the side and crashes where the first player was going to stand;
    /// everybody who joins before it lands is on board.
    fn board_ship(&mut self, entities: &mut Entities, player: EntityId, spot: Vec3) {
        let ship = match self.ship.filter(|&s| entities.get(s).is_some()) {
            Some(ship) => ship,
            None => {
                let (x, y) = from_iso(spot.x + SHIP_TARGET.x, spot.y + SHIP_TARGET.y);
                let start = spot + SHIP_OFFSET;
                let start = Vec3::new(start.x, start.y, start.z.min(CHUNK_SIZE_Z as f32 - 2.0));
                let ship = self.caveland.transport_mut().spawn_spaceship(entities, start);
                self.caveland.transport_mut().enable_crash(entities, ship, (x, y, spot.z.floor() as i32));
                self.ship = Some(ship);
                ship
            }
        };
        self.caveland.transport_mut().add_ship_content(entities, ship, player);
    }

    /// Make what the generator wants (portals, carts) for chunks that came into memory.
    fn scan_new_chunks(&mut self, entities: &mut Entities, world: &World) {
        let Some(spawner) = self.spawner.as_ref() else { return };
        let fresh: Vec<(i32, i32)> = world.loaded_chunks().map(|c| c.pos()).filter(|p| !self.scanned.contains(p)).collect();
        for (cx, cy) in fresh {
            self.scanned.insert((cx, cy));
            for x in cx * CHUNK_SIZE_X..(cx + 1) * CHUNK_SIZE_X {
                for y in cy * CHUNK_SIZE_Y..(cy + 1) * CHUNK_SIZE_Y {
                    for z in 0..CHUNK_SIZE_Z {
                        for spawn in spawner.spawn_entities(x, y, z) {
                            self.caveland.spawn_from_generator(entities, &spawn);
                        }
                    }
                }
            }
        }
    }

    /// A few tools near the start and one evil robot a little further away.
    fn scatter(&mut self, entities: &mut Entities, world: &World, spot: Vec3) {
        use CollectibleType::*;
        let at = |dx: f32, dy: f32| {
            let (x, y) = from_iso(spot.x + dx, spot.y + dy);
            let (gx, gy) = to_iso(x, y);
            Vec3::new(gx, gy, ground_height(world, x, y) + 0.5)
        };
        for (i, kind) in [Wood, Coal, Torch, Explosives, Iron, Iron].into_iter().enumerate() {
            let angle = i as f32 * std::f32::consts::TAU / 6.0;
            let position = at(angle.cos() * 2.5, angle.sin() * 2.5);
            self.caveland.spawn_collectible(entities, Item::new(kind), position);
        }
        let robot = at(7.0, 3.0);
        self.caveland.spawn_robot(entities, Team::Robots, robot);
        // The story map has a little camp at the crash site: a shop, a flag that sets the respawn
        // point, and a bird. (The Java game keeps these in its saved map; the generator makes none.)
        if self.scenario {
            let shop = at(2.0, -1.5);
            self.caveland.spawn_shopkeeper(entities, shop);
            let flag = at(-2.5, 1.5) + Vec3::Z;
            self.caveland.spawn_flag(entities, Team::Neutral, flag);
            let bird = at(0.0, -4.0) + Vec3::Z * 2.0;
            self.caveland.spawn_bird(entities, bird);
        }
    }

    /// What a player holds down this tick.
    pub fn controls(&mut self, entities: &mut Entities, world: &World, id: EntityId, input: PlayerInput) {
        let controls = Controls { up: input.up, down: input.down, left: input.left, right: input.right, jump: input.jump };
        self.caveland.set_controls(entities, world, id, controls);
    }

    /// A client's one-off action. Unknown names and nonsense arguments are ignored.
    pub fn act(&mut self, entities: &mut Entities, world: &mut World, id: EntityId, name: &str, arg: i32) {
        let action = match name {
            "attack" => Action::Attack,
            "release_attack" => Action::ReleaseAttack,
            "prepare_throw" => Action::PrepareThrow,
            "throw" => Action::Throw,
            "drop" => Action::Drop,
            "use" => Action::UseItem,
            "interact" => Action::Interact,
            "switch_left" => Action::SwitchItems { left: true },
            "switch_right" => Action::SwitchItems { left: false },
            "craft" => match usize::try_from(arg) {
                Ok(index) => Action::Craft(index),
                Err(_) => return,
            },
            "choose" => match u8::try_from(arg) {
                Ok(option) => Action::Choose(option),
                Err(_) => return,
            },
            "cancel" => Action::Cancel,
            // The answer to the offer of a lift construction site.
            "confirm_lift" => {
                if let Some(site) = self.pending_lift.remove(&id) {
                    self.caveland.confirm_lift_site(world, site);
                }
                return;
            }
            "decline_lift" => {
                self.pending_lift.remove(&id);
                return;
            }
            _ => return,
        };
        self.caveland.act(entities, world, id, action);
    }

    /// One fixed step of the rules. `tick` is the server's step counter after this step.
    pub fn tick(&mut self, entities: &mut Entities, world: &mut World, tick: u64, dt: f32) {
        self.scan_new_chunks(entities, world);
        let events = self.caveland.tick(entities, world, dt);
        let (mut edits, mut happenings) = (Vec::new(), Vec::new());
        for event in &events {
            match event {
                GameEvent::BlockDestroyed { cell, .. } => edits.push(Edit { x: cell.0, y: cell.1, z: cell.2, block: 0 }),
                GameEvent::ItemPlaced { cell, block } => {
                    edits.push(Edit { x: cell.0, y: cell.1, z: cell.2, block: Block::new(*block, 0).raw() })
                }
                GameEvent::PlayerDied { player } => {
                    self.respawn(entities, *player);
                    happenings.push(describe(event));
                }
                other => happenings.push(describe(other)),
            }
        }
        // What the rest of the rules report: dialogs and notices, exact block changes, vehicles.
        for event in self.caveland.drain_extra_events() {
            self.extra_event(event, &mut edits, &mut happenings);
        }
        for event in self.caveland.drain_transport_events() {
            self.transport_event(event, &mut happenings);
        }
        for edit in edits {
            self.outbox.push(ServerMsg::BlockSet(edit));
        }
        happenings.retain(|h| !h.is_null());
        if !happenings.is_empty() {
            self.outbox.push(ServerMsg::Rules { kind: "events".into(), data: Value::Array(happenings) });
        }
        if tick % THINGS_EVERY == 0 {
            self.outbox.push(self.things_message(entities, tick));
        }
        if self.powered_dirty || tick % POWER_EVERY == 0 {
            self.powered_dirty = false;
            let mut cells: Vec<_> = self.powered.iter().copied().collect();
            cells.sort_unstable();
            cells.truncate(512);
            self.outbox.push(ServerMsg::Rules { kind: "power".into(), data: json!({ "cells": cells }) });
        }
        if tick % STATE_EVERY == 0 {
            let state = self.state(entities).to_string();
            if state != self.last_state {
                self.outbox.push(ServerMsg::Rules { kind: "state".into(), data: serde_json::from_str(&state).expect("just made") });
                self.last_state = state;
            }
        }
    }

    /// A player who died comes back at the start, under the same id, with an empty pack.
    fn respawn(&mut self, entities: &mut Entities, id: EntityId) {
        let Some(&number) = self.numbers.get(&id) else { return }; // they left in the meantime
        let spot = self.flag_respawn.or(self.spawn).unwrap_or(Vec3::new(0.0, 0.0, 10.0));
        self.caveland.spawn_player_as(entities, id, number, spot);
        self.riding.remove(&id);
    }

    /// A message for one player. Everybody receives it (the server only has a broadcast), the
    /// clients keep what has their id in `to`.
    fn private(&mut self, player: EntityId, kind: &str, mut data: Value) {
        data["to"] = json!(player);
        self.outbox.push(ServerMsg::Rules { kind: kind.into(), data });
    }

    fn extra_event(&mut self, event: ExtraEvent, edits: &mut Vec<Edit>, happenings: &mut Vec<Value>) {
        match event {
            ExtraEvent::DialogOpened { player, dialog } => {
                let mode = match dialog.mode {
                    DialogMode::Simple => "simple",
                    DialogMode::Boolean => "boolean",
                    DialogMode::Selection => "selection",
                };
                let options: Vec<Value> = dialog.options.iter().map(|o| json!({"id": o.id, "label": o.label})).collect();
                let money = self.caveland.money();
                self.private(player, "dialog", json!({"title": dialog.title, "text": dialog.text, "mode": mode, "options": options, "money": money}));
            }
            ExtraEvent::DialogClosed { player } => self.private(player, "dialog_closed", json!({})),
            ExtraEvent::BlockChanged { cell, block } => edits.push(Edit { x: cell.0, y: cell.1, z: cell.2, block: block.raw() }),
            ExtraEvent::Built { cell, block } => happenings.push(json!({"t": "built", "cell": [cell.0, cell.1, cell.2], "block": block})),
            ExtraEvent::Bought { player, kind, price } => happenings.push(json!({"t": "bought", "player": player, "item": kind.name(), "price": price})),
            ExtraEvent::FlagCaptured { flag, team } => happenings.push(json!({"t": "flag", "flag": flag, "team": team.id()})),
            ExtraEvent::RespawnSet { cell } => {
                self.flag_respawn = Some(self.caveland.respawn_position());
                happenings.push(json!({"t": "respawn_set", "cell": [cell.0, cell.1, cell.2]}));
            }
            ExtraEvent::RobotBuilt { factory, robot, variant } => {
                happenings.push(json!({"t": "robot_built", "robot": robot, "variant": format!("{variant:?}").to_lowercase(), "cell": [factory.0, factory.1, factory.2]}))
            }
            ExtraEvent::PowerChanged { cell, powered } => {
                let changed = if powered { self.powered.insert(cell) } else { self.powered.remove(&cell) };
                self.powered_dirty |= changed;
            }
            ExtraEvent::TurretShot { from, to, .. } => happenings.push(json!({"t": "shot", "from": pos(from), "to": pos(to)})),
            ExtraEvent::TutorialStep { step } => happenings.push(json!({"t": "tutorial", "step": step})),
            ExtraEvent::EndFightStarted => happenings.push(json!({"t": "end_fight"})),
        }
    }

    fn transport_event(&mut self, event: TransportEvent, happenings: &mut Vec<Value>) {
        match event {
            TransportEvent::Teleported { entity, to } => happenings.push(json!({"t": "teleported", "entity": entity, "pos": pos(to)})),
            TransportEvent::Boarded { cart, passenger } => {
                self.riding.insert(passenger);
                happenings.push(json!({"t": "boarded", "cart": cart, "player": passenger}));
            }
            TransportEvent::Left { cart, passenger } => {
                self.riding.remove(&passenger);
                happenings.push(json!({"t": "left", "cart": cart, "player": passenger}));
            }
            TransportEvent::SoundStopped { name, entity } => happenings.push(json!({"t": "sound_stop", "name": name, "entity": entity})),
            TransportEvent::ShipCrashed { position, .. } => happenings.push(json!({"t": "ship_crashed", "pos": pos(position)})),
            TransportEvent::IntroCutsceneCompleted => self.intro_done = true,
            TransportEvent::LiftSiteOffered { player, site, .. } => {
                self.pending_lift.insert(player, site);
                self.private(player, "lift_offer", json!({"site": [site.0, site.1, site.2]}));
            }
        }
    }

    /// A console line of a player. Only the host (the player who has been here longest) may run
    /// them: `give` and `tpplayer` are cheats.
    pub fn command(&mut self, entities: &mut Entities, player: EntityId, line: &str, host: bool) {
        let line = line.trim().trim_start_matches(['/', ':']);
        let name = line.split_whitespace().next().unwrap_or("");
        let reply = if !COMMANDS.iter().any(|(n, _)| *n == name) {
            Err(format!("unknown command '{name}' (try: {})", COMMANDS.iter().map(|(n, _)| *n).collect::<Vec<_>>().join(", ")))
        } else if !host {
            Err("only the host can use commands".to_string())
        } else {
            match self.caveland.run_command(entities, player, line) {
                Ok(CommandOutcome::Done(text)) => Ok(text.to_string()),
                Ok(CommandOutcome::PortalTarget(cell)) => {
                    // The portal nearest to the player is the selected one.
                    let at = entities.get(player).map(|e| e.position).unwrap_or_default();
                    let nearest = self
                        .caveland
                        .things()
                        .into_iter()
                        .filter(|(_, k)| matches!(k, EntityKind::Portal | EntityKind::ExitPortal))
                        .filter_map(|(id, _)| entities.get(id).map(|e| (e.position.distance(at), id)))
                        .min_by(|a, b| a.0.total_cmp(&b.0));
                    match nearest {
                        Some((_, id)) if self.caveland.transport_mut().set_portal_target(&[id], cell) => Ok("portal target set".to_string()),
                        _ => Err("no portal nearby".to_string()),
                    }
                }
                Err(e) => Err(e),
            }
        };
        let (ok, text) = match reply {
            Ok(text) => (true, text),
            Err(text) => (false, text),
        };
        self.private(player, "console", json!({"ok": ok, "text": text}));
    }

    /// Keep what the blocks do not: money, machines, the respawn point, the intro. Next to the save
    /// slot's chunks.
    pub fn save(&self, dir: &Path, entities: &Entities) -> io::Result<()> {
        let state: Value = serde_json::from_str(&self.caveland.save_state(entities)).map_err(io::Error::other)?;
        let sidecar = json!({
            "intro_done": self.intro_done,
            "respawn": self.flag_respawn.map(|p| p.to_array()),
            "state": state,
        });
        std::fs::create_dir_all(dir)?;
        std::fs::write(dir.join(SIDECAR), sidecar.to_string())
    }

    /// Read what [`CavelandMode::save`] wrote. A slot without the file starts fresh; one that cannot
    /// be read is reported and also starts fresh (the file is not touched until the next save).
    pub fn load(&mut self, dir: &Path, entities: &Entities) -> Result<(), String> {
        let text = match std::fs::read_to_string(dir.join(SIDECAR)) {
            Ok(text) => text,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(format!("{}: {e}", dir.join(SIDECAR).display())),
        };
        let sidecar: Value = serde_json::from_str(&text).map_err(|e| format!("{}: {e}", dir.join(SIDECAR).display()))?;
        self.intro_done = sidecar["intro_done"].as_bool().unwrap_or(false);
        self.flag_respawn = serde_json::from_value::<Option<[f32; 3]>>(sidecar["respawn"].clone()).ok().flatten().map(Vec3::from);
        self.caveland.load_state(entities, &sidecar["state"].to_string())
    }

    fn things_message(&self, entities: &Entities, tick: u64) -> ServerMsg {
        let players: Vec<Vec3> = self.numbers.keys().filter_map(|&id| entities.get(id).map(|e| e.position)).collect();
        let things = self
            .caveland
            .things()
            .into_iter()
            .filter_map(|(id, kind): (EntityId, EntityKind)| {
                let e = entities.get(id)?;
                players.iter().any(|p| p.distance(e.position) <= THING_RANGE).then(|| ThingState { id, kind: kind.name(), pos: e.position.to_array() })
            })
            .take(MAX_THINGS)
            .collect();
        ServerMsg::Things { tick, things }
    }

    /// Everybody's health and pack: `{"<player id>": {health, jetpack, items, recipes}}`.
    pub fn state(&self, entities: &Entities) -> Value {
        let mut players = serde_json::Map::new();
        let mut ids: Vec<_> = self.numbers.keys().copied().collect();
        ids.sort_unstable();
        for id in ids {
            let Some(view) = self.caveland.player_view(entities, id) else { continue };
            players.insert(
                id.to_string(),
                json!({
                    "health": view.health.round(),
                    "jetpack": (view.jetpack * 100.0).round() / 100.0,
                    "items": view.items,
                    "recipes": view.recipes.iter().map(|(name, ok)| json!([name, ok])).collect::<Vec<_>>(),
                    // Hidden players (inside the spaceship) are not drawn; riders are moved by the
                    // server, so the client does not predict them.
                    "hidden": self.caveland.is_hidden(id),
                    "riding": self.riding.contains(&id) || self.caveland.is_hidden(id),
                    "money": self.caveland.money(),
                }),
            );
        }
        Value::Object(players)
    }

    /// Take what the last steps want to tell everybody.
    pub fn drain_outbox(&mut self) -> Vec<ServerMsg> {
        std::mem::take(&mut self.outbox)
    }
}

fn round2(v: f32) -> f64 {
    ((v * 100.0).round() / 100.0) as f64
}

fn pos(p: Vec3) -> Value {
    json!([round2(p.x), round2(p.y), round2(p.z)])
}

/// A game event as the client gets it. `Null` for the ones that have no use on the client.
fn describe(event: &GameEvent) -> Value {
    match event {
        GameEvent::Sound { name, position } => json!({"t": "sound", "name": name, "pos": pos(*position)}),
        GameEvent::BlockDamaged { cell, .. } | GameEvent::HardHit { cell } => {
            let (gx, gy) = to_iso(cell.0, cell.1);
            json!({"t": "dust", "pos": pos(Vec3::new(gx, gy, cell.2 as f32 + 0.5))})
        }
        GameEvent::Explosion { position, radius, .. } => json!({"t": "explosion", "pos": pos(*position), "radius": radius}),
        GameEvent::ItemPicked { player, kind } => json!({"t": "picked", "player": player, "item": kind.name()}),
        GameEvent::MoneyPicked { player, total } => json!({"t": "money", "player": player, "total": total}),
        GameEvent::PlayerDamaged { player, health } => json!({"t": "damaged", "player": player, "health": health.round()}),
        GameEvent::PlayerDied { player } => json!({"t": "died", "player": player}),
        GameEvent::Crafted { player, result } => {
            let name = match result {
                RecipeResult::Item(kind) => kind.name(),
                RecipeResult::MineCart => "Minecart",
            };
            json!({"t": "crafted", "player": player, "item": name})
        }
        GameEvent::RobotDestroyed { position, .. } => json!({"t": "robot_destroyed", "pos": pos(*position)}),
        // Block changes travel as `BlockSet`; the oven's product is a thing like any other.
        GameEvent::BlockDestroyed { .. } | GameEvent::ItemPlaced { .. } | GameEvent::OvenProduced { .. } => Value::Null,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wurfel_sim::block::id;
    use wurfel_sim::{AirGenerator, World};

    const DT: f32 = 1.0 / 60.0;

    /// A stone floor at z = 0 (surface at height 1) around the origin.
    fn flat_world() -> World {
        let mut world = World::new(AirGenerator);
        for x in -20..20 {
            for y in -40..40 {
                world.set(x, y, 0, Block::new(id::STONE, 0));
            }
        }
        world
    }

    struct Setup {
        world: World,
        entities: Entities,
        mode: CavelandMode,
        player: EntityId,
        tick: u64,
        sent: Vec<ServerMsg>,
    }

    impl Setup {
        fn new() -> Setup {
            let mut world = flat_world();
            let mut mode = CavelandMode::new(&mut world, 1);
            let mut entities = Entities::new();
            let (gx, gy) = to_iso(0, 0);
            let player = mode.spawn_player(&mut entities, &world, Vec3::new(gx, gy, 1.0));
            // Take away the scattered things: the tests set the scene themselves.
            for (id, _) in mode.caveland().things() {
                entities.remove(id);
            }
            let mut setup = Setup { world, entities, mode, player, tick: 0, sent: Vec::new() };
            setup.run(2);
            setup
        }

        fn run(&mut self, steps: u32) {
            for _ in 0..steps {
                self.tick += 1;
                self.mode.tick(&mut self.entities, &mut self.world, self.tick, DT);
                self.sent.extend(self.mode.drain_outbox());
            }
        }

        fn act(&mut self, name: &str) {
            self.mode.act(&mut self.entities, &mut self.world, self.player, name, 0);
        }

        fn give(&mut self, kinds: &[CollectibleType]) {
            for &kind in kinds {
                assert!(self.mode.caveland.player_mut(self.player).unwrap().inventory.add(Item::new(kind)));
            }
        }

        fn events(&self) -> Vec<Value> {
            self.sent
                .iter()
                .filter_map(|m| match m {
                    ServerMsg::Rules { kind, data } if kind == "events" => data.as_array().cloned(),
                    _ => None,
                })
                .flatten()
                .collect()
        }

        fn state_of(&self, id: EntityId) -> Value {
            self.sent
                .iter()
                .rev()
                .find_map(|m| match m {
                    ServerMsg::Rules { kind, data } if kind == "state" => data.get(id.to_string()).cloned(),
                    _ => None,
                })
                .expect("a state message")
        }

        fn edits(&self) -> Vec<Edit> {
            self.sent.iter().filter_map(|m| if let ServerMsg::BlockSet(e) = m { Some(*e) } else { None }).collect()
        }
    }

    #[test]
    fn the_first_player_finds_things_lying_around_and_an_enemy() {
        let mut world = flat_world();
        let mut mode = CavelandMode::new(&mut world, 1);
        let mut entities = Entities::new();
        mode.spawn_player(&mut entities, &world, Vec3::new(0.0, 0.0, 1.0));
        let kinds: Vec<String> = mode.caveland().things().iter().map(|(_, k)| k.name()).collect();
        assert_eq!(kinds, ["Wood", "Coal", "Torch", "Explosives", "Iron", "Iron", "robot"]);
        mode.spawn_player(&mut entities, &world, Vec3::new(0.0, 0.0, 1.0));
        assert_eq!(mode.caveland().things().len(), 7, "only the first player starts the scene");
    }

    #[test]
    fn the_story_map_adds_a_camp_with_a_shop_a_flag_and_a_bird() {
        let mut world = flat_world();
        let mut mode = CavelandMode::new(&mut world, 1);
        mode.configure("caveland", 1);
        let mut entities = Entities::new();
        mode.spawn_player(&mut entities, &world, Vec3::new(0.0, 0.0, 1.0));
        let kinds: Vec<String> = mode.caveland().things().iter().map(|(_, k)| k.name()).collect();
        for expected in ["shopkeeper", "flag", "bird", "robot", "spaceship"] {
            assert!(kinds.iter().any(|k| k == expected), "{expected} in {kinds:?}");
        }
        assert!(mode.caveland().things().iter().all(|(id, _)| entities.get(*id).is_some()));
    }

    #[test]
    fn digging_a_block_sends_its_removal_and_drops_loot() {
        let mut s = Setup::new();
        // Surround the player with breakable blocks at their feet's height so any swing direction hits.
        let p = s.entities.get(s.player).unwrap().position;
        let (x, y) = from_iso(p.x, p.y);
        for dx in -2..=2 {
            for dy in -4..=4 {
                if (dx, dy) != (0, 0) {
                    s.world.set(x + dx, y + dy, 1, Block::new(id::DIRT, 0));
                }
            }
        }
        for _ in 0..4 {
            s.act("attack");
            s.run(30);
        }
        let removed = s.edits().into_iter().filter(|e| e.block == 0).count();
        assert!(removed > 0, "the swings removed blocks and told everybody: {:?}", s.edits());
        for e in s.edits().into_iter().filter(|e| e.block == 0) {
            assert!(s.world.get(e.x, e.y, e.z).is_air(), "the server world agrees with what it sent");
        }
        assert!(s.events().iter().any(|e| e["t"] == "sound"), "digging is heard");
    }

    #[test]
    fn walking_onto_an_item_picks_it_up_and_the_state_shows_it() {
        let mut s = Setup::new();
        let p = s.entities.get(s.player).unwrap().position;
        s.mode.caveland.spawn_collectible(&mut s.entities, Item::new(CollectibleType::Torch), p + Vec3::new(0.0, 0.0, 0.2));
        s.sent.clear();
        s.run(60);
        let state = s.state_of(s.player);
        assert_eq!(state["items"], json!(["Torch"]));
        assert_eq!(state["health"], json!(100.0));
        assert!(s.events().iter().any(|e| e["t"] == "picked" && e["item"] == "Torch"), "{:?}", s.events());
        let things = s.sent.iter().rev().find_map(|m| if let ServerMsg::Things { things, .. } = m { Some(things.clone()) } else { None }).unwrap();
        assert!(things.is_empty(), "a picked-up item is no longer lying around: {things:?}");
    }

    #[test]
    fn crafting_uses_the_ingredients_and_throw_drop_work_through_actions() {
        let mut s = Setup::new();
        s.give(&[CollectibleType::Wood, CollectibleType::Coal]);
        s.run(12);
        assert_eq!(s.state_of(s.player)["recipes"][0], json!(["Torch", true]), "craftable recipes are listed first");
        s.mode.act(&mut s.entities, &mut s.world, s.player, "craft", 0);
        s.run(12);
        assert_eq!(s.state_of(s.player)["items"], json!(["Torch"]));
        assert!(s.events().iter().any(|e| e["t"] == "crafted" && e["item"] == "Torch"));

        s.act("drop");
        s.run(12);
        assert_eq!(s.state_of(s.player)["items"], json!([]));
        assert_eq!(s.mode.caveland().things().len(), 1, "the torch lies on the ground");
        s.mode.act(&mut s.entities, &mut s.world, s.player, "craft", -1); // nonsense: ignored
        s.mode.act(&mut s.entities, &mut s.world, s.player, "fly", 0);
    }

    #[test]
    fn a_torch_in_hand_is_placed_as_a_block_everybody_hears_about() {
        let mut s = Setup::new();
        s.give(&[CollectibleType::Torch]);
        s.sent.clear();
        s.act("use");
        s.run(2);
        let torch = s.edits().into_iter().find(|e| e.block != 0).expect("a block was placed");
        assert_eq!(Block::from_raw(torch.block).id(), caveland_sim::blocks::ids::TORCH);
        assert_eq!(s.world.get(torch.x, torch.y, torch.z), Block::from_raw(torch.block));
    }

    #[test]
    fn a_player_who_dies_comes_back_as_the_same_player_at_the_start() {
        let mut s = Setup::new();
        s.give(&[CollectibleType::Wood]);
        let start = s.entities.get(s.player).unwrap().position;
        s.entities.get_mut(s.player).unwrap().position += Vec3::new(3.0, 0.0, 0.0);
        s.mode.caveland.damage_entity(&mut s.entities, s.player, 100.0);
        s.run(2);
        let again = s.entities.get(s.player).expect("the player is back under their id");
        assert_eq!(again.health(), 100.0);
        assert!(again.position.distance(start) < 0.5, "at the start: {:?} vs {start:?}", again.position);
        assert!(s.events().iter().any(|e| e["t"] == "died" && e["player"] == s.player));
        s.run(12);
        assert_eq!(s.state_of(s.player)["items"], json!([]), "the pack is lost");
    }

    #[test]
    fn a_player_who_left_is_not_respawned() {
        let mut s = Setup::new();
        s.mode.remove_player(s.player);
        s.mode.caveland.damage_entity(&mut s.entities, s.player, 100.0);
        s.run(2);
        assert!(s.entities.get(s.player).is_none());
    }

    fn rules<'a>(sent: &'a [ServerMsg], kind: &'a str) -> impl Iterator<Item = &'a Value> + 'a {
        sent.iter().filter_map(move |m| match m {
            ServerMsg::Rules { kind: k, data } if k == kind => Some(data),
            _ => None,
        })
    }

    fn near_player(s: &Setup) -> Vec3 {
        s.entities.get(s.player).unwrap().position + Vec3::new(0.4, 0.0, 0.0)
    }

    #[test]
    fn the_shopkeeper_opens_a_dialog_for_that_player_and_selling_takes_money() {
        let mut s = Setup::new();
        s.mode.caveland.set_money(100);
        let at = near_player(&s);
        s.mode.caveland.spawn_shopkeeper(&mut s.entities, at);
        s.sent.clear();
        s.act("interact");
        s.run(2);
        let dialog = rules(&s.sent, "dialog").next().expect("a dialog message").clone();
        assert_eq!(dialog["to"], json!(s.player), "it is addressed to the player who asked");
        assert_eq!(dialog["mode"], "selection");
        assert_eq!(dialog["money"], 100);
        assert!(dialog["options"].as_array().unwrap().iter().any(|o| o["label"].as_str().unwrap().starts_with("Buy Torch")), "{dialog}");
        s.sent.clear();
        s.mode.act(&mut s.entities, &mut s.world, s.player, "choose", 0);
        s.run(12);
        assert!(s.events().iter().any(|e| e["t"] == "bought" && e["item"] == "Torch" && e["price"] == 3), "{:?}", s.events());
        assert_eq!(s.state_of(s.player)["items"], json!(["Torch"]));
        assert_eq!(s.state_of(s.player)["money"], 97);
        s.mode.act(&mut s.entities, &mut s.world, s.player, "choose", 999); // out of range: ignored
        s.mode.act(&mut s.entities, &mut s.world, s.player, "choose", -1);
    }

    #[test]
    fn closing_a_dialog_is_told_to_its_owner() {
        let mut s = Setup::new();
        let at = near_player(&s);
        s.mode.caveland.spawn_shopkeeper(&mut s.entities, at);
        s.act("interact");
        s.run(2);
        s.sent.clear();
        s.act("cancel");
        s.run(2);
        let closed = rules(&s.sent, "dialog_closed").next().expect("the owner hears it closed");
        assert_eq!(closed["to"], json!(s.player));
    }

    #[test]
    fn a_flag_moves_the_respawn_point_and_the_next_death_comes_back_there() {
        let mut s = Setup::new();
        let start = s.entities.get(s.player).unwrap().position;
        let at = near_player(&s);
        s.mode.caveland.spawn_flag(&mut s.entities, Team::Neutral, at + Vec3::new(0.0, 0.0, 1.0));
        s.sent.clear();
        s.act("interact");
        s.run(2);
        assert!(s.events().iter().any(|e| e["t"] == "respawn_set"), "{:?}", s.events());
        assert!(s.events().iter().any(|e| e["t"] == "flag" && e["team"] == Team::Player.id()));
        s.entities.get_mut(s.player).unwrap().position += Vec3::new(5.0, 0.0, 0.0);
        s.mode.caveland.damage_entity(&mut s.entities, s.player, 100.0);
        s.run(2);
        let back = s.entities.get(s.player).unwrap().position;
        assert!(back.distance(start) > 0.5, "not at the old start any more: {back:?} vs {start:?}");
        assert_eq!(back, s.mode.flag_respawn.unwrap(), "at the flag's respawn point");
    }

    #[test]
    fn commands_are_for_the_host_only_and_answer_privately() {
        let mut s = Setup::new();
        s.sent.clear();
        s.mode.command(&mut s.entities, s.player, "give Torch", false);
        s.mode.command(&mut s.entities, s.player, "fly", true);
        s.mode.command(&mut s.entities, s.player, "/give Torch", true);
        s.mode.command(&mut s.entities, s.player, "give Unobtainium", true);
        s.sent.extend(s.mode.drain_outbox());
        let answers: Vec<&Value> = rules(&s.sent, "console").collect();
        assert_eq!(answers.len(), 4);
        assert!(answers.iter().all(|a| a["to"] == json!(s.player)));
        assert_eq!((answers[0]["ok"].clone(), answers[0]["text"].clone()), (json!(false), json!("only the host can use commands")));
        assert_eq!(answers[1]["ok"], false);
        assert!(answers[1]["text"].as_str().unwrap().contains("unknown command 'fly'"));
        assert_eq!(answers[2]["ok"], true, "a leading slash is allowed");
        assert_eq!(answers[3]["ok"], false, "bad item names are explained: {}", answers[3]);
        s.run(12);
        assert_eq!(s.state_of(s.player)["items"], json!(["Torch"]), "only the allowed give worked");
    }

    #[test]
    fn built_machines_and_power_changes_are_reported_and_the_powered_set_is_resent() {
        let mut s = Setup::new();
        s.mode.powered.insert((1, 2, 3));
        s.mode.powered_dirty = true;
        s.sent.clear();
        s.run(1);
        let power = rules(&s.sent, "power").next().expect("a power message");
        assert_eq!(power["cells"], json!([[1, 2, 3]]));
        s.sent.clear();
        s.run(POWER_EVERY as u32 + 1);
        assert_eq!(rules(&s.sent, "power").count(), 1, "repeated for players who joined later, not every tick");
    }

    #[test]
    fn the_generators_portals_appear_when_their_chunk_comes_into_memory() {
        use wurfel_sim::generator::create_generator;
        let generator = create_generator("caveland", 1).unwrap();
        let mut world = World::new(generator);
        let mut mode = CavelandMode::new(&mut world, 1);
        mode.configure("caveland", 1);
        let mut entities = Entities::new();
        // Cave 0's portal is at (21, 1212, 4), in chunk (2, 30); stand next to it.
        world.load_chunk(2, 30);
        let (gx, gy) = to_iso(21, 1212);
        mode.spawn_player(&mut entities, &world, Vec3::new(gx, gy, 6.0));
        mode.tick(&mut entities, &mut world, 2, DT);
        let sent = mode.drain_outbox();
        let things = sent.iter().find_map(|m| if let ServerMsg::Things { things, .. } = m { Some(things.clone()) } else { None }).unwrap();
        assert!(things.iter().any(|t| t.kind == "exit_portal"), "{things:?}");
        let before = mode.caveland().things().len();
        mode.tick(&mut entities, &mut world, 3, DT);
        assert_eq!(mode.caveland().things().len(), before, "a chunk is only scanned once");
    }

    #[test]
    fn the_intro_ship_carries_the_first_player_hidden_and_crashes_once() {
        let mut world = flat_world();
        let mut mode = CavelandMode::new(&mut world, 1);
        mode.configure("caveland", 1);
        let mut entities = Entities::new();
        let (gx, gy) = to_iso(0, 0);
        let player = mode.spawn_player(&mut entities, &world, Vec3::new(gx, gy, 1.0));
        let mut sent = Vec::new();
        let mut crashed_at = None;
        for tick in 1..=60 * 60u64 {
            mode.tick(&mut entities, &mut world, tick, DT);
            sent.extend(mode.drain_outbox());
            if crashed_at.is_none() && sent.iter().any(|m| matches!(m, ServerMsg::Rules { kind, data } if kind == "events" && data.as_array().unwrap().iter().any(|e| e["t"] == "ship_crashed"))) {
                crashed_at = Some(tick);
                break;
            }
        }
        assert!(crashed_at.is_some(), "the ship came down within a minute");
        let early = sent.iter().find_map(|m| match m {
            ServerMsg::Rules { kind, data } if kind == "state" => data.get(player.to_string()).cloned(),
            _ => None,
        });
        assert_eq!(early.expect("a state")["hidden"], true, "on board, nobody sees the player");
        assert!(mode.intro_done, "the crash is remembered");
        mode.tick(&mut entities, &mut world, 100_000, DT);
        let state = mode.state(&entities);
        assert_eq!(state[player.to_string()]["hidden"], false, "the player climbs out");
        let landed = entities.get(player).unwrap().position;
        let away = Vec3::new(landed.x - gx, landed.y - gy, 0.0).length();
        assert!(away < 15.0, "the crash is near where the player was going to start: {away} blocks away at {landed:?}");
        // A player joining after the crash starts normally.
        let later = mode.spawn_player(&mut entities, &world, Vec3::new(gx, gy, 1.0));
        assert!(!mode.caveland().is_hidden(later));
    }

    #[test]
    fn the_sidecar_keeps_money_respawn_and_the_intro_between_runs() {
        let dir = std::env::temp_dir().join(format!("wurfel-caveland-sidecar-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut s = Setup::new();
        s.mode.caveland.set_money(77);
        s.mode.flag_respawn = Some(Vec3::new(3.0, 4.0, 5.0));
        s.mode.intro_done = true;
        s.mode.save(&dir, &s.entities).unwrap();

        let mut world = flat_world();
        let mut fresh = CavelandMode::new(&mut world, 1);
        assert_eq!(fresh.caveland().money(), 0);
        fresh.load(&dir, &s.entities).unwrap();
        assert_eq!(fresh.caveland().money(), 77);
        assert_eq!(fresh.flag_respawn, Some(Vec3::new(3.0, 4.0, 5.0)));
        assert!(fresh.intro_done);

        // No file: a fresh save. A broken file: an error that names it.
        let empty = dir.join("other");
        assert!(fresh.load(&empty, &s.entities).is_ok());
        std::fs::create_dir_all(&empty).unwrap();
        std::fs::write(empty.join(SIDECAR), "{ not json").unwrap();
        let error = fresh.load(&empty, &s.entities).unwrap_err();
        assert!(error.contains("caveland.json"), "{error}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn state_is_only_sent_when_it_changes_and_things_come_with_every_snapshot() {
        let mut s = Setup::new();
        s.run(12); // the first state goes out
        assert!(s.sent.iter().any(|m| matches!(m, ServerMsg::Rules { kind, .. } if kind == "state")));
        s.sent.clear();
        s.run(60);
        let states = s.sent.iter().filter(|m| matches!(m, ServerMsg::Rules { kind, .. } if kind == "state")).count();
        let things = s.sent.iter().filter(|m| matches!(m, ServerMsg::Things { .. })).count();
        assert_eq!(states, 0, "nothing changed in a second of standing still");
        assert_eq!(things, 30, "30 snapshots per second at 60 ticks");
    }
}
