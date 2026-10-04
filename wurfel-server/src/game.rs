//! Server-side game state. No networking in here, so it can be unit tested directly.

use std::collections::HashMap;

use glam::Vec3;
use wurfel_sim::block::id;
use wurfel_sim::entity::physics::occupied_cells;
use wurfel_sim::entity::{Entities, EntityId, Event};
use wurfel_sim::grid::to_iso;
use wurfel_sim::player::{apply_input, new_player, spawn_points, PlayerInput, TICK_DT, TICK_RATE};
use wurfel_sim::protocol::{ClientMsg, Edit, PlayerState, ServerMsg};
use wurfel_sim::generator::{create_generator, generators, Generator};
use wurfel_sim::grid::{chunk_of, from_iso};
use wurfel_sim::protocol::{clean_name, encode_chunk, PlayerInfo, WorldInfo};

use crate::maps::OpenedWorld;

/// Placeholder while a disk-backed world replaces the generator-only one in `from_opened`.
struct NoGenerator;
impl Generator for NoGenerator {
    fn generate(&self, _x: i32, _y: i32, _z: i32) -> Block {
        Block::AIR
    }
}
use wurfel_sim::{Block, World, CHUNK_SIZE_Z};

/// How far from a player's body centre (in blocks) they may place or break blocks.
pub const REACH: f32 = 12.0;

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

pub struct Game {
    spec: WorldSpec,
    peak: (i32, i32),
    world: World,
    /// Every player is an entity; the player id is the entity id.
    entities: Entities,
    inputs: HashMap<EntityId, PlayerInput>,
    roster: HashMap<EntityId, PlayerInfo>,
    spawned: usize,
    tick: u64,
}

