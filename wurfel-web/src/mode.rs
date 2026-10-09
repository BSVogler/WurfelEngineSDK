//! Game modes on the client: what a mode adds to the engine's client (`web.rs`), which knows only
//! the [`ClientMode`] trait. A mode predicts the local player with its own rules, turns its keys
//! into actions for the server, reads its news (`ServerMsg::Rules`) and draws what is its own. What
//! its news mean for the engine's parts (sounds, particles, the camera, the page's HUD) it hands
//! back as [`Effect`]s, which the engine applies.
//!
//! The modes the client knows are listed in [`create`], the one place that names them.

// Only the browser build (`web.rs`) joins worlds; natively the modes are reached by their tests.
#![cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]

use glam::Vec3;
use serde_json::Value;
use wurfel_sim::entity::{Entities, EntityId, Event};
use wurfel_sim::light::PointLight;
use wurfel_sim::player::PlayerInput;
use wurfel_sim::protocol::ThingState;
use wurfel_sim::World;

use crate::mesh::Vertex;
use crate::sprites::Sprites;

/// What a mode's news asks of the engine's client.
#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    /// Call a method of the page's HUD (`wurfelHud` in `hud.js`), with a string or no argument.
    Hud { method: &'static str, arg: Option<String> },
    /// A sound once, at a place.
    Sound { name: String, pos: Vec3 },
    /// A sound that goes on until [`Effect::StopLoop`] (one per name); again moves it.
    Loop { name: String, pos: Vec3 },
    StopLoop { name: String },
    /// Put our player here at this speed: the server moved it, there is no error to fade out.
    PlaceLocalPlayer { pos: Vec3, vel: Vec3 },
    /// A fire that burns on and lights its surroundings.
    Fire { pos: Vec3 },
    /// A puff of particles of a colour.
    Burst { pos: Vec3, color: [f32; 3] },
    /// A picture that shows for a moment (the sprite of `kind`, see `sprites::entity_art`).
    Flash { kind: &'static str, pos: Vec3, seconds: f32 },
    /// Dirt knocked off a block that did not give way.
    DirtKick { pos: Vec3 },
    /// A turret shot: the muzzle flash and the trail.
    Shot { from: Vec3, to: Vec3 },
    /// A robot fell apart.
    RobotBroke { pos: Vec3 },
    /// A block was hit and still stands, with the health it has left.
    BlockDamaged { cell: (i32, i32, i32), health: u8 },
    /// An explosion: particles, and a shake that is stronger the nearer we are.
    Blast { pos: Vec3, radius: i32 },
    /// Shake the camera.
    Shake { amplitude: f32, millis: f32 },
    /// A player did an animated move (`ok`: the rules accepted it).
    Announced { player: u32, name: String, ok: bool },
    /// We died and are back at the start: say so.
    Respawned { message: String },
}

pub trait ClientMode {
    /// Our own player, simulated locally with the mode's rules (prediction).
    fn spawn_local_player(&mut self, entities: &mut Entities, pos: Vec3) -> EntityId;

    /// News of the mode (`ServerMsg::Rules`). `me` is our player id, `ours` where our player is.
    fn on_rules(&mut self, kind: &str, data: &Value, me: u32, ours: Vec3) -> Vec<Effect>;

    /// The server carries our player (a vehicle): nothing of ours is predicted meanwhile.
    fn riding(&self) -> bool;

    /// Players that are not drawn.
    fn is_hidden(&self, player: u32) -> bool;

    /// Apply the held keys to our player for one step (instead of the engine's walking).
    fn apply_input(&mut self, entities: &mut Entities, world: &World, id: EntityId, input: PlayerInput);

    /// One fixed step of the rules for our player (instead of the engine's entity update); returns
    /// the engine events for sounds and particles.
    fn tick(&mut self, entities: &mut Entities, world: &mut World, dt: f32) -> Vec<Event>;

    /// Where the server would have our player after `plan`, starting from what it reported, without
    /// touching the live player. Position and velocity.
    fn replay(&mut self, world: &mut World, pos: Vec3, vel: Vec3, plan: &[(PlayerInput, u32)]) -> Option<(Vec3, Vec3)>;

    /// The action a key sends to the server (`pressed` is false when it comes up).
    fn key_action(&self, key: &str, pressed: bool) -> Option<(&'static str, i32)>;

    /// The action a mouse button sends.
    fn mouse_action(&self, button: i16, pressed: bool) -> Option<(&'static str, i32)>;

    /// The players whose exhaust burns, as far as the server said (for the ones we do not predict).
    fn exhausting(&self) -> Vec<u32> {
        Vec::new()
    }

    /// The players with a lit explosive in their pack: they throw sparks.
    fn carriers_burning(&self) -> Vec<u32> {
        Vec::new()
    }

    /// The cells of machines that burn (ovens): they smoke.
    fn burning_cells(&self) -> Vec<(i32, i32, i32)> {
        Vec::new()
    }

    /// Our player's exhaust (a jetpack) burns.
    fn exhaust(&self, _id: EntityId) -> bool {
        false
    }

    /// Lights of the mode besides the sun and the particles.
    fn lights(&self, _world: &World, _things: &[ThingState]) -> Vec<PointLight> {
        Vec::new()
    }

    /// Draw a thing that has no sprite.
    fn push_thing(&self, _out: &mut Vec<Vertex>, _thing: &ThingState) {}

    /// Draw what the mode adds on top of the world (aiming arcs, signs...). `local` is where our
    /// player is, `sprites` the atlas once loaded.
    fn push_overlays(&mut self, _out: &mut Vec<Vertex>, _world: &World, _local: Option<Vec3>, _sprites: Option<&Sprites>) {}

    /// The mode's console commands, which the server answers: `(name, help)`.
    fn commands(&self) -> &'static [(&'static str, &'static str)] {
        &[]
    }
}

/// Register what the modes bring to the engine (their map generators, for the menu's previews).
/// Call once at startup; calling again is harmless.
pub fn install() {
    static REGISTER: std::sync::Once = std::sync::Once::new();
    REGISTER.call_once(caveland_sim::register);
}

/// Start the mode called `name` on `world` (it may bring its own block rules); `None` for the plain
/// engine and modes this client does not know.
pub fn create(name: &str, world: &mut World) -> Option<Box<dyn ClientMode>> {
    match name {
        crate::caveland_client::MODE => Some(Box::new(crate::caveland_client::CavelandClient::start(world))),
        _ => None,
    }
}
