//! Authoritative multiplayer server. Also serves the built browser client, so one URL is enough.
//!
//!     wurfel-server [--port 3000] [--static ../wurfel-web/dist] [--seed 1] [--generator island]
//!                   [--lag-ms 0] [--maps-dir ./maps] [--skip-intro]
//!
//! The server holds exactly one world in memory (a map and one of its save slots, like the Java
//! engine's single `Map`). It streams the chunks around each player to their browser; the browser
//! never generates terrain. Maps and savegames live in `--maps-dir`, in the Java engine's layout
//! (existing Java maps work), and changed chunks are saved periodically and on shutdown.
//!
//! Everything goes over one WebSocket at `ws://host/ws`. A connection starts in the lobby (it can
//! list maps, load a save while nobody is playing, create a map) and then sends `Join` to play.
//!
//! On Ctrl-C or SIGTERM the server saves, sends every player `ServerRestarting`, closes their sockets
//! and exits; the browser client keeps its view and rejoins the next server (see wurfel-web/README.md).
//!
//! `--lag-ms` delays everything the server sends by that many milliseconds, to test how the client
//! copes with a slow connection.

mod caveland_mode;
mod friends;
mod game;
mod interest;
mod maps;
mod pings;

use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{DefaultBodyLimit, State};
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;
use futures_util::{SinkExt, StreamExt};
use tokio::sync::{broadcast, mpsc, watch};
use tower_http::services::ServeDir;
use wurfel_sim::player::TICK_RATE;
use wurfel_sim::protocol::{
    ClientMsg, GeneratorSummary, MapSummary, SaveSummary, ServerMsg, ServerStats, SlotChoice,
};

use game::{Game, WorldSpec};
use interest::Interest;
use maps::{MapCreate, MapInfo, MapStore};

/// Changed chunks are written to disk this often.
const AUTOSAVE_EVERY: Duration = Duration::from_secs(30);
/// After a shutdown signal the server waits this long for the connections to close, then exits.
const SHUTDOWN_GRACE: Duration = Duration::from_secs(5);

const TICK: Duration = Duration::from_micros(1_000_000 / TICK_RATE as u64);
/// Snapshots go out every this many ticks (30 per second at 60 ticks per second).
const SNAPSHOT_EVERY: u64 = 2;
/// Server statistics go out once per second.
const STATS_EVERY: u64 = TICK_RATE as u64;
/// Reject anything larger: legitimate client messages are tiny.
const MAX_MESSAGE_BYTES: usize = 1024;
/// A client that cannot keep up with this many queued messages is disconnected.
const OUTGOING_QUEUE: usize = 512;
/// Every player gets the chunks within this many chunks of them (5 x 5 chunks), a few at a time.
const CHUNK_RADIUS: i32 = 2;
const CHUNKS_PER_UPDATE: usize = 3;
const CHUNK_UPDATE_EVERY: Duration = Duration::from_millis(50);

/// Traffic totals, shared by all connections.
#[derive(Default)]
struct NetCounters {
    bytes_in: AtomicU64,
    bytes_out: AtomicU64,
}

#[derive(Clone)]
struct Shared {
    game: Arc<Mutex<Game>>,
    maps: Arc<MapStore>,
    /// Pre-serialised messages for everybody in the world.
    tx: broadcast::Sender<Arc<str>>,
    /// Messages for connections that are still in the lobby.
    lobby_tx: broadcast::Sender<Arc<str>>,
    net: Arc<NetCounters>,
    /// Each player's own measurement of their round trip, for the Tab player list.
    pings: Arc<Mutex<pings::Pings>>,
    /// Hearts in the Tab player list: who is friends with whom.
    friends: Arc<Mutex<friends::Friends>>,
    lag: Duration,
    started: Instant,
    /// Set to true when the server is shutting down: every connection says goodbye and closes.
    closing: watch::Sender<bool>,
    /// WebSocket connections that are still being served.
    open_sockets: Arc<AtomicUsize>,
}

