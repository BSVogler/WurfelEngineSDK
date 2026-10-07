//! Server-side game state. No networking in here, so it can be unit tested directly.

use std::collections::{HashMap, HashSet};

use glam::Vec3;
use wurfel_sim::animation::{AnimatedBlocks, BlockAnimation};
use wurfel_sim::block::id;
use wurfel_sim::entity::benchmark::{benchmark_ball, BenchmarkSpawner};
use wurfel_sim::particle::Rng;
use wurfel_sim::entity::physics::occupied_cells;
use wurfel_sim::entity::{Entities, EntityId, Event};
use wurfel_sim::grid::to_iso;
use wurfel_sim::player::{apply_input, new_player, spawn_points, PlayerInput, TICK_DT, TICK_RATE};
use wurfel_sim::protocol::{
    editor_block_values, ClientMsg, Edit, PlayerState, ServerMsg, ThingState, EDITOR_THING_KINDS, MAX_EDITOR_THINGS, MAX_FILL_CELLS,
};
use wurfel_sim::generator::{create_generator, generators, Generator};
use wurfel_sim::grid::{chunk_of, from_iso};
use wurfel_sim::protocol::{clean_name, encode_chunk, PlayerInfo, WorldInfo};

use crate::caveland_mode::CavelandMode;
use crate::maps::{default_game_mode, OpenedWorld};

mod console;
pub use console::init_admin_token;

/// Most cells the server animates at once. Every change of an animated block goes to every client,
/// so this bounds the traffic (the waves are not counted: clients animate those themselves).
#[allow(dead_code)] // used by animate_block, which game rules and tests call
const MAX_SERVER_ANIMATED: usize = 1024;
/// Most edits in one `BlocksSet` of animation changes.
const MAX_ANIMATION_EDITS: usize = 512;
/// Most benchmark balls alive at once.
pub const MAX_BENCHMARK_BALLS: usize = 100;
/// Entities (players, balls, things) the benchmark must leave room for: no ball is added beyond it.
const MAX_ENTITIES_FOR_BENCHMARK: usize = 200;
/// A ball disappears after this many ticks (60 s), so a forgotten benchmark cleans up after itself.
const BALL_LIFETIME_TICKS: u64 = 60 * TICK_RATE as u64;
/// The balls are sent as things every this many ticks.
const BALL_THINGS_EVERY: u64 = 2;
/// Name of the thing kind the clients get for a ball.
const BALL_KIND: &str = "Benchmark Ball";

/// Placeholder while a disk-backed world replaces the generator-only one in `from_opened`.
struct NoGenerator;
impl Generator for NoGenerator {
    fn generate(&self, _x: i32, _y: i32, _z: i32) -> Block {
        Block::AIR
    }
}
use wurfel_sim::{Block, World, CHUNK_SIZE_Z};

/// How far from a player's body centre (ground units, like the world's isometric positions) the
/// editor may place or break blocks and spawn, move or delete things. Only a player who switched
/// the editor on can do any of that, so this is the reach of editors. It is wide on purpose: the
/// editor's camera pans away from the player (the client keeps it within `EDITOR_PAN_LIMIT`, half
/// of this, so the cursor still has room), which a reach of a few blocks would make useless. A
/// player outside the editor has no reach at all.
pub const EDITOR_REACH: f32 = 48.0;

/// The file of the things placed with the editor, in the save slot's folder (JSON, not the Java
/// format, which has no such thing in the map files the Rust server reads).
const THINGS_FILE: &str = "editor-things.json";

/// Blocks a client may place. Water is excluded on purpose: it would flood the island.
const PLACEABLE: [u8; 4] = [id::GRASS, id::DIRT, id::STONE, id::SAND];

/// What the server has loaded: a map (by display name) and its save slot, and the generator that
/// makes up every chunk the map has no data for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorldSpec {
    /// Display name of the map.
    pub map: String,
    /// The map's folder name.
    pub map_id: String,
    pub slot: u32,
    /// Generator id, see `wurfel_sim::generator`.
    pub generator: String,
    pub seed: u64,
}

/// What a player is pressing, and how far the server has got with the client's input stream.
#[derive(Default)]
struct InputSlot {
    input: PlayerInput,
    /// Sequence number of `input` (0 before the first message).
    seq: u32,
    /// Ticks simulated since `input` arrived.
    ticks: u32,
    /// A jump was pressed and no tick has seen it yet: the next tick jumps even if the key was
    /// already released again, so a quick tap is never lost.
    jump_pending: bool,
}

pub struct Game {
    spec: WorldSpec,
    peak: (i32, i32),
    world: World,
    /// Every player is an entity; the player id is the entity id.
    entities: Entities,
    inputs: HashMap<EntityId, InputSlot>,
    roster: HashMap<EntityId, PlayerInfo>,
    /// Players who switched the map editor on: only they may edit blocks.
    editors: HashSet<EntityId>,
    /// The things placed with the editor (plain engine only), see [`Game::edit_things`].
    things: Vec<ThingState>,
    next_thing: u32,
    /// `things` changed: send them to everybody (and save them) soon.
    things_dirty: bool,
    things_unsaved: bool,
    spawned: usize,
    tick: u64,
    /// The rules the world is played by (`engine` or `caveland`).
    gamemode: String,
    /// Present in the Caveland game mode.
    mode: Option<CavelandMode>,
    /// Where the game mode keeps its own files (the save slot's folder), if the world is a map.
    slot_dir: Option<std::path::PathBuf>,
    /// Blocks the server animates (not the waves, see [`Game::animate_block`]).
    animated: AnimatedBlocks,
    /// Value changes of animated blocks since the last `drain_outbox`.
    animation_edits: Vec<Edit>,
    /// The running benchmark (`benchmark` console command) and the player who started it.
    benchmark: Option<(BenchmarkSpawner, EntityId)>,
    /// Benchmark balls alive, with the tick they appeared.
    balls: Vec<(EntityId, u64)>,
    /// How long the last tick took, for the benchmark's "is the server still fast" test.
    last_tick_secs: f32,
    /// Answers to engine console lines, sent with the next `drain_outbox`.
    outbox: Vec<ServerMsg>,
    /// Players who logged in with `auth <token>` (see [`Game::is_admin`]).
    admins: HashSet<EntityId>,
    /// Wrong `auth` tokens per player, to stop guessing.
    auth_failures: HashMap<EntityId, u8>,
    /// Players who just logged in with the admin token, for the server to remember with their user.
    admin_grants: Vec<EntityId>,
}

impl Game {
    /// A world with the given generator, or an error naming what is wrong.
    pub fn new(spec: WorldSpec) -> Result<Self, String> {
        let generator = create_generator(&spec.generator, spec.seed).ok_or_else(|| {
            let known: Vec<&str> = generators().iter().map(|g| g.id).collect();
            format!("unknown generator '{}' (available: {})", spec.generator, known.join(", "))
        })?;
        let spawn = generator.spawn_point();
        let mode = default_game_mode(&spec.generator);
        let mut game = Self::with_generator(spec, generator, spawn);
        game.set_game_mode(mode);
        Ok(game)
    }

    /// Shorthand for tests.
    #[cfg(test)]
    pub fn island(seed: u64) -> Self {
        Self::new(WorldSpec { map: "Island".into(), map_id: "island".into(), slot: 0, generator: "island".into(), seed })
            .expect("the island generator exists")
    }

    fn with_generator(spec: WorldSpec, generator: Box<dyn Generator>, spawn: (i32, i32)) -> Self {
        let mut world = World::new(generator);
        // Players spawn here: have the ground ready before the first one arrives.
        world.load_area(chunk_of(spawn.0, spawn.1), 1);
        Game {
            spec,
            peak: spawn,
            world,
            entities: Entities::new(),
            inputs: HashMap::new(),
            roster: HashMap::new(),
            editors: HashSet::new(),
            things: Vec::new(),
            next_thing: 1,
            things_dirty: false,
            things_unsaved: false,
            spawned: 0,
            tick: 0,
            gamemode: "engine".to_string(),
            mode: None,
            slot_dir: None,
            animated: AnimatedBlocks::new(),
            animation_edits: Vec::new(),
            benchmark: None,
            balls: Vec::new(),
            last_tick_secs: 0.0,
            outbox: Vec::new(),
            admins: HashSet::new(),
            auth_failures: HashMap::new(),
            admin_grants: Vec::new(),
        }
    }

    /// Play by the rules of a game mode. Call before the first player joins; unknown names mean the
    /// plain engine.
    pub fn set_game_mode(&mut self, gamemode: &str) {
        if gamemode == "caveland" {
            let mut mode = CavelandMode::new(&mut self.world, self.spec.seed);
            mode.configure(&self.spec.generator, self.spec.seed);
            self.mode = Some(mode);
            self.gamemode = "caveland".to_string();
        } else {
            self.mode = None;
            self.gamemode = "engine".to_string();
        }
    }

