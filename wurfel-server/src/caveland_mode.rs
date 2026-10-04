//! The Caveland game mode of the server: the glue between [`caveland_sim::Caveland`] and the
//! network. The rules live in `caveland-sim`; this file turns what they report into messages and
//! turns client messages into rule calls.
//!
//! Everything a mode says to the clients travels in the engine's generic messages: block changes as
//! `BlockSet`, the other entities as `Things`, and the rest as `Rules` with a `kind` of `state`
//! (health and pack of every player, sent when it changes) or `events` (sounds and happenings).

use std::collections::HashMap;

use caveland_sim::collectible::{CollectibleType, Item};
use caveland_sim::crafting::RecipeResult;
use caveland_sim::{Action, Caveland, Controls, EntityKind, GameEvent, Team, Tuning};
use glam::Vec3;
use serde_json::{json, Value};
use wurfel_sim::entity::physics::ground_height;
use wurfel_sim::entity::{Entities, EntityId};
use wurfel_sim::grid::{from_iso, to_iso};
use wurfel_sim::player::PlayerInput;
use wurfel_sim::protocol::{Edit, ServerMsg, ThingState};
use wurfel_sim::{Block, World};

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
        }
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
        id
    }

    pub fn remove_player(&mut self, id: EntityId) {
        self.numbers.remove(&id);
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
            _ => return,
        };
        self.caveland.act(entities, world, id, action);
    }

    /// One fixed step of the rules. `tick` is the server's step counter after this step.
    pub fn tick(&mut self, entities: &mut Entities, world: &mut World, tick: u64, dt: f32) {
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
        let spot = self.spawn.unwrap_or(Vec3::new(0.0, 0.0, 10.0));
        self.caveland.spawn_player_as(entities, id, number, spot);
    }

    fn things_message(&self, entities: &Entities, tick: u64) -> ServerMsg {
        let things = self
            .caveland
            .things()
            .into_iter()
            .filter_map(|(id, kind): (EntityId, EntityKind)| {
                let e = entities.get(id)?;
                Some(ThingState { id, kind: kind.name(), pos: e.position.to_array() })
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