fn arg(name: &str) -> Option<String> {
    let args: Vec<String> = std::env::args().collect();
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1).cloned())
}

#[tokio::main]
async fn main() {
    let port: u16 = arg("--port").and_then(|p| p.parse().ok()).unwrap_or(3000);
    let seed: u64 = arg("--seed").and_then(|s| s.parse().ok()).unwrap_or(1);
    let generator = arg("--generator").unwrap_or_else(|| "island".to_string());
    let lag = Duration::from_millis(arg("--lag-ms").and_then(|s| s.parse().ok()).unwrap_or(0));
    if std::env::args().any(|a| a == "--skip-intro") {
        caveland_mode::SKIP_INTRO.store(true, Ordering::Relaxed);
    }
    let static_dir = arg("--static").unwrap_or_else(|| "../wurfel-web/dist".to_string());

    let maps_dir = arg("--maps-dir").unwrap_or_else(default_maps_dir);
    let store = match MapStore::open(maps_dir.clone().into()).and_then(|store| {
        store.ensure_default_map().map_err(|e| std::io::Error::other(e.to_string()))?;
        Ok(store)
    }) {
        Ok(store) => store,
        Err(e) => {
            eprintln!("wurfel-server: cannot use the maps folder '{maps_dir}': {e}");
            std::process::exit(1);
        }
    };
    // `--generator` starts a throwaway in-memory world (handy for development); otherwise the
    // first map's newest save is loaded, or a new save is started in it.
    let game = if arg("--generator").is_some() {
        let spec = WorldSpec { map: generator.clone(), map_id: generator.clone(), slot: 0, generator: generator.clone(), seed };
        Game::new(spec)
    } else {
        initial_world(&store)
    };
    let game = match game {
        Ok(game) => game,
        Err(e) => {
            eprintln!("wurfel-server: cannot create the world: {e}");
            std::process::exit(1);
        }
    };
    eprintln!("wurfel-server: world '{}' (save {}), maps in {maps_dir}", game.spec().map, game.spec().slot);
    if arg("--generator").is_none() {
        store.remember_active(&game.spec().map_id, game.spec().slot);
    }
    let (tx, _) = broadcast::channel(256);
    let (lobby_tx, _) = broadcast::channel(16);
    let shared = Shared {
        game: Arc::new(Mutex::new(game)),
        maps: Arc::new(store),
        tx,
        lobby_tx,
        net: Arc::default(),
        pings: Arc::default(),
        friends: Arc::default(),
        lag,
        started: Instant::now(),
        closing: watch::channel(false).0,
        open_sockets: Arc::default(),
    };

    tokio::spawn(tick_loop(shared.clone()));

    let app = Router::new()
        .route("/ws", get(ws_handler))
        .layer(DefaultBodyLimit::max(1024))
        .fallback_service(ServeDir::new(&static_dir))
        .with_state(shared.clone());

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    let listener = tokio::net::TcpListener::bind(addr).await.expect("cannot bind port");
    eprintln!(
        "wurfel-server: http://localhost:{port}  (world: {generator}, seed {seed}; serving {static_dir}{})",
        if lag.is_zero() { String::new() } else { format!("; simulating {} ms of lag", lag.as_millis()) }
    );
    let on_shutdown = shared.clone();
    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            shutdown_signal().await;
            eprintln!("wurfel-server: shutting down, saving the world");
            save_world(&on_shutdown);
            // The world is safe: tell the players and close their sockets, so they reconnect to the
            // next server instead of seeing a dead connection.
            let _ = on_shutdown.closing.send(true);
        })
        .await
        .expect("server error");
    // axum does not wait for upgraded (WebSocket) connections: give them time to send their goodbye
    // before the process exits. A client that does not answer must not keep the old server alive.
    let deadline = Instant::now() + SHUTDOWN_GRACE;
    while shared.open_sockets.load(Ordering::Relaxed) > 0 && Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// Counts a WebSocket connection while it exists.