    /// Tell the game rules who is friends with whom.
    pub fn set_friends(&mut self, pairs: &[(u32, u32)]) {
        if let Some(mode) = self.mode.as_mut() {
            mode.set_friends(pairs);
        }
    }

    #[cfg(test)]
    pub fn game_mode(&self) -> &str {
        &self.gamemode
    }

    /// Messages the game mode wants everybody to get (block changes, things, rule news). Empty in
    /// the plain engine.
    pub fn drain_outbox(&mut self) -> Vec<ServerMsg> {
        let mut out = self.mode.as_mut().map(CavelandMode::drain_outbox).unwrap_or_default();
        out.append(&mut self.outbox);
        // Animated blocks: one message per batch, each cell at most once (the newest value).
        let mut edits = std::mem::take(&mut self.animation_edits);
        edits.reverse();
        let mut seen = HashSet::new();
        edits.retain(|e| seen.insert((e.x, e.y, e.z)));
        edits.reverse();
        for batch in edits.chunks(MAX_ANIMATION_EDITS) {
            out.push(ServerMsg::BlocksSet { edits: batch.to_vec() });
        }
        if self.mode.is_none() {
            out.extend(self.things_message());
        }
        out
    }

    /// Animate the block at `cell` on the server: its value steps through `animation` and every
    /// change is sent to the clients (as `BlocksSet`, at most once per cell and tick). This is for
    /// animations that are part of the game state. The sea is not: its frames are cosmetic and
    /// start from the position, so the clients run them locally (see `wurfel-web`), which costs no
    /// bandwidth and does not make every water chunk "modified" for the autosave. At most
    /// [`MAX_SERVER_ANIMATED`] cells; returns an error beyond that.
    #[allow(dead_code)]
    pub fn animate_block(&mut self, cell: (i32, i32, i32), animation: BlockAnimation) -> Result<(), String> {
        if self.animated.len() >= MAX_SERVER_ANIMATED {
            return Err(format!("at most {MAX_SERVER_ANIMATED} animated blocks"));
        }
        self.world.load_chunk(chunk_of(cell.0, cell.1).0, chunk_of(cell.0, cell.1).1);
        self.animated.add(&self.world, cell, animation);
        Ok(())
    }

    /// How many blocks the server animates.
    #[allow(dead_code)]
    pub fn animated_count(&self) -> usize {
        self.animated.len()
    }

    /// The console command `benchmark`: spawn a bouncing ball above `player` and keep adding more
    /// (the Java benchmark) while the server keeps up. Only the host may, and only in the plain
    /// engine; at most [`MAX_BENCHMARK_BALLS`] balls live at once and each vanishes after a
    /// minute. Calling it again with the benchmark running just adds one more ball (up to the cap).
    pub fn spawn_benchmark_ball(&mut self, player: EntityId) -> Result<(), String> {
        if !self.is_admin(player) {
            return Err("only the host or an admin can start the benchmark".into());
        }
        if self.mode.is_some() {
            return Err("the benchmark is only available in the plain engine".into());
        }
        let at = self.entities.get(player).ok_or("no such player")?.position + Vec3::new(0.0, 0.0, 3.0);
        if !self.benchmark_has_room() {
            return Err(format!("too many entities (at most {MAX_BENCHMARK_BALLS} balls)"));
        }
        let ball = benchmark_ball(at, &mut Rng::new(self.tick ^ 0x5eed));
        let id = self.entities.spawn(ball);
        self.balls.push((id, self.tick));
        let seed = self.tick.wrapping_mul(0x9E37_79B9) | 1;
        self.benchmark.get_or_insert_with(|| (BenchmarkSpawner::new(seed), player));
        Ok(())
    }

    fn benchmark_has_room(&self) -> bool {
        self.balls.len() < MAX_BENCHMARK_BALLS && self.entities.len() < MAX_ENTITIES_FOR_BENCHMARK
    }

    /// Balls that are old are removed; the spawner adds one when it is due and there is room.
    fn update_benchmark(&mut self) {
        let tick = self.tick;
        let had_balls = !self.balls.is_empty();
        let entities = &mut self.entities;
        self.balls.retain(|&(id, born)| {
            let alive = tick.saturating_sub(born) < BALL_LIFETIME_TICKS && entities.get(id).is_some();
            if !alive {
                entities.remove(id);
            }
            alive
        });
        if had_balls && self.balls.is_empty() {
            self.things_dirty = true; // tell the clients the last ball is gone
        }
        let Some((_, owner)) = self.benchmark.as_ref() else { return };
        let Some(owner_position) = self.entities.get(*owner).map(|e| e.position) else {
            self.benchmark = None; // the host left
            return;
        };
        if !self.benchmark_has_room() {
            // Full: stop adding until balls expire (the spawner would only fill the gap again and again).
            return;
        }
        let frame = self.last_tick_secs;
        if let Some((spawner, _)) = self.benchmark.as_mut() {
            if let Some(ball) = spawner.update(TICK_DT, frame, owner_position + Vec3::new(0.0, 0.0, 3.0)) {
                let id = self.entities.spawn(ball);
                self.balls.push((id, tick));
            }
        }
    }

    /// The things for everybody in the plain engine: the editor's things plus the benchmark balls
    /// (one message, because a client replaces its whole list). Sent right after an editor change,
    /// once a second so that a player who joined later gets the editor's things, and every
    /// [`BALL_THINGS_EVERY`] ticks while balls exist.
    fn things_message(&mut self) -> Option<ServerMsg> {
        let balls_due = !self.balls.is_empty() && self.tick % BALL_THINGS_EVERY == 0;
        let editor_due = self.things_dirty || (!self.things.is_empty() && self.tick % TICK_RATE as u64 == 0);
        self.things_dirty = false;
        if !balls_due && !editor_due {
            return None;
        }
        let mut things = self.things.clone();
        things.extend(self.balls.iter().filter_map(|&(id, _)| {
            Some(ThingState { id, kind: BALL_KIND.to_string(), pos: self.entities.get(id)?.position.to_array() })
        }));
        Some(ServerMsg::Things { tick: self.tick, things })
    }

    /// The chunk the player stands in, if the player exists.
    pub fn player_chunk(&self, id: u32) -> Option<(i32, i32)> {
        let p = self.entities.get(id)?.position;
        let (x, y) = from_iso(p.x, p.y);
        Some(chunk_of(x, y))
    }

    /// A chunk ready to be sent to a client, generated or loaded first if needed.
    pub fn chunk_message(&mut self, cx: i32, cy: i32) -> Vec<u8> {
        self.world.load_chunk(cx, cy);
        encode_chunk(self.world.chunk(cx, cy).expect("the server world can always make a chunk"))
    }

    /// A world backed by a map's save slot on disk: chunks come from the files, and the generator
    /// fills in whatever the map has no data for.
    pub fn from_opened(opened: OpenedWorld) -> Self {
        let spawn = opened.generator.spawn_point();
        let spec = WorldSpec {
            map: opened.map.name.clone(),
            map_id: opened.map.id.clone(),
            slot: opened.slot,
            generator: opened.map.generator.clone(),
            seed: opened.map.seed,
        };
        let mut game = Self::with_generator(spec, Box::new(NoGenerator), spawn);
        let slot_dir = opened.store.slot_dir();
        game.world = World::with_store(opened.generator, opened.store);
        game.world.load_area(chunk_of(spawn.0, spawn.1), 1);
        game.set_game_mode(&opened.map.gamemode);
        if let Some(mode) = game.mode.as_mut() {
            if let Err(e) = mode.load(&slot_dir, &game.entities) {
                eprintln!("wurfel-server: the Caveland state of this save could not be read, starting fresh: {e}");
            }
        }
        if game.mode.is_none() {
            game.load_things(&slot_dir);
        }
        game.slot_dir = Some(slot_dir);
        game
    }

