//! Messages exchanged between the browser and the server. JSON for now: readable in the browser's
//! network tab while the protocol is still changing. Swap for a binary format once it settles.

use serde::{Deserialize, Serialize};

use crate::chunk::Chunk;
use crate::{Block, CHUNK_SIZE_X, CHUNK_SIZE_Y, CHUNK_SIZE_Z};

pub use crate::player::PlayerInput;

/// How a message wants to travel. Today everything goes over one WebSocket (TCP, so everything is
/// reliable and ordered). The split is here so that a second, unreliable transport can carry the
/// real-time traffic later without changing the messages: a WebRTC data channel or WebTransport
/// datagrams in browsers (raw UDP is not available there), plain UDP for native clients.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Channel {
    /// Must arrive, in order: joining, chunks, block changes, lobby and map operations.
    Reliable,
    /// Only the newest matters and a lost one is harmless: positions, input state, pings, statistics.
    /// A sender must repeat state instead of relying on a single message getting through.
    Unreliable,
}

/// A block that differs from what the generator would produce.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Edit {
    pub x: i32,
    pub y: i32,
    pub z: i32,
    /// Raw packed block, see [`crate::Block::raw`]. 0 is air.
    pub block: u16,
}

/// Where a player is and how it moves, in blocks and blocks per second (see
/// [`crate::entity::physics`] for the frame).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PlayerState {
    pub id: u32,
    pub pos: [f32; 3],
    pub vel: [f32; 3],
    /// Sequence number of the last [`ClientMsg::Input`] the server applied for this player.
    #[serde(default)]
    pub input_seq: u32,
    /// How many physics ticks the server has simulated under that input; `pos` and `vel` are the
    /// state after exactly these ticks. The owner replays the rest of its own steps from there.
    #[serde(default)]
    pub input_ticks: u32,
}

/// Server health, sent about once a second.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ServerStats {
    pub players: u32,
    pub entities: u32,
    pub loaded_chunks: u32,
    /// Time spent simulating one tick: average and worst over the last second, in milliseconds.
    pub tick_ms_avg: f32,
    pub tick_ms_max: f32,
    /// Bytes the server sent / received in total since it started.
    pub bytes_out: u64,
    pub bytes_in: u64,
    pub uptime_s: u32,
}

/// Who a player is, as chosen in the menu.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlayerInfo {
    pub id: u32,
    pub name: String,
    /// Display colour, red green blue.
    pub color: [u8; 3],
}

pub const MAX_NAME_CHARS: usize = 16;

/// A name the server is willing to show to others: no control characters, trimmed, at most
/// [`MAX_NAME_CHARS`] characters. Empty after cleaning means the caller should pick a default.
pub fn clean_name(name: &str) -> String {
    let filtered: String = name.chars().filter(|c| !c.is_control()).collect();
    filtered.trim().chars().take(MAX_NAME_CHARS).collect::<String>().trim().to_string()
}

fn default_color() -> [u8; 3] {
    [230, 190, 50]
}

/// The world the server has loaded, as the lobby shows it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorldInfo {
    /// Display name of the map.
    pub map: String,
    /// The map's folder name, what `LoadMap` takes.
    pub map_id: String,
    pub slot: u32,
    pub generator: String,
    pub seed: u64,
    /// Players that have joined (connections that are only looking at the lobby do not count).
    pub players: u32,
    /// The rules the world is played by, e.g. `engine` or `caveland`.
    #[serde(default = "default_game_mode")]
    pub gamemode: String,
}

/// Identifies this build: the crate version plus the git commit it was built from (`0.1.0+1a2b3c4d`,
/// just the version outside a git checkout). Server and client both carry the one of `wurfel-sim`,
/// so they match exactly when they were built from the same commit.
pub fn build_id() -> String {
    let hash = env!("WURFEL_GIT_HASH");
    if hash.is_empty() {
        env!("CARGO_PKG_VERSION").to_string()
    } else {
        format!("{}+{hash}", env!("CARGO_PKG_VERSION"))
    }
}