struct OpenSocket(Arc<AtomicUsize>);

impl OpenSocket {
    fn new(counter: &Arc<AtomicUsize>) -> Self {
        counter.fetch_add(1, Ordering::Relaxed);
        OpenSocket(counter.clone())
    }
}

impl Drop for OpenSocket {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::Relaxed);
    }
}

/// Resolves on Ctrl-C, and on SIGTERM (what `kill`, systemd and docker send) where there is one.
async fn shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        match signal(SignalKind::terminate()) {
            Ok(mut term) => {
                tokio::select! {
                    _ = tokio::signal::ctrl_c() => {}
                    _ = term.recv() => {}
                }
                return;
            }
            Err(e) => eprintln!("wurfel-server: cannot listen for SIGTERM: {e}"),
        }
    }
    let _ = tokio::signal::ctrl_c().await;
}

/// Where the Java engine keeps its maps (`WorkingDirectory.getMapsFolder`): a folder named after the
/// application in the user's data folder, plus `maps`.
fn java_maps_folder(application: &str, os: &str, home: &str, appdata: Option<&str>) -> std::path::PathBuf {
    use std::path::PathBuf;
    match (os, appdata) {
        ("macos", _) => PathBuf::from(home).join("Library/Application Support").join(application).join("maps"),
        ("windows", Some(appdata)) => PathBuf::from(appdata).join(application).join("maps"),
        _ => PathBuf::from(home).join(format!(".{application}")).join("maps"),
    }
}

/// With no `--maps-dir`, use the maps of an existing Caveland or Wurfel Engine installation if
/// there is one (so their saves show up unchanged), and a `maps` folder next to the server otherwise.
fn default_maps_dir() -> String {
    let home = std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")).unwrap_or_else(|_| ".".to_string());
    let appdata = std::env::var("APPDATA").ok();
    for application in ["Caveland", "Wurfel Engine"] {
        let folder = java_maps_folder(application, std::env::consts::OS, &home, appdata.as_deref());
        let has_maps = std::fs::read_dir(&folder)
            .map(|entries| entries.flatten().any(|e| e.path().is_dir()))
            .unwrap_or(false);
        if has_maps {
            eprintln!("wurfel-server: using the existing {application} maps in {}", folder.display());
            return folder.to_string_lossy().into_owned();
        }
    }
    "maps".to_string()
}

/// The first map of the store, in its newest save (or a new one if it has none).
fn initial_world(store: &MapStore) -> Result<Game, String> {
    // A restart (an update) comes back with the world that was running, so reconnecting players land in it.
    if let Some((map, slot)) = store.last_active() {
        if let Ok(opened) = store.open_world(&map, slot) {
            return Ok(Game::from_opened(opened));
        }
    }
    let map =store.list().map_err(|e| e.to_string())?.into_iter().next().ok_or("the maps folder has no maps")?;
    let slot = match map.saves.last() {
        Some(save) => save.slot,
        None => store.new_save_slot(&map.id).map_err(|e| e.to_string())?,
    };
    Ok(Game::from_opened(store.open_world(&map.id, slot).map_err(|e| e.to_string())?))
}

fn save_world(shared: &Shared) {
    match shared.game.lock().unwrap().save() {
        Ok(0) => {}
        Ok(n) => eprintln!("wurfel-server: saved {n} chunk(s)"),
        Err(e) => eprintln!("wurfel-server: saving failed: {e}"),
    }
}

fn map_summary(map: &MapInfo) -> MapSummary {
    MapSummary {
        id: map.id.clone(),
        name: map.name.clone(),
        description: map.description.clone(),
        generator: map.generator.clone(),
        seed: map.seed,
        gamemode: map.gamemode.clone(),
        saves: map.saves.iter().map(|s| SaveSummary { slot: s.slot, modified: s.modified.clone() }).collect(),
    }
}