    /// Read the things the editor placed in this save slot. A file that is missing means none; one
    /// that cannot be read is reported and ignored (it stays on disk until the next save).
    fn load_things(&mut self, dir: &std::path::Path) {
        let path = dir.join(THINGS_FILE);
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return,
            Err(e) => return eprintln!("wurfel-server: {}: {e}", path.display()),
        };
        match serde_json::from_str::<Vec<(String, [f32; 3])>>(&text) {
            // Through the same checks as a spawn, so a hand-edited file cannot place anything the editor could not.
            Ok(list) => {
                for (kind, pos) in list.into_iter().take(MAX_EDITOR_THINGS) {
                    if EDITOR_THING_KINDS.contains(&kind.as_str()) && thing_position_is_valid(pos) {
                        self.things.push(ThingState { id: self.next_thing, kind, pos });
                        self.next_thing += 1;
                    }
                }
            }
            Err(e) => eprintln!("wurfel-server: {}: {e}, starting without placed things", path.display()),
        }
    }

    /// Write the chunks that changed to disk. Returns how many were written.
    pub fn save(&mut self) -> std::io::Result<usize> {
        if let (Some(mode), Some(dir)) = (self.mode.as_ref(), self.slot_dir.as_ref()) {
            mode.save(dir, &self.entities)?;
        }
        if let (true, Some(dir)) = (self.things_unsaved, self.slot_dir.as_ref()) {
            let list: Vec<(&str, [f32; 3])> = self.things.iter().map(|t| (t.kind.as_str(), t.pos)).collect();
            std::fs::create_dir_all(dir)?;
            std::fs::write(dir.join(THINGS_FILE), serde_json::to_string(&list).map_err(std::io::Error::other)?)?;
            self.things_unsaved = false;
        }
        self.world.save_modified()
    }

    pub fn info(&self) -> WorldInfo {
        WorldInfo {
            map: self.spec.map.clone(),
            map_id: self.spec.map_id.clone(),
            slot: self.spec.slot,
            generator: self.spec.generator.clone(),
            seed: self.spec.seed,
            players: self.inputs.len() as u32,
            gamemode: self.gamemode.clone(),
        }
    }

    pub fn spec(&self) -> &WorldSpec {
        &self.spec
    }

    /// Add a player standing near the mountain peak and return their id.
    #[cfg(test)]
    pub fn add_player(&mut self) -> u32 {
        self.add_player_as("", [230, 190, 50])
    }

    /// Add a player with the name and colour they chose. A name that is empty after cleaning
    /// becomes `Player <id>`.
    pub fn add_player_as(&mut self, name: &str, color: [u8; 3]) -> u32 {
        let points = spawn_points(&self.world, self.peak);
        let spot = points[self.spawned % points.len()];
        self.spawned += 1;
        let id = match self.mode.as_mut() {
            Some(mode) => mode.spawn_player(&mut self.entities, &self.world, spot),
            None => self.entities.spawn(new_player(spot)),
        };
        self.inputs.insert(id, InputSlot::default());
        let name = clean_name(name);
        let name = if name.is_empty() { format!("Player {id}") } else { name };
        self.roster.insert(id, PlayerInfo { id, name, color });
        id
    }

    pub fn player_info(&self, id: u32) -> Option<PlayerInfo> {
        self.roster.get(&id).cloned()
    }

    pub fn remove_player(&mut self, id: u32) {
        self.entities.remove(id);
        self.inputs.remove(&id);
        self.roster.remove(&id);
        self.editors.remove(&id);
        self.admins.remove(&id);
        self.auth_failures.remove(&id);
        self.admin_grants.retain(|&p| p != id);
        if self.benchmark.as_ref().is_some_and(|(_, owner)| *owner == id) {
            self.benchmark = None;
        }
        if let Some(mode) = self.mode.as_mut() {
            mode.remove_player(id);
        }
    }

    pub fn player_count(&self) -> usize {
        self.inputs.len()
    }

    pub fn entity_count(&self) -> usize {
        self.entities.len()
    }

    pub fn loaded_chunks(&self) -> usize {
        self.world.loaded_chunks().count()
    }

    /// Number of physics steps simulated so far.
    pub fn tick_count(&self) -> u64 {
        self.tick
    }

    pub fn welcome(&self, your_id: u32) -> ServerMsg {
        ServerMsg::Welcome {
            your_id,
            map: self.spec.map.clone(),
            slot: self.spec.slot,
            generator: self.spec.generator.clone(),
            seed: self.spec.seed,
            tick_rate: TICK_RATE,
            players: self.states(),
            roster: self.roster_list(),
            gamemode: self.gamemode.clone(),
            build: wurfel_sim::protocol::build_id(),
        }
    }

    fn roster_list(&self) -> Vec<PlayerInfo> {
        let mut roster: Vec<_> = self.roster.values().cloned().collect();
        roster.sort_by_key(|p| p.id);
        roster
    }

    pub fn snapshot(&self) -> ServerMsg {
        ServerMsg::Snapshot { tick: self.tick, players: self.states() }
    }

    fn states(&self) -> Vec<PlayerState> {
        self.entities
            .iter()
            .filter_map(|e| {
                let body = e.body.as_ref()?;
                // Items and robots of a game mode are not players: they come as `Things`.
                let slot = Some(self.inputs.get(&e.id())?);
                Some(PlayerState {
                    id: e.id(),
                    pos: e.position.to_array(),
                    vel: body.movement.to_array(),
                    input_seq: slot.map_or(0, |s| s.seq),
                    input_ticks: slot.map_or(0, |s| s.ticks),
                })
            })
            .collect()
    }

    /// One fixed physics step. The browser runs the same step for prediction.
    pub fn tick(&mut self) -> Vec<Event> {
        let started = std::time::Instant::now();
        let events = self.step();
        self.last_tick_secs = started.elapsed().as_secs_f32();
        events
    }

    fn step(&mut self) -> Vec<Event> {
        // Physics reads blocks, so the chunks around every player must be in memory before the step.
        let centres: Vec<_> = self.entities.iter().map(|e| from_iso(e.position.x, e.position.y)).collect();
        for (x, y) in centres {
            self.world.load_area(chunk_of(x, y), 1);
        }
        for (&id, slot) in &mut self.inputs {
            if self.entities.get(id).is_some() {
                let input = PlayerInput { jump: slot.input.jump || slot.jump_pending, ..slot.input };
                match self.mode.as_mut() {
                    Some(mode) => mode.controls(&mut self.entities, &self.world, id, input),
                    None => apply_input(self.entities.get_mut(id).expect("checked above"), input, &self.world),
                }
                slot.jump_pending = false;
                slot.ticks = slot.ticks.saturating_add(1);
            }
        }
        self.tick += 1;
        self.update_benchmark();
        for (x, y, z) in self.animated.update_changes(&mut self.world, TICK_DT) {
            let block = self.world.get(x, y, z).raw();
            self.animation_edits.push(Edit { x, y, z, block });
        }
        match self.mode.as_mut() {
            Some(mode) => {
                mode.tick(&mut self.entities, &mut self.world, self.tick, TICK_DT);
                Vec::new()
            }
            None => self.entities.update(&self.world, TICK_DT),
        }
    }

    /// Apply a message from a client. Returns a message to broadcast to everyone, if any.
    /// Anything invalid is ignored: the client is not trusted.
    pub fn handle(&mut self, player: u32, msg: ClientMsg) -> Option<ServerMsg> {
        match msg {
            ClientMsg::Input { seq, input } => {
                if let Some(slot) = self.inputs.get_mut(&player) {
                    // Older or repeated messages (reordering, resends) change nothing.
                    if seq > slot.seq {
                        slot.jump_pending |= input.jump;
                        slot.input = input;
                        slot.seq = seq;
                        slot.ticks = 0;
                    }
                }
                None
            }
            ClientMsg::Editor { on } => {
                if self.mode.is_none() && self.inputs.contains_key(&player) {
                    if on {
                        self.editors.insert(player);
                    } else {
                        self.editors.remove(&player);
                    }
                }
                None
            }
            // Only from the editor, and a game mode has its own rules for changing blocks (digging).
            ClientMsg::SetBlock { x, y, z, block } if self.mode.is_none() && self.editors.contains(&player) => self.set_block(player, Edit { x, y, z, block }),
            ClientMsg::FillBlocks { x1, y1, x2, y2, z, block } if self.mode.is_none() && self.editors.contains(&player) => {
                self.fill_blocks(player, (x1, y1, x2, y2), z, block)
            }
            ClientMsg::SpawnThing { .. } | ClientMsg::MoveThing { .. } | ClientMsg::DeleteThing { .. } if self.mode.is_none() && self.editors.contains(&player) => {
                self.edit_things(player, msg);
                None // the change goes out with the next `things_outbox`
            }
            ClientMsg::SaveWorld if self.mode.is_none() && self.editors.contains(&player) => Some(match self.save() {
                Ok(chunks) => ServerMsg::Saved { chunks: chunks as u32, error: None },
                Err(e) => ServerMsg::Saved { chunks: 0, error: Some(e.to_string()) },
            }),
            ClientMsg::SetBlock { .. }
            | ClientMsg::FillBlocks { .. }
            | ClientMsg::SpawnThing { .. }
            | ClientMsg::MoveThing { .. }
            | ClientMsg::DeleteThing { .. }
            | ClientMsg::SaveWorld => None,
            ClientMsg::Command { line, path } => {
                // Only admins (and the host) may use cheats and change the world.
                let admin = self.is_admin(player);
                let line = line.trim().trim_start_matches(['/', ':']);
                let name = line.split_whitespace().next().unwrap_or("").to_lowercase();
                match self.mode.as_mut() {
                    // The game mode's own commands (Caveland: `give`, `tpplayer`...).
                    Some(mode) if CavelandMode::has_command(&name) => mode.command(&mut self.entities, &mut self.world, player, line, admin),
                    _ => self.engine_command(player, line, &path),
                }
                None
            }
            ClientMsg::Action { name, arg } => {
                if let Some(mode) = self.mode.as_mut() {
                    mode.act(&mut self.entities, &mut self.world, player, &name, arg);
                }
                None
            }
            // Answered by the connection itself (pings, lobby requests, joining): no game state needed.
            ClientMsg::Ping { .. }
            | ClientMsg::Heart { .. }
            | ClientMsg::ListMaps
            | ClientMsg::GetWorld
            | ClientMsg::LoadMap { .. }
            | ClientMsg::CreateMap { .. }
            | ClientMsg::Join { .. } => None,
        }
    }

    fn set_block(&mut self, player: u32, edit: Edit) -> Option<ServerMsg> {
        self.apply_edit(player, edit).map(ServerMsg::BlockSet)
    }

    /// The editor's bucket: every column of the rectangle on one layer, each checked like a single
    /// `SetBlock` (reach, placeable, nobody inside). Nothing happens for a rectangle over the limit.
    fn fill_blocks(&mut self, player: u32, (x1, y1, x2, y2): (i32, i32, i32, i32), z: i32, block: u16) -> Option<ServerMsg> {
        let (xs, ys) = (x1.abs_diff(x2) as usize + 1, y1.abs_diff(y2) as usize + 1);
        if xs.saturating_mul(ys) > MAX_FILL_CELLS {
            return None;
        }
        let mut edits = Vec::new();
        for y in y1.min(y2)..=y1.max(y2) {
            for x in x1.min(x2)..=x1.max(x2) {
                edits.extend(self.apply_edit(player, Edit { x, y, z, block }));
            }
        }
        (!edits.is_empty()).then_some(ServerMsg::BlocksSet { edits })
    }

    /// Is `point` (ground units) within the editor's reach of the player's body?
    fn in_reach(&self, player: u32, point: Vec3) -> bool {
        self.entities.get(player).is_some_and(|e| (e.position + Vec3::new(0.0, 0.0, 0.7)).distance(point) <= EDITOR_REACH)
    }

    /// The editor's spawn, move and delete of things. Anything the rules refuse is ignored: an
    /// unknown kind, a position that is not finite or outside the world's height, one out of
    /// reach (the thing's old place too, when moving), too many things, an unknown id.
    fn edit_things(&mut self, player: u32, msg: ClientMsg) {
        let reachable = |game: &Game, pos: [f32; 3]| thing_position_is_valid(pos) && game.in_reach(player, Vec3::from(pos));
        match msg {
            ClientMsg::SpawnThing { kind, pos } => {
                if self.things.len() < MAX_EDITOR_THINGS && EDITOR_THING_KINDS.contains(&kind.as_str()) && reachable(self, pos) {
                    self.things.push(ThingState { id: self.next_thing, kind, pos });
                    self.next_thing += 1;
                } else {
                    return;
                }
            }
            ClientMsg::MoveThing { id, pos } => {
                let Some(index) = self.things.iter().position(|t| t.id == id) else { return };
                if !reachable(self, pos) || !reachable(self, self.things[index].pos) {
                    return;
                }
                self.things[index].pos = pos;
            }
            ClientMsg::DeleteThing { id } => {
                let Some(index) = self.things.iter().position(|t| t.id == id) else { return };
                if !reachable(self, self.things[index].pos) {
                    return;
                }
                self.things.remove(index);
            }
            _ => return,
        }
        self.things_dirty = true;
        self.things_unsaved = true;
    }

    /// Change one block if the rules allow it; returns what changed.
    fn apply_edit(&mut self, player: u32, edit: Edit) -> Option<Edit> {
        let wanted = Block::from_raw(edit.block);
        // Air is exactly 0; a placeable block may carry a value, as many as it has pictures.
        let allowed = if wanted.is_air() { wanted.raw() == 0 } else { PLACEABLE.contains(&wanted.id()) && wanted.value() < editor_block_values(wanted.id()) };
        if !allowed || !(0..CHUNK_SIZE_Z).contains(&edit.z) {
            return None;
        }
        let (gx, gy) = to_iso(edit.x, edit.y);
        if !self.in_reach(player, Vec3::new(gx, gy, edit.z as f32 + 0.5)) {
            return None;
        }
        // Never fill a cell somebody is standing in. Breaking is fine.
        let occupied = !wanted.is_air()
            && self
                .entities
                .iter()
                .any(|e| occupied_cells(e.position, e.dimension_z).contains(&(edit.x, edit.y, edit.z)));
        if occupied || self.world.get(edit.x, edit.y, edit.z) == wanted {
            return None;
        }
        self.world.set(edit.x, edit.y, edit.z, wanted);
        Some(edit)
    }
}

