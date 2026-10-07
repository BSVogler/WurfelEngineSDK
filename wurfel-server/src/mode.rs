//! Game modes: rules a world is played by on top of the plain engine. The engine's [`Game`] runs
//! players, physics, the editor and the console; a mode takes over the players, adds its own
//! entities, actions and commands, and keeps its own state next to the save.
//!
//! [`Game`] only knows the [`GameMode`] trait. The modes this server offers are listed in
//! [`installed`], the one place that names them.
//!
//! [`Game`]: crate::game::Game

use std::io;
use std::path::Path;
use std::sync::Once;

use glam::Vec3;
use wurfel_sim::entity::{Entities, EntityId};
use wurfel_sim::player::PlayerInput;
use wurfel_sim::protocol::ServerMsg;
use wurfel_sim::World;

use crate::game::WorldSpec;

/// The plain engine: no mode at all.
pub const ENGINE: &str = "engine";

pub trait GameMode: Send {
    /// The mode's name, as maps and clients know it.
    fn name(&self) -> &'static str;

    /// Add a player at `spot` (feet) and return its entity id.
    fn spawn_player(&mut self, entities: &mut Entities, world: &World, spot: Vec3) -> EntityId;

    /// The player left; the engine has already removed its entity.
    fn remove_player(&mut self, id: EntityId);

    /// Apply a player's held keys for one tick (instead of the engine's walking).
    fn controls(&mut self, entities: &mut Entities, world: &World, id: EntityId, input: PlayerInput);

    /// A one-off action of a player (`ClientMsg::Action`).
    fn act(&mut self, entities: &mut Entities, world: &mut World, id: EntityId, name: &str, arg: i32);

    /// One fixed step of the rules (instead of the engine's entity update).
    fn tick(&mut self, entities: &mut Entities, world: &mut World, tick: u64, dt: f32);

    /// Is `name` one of the mode's console commands (the rest are the engine's)?
    fn has_command(&self, name: &str) -> bool;

    /// Run one of the mode's console lines for `player`. `admin` says whether the player may cheat.
    /// The answer goes to the player's console (`Err` for a refusal or a failure).
    fn command(&mut self, entities: &mut Entities, world: &mut World, player: EntityId, line: &str, admin: bool) -> Result<String, String>;

    /// Write the mode's own state into the save slot's folder.
    fn save(&self, dir: &Path, entities: &Entities) -> io::Result<()>;

    /// Read what [`GameMode::save`] wrote. A missing file is a fresh start, not an error.
    fn load(&mut self, dir: &Path, entities: &Entities) -> Result<(), String>;

    /// Messages for everybody since the last call (things, rule news, block changes).
    fn drain_outbox(&mut self) -> Vec<ServerMsg>;

    /// Who is friends with whom (pairs of player ids).
    fn set_friends(&mut self, _pairs: &[(u32, u32)]) {}
}

/// A mode the server can play.
pub struct ModeInfo {
    pub name: &'static str,
    /// Maps made with this generator are played by this mode unless they say otherwise.
    pub generator: Option<&'static str>,
    /// Start the mode on a freshly made world.
    pub create: fn(&mut World, &WorldSpec) -> Box<dyn GameMode>,
}

/// The modes this server offers, besides the plain engine. The first call also registers what the
/// modes bring to the engine (their map generators).
pub fn installed() -> &'static [ModeInfo] {
    static REGISTER: Once = Once::new();
    REGISTER.call_once(caveland_sim::register);
    &[crate::caveland_mode::MODE]
}

/// Register what the modes bring to the engine. Call once at startup, before maps are read (and in
/// tests that use a mode's generator); calling again is harmless.
pub fn install() {
    installed();
}

/// Every mode name a map may use, the engine first.
pub fn names() -> Vec<&'static str> {
    std::iter::once(ENGINE).chain(installed().iter().map(|m| m.name)).collect()
}

/// The mode a map made with `generator` is played by when it does not name one.
pub fn default_for(generator: &str) -> &'static str {
    installed().iter().find(|m| m.generator == Some(generator)).map_or(ENGINE, |m| m.name)
}

/// Start the mode called `name` on `world`; `None` for the plain engine and unknown names.
pub fn create(name: &str, world: &mut World, spec: &WorldSpec) -> Option<Box<dyn GameMode>> {
    installed().iter().find(|m| m.name == name).map(|m| (m.create)(world, spec))
}