fn lobby_message(shared: &Shared) -> Arc<str> {
    let world = shared.game.lock().unwrap().info();
    let generators = wurfel_sim::generator::generators()
        .iter()
        .map(|g| GeneratorSummary {
            id: g.id.to_string(),
            name: g.name.to_string(),
            description: g.description.to_string(),
            uses_seed: g.uses_seed,
        })
        .collect();
    encode(&ServerMsg::Lobby { world, generators, build: wurfel_sim::protocol::build_id() })
}

fn failed(request: &str, message: impl Into<String>) -> Arc<str> {
    encode(&ServerMsg::Failed { request: request.to_string(), message: message.into() })
}

/// Handle a lobby request; the answer goes back to the asker (and `WorldChanged` to every lobby).
fn lobby_request(shared: &Shared, msg: ClientMsg) -> Option<Arc<str>> {
    match msg {
        ClientMsg::GetWorld => Some(lobby_message(shared)),
        ClientMsg::ListMaps => Some(match shared.maps.list() {
            Ok(maps) => encode(&ServerMsg::Maps { maps: maps.iter().map(map_summary).collect() }),
            Err(e) => failed("ListMaps", format!("cannot read the maps: {e}")),
        }),
        ClientMsg::CreateMap { id, name, description, generator, seed, gamemode } => {
            Some(match shared.maps.create_map(&MapCreate { id, name, description, generator, seed, gamemode }) {
                Ok(map) => encode(&ServerMsg::MapCreated { map: map_summary(&map) }),
                Err(e) => failed("CreateMap", e.to_string()),
            })
        }
        ClientMsg::LoadMap { map, slot } => {
            let mut game = shared.game.lock().unwrap();
            if game.player_count() > 0 {
                return Some(failed("LoadMap", "Someone is playing on this server. You can load another save when it is empty."));
            }
            let opened = match slot {
                SlotChoice::Existing(slot) => shared.maps.open_world(&map, slot),
                SlotChoice::New => shared.maps.new_save_slot(&map).and_then(|slot| shared.maps.open_world(&map, slot)),
            };
            match opened {
                Ok(opened) => {
                    if let Err(e) = game.save() {
                        eprintln!("wurfel-server: saving the old world failed: {e}");
                    }
                    *game = Game::from_opened(opened);
                    shared.maps.remember_active(&game.spec().map_id, game.spec().slot);
                    let changed = encode(&ServerMsg::WorldChanged { world: game.info() });
                    eprintln!("wurfel-server: loaded '{}' save {}", game.spec().map, game.spec().slot);
                    let _ = shared.lobby_tx.send(changed.clone());
                    None // the asker is a lobby connection too and gets WorldChanged
                }
                Err(e) => Some(failed("LoadMap", e.to_string())),
            }
        }
        _ => None,
    }
}

fn encode(msg: &ServerMsg) -> Arc<str> {
    serde_json::to_string(msg).expect("server messages always serialise").into()
}

// ------------------------------------------------------------------------------------------ HTTP

// ------------------------------------------------------------------------------------ simulation

/// Time spent simulating, summed over the window between two statistics messages.
#[derive(Default)]
struct TickWindow {
    sum_ms: f32,
    max_ms: f32,
    ticks: u32,
}