/// A position the editor may put a thing at: finite, and inside the world's height.
fn thing_position_is_valid(pos: [f32; 3]) -> bool {
    pos.iter().all(|c| c.is_finite()) && (0.0..CHUNK_SIZE_Z as f32).contains(&pos[2])
}

#[cfg(test)]
mod tests {
    use super::*;
    use wurfel_sim::entity::physics::ground_height;
    use wurfel_sim::grid::from_iso;

    fn run(game: &mut Game, ticks: u32) -> Vec<Event> {
        (0..ticks).flat_map(|_| game.tick()).collect()
    }

    /// A player who has switched the map editor on, the only kind that may edit blocks.
    impl Game {
        fn add_editor(&mut self) -> u32 {
            let id = self.add_player();
            self.handle(id, ClientMsg::Editor { on: true });
            id
        }
    }

    #[test]
    fn only_players_in_the_editor_may_edit_blocks() {
        let mut game = Game::island(1);
        let (plain, editor) = (game.add_player(), game.add_editor());
        let (x, y) = neighbour(&game, plain);
        let z = 9; // in the air within reach, as in the test above (the world is taller than the reach)
        let stone = Block::new(id::STONE, 0).raw();
        assert!(game.handle(plain, ClientMsg::SetBlock { x, y, z, block: stone }).is_none(), "not in the editor");
        assert!(game.world.get(x, y, z).is_air());
        assert!(game.handle(editor, ClientMsg::SetBlock { x, y, z, block: stone }).is_some());
        // Leaving the editor ends the permission; leaving the game forgets it.
        game.handle(editor, ClientMsg::Editor { on: false });
        assert!(game.handle(editor, ClientMsg::SetBlock { x, y, z, block: 0 }).is_none());
        game.handle(editor, ClientMsg::Editor { on: true });
        game.remove_player(editor);
        assert!(!game.editors.contains(&editor));
        // An unknown player id cannot register as an editor.
        game.handle(99, ClientMsg::Editor { on: true });
        assert!(!game.editors.contains(&99));
    }

    fn state(game: &Game, id: u32) -> PlayerState {
        match game.snapshot() {
            ServerMsg::Snapshot { players, .. } => players.into_iter().find(|p| p.id == id).unwrap(),
            _ => unreachable!(),
        }
    }

    fn column_of(p: &PlayerState) -> (i32, i32) {
        from_iso(p.pos[0], p.pos[1])
    }

    #[test]
    fn players_spawn_on_the_ground_with_distinct_ids_and_stay_there() {
        let mut game = Game::island(1);
        let (a, b) = (game.add_player(), game.add_player());
        assert_ne!(a, b);
        assert_eq!(game.player_count(), 2);
        run(&mut game, 60);
        for id in [a, b] {
            let s = state(&game, id);
            let (x, y) = column_of(&s);
            assert_eq!(s.pos[2], ground_height(&game.world, x, y), "standing on the ground");
            assert_eq!(s.vel, [0.0, 0.0, 0.0]);
        }
        game.remove_player(a);
        assert_eq!(game.player_count(), 1);
        assert_eq!(game.entity_count(), 1);
    }

    #[test]
    fn input_moves_a_player_and_jump_leaves_the_ground() {
        let mut game = Game::island(1);
        let id = game.add_player();
        let before = state(&game, id);

        game.handle(id, ClientMsg::Input { seq: 1, input: PlayerInput { right: true, ..Default::default() } });
        run(&mut game, 20);
        let moved = state(&game, id);
        assert_ne!(moved.pos, before.pos);

        game.handle(id, ClientMsg::Input { seq: 2, input: PlayerInput { jump: true, ..Default::default() } });
        let mut peak = f32::MIN;
        let mut landed = false;
        for _ in 0..90 {
            let events = game.tick();
            landed |= events.contains(&Event::Landed(id));
            peak = peak.max(state(&game, id).pos[2]);
            game.handle(id, ClientMsg::Input { seq: 3, input: PlayerInput::default() });
        }
        assert!(peak > moved.pos[2] + 0.5, "jumped: peak {peak}, was at {}", moved.pos[2]);
        assert!(landed, "and came back down");
    }

    fn press(game: &mut Game, id: u32, seq: u32, input: PlayerInput) {
        game.handle(id, ClientMsg::Input { seq, input });
    }

