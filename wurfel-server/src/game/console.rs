//! The engine's console commands on the server (`wurfel_sim::console`): the ones that change the
//! shared world (`killall`, `fillwithair`, `save`, `teleport`...) arrive here as forwarded lines and
//! the answer goes back to the player who typed them as a `Rules` message of kind `console`.
//!
//! Caveland's own commands (`give`, `tpplayer`...) go to the game mode instead, see
//! [`Game::command`].

use std::sync::OnceLock;

use glam::Vec3;
use wurfel_sim::console::{CVarTarget, Console, ConsoleHost, OutputLine, Side};
use wurfel_sim::cvar::CVarSystem;
use wurfel_sim::entity::physics::ground_height;
use wurfel_sim::entity::EntityId;
use wurfel_sim::grid::to_iso;
use wurfel_sim::protocol::{Edit, ServerMsg};
use wurfel_sim::{Block, CHUNK_SIZE_X, CHUNK_SIZE_Y, CHUNK_SIZE_Z};

use super::Game;

/// Wrong `auth` tokens a connection may try before `auth` stops answering it.
const MAX_AUTH_FAILURES: u8 = 5;

/// The token `auth <token>` has to match, set once at startup ([`init_admin_token`]).
static ADMIN_TOKEN: OnceLock<String> = OnceLock::new();

/// The admin token: `WURFEL_ADMIN_TOKEN` if set (and not empty), otherwise a random one. Returns
/// the token and whether it was generated, so `main` can print it for the operator.
pub fn init_admin_token() -> (&'static str, bool) {
    let from_env = std::env::var("WURFEL_ADMIN_TOKEN").ok().filter(|t| !t.trim().is_empty());
    let generated = from_env.is_none();
    let token = ADMIN_TOKEN.get_or_init(|| from_env.map(|t| t.trim().to_string()).unwrap_or_else(crate::users::random_secret));
    (token, generated)
}

/// Compare without stopping at the first wrong byte, so the time taken says nothing about the token.
fn token_matches(given: &str, token: &str) -> bool {
    given.len() == token.len() && given.bytes().zip(token.bytes()).fold(0u8, |acc, (a, b)| acc | (a ^ b)) == 0
}

/// The game seen by one player's console line.
struct ServerHost<'a> {
    game: &'a mut Game,
    player: EntityId,
}