async fn tick_loop(shared: Shared) {
    let mut interval = tokio::time::interval(TICK);
    // If the server falls behind it slows down instead of rushing to catch up: the physics step is
    // fixed, so a burst of catch-up ticks would only make the lag worse.
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut window = TickWindow::default();
    let mut last_save = Instant::now();
    loop {
        interval.tick().await;
        if last_save.elapsed() >= AUTOSAVE_EVERY {
            last_save = Instant::now();
            save_world(&shared);
        }
        let started = Instant::now();
        let (snapshot, outbox, tick, counts) = {
            let mut game = shared.game.lock().unwrap();
            if game.player_count() == 0 {
                continue; // nobody is watching: do not simulate
            }
            game.tick();
            let tick = game.tick_count();
            let snapshot = (tick % SNAPSHOT_EVERY == 0).then(|| game.snapshot());
            (snapshot, game.drain_outbox(), tick, (game.player_count(), game.entity_count(), game.loaded_chunks()))
        };
        // What a game mode has to say (block changes, things, rule news) goes before the snapshot.
        for msg in outbox {
            let _ = shared.tx.send(encode(&msg));
        }
        if let Some(snapshot) = snapshot {
            let _ = shared.tx.send(encode(&snapshot)); // no receivers is fine
        }

        let spent = started.elapsed().as_secs_f32() * 1000.0;
        window.sum_ms += spent;
        window.max_ms = window.max_ms.max(spent);
        window.ticks += 1;

        if tick % STATS_EVERY == 0 {
            let stats = ServerStats {
                players: counts.0 as u32,
                entities: counts.1 as u32,
                loaded_chunks: counts.2 as u32,
                tick_ms_avg: window.sum_ms / window.ticks.max(1) as f32,
                tick_ms_max: window.max_ms,
                bytes_out: shared.net.bytes_out.load(Ordering::Relaxed),
                bytes_in: shared.net.bytes_in.load(Ordering::Relaxed),
                uptime_s: shared.started.elapsed().as_secs() as u32,
            };
            window = TickWindow::default();
            let _ = shared.tx.send(encode(&ServerMsg::Stats(stats)));
            let list = shared.pings.lock().unwrap().list();
            if !list.is_empty() {
                let _ = shared.tx.send(encode(&ServerMsg::Pings { list }));
            }
        }
    }
}

/// Give the game rules the current friendships (the turrets spare their owner's friends).
fn sync_friends(shared: &Shared) {
    let pairs = shared.friends.lock().unwrap().pairs();
    shared.game.lock().unwrap().set_friends(&pairs);
}

/// Tell everybody the friends and invites of each of `players` (each client acts on its own).
fn broadcast_friends(shared: &Shared, players: &[u32]) {
    let friends = shared.friends.lock().unwrap();
    for &player in players {
        let view = friends.view(player);
        let _ = shared.tx.send(encode(&ServerMsg::Friends {
            player,
            friends: view.friends,
            sent: view.sent,
            received: view.received,
        }));
    }
}

// ------------------------------------------------------------------------------------ WebSocket

async fn ws_handler(ws: WebSocketUpgrade, State(shared): State<Shared>) -> impl IntoResponse {
    ws.max_message_size(MAX_MESSAGE_BYTES).on_upgrade(move |socket| client(socket, shared))
}

/// What goes out on a connection: JSON text, or a binary chunk.
enum Payload {
    Text(Arc<str>),
    Binary(Vec<u8>),
    /// A normal close frame; the writer stops after sending it.
    Close,
}

/// A message waiting to be written, and when it may be written (to simulate lag).
type Outgoing = (Instant, Payload);

/// Writes queued messages to the socket in order, each no earlier than its deadline.
async fn writer(mut sink: futures_util::stream::SplitSink<WebSocket, Message>, mut queue: mpsc::Receiver<Outgoing>, net: Arc<NetCounters>) {
    while let Some((due, payload)) = queue.recv().await {
        tokio::time::sleep_until(tokio::time::Instant::from_std(due)).await;
        let (size, message) = match payload {
            Payload::Text(text) => (text.len(), Message::Text(text.to_string().into())),
            Payload::Binary(bytes) => (bytes.len(), Message::Binary(bytes.into())),
            Payload::Close => {
                let _ = sink.send(Message::Close(None)).await;
                break;
            }
        };
        net.bytes_out.fetch_add(size as u64, Ordering::Relaxed);
        if sink.send(message).await.is_err() {
            break;
        }
    }
}