    #[test]
    fn snapshots_carry_the_input_ack_and_count_ticks_since_it() {
        let mut game = Game::island(1);
        let id = game.add_player();
        assert_eq!((state(&game, id).input_seq, state(&game, id).input_ticks), (0, 0));
        run(&mut game, 5);
        assert_eq!((state(&game, id).input_seq, state(&game, id).input_ticks), (0, 5));

        press(&mut game, id, 1, PlayerInput { right: true, ..Default::default() });
        assert_eq!((state(&game, id).input_seq, state(&game, id).input_ticks), (1, 0), "a new input restarts the count");
        run(&mut game, 7);
        assert_eq!((state(&game, id).input_seq, state(&game, id).input_ticks), (1, 7));
    }

    #[test]
    fn stale_and_repeated_inputs_are_ignored() {
        let mut game = Game::island(1);
        let id = game.add_player();
        press(&mut game, id, 5, PlayerInput { right: true, ..Default::default() });
        run(&mut game, 3);
        let walking_at = state(&game, id).pos;
        // An older message arriving late, and a resend of the current one.
        press(&mut game, id, 4, PlayerInput::default());
        press(&mut game, id, 5, PlayerInput::default());
        run(&mut game, 3);
        let s = state(&game, id);
        assert_eq!((s.input_seq, s.input_ticks), (5, 6), "the count was not reset");
        assert_ne!(s.pos, walking_at, "still walking");
    }

    #[test]
    fn a_jump_tap_shorter_than_a_tick_still_jumps() {
        let mut game = Game::island(1);
        let id = game.add_player();
        run(&mut game, 60); // settle on the ground
        // Pressed and released again before the server ticks: the second message arrives first.
        press(&mut game, id, 1, PlayerInput { jump: true, ..Default::default() });
        press(&mut game, id, 2, PlayerInput::default());
        game.tick();
        assert!(state(&game, id).vel[2] > 0.0, "left the ground: vel {:?}", state(&game, id).vel);
    }

    #[test]
    fn hostile_input_is_harmless() {
        let mut game = Game::island(1);
        let id = game.add_player();
        assert!(game.handle(99, ClientMsg::Input { seq: 1, input: PlayerInput { up: true, ..Default::default() } }).is_none());
        run(&mut game, 5);
        assert!(state(&game, id).pos.iter().all(|v| v.is_finite()));
    }

    /// A column next to a player that is not the player's own.
    fn neighbour(game: &Game, id: u32) -> (i32, i32) {
        let s = state(game, id);
        let (x, y) = column_of(&s);
        wurfel_sim::grid::lower_right(x, y)
    }

    #[test]
    fn placing_and_breaking_blocks_is_broadcast_and_remembered_for_joiners() {
        let mut game = Game::island(1);
        let id = game.add_editor();
        let (x, y) = neighbour(&game, id);
        let stone = Block::new(id::STONE, 0).raw();
        let z = 9; // up in the air within reach of the island peak, nobody is there

        let sent = game.handle(id, ClientMsg::SetBlock { x, y, z, block: stone });
        assert_eq!(sent, Some(ServerMsg::BlockSet(Edit { x, y, z, block: stone })));
        assert_eq!(game.world.get(x, y, z), Block::new(id::STONE, 0));

        // Somebody who joins later gets the edit as part of the chunk.
        let (cx, cy) = chunk_of(x, y);
        let chunk = wurfel_sim::protocol::decode_chunk(&game.chunk_message(cx, cy)).unwrap();
        assert_eq!(chunk.get(x.rem_euclid(10), y.rem_euclid(40), z), Block::new(id::STONE, 0));
        let late = game.add_player();
        match game.welcome(late) {
            ServerMsg::Welcome { players, .. } => assert_eq!(players.len(), 2),
            other => panic!("unexpected {other:?}"),
        }

        assert!(game.handle(id, ClientMsg::SetBlock { x, y, z, block: 0 }).is_some());
        assert!(game.world.get(x, y, z).is_air());
    }

    #[test]
    fn the_bucket_fills_a_rectangle_and_skips_what_a_single_edit_would_refuse() {
        let mut game = Game::island(1);
        let (plain, editor) = (game.add_player(), game.add_editor());
        let (x, y) = neighbour(&game, editor);
        let z = 9;
        let stone = Block::new(id::STONE, 0).raw();
        let fill = |x1, y1, x2, y2, block| ClientMsg::FillBlocks { x1, y1, x2, y2, z, block };

        assert!(game.handle(plain, fill(x, y, x + 1, y, stone)).is_none(), "not in the editor");
        // Corners may come in any order; both are included.
        let Some(ServerMsg::BlocksSet { edits }) = game.handle(editor, fill(x + 1, y + 1, x, y, stone)) else { panic!("no answer") };
        assert_eq!(edits.len(), 4);
        assert!(edits.iter().all(|e| e.z == z && e.block == stone));
        assert!(edits.iter().all(|e| game.world.get(e.x, e.y, z) == Block::new(id::STONE, 0)));
        // Filling again changes nothing, so there is nothing to tell.
        assert!(game.handle(editor, fill(x, y, x + 1, y + 1, stone)).is_none());
        // Cells out of reach are skipped, the rest is still done.
        let Some(ServerMsg::BlocksSet { edits }) = game.handle(editor, fill(x, y, x + 100, y, stone)) else { panic!("no answer") };
        assert!(edits.len() > 20 && edits.len() < 100, "{}", edits.len());
        // Over the size limit, invalid blocks and invalid layers do nothing.
        assert!(game.handle(editor, fill(x, y, x + 20, y + 20, stone)).is_none(), "too big (441 columns)");
        assert!(game.handle(editor, fill(x, y, x, y, Block::new(id::WATER, 0).raw())).is_none());
        assert!(game.handle(editor, ClientMsg::FillBlocks { x1: x, y1: y, x2: x, y2: y, z: CHUNK_SIZE_Z, block: stone }).is_none());
    }

    #[test]
    fn invalid_edits_are_rejected() {
        let mut game = Game::island(1);
        let id = game.add_editor();
        let (x, y) = neighbour(&game, id);
        let stone = Block::new(id::STONE, 0).raw();
        let top = CHUNK_SIZE_Z - 1;
        let cases = [
            ("above the world", ClientMsg::SetBlock { x, y, z: CHUNK_SIZE_Z, block: stone }),
            ("below the world", ClientMsg::SetBlock { x, y, z: -1, block: stone }),
            ("outside the world", ClientMsg::SetBlock { x: 10_000, y, z: 3, block: stone }),
            ("out of reach", ClientMsg::SetBlock { x: x + 100, y, z: 3, block: stone }),
            ("water is not placeable", ClientMsg::SetBlock { x, y, z: top, block: Block::new(id::WATER, 0).raw() }),
            ("unknown block id", ClientMsg::SetBlock { x, y, z: top, block: 200 }),
            ("a value the block has no picture for", ClientMsg::SetBlock { x, y, z: top, block: Block::new(id::STONE, 5).raw() }),
            ("a value on a block with one picture", ClientMsg::SetBlock { x, y, z: top, block: Block::new(id::GRASS, 1).raw() }),
            ("air with a value", ClientMsg::SetBlock { x, y, z: top, block: Block::new(id::AIR, 1).raw() }),
            ("already air", ClientMsg::SetBlock { x, y, z: top, block: 0 }),
        ];
        for (name, msg) in cases {
            assert!(game.handle(id, msg).is_none(), "{name} should be rejected");
        }
    }

    #[test]
    fn a_block_keeps_the_value_the_editor_gave_it() {
        let mut game = Game::island(1);
        let id = game.add_editor();
        let (x, y) = neighbour(&game, id);
        let z = 9;
        let variant = Block::new(id::STONE, 1);
        assert_eq!(game.handle(id, ClientMsg::SetBlock { x, y, z, block: variant.raw() }), Some(ServerMsg::BlockSet(Edit { x, y, z, block: variant.raw() })));
        assert_eq!(game.world.get(x, y, z), variant);
        // Joiners get the value with the chunk.
        let (cx, cy) = chunk_of(x, y);
        let chunk = wurfel_sim::protocol::decode_chunk(&game.chunk_message(cx, cy)).unwrap();
        assert_eq!(chunk.get(x.rem_euclid(10), y.rem_euclid(40), z), variant);
        // Another value of the same block is a change; the same one is not.
        assert!(game.handle(id, ClientMsg::SetBlock { x, y, z, block: variant.raw() }).is_none());
        assert!(game.handle(id, ClientMsg::SetBlock { x, y, z, block: Block::new(id::STONE, 0).raw() }).is_some());
    }

    /// Where the editor puts a thing so that the player (an editor) reaches it.
    fn near(game: &Game, id: u32) -> [f32; 3] {
        let s = state(game, id);
        [s.pos[0] + 2.0, s.pos[1], s.pos[2]]
    }

    fn things_of(game: &mut Game) -> Vec<ThingState> {
        match game.drain_outbox().pop() {
            Some(ServerMsg::Things { things, .. }) => things,
            other => panic!("expected the things, got {other:?}"),
        }
    }