impl ConsoleHost for ServerHost<'_> {
    fn cvars(&mut self, _target: &CVarTarget) -> Option<&mut CVarSystem> {
        // The server's simulation does not read cvars (yet): there is nothing to show or set.
        None
    }

    fn is_admin(&self) -> bool {
        self.game.is_admin(self.player)
    }

    fn authenticate(&mut self, given: &str) -> bool {
        let failures = self.game.auth_failures.entry(self.player).or_default();
        if *failures >= MAX_AUTH_FAILURES {
            return false;
        }
        let ok = ADMIN_TOKEN.get().is_some_and(|token| token_matches(given, token));
        if ok {
            self.game.auth_failures.remove(&self.player);
            self.game.admins.insert(self.player);
            self.game.admin_grants.push(self.player);
            let name = self.game.roster.get(&self.player).map_or("?", |p| p.name.as_str());
            eprintln!("wurfel-server: player {} ({name}) logged in as administrator", self.player);
        } else {
            *failures += 1;
            eprintln!("wurfel-server: player {} gave a wrong admin token ({failures}/{MAX_AUTH_FAILURES})", self.player);
        }
        ok
    }

    fn spawn_benchmark_ball(&mut self) -> Result<(), String> {
        self.game.spawn_benchmark_ball(self.player)
    }

    fn kill_all_entities(&mut self) -> Result<usize, String> {
        if self.game.mode.is_some() {
            // The game mode owns its robots, carts and items: removing them behind its back would
            // leave it with dangling ids.
            return Err("killall is only available in the plain engine".into());
        }
        let balls = std::mem::take(&mut self.game.balls);
        for &(id, _) in &balls {
            self.game.entities.remove(id);
        }
        self.game.benchmark = None;
        let things = std::mem::take(&mut self.game.things).len();
        if things > 0 {
            self.game.things_unsaved = true;
        }
        self.game.things_dirty = true;
        Ok(balls.len() + things)
    }

    fn fill_chunk_with_air(&mut self, chunk_x: i32, chunk_y: i32) -> Result<(), String> {
        if self.game.mode.is_some() {
            return Err("fillwithair is only available in the plain engine".into());
        }
        let game = &mut *self.game;
        game.world.load_chunk(chunk_x, chunk_y);
        let (left, top) = (chunk_x * CHUNK_SIZE_X, chunk_y * CHUNK_SIZE_Y);
        let mut edits = Vec::new();
        for z in 0..CHUNK_SIZE_Z {
            for y in top..top + CHUNK_SIZE_Y {
                for x in left..left + CHUNK_SIZE_X {
                    if !game.world.get(x, y, z).is_air() {
                        game.world.set(x, y, z, Block::AIR);
                        edits.push(Edit { x, y, z, block: 0 });
                    }
                }
            }
        }
        // Sent with the animated blocks, which go out in batches already.
        game.animation_edits.extend(edits);
        Ok(())
    }

    fn print_map(&mut self, x: i32, y: i32, z: i32, width: i32, height: i32) -> Result<Vec<String>, String> {
        if !(0..CHUNK_SIZE_Z).contains(&z) {
            return Err(format!("z must be 0..{}", CHUNK_SIZE_Z - 1));
        }
        let world = &mut self.game.world;
        let mut rows = vec![format!("x {x}..{} y {y}..{} at z {z}: . air, otherwise the block id in base 36", x + width - 1, y + height - 1)];
        for row in y..y + height {
            let mut line = String::with_capacity(width as usize);
            for column in x..x + width {
                let (cx, cy) = wurfel_sim::grid::chunk_of(column, row);
                world.load_chunk(cx, cy);
                let block = world.get(column, row, z);
                line.push(if block.is_air() { '.' } else { char::from_digit(u32::from(block.id()) % 36, 36).unwrap_or('?') });
            }
            rows.push(line);
        }
        Ok(rows)
    }

    fn save(&mut self) -> Result<String, String> {
        self.game.save().map(|chunks| format!("saved {chunks} chunks")).map_err(|e| e.to_string())
    }

    fn load_map(&mut self, _name: &str) -> Result<String, String> {
        Err("maps are changed in the lobby: leave the game and pick one in the menu".into())
    }

    fn teleport_player(&mut self, x: i32, y: i32) -> Result<(), String> {
        let game = &mut *self.game;
        let (cx, cy) = wurfel_sim::grid::chunk_of(x, y);
        game.world.load_area((cx, cy), 1);
        let z = ground_height(&game.world, x, y);
        let (gx, gy) = to_iso(x, y);
        let entity = game.entities.get_mut(self.player).ok_or("you are not in the world")?;
        entity.position = Vec3::new(gx, gy, z);
        if let Some(body) = entity.body.as_mut() {
            body.movement = Vec3::ZERO;
        }
        // The client's prediction is too far off now and jumps to the server's position.
        Ok(())
    }
}

impl Game {
    /// May `player` change the world and use the cheats? Whoever logged in with `auth <token>`, and
    /// the host (the player who has been here longest), which keeps a game among friends simple.
    pub(super) fn is_admin(&self, player: EntityId) -> bool {
        self.admins.contains(&player) || self.inputs.keys().min() == Some(&player)
    }

    /// The player belongs to a user who is an admin already (see `users.rs`).
    pub fn set_admin(&mut self, player: EntityId) {
        self.admins.insert(player);
    }

    /// Did `player` just log in with the admin token? Then their user should keep the right.
    pub fn take_admin_grant(&mut self, player: EntityId) -> bool {
        let before = self.admin_grants.len();
        self.admin_grants.retain(|&p| p != player);
        self.admin_grants.len() != before
    }

    /// Run an engine console line for `player` (`path` is their console's `cd` path) and queue the
    /// answer for them.
    pub(super) fn engine_command(&mut self, player: EntityId, line: &str, path: &str) {
        let lines = Console::new(Side::Server).execute_forwarded(line, path, &mut ServerHost { game: self, player });
        self.console_reply(player, lines);
    }

    /// Keep a console answer for the player who asked; the connection sends it to them alone
    /// ([`Game::take_replies`]).
    pub(super) fn console_reply(&mut self, player: EntityId, lines: Vec<OutputLine>) {
        self.replies.push((player, ServerMsg::ConsoleReply { lines }));
    }

    /// Messages for `player` only (console answers), to be sent on their connection.
    pub fn take_replies(&mut self, player: EntityId) -> Vec<ServerMsg> {
        let (mine, others) = std::mem::take(&mut self.replies).into_iter().partition(|(p, _)| *p == player);
        self.replies = others;
        mine.into_iter().map(|(_, m)| m).collect()
    }

}