async fn client(socket: WebSocket, shared: Shared) {
    let _open = OpenSocket::new(&shared.open_sockets);
    let (sink, mut stream) = socket.split();
    let (queue, queue_rx) = mpsc::channel::<Outgoing>(OUTGOING_QUEUE);
    let writer_task = tokio::spawn(writer(sink, queue_rx, shared.net.clone()));
    let send = |payload: Payload| queue.try_send((Instant::now() + shared.lag, payload)).is_ok();

    // Every connection starts in the lobby: it learns what is running and can look around.
    let mut lobby_updates = shared.lobby_tx.subscribe();
    let mut alive = send(Payload::Text(lobby_message(&shared)));

    // Set once the client joins. The world broadcast is only subscribed to then, so a connection
    // that stays in the lobby does not pile up snapshots it never reads.
    let mut player: Option<(u32, broadcast::Receiver<Arc<str>>)> = None;
    // The chunks around the player are streamed, nearest first, a few at a time.
    let mut interest = Interest::new(CHUNK_RADIUS, CHUNKS_PER_UPDATE);
    let mut chunk_timer = tokio::time::interval(CHUNK_UPDATE_EVERY);
    chunk_timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut closing = shared.closing.subscribe();
    let mut said_goodbye = false;

    while alive {
        tokio::select! {
            // The server is shutting down (the world is already saved): say so, then close properly.
            _ = closing.changed() => {
                if player.is_some() {
                    send(Payload::Text(encode(&ServerMsg::ServerRestarting)));
                }
                said_goodbye = send(Payload::Close);
                break;
            },
            incoming = stream.next() => match incoming {
                Some(Ok(Message::Text(text))) => {
                    shared.net.bytes_in.fetch_add(text.len() as u64, Ordering::Relaxed);
                    // Malformed messages are dropped, never fatal.
                    let Ok(msg) = serde_json::from_str::<ClientMsg>(&text) else { continue };
                    match msg {
                        ClientMsg::Ping { client_time, rtt_ms } => {
                            if let (Some((id, _)), Some(rtt)) = (&player, rtt_ms) {
                                shared.pings.lock().unwrap().set(*id, rtt);
                            }
                            let tick = shared.game.lock().unwrap().tick_count();
                            alive = send(Payload::Text(encode(&ServerMsg::Pong { client_time, tick })));
                        }
                        ClientMsg::Join { name, color } if player.is_none() => {
                            // Subscribe and register under one lock so no broadcast can slip
                            // between the welcome and the first update.
                            let (id, welcome, updates) = {
                                let mut game = shared.game.lock().unwrap();
                                let updates = shared.tx.subscribe();
                                let id = game.add_player_as(&name, color);
                                (id, encode(&game.welcome(id)), updates)
                            };
                            let info = shared.game.lock().unwrap().player_info(id);
                            if let Some(player) = info {
                                eprintln!("player {id} ({}) joined", player.name);
                                let _ = shared.tx.send(encode(&ServerMsg::PlayerJoined { player }));
                            }
                            player = Some((id, updates));
                            alive = send(Payload::Text(welcome));
                        }
                        ClientMsg::Join { .. } => {}
                        ClientMsg::Heart { to, on } => {
                            if let Some((id, _)) = &player {
                                let known = shared.game.lock().unwrap().player_info(to).is_some();
                                if known && shared.friends.lock().unwrap().heart(*id, to, on) {
                                    broadcast_friends(&shared, &[*id, to]);
                                    sync_friends(&shared);
                                }
                            }
                        }
                        ClientMsg::Input { .. } | ClientMsg::Editor { .. } | ClientMsg::SetBlock { .. } | ClientMsg::Action { .. } | ClientMsg::Command { .. } => {
                            if let Some((id, _)) = &player {
                                let broadcast = shared.game.lock().unwrap().handle(*id, msg);
                                if let Some(msg) = broadcast {
                                    let _ = shared.tx.send(encode(&msg));
                                }
                            }
                        }
                        lobby_message_kind => {
                            // Maps and worlds can only be changed from the lobby.
                            let request = format!("{:?}", lobby_message_kind).split([' ', '{']).next().unwrap_or("").to_string();
                            if player.is_some() {
                                alive = send(Payload::Text(failed(&request, "Leave the game first.")));
                            } else if let Some(answer) = lobby_request(&shared, lobby_message_kind) {
                                alive = send(Payload::Text(answer));
                            }
                        }
                    }
                }
                Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                Some(Ok(_)) => {} // pings are answered by axum; binary is not part of the protocol
            },
            lobby = lobby_updates.recv(), if player.is_none() => match lobby {
                Ok(text) => alive = send(Payload::Text(text)),
                Err(broadcast::error::RecvError::Lagged(_)) => {} // only the newest lobby state matters
                Err(broadcast::error::RecvError::Closed) => break,
            },
            outgoing = async { player.as_mut().unwrap().1.recv().await }, if player.is_some() => match outgoing {
                Ok(text) => alive = send(Payload::Text(text)),
                // A slow client misses some snapshots; the next one supersedes them. A missed
                // BlockSet is a real loss though, so drop the connection and let it rejoin.
                Err(broadcast::error::RecvError::Lagged(_)) | Err(broadcast::error::RecvError::Closed) => break,
            },
            _ = chunk_timer.tick(), if player.is_some() => {
                let id = player.as_ref().map(|(id, _)| *id).unwrap_or_default();
                let (chunks, unloads) = {
                    let mut game = shared.game.lock().unwrap();
                    let Some(center) = game.player_chunk(id) else { break };
                    let changes = interest.update(center);
                    let chunks: Vec<Vec<u8>> = changes.send.iter().map(|&(cx, cy)| game.chunk_message(cx, cy)).collect();
                    (chunks, changes.unload)
                };
                for bytes in chunks {
                    alive &= send(Payload::Binary(bytes));
                }
                for (cx, cy) in unloads {
                    alive &= send(Payload::Text(encode(&ServerMsg::ChunkUnload { cx, cy })));
                }
            },
        }
    }

    drop(queue);
    if said_goodbye {
        // Let the writer flush the notice and the close frame (it stops by itself after the close).
        let _ = tokio::time::timeout(SHUTDOWN_GRACE, writer_task).await;
    } else {
        writer_task.abort();
    }
    if let Some((id, _)) = player {
        shared.game.lock().unwrap().remove_player(id);
        shared.pings.lock().unwrap().remove(id);
        let affected = shared.friends.lock().unwrap().remove_player(id);
        broadcast_friends(&shared, &affected);
        sync_friends(&shared);
        let _ = shared.tx.send(encode(&ServerMsg::PlayerLeft { id }));
        eprintln!("player {id} left");
    }
}

#[cfg(test)]
mod tests {
    use super::java_maps_folder;
    use std::path::PathBuf;

    #[test]
    fn java_maps_folders_match_the_java_working_directory_rules() {
        assert_eq!(
            java_maps_folder("Caveland", "macos", "/Users/me", None),
            PathBuf::from("/Users/me/Library/Application Support/Caveland/maps")
        );
        assert_eq!(java_maps_folder("Wurfel Engine", "linux", "/home/me", None), PathBuf::from("/home/me/.Wurfel Engine/maps"));
        assert_eq!(
            java_maps_folder("Caveland", "windows", "C:/Users/me", Some("C:/Users/me/AppData/Roaming")),
            PathBuf::from("C:/Users/me/AppData/Roaming/Caveland/maps")
        );
        assert_eq!(java_maps_folder("Caveland", "windows", "C:/Users/me", None), PathBuf::from("C:/Users/me/.Caveland/maps"));
    }
}