    #[test]
    fn the_editor_spawns_moves_and_deletes_things_and_everybody_is_told() {
        let mut game = Game::island(1);
        let (plain, editor) = (game.add_player(), game.add_editor());
        let pos = near(&game, editor);
        game.tick(); // tick 0 would count as the start of a second
        let spawn = |kind: &str, pos| ClientMsg::SpawnThing { kind: kind.into(), pos };

        assert!(game.drain_outbox().is_empty(), "nothing to say while there are no things");
        game.handle(plain, spawn("Wood", pos));
        assert!(game.drain_outbox().is_empty(), "only from the editor");
        assert!(game.handle(editor, spawn("Wood", pos)).is_none(), "the answer is the things message, not a reply");
        assert_eq!(things_of(&mut game), vec![ThingState { id: 1, kind: "Wood".into(), pos }]);
        assert!(game.drain_outbox().is_empty(), "sent once, then only about once a second");

        let moved = [pos[0] + 1.0, pos[1], pos[2]];
        game.handle(editor, ClientMsg::MoveThing { id: 1, pos: moved });
        assert_eq!(things_of(&mut game)[0].pos, moved);
        game.handle(plain, ClientMsg::DeleteThing { id: 1 });
        assert!(game.drain_outbox().is_empty());
        game.handle(editor, ClientMsg::DeleteThing { id: 1 });
        assert!(things_of(&mut game).is_empty(), "the empty list tells the clients to forget it");

        // A joiner gets them with the periodic message.
        game.handle(editor, spawn("Torch", pos));
        game.drain_outbox();
        let late = game.tick_count() + 60 - game.tick_count() % 60;
        while game.tick_count() < late {
            game.tick();
        }
        assert_eq!(things_of(&mut game).len(), 1);
    }

    #[test]
    fn invalid_things_are_refused() {
        let mut game = Game::island(1);
        let id = game.add_editor();
        let pos = near(&game, id);
        game.tick();
        let cases = [
            ("unknown kind", ClientMsg::SpawnThing { kind: "Nuke".into(), pos }),
            ("not a number", ClientMsg::SpawnThing { kind: "Wood".into(), pos: [f32::NAN, 0.0, 1.0] }),
            ("infinite", ClientMsg::SpawnThing { kind: "Wood".into(), pos: [f32::INFINITY, 0.0, 1.0] }),
            ("above the world", ClientMsg::SpawnThing { kind: "Wood".into(), pos: [pos[0], pos[1], CHUNK_SIZE_Z as f32] }),
            ("below the world", ClientMsg::SpawnThing { kind: "Wood".into(), pos: [pos[0], pos[1], -0.5] }),
            ("out of reach", ClientMsg::SpawnThing { kind: "Wood".into(), pos: [pos[0] + 500.0, pos[1], pos[2]] }),
            ("unknown id", ClientMsg::MoveThing { id: 77, pos }),
            ("unknown id", ClientMsg::DeleteThing { id: 77 }),
        ];
        for (name, msg) in cases {
            game.handle(id, msg);
            assert!(game.drain_outbox().is_empty(), "{name} should be refused");
        }
        // A thing cannot be dragged out of reach, nor grabbed from out of reach.
        game.handle(id, ClientMsg::SpawnThing { kind: "Wood".into(), pos });
        game.drain_outbox();
        game.handle(id, ClientMsg::MoveThing { id: 1, pos: [pos[0] + 500.0, pos[1], pos[2]] });
        assert!(game.drain_outbox().is_empty());
        game.things[0].pos[0] += 500.0;
        game.handle(id, ClientMsg::DeleteThing { id: 1 });
        assert!(game.drain_outbox().is_empty());
        game.things[0].pos[0] -= 500.0;
        // The count is limited.
        for _ in 0..MAX_EDITOR_THINGS + 5 {
            game.handle(id, ClientMsg::SpawnThing { kind: "Wood".into(), pos });
        }
        assert_eq!(game.things.len(), MAX_EDITOR_THINGS);
    }