impl Game {
    /// A world with the given generator, or an error naming what is wrong.
    pub fn new(spec: WorldSpec) -> Result<Self, String> {
        let generator = create_generator(&spec.generator, spec.seed).ok_or_else(|| {
            let known: Vec<&str> = generators().iter().map(|g| g.id).collect();
            format!("unknown generator '{}' (available: {})", spec.generator, known.join(", "))
        })?;
        let spawn = generator.spawn_point();
        Ok(Self::with_generator(spec, generator, spawn))
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
            spawned: 0,
            tick: 0,
        }
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
        game.world = World::with_store(opened.generator, opened.store);
        game.world.load_area(chunk_of(spawn.0, spawn.1), 1);
        game
    }

    /// Write the chunks that changed to disk. Returns how many were written.
    pub fn save(&mut self) -> std::io::Result<usize> {
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
        }
    }

    pub fn spec(&self) -> &WorldSpec {
        &self.spec
    }

    /// Add a player standing near the mountain peak and return their id.
    pub fn add_player(&mut self) -> u32 {
        self.add_player_as("", [230, 190, 50])
    }

    /// Add a player with the name and colour they chose. A name that is empty after cleaning
    /// becomes `Player <id>`.
    pub fn add_player_as(&mut self, name: &str, color: [u8; 3]) -> u32 {
        let points = spawn_points(&self.world, self.peak);
        let spot = points[self.spawned % points.len()];
        self.spawned += 1;
        let id = self.entities.spawn(new_player(spot));
        self.inputs.insert(id, PlayerInput::default());
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
                Some(PlayerState { id: e.id(), pos: e.position.to_array(), vel: body.movement.to_array() })
            })
            .collect()
    }

    /// One fixed physics step. The browser runs the same step for prediction.
    pub fn tick(&mut self) -> Vec<Event> {
        // Physics reads blocks, so the chunks around every player must be in memory before the step.
        let centres: Vec<_> = self.entities.iter().map(|e| from_iso(e.position.x, e.position.y)).collect();
        for (x, y) in centres {
            self.world.load_area(chunk_of(x, y), 1);
        }
        for (&id, &input) in &self.inputs {
            if let Some(entity) = self.entities.get_mut(id) {
                apply_input(entity, input, &self.world);
            }
        }
        self.tick += 1;
        self.entities.update(&self.world, TICK_DT)
    }

    /// Apply a message from a client. Returns a message to broadcast to everyone, if any.
    /// Anything invalid is ignored: the client is not trusted.
    pub fn handle(&mut self, player: u32, msg: ClientMsg) -> Option<ServerMsg> {
        match msg {
            ClientMsg::Input(input) => {
                if let Some(slot) = self.inputs.get_mut(&player) {
                    *slot = input;
                }
                None
            }
            ClientMsg::SetBlock { x, y, z, block } => self.set_block(player, Edit { x, y, z, block }),
            // Answered by the connection itself (pings, lobby requests, joining): no game state needed.
            ClientMsg::Ping { .. }
            | ClientMsg::ListMaps
            | ClientMsg::GetWorld
            | ClientMsg::LoadMap { .. }
            | ClientMsg::CreateMap { .. }
            | ClientMsg::Join { .. } => None,
        }
    }

    fn set_block(&mut self, player: u32, edit: Edit) -> Option<ServerMsg> {
        let body_centre = self.entities.get(player)?.position + Vec3::new(0.0, 0.0, 0.7);
        let wanted = Block::from_raw(edit.block);
        let allowed = wanted.value() == 0 && (wanted.is_air() || PLACEABLE.contains(&wanted.id()));
        if !allowed || !(0..CHUNK_SIZE_Z).contains(&edit.z) {
            return None;
        }
        let (gx, gy) = to_iso(edit.x, edit.y);
        if body_centre.distance(Vec3::new(gx, gy, edit.z as f32 + 0.5)) > REACH {
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
        Some(ServerMsg::BlockSet(edit))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wurfel_sim::entity::physics::ground_height;
    use wurfel_sim::grid::from_iso;

    fn run(game: &mut Game, ticks: u32) -> Vec<Event> {
        (0..ticks).flat_map(|_| game.tick()).collect()
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

        game.handle(id, ClientMsg::Input(PlayerInput { right: true, ..Default::default() }));
        run(&mut game, 20);
        let moved = state(&game, id);
        assert_ne!(moved.pos, before.pos);

        game.handle(id, ClientMsg::Input(PlayerInput { jump: true, ..Default::default() }));
        let mut peak = f32::MIN;
        let mut landed = false;
        for _ in 0..90 {
            let events = game.tick();
            landed |= events.contains(&Event::Landed(id));
            peak = peak.max(state(&game, id).pos[2]);
            game.handle(id, ClientMsg::Input(PlayerInput::default()));
        }
        assert!(peak > moved.pos[2] + 0.5, "jumped: peak {peak}, was at {}", moved.pos[2]);
        assert!(landed, "and came back down");
    }

    #[test]
    fn hostile_input_is_harmless() {
        let mut game = Game::island(1);
        let id = game.add_player();
        assert!(game.handle(99, ClientMsg::Input(PlayerInput { up: true, ..Default::default() })).is_none());
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
        let id = game.add_player();
        let (x, y) = neighbour(&game, id);
        let stone = Block::new(id::STONE, 0).raw();
        let z = CHUNK_SIZE_Z - 1; // up in the air, nobody is there

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
    fn invalid_edits_are_rejected() {
        let mut game = Game::island(1);
        let id = game.add_player();
        let (x, y) = neighbour(&game, id);
        let stone = Block::new(id::STONE, 0).raw();
        let top = CHUNK_SIZE_Z - 1;
        let cases = [
            ("above the world", ClientMsg::SetBlock { x, y, z: CHUNK_SIZE_Z, block: stone }),
            ("below the world", ClientMsg::SetBlock { x, y, z: -1, block: stone }),
            ("outside the world", ClientMsg::SetBlock { x: 10_000, y, z: 3, block: stone }),
            ("out of reach", ClientMsg::SetBlock { x: x + 20, y, z: 3, block: stone }),
            ("water is not placeable", ClientMsg::SetBlock { x, y, z: top, block: Block::new(id::WATER, 0).raw() }),
            ("unknown block id", ClientMsg::SetBlock { x, y, z: top, block: 200 }),
            ("variant bits set", ClientMsg::SetBlock { x, y, z: top, block: Block::new(id::STONE, 5).raw() }),
            ("already air", ClientMsg::SetBlock { x, y, z: top, block: 0 }),
        ];
        for (name, msg) in cases {
            assert!(game.handle(id, msg).is_none(), "{name} should be rejected");
        }
    }

    #[test]
    fn nothing_can_be_placed_inside_a_player_but_beside_and_above_is_fine() {
        let mut game = Game::island(1);
        let id = game.add_player();
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
        let id = game.add_player();
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