/// Whether a server's build id means the client is out of date. A server that sends none (an older
/// one) never counts as a mismatch.
pub fn build_mismatch(client: &str, server: &str) -> bool {
    !client.is_empty() && !server.is_empty() && client != server
}

/// The mode of a map that does not name one.
pub fn default_game_mode() -> String {
    "engine".to_string()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SaveSummary {
    pub slot: u32,
    /// ISO 8601 time of the newest change, if the save has data.
    pub modified: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MapSummary {
    pub id: String,
    pub name: String,
    pub description: String,
    pub generator: String,
    pub seed: u64,
    #[serde(default = "default_game_mode")]
    pub gamemode: String,
    pub saves: Vec<SaveSummary>,
}

/// A non-player entity of a game mode (an item, a robot...), as the client draws it. The engine does
/// not know what `kind` means: it is the mode's own name for it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ThingState {
    pub id: u32,
    pub kind: String,
    pub pos: [f32; 3],
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GeneratorSummary {
    pub id: String,
    pub name: String,
    pub description: String,
    pub uses_seed: bool,
}

/// Which save slot `LoadMap` means: an existing one by number, or `"new"` to start a fresh one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "RawSlot", into = "RawSlot")]
pub enum SlotChoice {
    Existing(u32),
    New,
}

#[derive(Serialize, Deserialize)]
#[serde(untagged)]
enum RawSlot {
    Number(u32),
    Word(String),
}

impl From<RawSlot> for SlotChoice {
    fn from(raw: RawSlot) -> Self {
        match raw {
            RawSlot::Number(n) => SlotChoice::Existing(n),
            // Anything that is not a number counts as "new": the only word the protocol defines.
            RawSlot::Word(_) => SlotChoice::New,
        }
    }
}

impl From<SlotChoice> for RawSlot {
    fn from(slot: SlotChoice) -> Self {
        match slot {
            SlotChoice::Existing(n) => RawSlot::Number(n),
            SlotChoice::New => RawSlot::Word("new".to_string()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ClientMsg {
    // ----- lobby: allowed before joining
    /// Ask for the maps on the server (answered with `Maps`).
    ListMaps,
    /// Ask for the loaded world again (answered with `Lobby`).
    GetWorld,
    /// Load a save of a map into the server. Refused while players are in the world.
    LoadMap { map: String, slot: SlotChoice },
    CreateMap {
        id: String,
        name: String,
        description: String,
        generator: String,
        seed: u64,
        /// Rules of the new map; empty picks the generator's usual one.
        #[serde(default)]
        gamemode: String,
    },
    /// Enter the loaded world as a player: answered with `Welcome`, then chunks start to flow.
    Join {
        #[serde(default)]
        name: String,
        #[serde(default = "default_color")]
        color: [u8; 3],
    },
    // ----- in the world
    /// What the player is pressing. Sent whenever it changes; the server keeps applying it.
    /// `seq` counts the changes (the first one is 1); the server ignores a `seq` that is not newer
    /// than the last one it applied, and echoes it in [`PlayerState::input_seq`]. A pressed jump is
    /// never lost, even if the next change arrives before the next tick.
    Input { seq: u32, input: PlayerInput },
    /// Place a block, or remove it with `block == 0`.
    SetBlock { x: i32, y: i32, z: i32, block: u16 },
    /// A one-off action of the game mode (for Caveland: `attack`, `throw`, `craft`...). The engine
    /// does not interpret it; unknown actions are ignored.
    Action {
        name: String,
        #[serde(default)]
        arg: i32,
    },
    /// A console line for the game mode (Caveland: `give Torch`, `tpplayer 0 0 10`). Whether it is
    /// allowed is up to the mode; the answer comes back as a `Rules` message of kind `console`.
    Command { line: String },
    /// Latency probe: the server answers with [`ServerMsg::Pong`] carrying the same `client_time`.
    ///
    /// `rtt_ms` is the latest round trip this client measured; the server shares it with everybody
    /// for the player list ([`ServerMsg::Pings`]). Absent in older clients.
    Ping {
        client_time: f64,
        #[serde(default)]
        rtt_ms: Option<f32>,
    },
}

impl ClientMsg {
    pub fn channel(&self) -> Channel {
        match self {
            ClientMsg::Input { .. } | ClientMsg::Ping { .. } => Channel::Unreliable,
            ClientMsg::SetBlock { .. }
            | ClientMsg::Action { .. }
            | ClientMsg::Command { .. }
            | ClientMsg::ListMaps
            | ClientMsg::GetWorld
            | ClientMsg::LoadMap { .. }
            | ClientMsg::CreateMap { .. }
            | ClientMsg::Join { .. } => Channel::Reliable,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ServerMsg {
    // ----- lobby
    /// First message on every connection: what is running, and the generators a new map can use.
    /// `build` is the server's [`build_id`] (empty from servers that predate it).
    Lobby {
        world: WorldInfo,
        generators: Vec<GeneratorSummary>,
        #[serde(default)]
        build: String,
    },
    Maps { maps: Vec<MapSummary> },
    /// A save was loaded: sent to every connection that is still in the lobby.
    WorldChanged { world: WorldInfo },
    MapCreated { map: MapSummary },
    /// A lobby request was refused. `request` is the type of the message it answers.
    Failed { request: String, message: String },
    // ----- in the world
    /// First message after connecting: everything needed to rebuild the world locally.
    Welcome {
        your_id: u32,
        /// Display name of the map the server has loaded.
        map: String,
        /// The save slot of that map.
        slot: u32,
        /// Generator id (informational: the client receives chunks, it does not generate them).
        generator: String,
        seed: u64,
        tick_rate: u32,
        players: Vec<PlayerState>,
        /// Name and colour of everybody in the world, you included.
        roster: Vec<PlayerInfo>,
        /// The rules this world is played by (`engine`, `caveland`).
        #[serde(default = "default_game_mode")]
        gamemode: String,
        /// The server's [`build_id`] (empty from servers that predate it).
        #[serde(default)]
        build: String,
    },
    /// Somebody joined (also sent for yourself, right after the welcome).
    PlayerJoined { player: PlayerInfo },
    /// State of all players, sent several times per second. `tick` counts server physics steps.
    Snapshot { tick: u64, players: Vec<PlayerState> },
    BlockSet(Edit),
    PlayerLeft { id: u32 },
    /// The server stopped sending this chunk because the player moved away: forget it.
    ChunkUnload { cx: i32, cy: i32 },
    /// The server is shutting down for an update (it has saved the world): sent to everybody just
    /// before it closes their sockets. A client keeps its view and rejoins when the server is back.
    ServerRestarting,
    /// Reply to [`ClientMsg::Ping`], to the sender only.
    Pong { client_time: f64, tick: u64 },
    Stats(ServerStats),
    /// Everybody's latest ping in milliseconds as `(player id, ms)`, about once a second. Players
    /// who have not reported one yet are left out.
    Pings { list: Vec<(u32, u32)> },
    /// The non-player entities of a game mode, sent with the snapshots.
    Things { tick: u64, things: Vec<ThingState> },
    /// Rules-specific news of a game mode: `kind` says what `data` is (Caveland: `state` with
    /// everybody's health and inventory, `events` with sounds and happenings). The engine only
    /// carries it.
    Rules { kind: String, data: serde_json::Value },
}

impl ServerMsg {
    pub fn channel(&self) -> Channel {
        match self {
            ServerMsg::Snapshot { .. }
            | ServerMsg::Pong { .. }
            | ServerMsg::Stats(_)
            | ServerMsg::Pings { .. }
            | ServerMsg::Things { .. } => Channel::Unreliable,
            ServerMsg::Welcome { .. }
            | ServerMsg::ServerRestarting
            | ServerMsg::BlockSet(_)
            | ServerMsg::PlayerLeft { .. }
            | ServerMsg::PlayerJoined { .. }
            | ServerMsg::ChunkUnload { .. }
            | ServerMsg::Lobby { .. }
            | ServerMsg::Maps { .. }
            | ServerMsg::WorldChanged { .. }
            | ServerMsg::MapCreated { .. }
            | ServerMsg::Rules { .. }
            | ServerMsg::Failed { .. } => Channel::Reliable,
        }
    }
}

// ------------------------------------------------------------------------------- chunk stream

/// First byte of a binary message that carries a chunk.
pub const CHUNK_MESSAGE: u8 = 1;
const CELLS: usize = (CHUNK_SIZE_X * CHUNK_SIZE_Y * CHUNK_SIZE_Z) as usize;
/// Binary header: kind, then the chunk x and y as little-endian `i32`.
const HEADER: usize = 1 + 4 + 4;

/// A chunk as sent over the network: run-length encoded `(id, value, health)` cells in storage
/// layer order. Lossless for every block (unlike the `.wec` file format, which cannot store a `~` byte),
/// and a typical chunk of mostly air or water is a few hundred bytes.
pub fn encode_chunk(chunk: &Chunk) -> Vec<u8> {
    let (cx, cy) = chunk.pos();
    let mut out = Vec::with_capacity(HEADER + 64);
    out.push(CHUNK_MESSAGE);
    out.extend_from_slice(&cx.to_le_bytes());
    out.extend_from_slice(&cy.to_le_bytes());

    let mut run: Option<([u8; 3], u16)> = None;
    // Layer by layer (z outermost), like the `.wec` format: terrain is made of flat layers, so the
    // runs are long.
    for z in 0..CHUNK_SIZE_Z {
        for lx in 0..CHUNK_SIZE_X {
            for ly in 0..CHUNK_SIZE_Y {
                let block = chunk.get(lx, ly, z);
                let cell = [block.id(), block.value(), chunk.health(lx, ly, z)];
                run = match run {
                    Some((current, n)) if current == cell && n < u16::MAX => Some((current, n + 1)),
                    Some((current, n)) => {
                        push_run(&mut out, current, n);
                        Some((cell, 1))
                    }
                    None => Some((cell, 1)),
                };
            }
        }
    }
    if let Some((cell, n)) = run {
        push_run(&mut out, cell, n);
    }
    out
}

fn push_run(out: &mut Vec<u8>, cell: [u8; 3], n: u16) {
    out.extend_from_slice(&n.to_le_bytes());
    out.extend_from_slice(&cell);
}

/// Reverse of [`encode_chunk`]. Anything malformed is an error, never a panic or a half chunk.
pub fn decode_chunk(bytes: &[u8]) -> Result<Chunk, String> {
    if bytes.len() < HEADER || bytes[0] != CHUNK_MESSAGE {
        return Err("not a chunk message".to_string());
    }
    let cx = i32::from_le_bytes(bytes[1..5].try_into().expect("4 bytes"));
    let cy = i32::from_le_bytes(bytes[5..9].try_into().expect("4 bytes"));
    let mut chunk = Chunk::new((cx, cy));

    let runs = &bytes[HEADER..];
    if runs.len() % 5 != 0 {
        return Err("chunk data is cut off".to_string());
    }
    let mut index = 0usize;
    for run in runs.chunks_exact(5) {
        let n = u16::from_le_bytes([run[0], run[1]]) as usize;
        if n == 0 || index + n > CELLS {
            return Err("chunk run is out of range".to_string());
        }
        let (block, health) = (Block::new(run[2], run[3]), run[4]);
        for cell in index..index + n {
            let layer = (CHUNK_SIZE_X * CHUNK_SIZE_Y) as usize;
            let z = (cell / layer) as i32;
            let lx = (cell % layer / CHUNK_SIZE_Y as usize) as i32;
            let ly = (cell % CHUNK_SIZE_Y as usize) as i32;
            chunk.set(lx, ly, z, block);
            if health != 0 {
                chunk.set_health(lx, ly, z, health);
            }
        }
        index += n;
    }
    if index != CELLS {
        return Err(format!("chunk has {index} cells, expected {CELLS}"));
    }
    chunk.mark_saved(); // freshly received, not a local modification
    Ok(chunk)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lobby_messages_round_trip_and_slots_accept_a_number_or_new() {
        let world = WorldInfo { map: "Island".into(), map_id: "island".into(), slot: 1, generator: "island".into(), seed: 4, players: 0, gamemode: "caveland".into() };
        let messages = [
            ServerMsg::Lobby { world: world.clone(), generators: vec![GeneratorSummary { id: "island".into(), name: "Island".into(), description: "d".into(), uses_seed: true }], build: build_id() },
            ServerMsg::Maps { maps: vec![MapSummary { id: "a".into(), name: "A".into(), description: "".into(), generator: "air".into(), seed: 1, gamemode: "engine".into(), saves: vec![SaveSummary { slot: 0, modified: None }] }] },
            ServerMsg::WorldChanged { world },
            ServerMsg::Failed { request: "LoadMap".into(), message: "nope".into() },
        ];
        for msg in messages {
            let json = serde_json::to_string(&msg).unwrap();
            assert_eq!(serde_json::from_str::<ServerMsg>(&json).unwrap(), msg, "{json}");
        }
        let load = |json: &str| serde_json::from_str::<ClientMsg>(json).unwrap();
        assert_eq!(load(r#"{"type":"LoadMap","map":"a","slot":3}"#), ClientMsg::LoadMap { map: "a".into(), slot: SlotChoice::Existing(3) });
        assert_eq!(load(r#"{"type":"LoadMap","map":"a","slot":"new"}"#), ClientMsg::LoadMap { map: "a".into(), slot: SlotChoice::New });
        assert_eq!(load(r#"{"type":"Join"}"#), ClientMsg::Join { name: String::new(), color: [230, 190, 50] }, "name and colour are optional");
        assert_eq!(load(r#"{"type":"Join","name":"Ann","color":[1,2,3]}"#), ClientMsg::Join { name: "Ann".into(), color: [1, 2, 3] });
        assert_eq!(load(r#"{"type":"ListMaps"}"#), ClientMsg::ListMaps);
        let create = ClientMsg::CreateMap { id: "x".into(), name: "X".into(), description: "".into(), generator: "island".into(), seed: 2, gamemode: "caveland".into() };
        assert_eq!(load(&serde_json::to_string(&create).unwrap()), create);
        assert!(serde_json::from_str::<ClientMsg>(r#"{"type":"LoadMap","map":"a","slot":-1}"#).is_err(), "a negative slot is rejected, not guessed");
        assert!(serde_json::from_str::<ClientMsg>(r#"{"type":"LoadMap","map":"a","slot":null}"#).is_err());
    }

    #[test]
    fn names_are_cleaned_for_display() {
        assert_eq!(clean_name("  Ann  "), "Ann");
        assert_eq!(clean_name("A\nn\u{7}"), "An");
        assert_eq!(clean_name(&"x".repeat(40)).chars().count(), MAX_NAME_CHARS);
        assert_eq!(clean_name("   \t "), "");
        assert_eq!(clean_name("Zoë 🙂"), "Zoë 🙂");
    }

    #[test]
    fn a_ping_without_rtt_from_an_older_client_still_parses() {
        let old: ClientMsg = serde_json::from_str(r#"{"type":"Ping","client_time":5.5}"#).unwrap();
        assert_eq!(old, ClientMsg::Ping { client_time: 5.5, rtt_ms: None });
        let new: ClientMsg = serde_json::from_str(r#"{"type":"Ping","client_time":5.5,"rtt_ms":12.5}"#).unwrap();
        assert_eq!(new, ClientMsg::Ping { client_time: 5.5, rtt_ms: Some(12.5) });
    }

    #[test]
    fn messages_say_which_channel_they_belong_on() {
        assert_eq!(ClientMsg::Input { seq: 1, input: PlayerInput::default() }.channel(), Channel::Unreliable);
        assert_eq!(ClientMsg::Ping { client_time: 0.0, rtt_ms: None }.channel(), Channel::Unreliable);
        assert_eq!(ClientMsg::SetBlock { x: 0, y: 0, z: 0, block: 0 }.channel(), Channel::Reliable);
        assert_eq!(ServerMsg::Snapshot { tick: 0, players: vec![] }.channel(), Channel::Unreliable);
        assert_eq!(ServerMsg::PlayerLeft { id: 1 }.channel(), Channel::Reliable);
        assert_eq!(ServerMsg::Pings { list: vec![] }.channel(), Channel::Unreliable);
        assert_eq!(ServerMsg::BlockSet(Edit { x: 0, y: 0, z: 0, block: 0 }).channel(), Channel::Reliable);
    }

    #[test]
    fn messages_round_trip_through_json() {
        let messages = [
            ClientMsg::SetBlock { x: -3, y: 7, z: 2, block: 3 },
            ClientMsg::Input { seq: 7, input: PlayerInput { up: true, jump: true, ..Default::default() } },
            ClientMsg::Ping { client_time: 1234.5, rtt_ms: Some(31.5) },
            ClientMsg::Ping { client_time: 1.0, rtt_ms: None },
        ];
        for msg in messages {
            let json = serde_json::to_string(&msg).unwrap();
            assert_eq!(serde_json::from_str::<ClientMsg>(&json).unwrap(), msg, "{json}");
        }

        let player = PlayerState { id: 4, pos: [1.5, -2.0, 3.0], vel: [0.0, 1.0, -9.8], input_seq: 12, input_ticks: 3 };
        let messages = [
            ServerMsg::Welcome {
                your_id: 4,
                map: "Island".to_string(),
                slot: 2,
                generator: "island".to_string(),
                seed: 1,
                tick_rate: 60,
                players: vec![player],
                roster: vec![PlayerInfo { id: 4, name: "Ann".into(), color: [1, 2, 3] }],
                gamemode: "caveland".to_string(),
                build: build_id(),
            },
            ServerMsg::ServerRestarting,
            ServerMsg::PlayerJoined { player: PlayerInfo { id: 5, name: "Bo".into(), color: [9, 9, 9] } },
            ServerMsg::ChunkUnload { cx: -3, cy: 7 },
            ServerMsg::Snapshot { tick: 99, players: vec![player] },
            ServerMsg::BlockSet(Edit { x: 0, y: 0, z: 1, block: 3 }),
            ServerMsg::PlayerLeft { id: 2 },
            ServerMsg::Pong { client_time: 1234.5, tick: 7 },
            ServerMsg::Pings { list: vec![(1, 20), (4, 135)] },
            ServerMsg::Stats(ServerStats {
                players: 2,
                entities: 2,
                loaded_chunks: 9,
                tick_ms_avg: 0.2,
                tick_ms_max: 1.5,
                bytes_out: 10,
                bytes_in: 5,
                uptime_s: 60,
            }),
        ];
        for msg in messages {
            let json = serde_json::to_string(&msg).unwrap();
            assert_eq!(serde_json::from_str::<ServerMsg>(&json).unwrap(), msg, "{json}");
        }
    }

    #[test]
    fn input_missing_fields_are_rejected_not_defaulted_silently() {
        // A typo'd key name must not turn into "no keys pressed" by accident.
        let parsed = serde_json::from_str::<ClientMsg>(r#"{"type":"Input","seq":1,"input":{"upp":true}}"#);
        assert!(parsed.is_err());
        // The sequence number is required too: without it the server could not order inputs.
        let parsed = serde_json::from_str::<ClientMsg>(
            r#"{"type":"Input","input":{"up":true,"down":false,"left":false,"right":false,"jump":false}}"#,
        );
        assert!(parsed.is_err());
    }

    #[test]
    fn player_state_without_input_ack_defaults_to_zero() {
        let state: PlayerState = serde_json::from_str(r#"{"id":1,"pos":[0,0,0],"vel":[0,0,0]}"#).unwrap();
        assert_eq!((state.input_seq, state.input_ticks), (0, 0));
    }

    // ----- chunk stream

    use crate::generator::create_generator;
    use crate::World;

    fn world_chunk(generator: &str, seed: u64, pos: (i32, i32)) -> Chunk {
        let mut world = World::new(create_generator(generator, seed).unwrap());
        world.load_chunk(pos.0, pos.1);
        world.chunk(pos.0, pos.1).unwrap().clone()
    }

    #[test]
    fn every_generator_survives_the_trip() {
        for info in crate::generator::generators() {
            for pos in [(0, 0), (-1, 2), (30, 31)] {
                let chunk = world_chunk(info.id, 7, pos);
                let bytes = encode_chunk(&chunk);
                let back = decode_chunk(&bytes).unwrap_or_else(|e| panic!("{} {pos:?}: {e}", info.id));
                assert_eq!(back.pos(), pos);
                for lx in 0..CHUNK_SIZE_X {
                    for ly in 0..CHUNK_SIZE_Y {
                        for z in 0..CHUNK_SIZE_Z {
                            assert_eq!(back.get(lx, ly, z), chunk.get(lx, ly, z), "{} {pos:?} at {lx},{ly},{z}", info.id);
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn blocks_the_wec_file_format_cannot_store_are_fine_on_the_wire() {
        let mut chunk = Chunk::new((4, -4));
        chunk.set(1, 2, 3, Block::new(126, 126)); // '~' is the .wec marker byte
        chunk.set(9, 39, 9, Block::new(255, 255));
        chunk.set_health(1, 2, 3, 42);
        let back = decode_chunk(&encode_chunk(&chunk)).unwrap();
        assert_eq!(back.get(1, 2, 3), Block::new(126, 126));
        assert_eq!(back.get(9, 39, 9), Block::new(255, 255));
        assert_eq!(back.health(1, 2, 3), 42);
        assert!(!back.is_modified(), "a received chunk is not a local modification");
    }

    #[test]
    fn chunks_are_small_on_the_wire() {
        let island = encode_chunk(&world_chunk("island", 1, (5, 5)));
        assert!(island.len() < 200, "a plain sea chunk should be tiny, got {} bytes", island.len());
        let air = encode_chunk(&Chunk::new((0, 0)));
        assert_eq!(air.len(), HEADER + 5, "all air is one run");
        // The worst case is bounded by the cell count.
        let noisy = encode_chunk(&world_chunk("blocktest", 1, (0, 0)));
        assert!(noisy.len() <= HEADER + 5 * CELLS);
    }

    #[test]
    fn malformed_chunk_messages_are_errors() {
        let good = encode_chunk(&world_chunk("island", 1, (0, 0)));
        assert!(decode_chunk(&[]).is_err());
        assert!(decode_chunk(&good[..4]).is_err(), "cut inside the header");
        assert!(decode_chunk(&good[..good.len() - 1]).is_err(), "cut inside a run");
        assert!(decode_chunk(&good[..good.len() - 5]).is_err(), "missing the last run: too few cells");
        let mut wrong_kind = good.clone();
        wrong_kind[0] = 9;
        assert!(decode_chunk(&wrong_kind).is_err());
        let mut too_many = good.clone();
        too_many.extend_from_slice(&[1, 0, 0, 0, 0]);
        assert!(decode_chunk(&too_many).is_err(), "more cells than a chunk has");
        let mut empty_run = good.clone();
        empty_run[HEADER] = 0;
        empty_run[HEADER + 1] = 0;
        assert!(decode_chunk(&empty_run).is_err(), "a run of zero cells");
        let mut huge = good[..HEADER].to_vec();
        huge.extend_from_slice(&[0xFF, 0xFF, 1, 0, 0]);
        assert!(decode_chunk(&huge).is_err(), "a run longer than the chunk");
    }
}

#[cfg(test)]
mod game_mode_tests {
    use super::*;

    #[test]
    fn game_mode_messages_round_trip() {
        let things = ServerMsg::Things { tick: 8, things: vec![ThingState { id: 3, kind: "torch".into(), pos: [1.0, 2.0, 3.0] }] };
        let rules = ServerMsg::Rules { kind: "state".into(), data: serde_json::json!({ "4": { "health": 80.0, "items": ["torch"] } }) };
        let action = ClientMsg::Action { name: "craft".into(), arg: 2 };
        for msg in [things, rules] {
            let json = serde_json::to_string(&msg).unwrap();
            assert_eq!(serde_json::from_str::<ServerMsg>(&json).unwrap(), msg, "{json}");
        }
        let json = serde_json::to_string(&action).unwrap();
        assert_eq!(serde_json::from_str::<ClientMsg>(&json).unwrap(), action, "{json}");
    }

    #[test]
    fn the_game_mode_is_optional_and_defaults_to_the_engine() {
        let create: ClientMsg = serde_json::from_str(r#"{"type":"CreateMap","id":"a","name":"A","description":"","generator":"air","seed":1}"#).unwrap();
        assert!(matches!(create, ClientMsg::CreateMap { gamemode, .. } if gamemode.is_empty()), "empty: the server picks");
        let action: ClientMsg = serde_json::from_str(r#"{"type":"Action","name":"attack"}"#).unwrap();
        assert_eq!(action, ClientMsg::Action { name: "attack".into(), arg: 0 });
        let world: WorldInfo = serde_json::from_str(r#"{"map":"m","map_id":"m","slot":0,"generator":"air","seed":1,"players":0}"#).unwrap();
        assert_eq!(world.gamemode, "engine");
    }

    #[test]
    fn the_build_id_is_optional_so_old_servers_still_parse() {
        let lobby: ServerMsg = serde_json::from_str(
            r#"{"type":"Lobby","world":{"map":"m","map_id":"m","slot":0,"generator":"air","seed":1,"players":0},"generators":[]}"#,
        )
        .unwrap();
        assert!(matches!(lobby, ServerMsg::Lobby { build, .. } if build.is_empty()));
        let welcome: ServerMsg = serde_json::from_str(
            r#"{"type":"Welcome","your_id":1,"map":"m","slot":0,"generator":"air","seed":1,"tick_rate":60,"players":[],"roster":[]}"#,
        )
        .unwrap();
        assert!(matches!(welcome, ServerMsg::Welcome { build, .. } if build.is_empty()));
        assert!(build_id().starts_with(env!("CARGO_PKG_VERSION")));
    }

    #[test]
    fn only_a_known_different_build_counts_as_a_mismatch() {
        assert!(!build_mismatch("0.1.0+a", "0.1.0+a"));
        assert!(build_mismatch("0.1.0+a", "0.1.0+b"));
        assert!(!build_mismatch("0.1.0+a", ""), "an old server sends no build id");
        assert!(!build_mismatch("", "0.1.0+b"));
    }

    #[test]
    fn the_restart_notice_is_a_reliable_message_without_fields() {
        assert_eq!(serde_json::to_string(&ServerMsg::ServerRestarting).unwrap(), r#"{"type":"ServerRestarting"}"#);
        assert_eq!(ServerMsg::ServerRestarting.channel(), Channel::Reliable);
    }

    #[test]
    fn game_mode_traffic_uses_the_right_channels() {
        assert_eq!(ClientMsg::Action { name: "x".into(), arg: 0 }.channel(), Channel::Reliable);
        assert_eq!(ClientMsg::Command { line: "give Torch".into() }.channel(), Channel::Reliable);
        let command: ClientMsg = serde_json::from_str(r#"{"type":"Command","line":"give Torch"}"#).unwrap();
        assert_eq!(command, ClientMsg::Command { line: "give Torch".into() });
        assert_eq!(ServerMsg::Things { tick: 0, things: vec![] }.channel(), Channel::Unreliable);
        assert_eq!(ServerMsg::Rules { kind: "events".into(), data: serde_json::Value::Null }.channel(), Channel::Reliable);
    }
}