    #[test]
    fn things_are_the_editors_only_in_the_plain_engine_and_survive_a_save() {
        let dir = std::env::temp_dir().join(format!("wurfel-things-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut game = Game::island(1);
        let id = game.add_editor();
        let pos = near(&game, id);
        game.handle(id, ClientMsg::SpawnThing { kind: "Coal".into(), pos });
        game.handle(id, ClientMsg::SpawnThing { kind: "flag".into(), pos });
        // Without a save slot there is nothing to write to; the world part still works.
        assert!(game.save().is_ok());
        game.slot_dir = Some(dir.clone());
        assert!(game.save().is_ok());
        let mut again = Game::island(1);
        again.load_things(&dir);
        assert_eq!(again.things, game.things);
        assert_eq!(again.next_thing, 3);
        // A damaged or hostile file places nothing it should not.
        std::fs::write(dir.join(THINGS_FILE), r#"[["Nuke",[0,0,1]],["Wood",[0,0,99]],["Wood",[0,0,1]]]"#).unwrap();
        let mut hostile = Game::island(1);
        hostile.load_things(&dir);
        assert_eq!(hostile.things.len(), 1);
        std::fs::write(dir.join(THINGS_FILE), "{").unwrap();
        let mut broken = Game::island(1);
        broken.load_things(&dir);
        assert!(broken.things.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_save_button_is_for_the_editor_and_answers_everybody() {
        let mut game = Game::island(1);
        let (plain, editor) = (game.add_player(), game.add_editor());
        assert!(game.handle(plain, ClientMsg::SaveWorld).is_none());
        assert!(matches!(game.handle(editor, ClientMsg::SaveWorld), Some(ServerMsg::Saved { error: None, .. })));
    }

    #[test]
    fn nothing_can_be_placed_inside_a_player_but_beside_and_above_is_fine() {
        let mut game = Game::island(1);
        let id = game.add_editor();
        let s = state(&game, id);
        let (x, y) = column_of(&s);
        let feet = s.pos[2] as i32;
        let stone = Block::new(id::STONE, 0).raw();

        // The cells the 1.4 tall body occupies are refused...
        for z in [feet, feet + 1] {
            assert!(game.handle(id, ClientMsg::SetBlock { x, y, z, block: stone }).is_none(), "z = {z}");
        }
        // ...the one above the head is not.
        assert!(game.handle(id, ClientMsg::SetBlock { x, y, z: feet + 2, block: stone }).is_some());
    }

    #[test]
    fn a_block_placed_under_a_player_does_not_teleport_them_but_physics_reacts() {
        let mut game = Game::island(1);
        let id = game.add_editor();
        run(&mut game, 30);
        let s = state(&game, id);
        let (x, y) = column_of(&s);
        // Digging out the block below makes the player fall one block.
        let below = s.pos[2] as i32 - 1;
        assert!(game.handle(id, ClientMsg::SetBlock { x, y, z: below, block: 0 }).is_some());
        let events = run(&mut game, 60);
        assert!(events.contains(&Event::Landed(id)));
        assert_eq!(state(&game, id).pos[2], s.pos[2] - 1.0);
    }

    #[test]
    fn every_registered_generator_makes_a_playable_world() {
        for info in generators() {
            let spec = WorldSpec { map: "t".into(), map_id: "t".into(), slot: 0, generator: info.id.into(), seed: 3 };
            let mut game = Game::new(spec).unwrap_or_else(|e| panic!("{}: {e}", info.id));
            let id = game.add_player();
            run(&mut game, 120);
            let s = state(&game, id);
            assert!(s.pos.iter().all(|v| v.is_finite()), "{}: {s:?}", info.id);
            // A world filled to the top (fullmap) has its surface at the ceiling, z = 10.
            assert!(s.pos[2] >= 0.0 && s.pos[2] <= CHUNK_SIZE_Z as f32, "{}: height {}", info.id, s.pos[2]);
            assert_eq!(s.vel[2], 0.0, "{}: the player should have come to rest", info.id);
            assert!(game.loaded_chunks() >= 9, "{}: the chunks around the player are loaded", info.id);
        }
    }

    #[test]
    fn different_generators_make_different_worlds() {
        let columns = |id: &str| -> Vec<u16> {
            let game = Game::new(WorldSpec { map: "t".into(), map_id: "t".into(), slot: 0, generator: id.into(), seed: 1 }).unwrap();
            (0..CHUNK_SIZE_Z).map(|z| game.world.get(3, 5, z).raw()).collect()
        };
        assert_ne!(columns("island"), columns("caveland"));
        assert_ne!(columns("island"), columns("blocktest"));
    }

    #[test]
    fn unknown_generators_are_an_error_not_a_panic() {
        let spec = WorldSpec { map: "x".into(), map_id: "x".into(), slot: 0, generator: "nope".into(), seed: 1 };
        let message = Game::new(spec).err().unwrap();
        assert!(message.contains("nope") && message.contains("island"), "{message}: it should name what exists");
    }

    #[test]
    fn welcome_names_the_world_and_generator() {
        let mut game = Game::island(5);
        let id = game.add_player();
        match game.welcome(id) {
            ServerMsg::Welcome { map, slot, generator, seed, tick_rate, .. } => {
                assert_eq!((map.as_str(), slot, generator.as_str(), seed, tick_rate), ("Island", 0, "island", 5, 60));
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn players_have_names_and_colours() {
        let mut game = Game::island(1);
        let a = game.add_player_as("  Ann  ", [1, 2, 3]);
        let b = game.add_player_as("\u{7}\n", [4, 5, 6]);
        assert_eq!(game.player_info(a).unwrap().name, "Ann");
        assert_eq!(game.player_info(b).unwrap().name, format!("Player {b}"), "an empty name gets a default");
        match game.welcome(b) {
            ServerMsg::Welcome { roster, .. } => assert_eq!(roster.iter().map(|p| p.color).collect::<Vec<_>>(), vec![[1, 2, 3], [4, 5, 6]]),
            other => panic!("{other:?}"),
        }
        game.remove_player(a);
        assert!(game.player_info(a).is_none());
    }

    #[test]
    fn snapshots_count_ticks() {
        let mut game = Game::island(1);
        game.add_player();
        run(&mut game, 7);
        match game.snapshot() {
            ServerMsg::Snapshot { tick, .. } => assert_eq!(tick, 7),
            _ => unreachable!(),
        }
        assert_eq!(game.tick_count(), 7);
        assert!(game.loaded_chunks() >= 9, "3 x 3 chunks around the spawn");
    }
}

#[cfg(test)]
mod game_mode_tests {
    use super::*;
    use wurfel_sim::grid::from_iso;

    fn caveland_game() -> Game {
        Game::new(WorldSpec { map: "c".into(), map_id: "c".into(), slot: 0, generator: "caveland".into(), seed: 1 }).unwrap()
    }

    fn run(game: &mut Game, ticks: u32) -> Vec<ServerMsg> {
        let mut out = Vec::new();
        for _ in 0..ticks {
            game.tick();
            out.extend(game.drain_outbox());
        }
        out
    }

    #[test]
    fn the_caveland_generator_is_played_with_caveland_rules_and_the_island_with_the_engine() {
        let game = caveland_game();
        assert_eq!(game.game_mode(), "caveland");
        assert_eq!(game.info().gamemode, "caveland");
        let id = game.player_info(0).map(|p| p.id).unwrap_or(0);
        assert!(matches!(game.welcome(id), ServerMsg::Welcome { gamemode, .. } if gamemode == "caveland"));
        let mut island = Game::island(1);
        assert_eq!(island.game_mode(), "engine");
        island.add_player();
        assert!(run(&mut island, 10).is_empty(), "the plain engine has nothing extra to say");
        let mut forced = Game::island(1);
        forced.set_game_mode("caveland");
        assert_eq!(forced.game_mode(), "caveland", "any map can be played with Caveland rules");
        forced.set_game_mode("whatever");
        assert_eq!(forced.game_mode(), "engine", "unknown modes are the engine");
    }

    #[test]
    fn caveland_things_are_not_players_and_arrive_as_things() {
        let mut game = caveland_game();
        let me = game.add_player();
        let msgs = run(&mut game, 4);
        let ServerMsg::Snapshot { players, .. } = game.snapshot() else { unreachable!() };
        assert_eq!(players.iter().map(|p| p.id).collect::<Vec<_>>(), vec![me], "items and the robot are not in the player list");
        let things: Vec<ThingState> = msgs
            .iter()
            .filter_map(|m| if let ServerMsg::Things { things, .. } = m { Some(things.clone()) } else { None })
            .next_back()
            .expect("things were sent");
        let kinds: Vec<&str> = things.iter().map(|t| t.kind.as_str()).collect();
        assert!(kinds.contains(&"Torch") && kinds.contains(&"robot"), "{kinds:?}");
    }

    #[test]
    fn caveland_clients_cannot_edit_blocks_directly_but_can_act() {
        let mut game = caveland_game();
        let me = game.add_player();
        run(&mut game, 4);
        let p = game.entities.get(me).unwrap().position;
        let (x, y) = from_iso(p.x, p.y);
        let edit = ClientMsg::SetBlock { x: x + 1, y, z: p.z as i32 + 1, block: Block::new(id::STONE, 0).raw() };
        assert_eq!(game.handle(me, edit), None, "digging and building go through the rules");
        // Actions are accepted and answered later by the tick; nonsense is ignored.
        assert_eq!(game.handle(me, ClientMsg::Action { name: "attack".into(), arg: 0 }), None);
        assert_eq!(game.handle(me, ClientMsg::Action { name: "nonsense".into(), arg: -5 }), None);
        run(&mut game, 30);
    }

    #[test]
    fn only_the_host_may_run_caveland_commands() {
        let mut game = caveland_game();
        let (host, guest) = (game.add_player(), game.add_player());
        for id in [guest, host] {
            game.handle(id, ClientMsg::Command { line: "give Torch".into(), path: String::new() });
        }
        let answers: Vec<(u64, bool)> = game
            .drain_outbox()
            .into_iter()
            .filter_map(|m| match m {
                ServerMsg::Rules { kind, data } if kind == "console" => Some((data["to"].as_u64().unwrap(), data["ok"].as_bool().unwrap())),
                _ => None,
            })
            .collect();
        assert_eq!(answers, vec![(guest as u64, false), (host as u64, true)]);
        // The plain engine has no Caveland commands: its own console answers.
        let mut engine = Game::island(1);
        let me = engine.add_player();
        assert_eq!(engine.handle(me, ClientMsg::Command { line: "give Torch".into(), path: String::new() }), None);
        let answer = console_answers(&mut engine).pop().unwrap();
        assert_eq!(answer["ok"], false);
        assert!(answer["text"].as_str().unwrap().contains("command not found"), "{answer}");
    }

    fn console_answers(game: &mut Game) -> Vec<serde_json::Value> {
        game.drain_outbox()
            .into_iter()
            .filter_map(|m| match m {
                ServerMsg::Rules { kind, data } if kind == "console" => Some(data),
                _ => None,
            })
            .collect()
    }

    fn command(game: &mut Game, id: u32, line: &str) -> serde_json::Value {
        game.handle(id, ClientMsg::Command { line: line.into(), path: String::new() });
        let mut answers = console_answers(game);
        assert_eq!(answers.len(), 1, "one answer for '{line}'");
        assert_eq!(answers[0]["to"], id);
        answers.pop().unwrap()
    }

    #[test]
    fn auth_with_the_admin_token_lets_a_guest_change_the_world() {
        let (token, _) = init_admin_token();
        let mut game = Game::island(1);
        let (_host, guest) = (game.add_player(), game.add_player());
        assert_eq!(command(&mut game, guest, "teleport 3 4")["ok"], false);
        let wrong = command(&mut game, guest, "auth nope");
        assert_eq!(wrong["ok"], false);
        assert!(!wrong["text"].as_str().unwrap().contains(token), "the token is never echoed");
        let right = command(&mut game, guest, &format!("auth {token}"));
        assert_eq!(right["ok"], true, "{right}");
        assert_eq!(command(&mut game, guest, "teleport 3 4")["ok"], true);
        // Leaving ends it: the id may come back as somebody else.
        game.remove_player(guest);
        assert!(!game.is_admin(guest));
    }

    #[test]
    fn auth_stops_answering_after_too_many_wrong_tokens() {
        let (token, _) = init_admin_token();
        let mut game = Game::island(1);
        let (_host, guest) = (game.add_player(), game.add_player());
        for _ in 0..5 {
            command(&mut game, guest, "auth wrong");
        }
        assert_eq!(command(&mut game, guest, &format!("auth {token}"))["ok"], false, "locked out");
        assert!(!game.is_admin(guest));
    }

    #[test]
    fn engine_commands_run_on_the_server_for_the_host() {
        let mut game = Game::island(1);
        let (host, guest) = (game.add_player(), game.add_player());
        // Changing the world is for the host.
        let refused = command(&mut game, guest, "teleport 3 4");
        assert_eq!(refused["ok"], false);
        assert!(refused["text"].as_str().unwrap().contains("permission denied"), "{refused}");

        let moved = command(&mut game, host, "teleport 3 4");
        assert_eq!(moved["ok"], true, "{moved}");
        let p = game.entities.get(host).unwrap().position;
        assert_eq!(from_iso(p.x, p.y), (3, 4));

        let map = command(&mut game, guest, "printmap 0 0 0 4 2");
        assert_eq!(map["ok"], true, "reading is for everybody: {map}");
        assert_eq!(map["lines"].as_array().unwrap().len(), 3, "a legend and two rows");

        game.handle(host, ClientMsg::Editor { on: true });
        let p = game.entities.get(host).unwrap().position;
        let (x, y) = from_iso(p.x, p.y);
        game.handle(host, ClientMsg::SpawnThing { kind: EDITOR_THING_KINDS[0].into(), pos: [p.x, p.y, p.z + 1.0] });
        assert_eq!(game.things.len(), 1);
        let killed = command(&mut game, host, "killall");
        assert_eq!(killed["text"], "disposed 1 entities");
        assert!(game.things.is_empty());

        let (cx, cy) = chunk_of(x, y);
        game.handle(host, ClientMsg::Command { line: format!("fillwithair {} {}", cx + 3, cy), path: String::new() });
        let out = game.drain_outbox();
        assert!(out.iter().any(|m| matches!(m, ServerMsg::Rules { data, .. } if data["ok"] == true)), "{out:?}");
        assert!(out.iter().any(|m| matches!(m, ServerMsg::BlocksSet { .. })), "the clients hear of it");
        let top = (cx + 3) * wurfel_sim::CHUNK_SIZE_X;
        assert!((0..CHUNK_SIZE_Z).all(|z| game.world.get(top, cy * wurfel_sim::CHUNK_SIZE_Y, z).is_air()));

        let unknown = command(&mut game, host, "loadmap other");
        assert_eq!(unknown["ok"], false);
        // Client commands are not run here.
        assert_eq!(command(&mut game, host, "fullscreen")["ok"], false);
    }

    #[test]
    fn moves_are_announced_as_action_events_with_the_rules_outcome() {
        let mut game = caveland_game();
        let me = game.add_player();
        run(&mut game, 120);
        let actions = |msgs: Vec<ServerMsg>| -> Vec<(String, bool)> {
            msgs.iter()
                .filter_map(|m| match m {
                    ServerMsg::Rules { kind, data } if kind == "events" => Some(data.as_array().unwrap().clone()),
                    _ => None,
                })
                .flatten()
                .filter(|e| e["t"] == "action" && e["player"] == me)
                .map(|e| (e["name"].as_str().unwrap().to_string(), e["ok"].as_bool().unwrap()))
                .collect()
        };
        game.handle(me, ClientMsg::Action { name: "attack".into(), arg: 0 });
        game.handle(me, ClientMsg::Action { name: "attack".into(), arg: 0 });
        game.handle(me, ClientMsg::Action { name: "throw".into(), arg: 0 });
        game.handle(me, ClientMsg::Action { name: "nonsense".into(), arg: 0 });
        let seen = actions(run(&mut game, 2));
        assert_eq!(
            seen,
            [("attack".to_string(), true), ("attack".to_string(), true), ("throw".to_string(), false)],
            "a throw without a prepared pose failed"
        );
    }

    #[test]
    fn engine_games_ignore_actions() {
        let mut game = Game::island(1);
        let me = game.add_player();
        assert_eq!(game.handle(me, ClientMsg::Action { name: "attack".into(), arg: 0 }), None);
        assert!(game.drain_outbox().is_empty());
    }

    // ---------------------------------------------------------------- animated blocks, benchmark

    fn ticks(game: &mut Game, n: u32) {
        for _ in 0..n {
            game.tick();
        }
    }

    fn above_a_player(game: &Game, id: u32) -> (i32, i32, i32) {
        let p = game.entities.get(id).unwrap().position;
        let (x, y) = from_iso(p.x, p.y);
        (x, y, 20)
    }

    #[test]
    fn animated_blocks_are_stepped_by_the_tick_and_reach_the_clients_batched_once_per_cell() {
        let mut game = Game::island(5);
        let me = game.add_player();
        let cell = above_a_player(&game, me);
        game.world.set(cell.0, cell.1, cell.2, Block::new(id::WATER, 0));
        game.animate_block(cell, BlockAnimation::new(vec![0.1; 3], true, true)).unwrap();
        assert!(game.drain_outbox().is_empty(), "nothing moved yet");
        ticks(&mut game, 7); // 7 / 60 s: past the first frame
        let out = game.drain_outbox();
        assert_eq!(out.len(), 1, "one batch: {out:?}");
        match &out[0] {
            ServerMsg::BlocksSet { edits } => assert_eq!(edits, &vec![Edit { x: cell.0, y: cell.1, z: cell.2, block: Block::new(id::WATER, 1).raw() }]),
            other => panic!("{other:?}"),
        }
        assert!(game.drain_outbox().is_empty(), "drained");
        // Several changes before a drain collapse into the newest value of the cell.
        ticks(&mut game, 6);
        ticks(&mut game, 6);
        let out = game.drain_outbox();
        assert_eq!(out.len(), 1);
        match &out[0] {
            ServerMsg::BlocksSet { edits } => {
                assert_eq!(edits.len(), 1);
                assert_eq!(Block::from_raw(edits[0].block), game.world.get(cell.0, cell.1, cell.2));
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_big_animation_is_split_into_bounded_messages_and_the_number_of_cells_is_capped() {
        let mut game = Game::island(5);
        game.add_player();
        for i in 0..MAX_SERVER_ANIMATED as i32 {
            game.world.set(i % 40, 30 + i / 40, 20, Block::new(id::WATER, 0));
            game.animate_block((i % 40, 30 + i / 40, 20), BlockAnimation::new(vec![0.05; 2], true, true)).unwrap();
        }
        assert!(game.animate_block((0, 0, 21), BlockAnimation::sea()).is_err(), "capped");
        assert_eq!(game.animated_count(), MAX_SERVER_ANIMATED);
        ticks(&mut game, 4);
        let out = game.drain_outbox();
        assert!(out.len() >= 2, "{} messages", out.len());
        for msg in &out {
            match msg {
                ServerMsg::BlocksSet { edits } => assert!(edits.len() <= MAX_ANIMATION_EDITS),
                other => panic!("{other:?}"),
            }
        }
    }

    #[test]
    fn the_sea_is_left_to_the_clients_so_water_chunks_stay_unmodified_and_quiet() {
        let mut game = Game::island(5);
        game.add_player();
        ticks(&mut game, 120);
        assert_eq!(game.animated_count(), 0);
        assert!(game.drain_outbox().is_empty(), "no animation traffic from the server");
    }

    fn benchmark_command(game: &mut Game, id: u32) {
        game.handle(id, ClientMsg::Command { line: "benchmark".into(), path: String::new() });
    }

    #[test]
    fn the_benchmark_is_for_the_host_only_and_not_in_a_game_mode() {
        let mut game = Game::island(5);
        let host = game.add_player();
        let guest = game.add_player();
        assert!(game.spawn_benchmark_ball(guest).is_err());
        benchmark_command(&mut game, guest);
        assert_eq!(game.balls.len(), 0);
        benchmark_command(&mut game, host);
        assert_eq!(game.balls.len(), 1);
        assert_eq!(game.entity_count(), 3);

        let mut caveland = caveland_game();
        let me = caveland.add_player();
        assert!(caveland.spawn_benchmark_ball(me).is_err());
    }

    #[test]
    fn balls_are_things_not_players_and_fall_and_bounce() {
        let mut game = Game::island(5);
        let host = game.add_player();
        game.spawn_benchmark_ball(host).unwrap();
        let ball = game.balls[0].0;
        let z0 = game.entities.get(ball).unwrap().position.z;
        ticks(&mut game, 30);
        assert!(game.entities.get(ball).unwrap().position.z < z0, "falling");
        let players = match game.snapshot() {
            ServerMsg::Snapshot { players, .. } => players,
            _ => unreachable!(),
        };
        assert_eq!(players.len(), 1, "the ball is not a player");
        let things = game.drain_outbox().into_iter().find_map(|m| match m {
            ServerMsg::Things { things, .. } => Some(things),
            _ => None,
        });
        let things = things.expect("balls are sent as things");
        assert_eq!((things.len(), things[0].kind.as_str(), things[0].id), (1, "Benchmark Ball", ball));
    }

    #[test]
    fn the_benchmark_cannot_overload_the_server() {
        let mut game = Game::island(5);
        let host = game.add_player();
        game.spawn_benchmark_ball(host).unwrap();
        // The spawner shortens its interval over time; give it ample ticks and keep the clock "fast".
        for _ in 0..(BALL_LIFETIME_TICKS as u32 / 2) {
            game.step();
            game.last_tick_secs = 0.0;
            assert!(game.balls.len() <= MAX_BENCHMARK_BALLS);
            assert!(game.entity_count() <= MAX_ENTITIES_FOR_BENCHMARK);
        }
        assert!(game.balls.len() > 5, "it does add balls: {}", game.balls.len());
        for _ in 0..MAX_BENCHMARK_BALLS * 2 {
            let _ = game.spawn_benchmark_ball(host);
        }
        assert!(game.balls.len() <= MAX_BENCHMARK_BALLS);
        assert!(game.spawn_benchmark_ball(host).is_err() || game.balls.len() < MAX_BENCHMARK_BALLS);
        // Balls expire, and the host leaving stops the benchmark.
        game.tick += BALL_LIFETIME_TICKS + 1;
        game.update_benchmark();
        assert!(game.balls.len() <= 1, "old balls are gone: {}", game.balls.len());
        game.remove_player(host);
        assert!(game.benchmark.is_none());
    }
}
