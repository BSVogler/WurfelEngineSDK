//! The client side of the Caveland game mode: what the keys do, how the things of the mode look,
//! how the local player is predicted with Caveland's rules, and what the HUD shows.
//!
//! The rules themselves are the `caveland-sim` crate, the same code the server runs. Like the
//! server, the client keeps the engine out of Caveland's way: [`CavelandClient`] is a
//! [`ClientMode`], and `mode::create` the only place outside this file that names it.

// Only the browser build joins worlds, so natively the mode is reached by the tests alone.
#![cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]

use std::collections::{HashMap, HashSet};

use caveland_sim::blocks::ids;
use caveland_sim::launcher::{preview_arc, Launcher, LauncherKind, PreviewDot};
use caveland_sim::{Caveland, Controls, Tuning};
use glam::Vec3;
use serde_json::Value;
use wurfel_sim::entity::{Entities, EntityId, Event};
use wurfel_sim::grid::to_iso;
use wurfel_sim::light::PointLight;
use wurfel_sim::player::{PlayerInput, TICK_DT};
use wurfel_sim::protocol::ThingState;
use wurfel_sim::World;

use crate::mesh::{self, Vertex};
use crate::mode::{ClientMode, Effect};
use crate::sprites::Sprites;

/// The name of the mode in `Welcome::gamemode`.
pub const MODE: &str = "caveland";

pub fn controls(input: PlayerInput) -> Controls {
    Controls { up: input.up, down: input.down, left: input.left, right: input.right, jump: input.jump, heading: input.heading }
}

/// The Caveland mode of a session: the ruleset that predicts our player, and what the server last
/// said about the world (power, launchers, who rides or is hidden, what the use button would hit).
pub struct CavelandClient {
    caveland: Caveland,
    /// The server moves us (a cart, the intro ship).
    riding: bool,
    /// Players that are not drawn (inside the intro ship).
    hidden: HashSet<u32>,
    /// Cells of power blocks (torches, turrets, stations) that have power.
    powered: HashSet<(i32, i32, i32)>,
    /// Catapults and cannons as the server last described them.
    launchers: Vec<LauncherInfo>,
    /// The aiming menu of one of them is open: the arc preview is drawn.
    aiming: bool,
    /// The preview dots and what they were computed for.
    preview: Option<(PreviewKey, Vec<PreviewDot>)>,
    /// Where the interaction sign floats: the thing the use button would act on.
    interact_focus: Option<Vec3>,
    /// Players whose jetpack burns, as the server last said.
    jetpacks: HashSet<u32>,
    /// Cells of the ovens that burn.
    ovens: Vec<(i32, i32, i32)>,
    /// Players with a lit explosive in the pack.
    burning: HashSet<u32>,
}

impl CavelandClient {
    /// A fresh session, with the world following Caveland's blocks.
    pub fn start(world: &mut World) -> Self {
        Caveland::install(world);
        CavelandClient {
            caveland: Caveland::new(Tuning::default(), 1),
            riding: false,
            hidden: HashSet::new(),
            powered: HashSet::new(),
            launchers: Vec::new(),
            aiming: false,
            preview: None,
            interact_focus: None,
            jetpacks: HashSet::new(),
            ovens: Vec::new(),
            burning: HashSet::new(),
        }
    }

    /// The happenings of an `events` message as effects. `ours` is where our player is.
    fn effects(happenings: Vec<Happening>, me: u32, ours: Vec3) -> Vec<Effect> {
        let mut out = Vec::new();
        let mut extra = Vec::new();
        for happening in happenings {
            out.push(match happening {
                // Our own jump already made its sound when we pressed the key.
                Happening::Sound { name, pos } if name == "urfJump" && pos.distance(ours) < 2.0 => continue,
                // The cart's rolling goes on until the server says it stops.
                Happening::Sound { name, pos } if name == "wagon" => Effect::Loop { name, pos },
                // A robot hit something: its hit sprite shows for 300 ms (`Robot.performAttack`).
                Happening::Sound { name, pos } if name == "robotHit" => {
                    extra.push(Effect::Flash { kind: "hit_flash", pos, seconds: 0.3 });
                    Effect::Sound { name, pos }
                }
                Happening::Sound { name, pos } => Effect::Sound { name, pos },
                Happening::SoundStopped { name } => Effect::StopLoop { name },
                Happening::Teleported { entity, pos } if entity == me => Effect::PlaceLocalPlayer { pos, vel: Vec3::ZERO },
                // Thrown by a catapult or cannon: only a position and a velocity, then the same
                // movement rules as the server (control by speed, bounces) run the flight.
                Happening::Launched { player, pos, vel } if player == me => Effect::PlaceLocalPlayer { pos, vel },
                // Others are interpolated from the snapshots; a big jump counts as a teleport there.
                Happening::Launched { .. } | Happening::Teleported { .. } => continue,
                // The wreck burns (`ParticleType.FIRE`) and lights its surroundings.
                Happening::ShipCrashed { pos } => Effect::Fire { pos },
                Happening::Dust { pos } => Effect::Burst { pos, color: [0.6, 0.55, 0.5] },
                Happening::HardHit { pos } => Effect::DirtKick { pos },
                Happening::Shot { from, to } => Effect::Shot { from, to },
                Happening::RobotDestroyed { pos } => Effect::RobotBroke { pos },
                Happening::BlockDamaged { cell, health } => Effect::BlockDamaged { cell, health },
                Happening::Explosion { pos, radius } => Effect::Blast { pos, radius },
                Happening::Toast(text) => Effect::Hud { method: "toast", arg: Some(text) },
                Happening::Action { player, name, ok } => Effect::Announced { player, name, ok },
                Happening::Hurt => Effect::Shake { amplitude: 14.0, millis: 220.0 },
                Happening::Died => Effect::Respawned { message: "You died. Back at the start.".into() },
            });
        }
        out.extend(extra);
        out
    }
}

impl ClientMode for CavelandClient {
    fn spawn_local_player(&mut self, entities: &mut Entities, pos: Vec3) -> EntityId {
        self.caveland.spawn_player(entities, 0, pos)
    }

    fn on_rules(&mut self, kind: &str, data: &Value, me: u32, ours: Vec3) -> Vec<Effect> {
        let hud = |method: &'static str, arg: Option<String>| vec![Effect::Hud { method, arg }];
        match kind {
            "state" => {
                let flags = parse_flags(data);
                self.hidden = flags.iter().filter(|(_, f)| f.hidden).map(|(&id, _)| id).collect();
                self.riding = flags.get(&me).is_some_and(|f| f.riding);
                self.burning = flags.iter().filter(|(_, f)| f.burning).map(|(&id, _)| id).collect();
                self.jetpacks = flags.iter().filter(|(_, f)| f.jetpack).map(|(&id, _)| id).collect();
                parse_state(data, me).map_or_else(Vec::new, |state| hud("update", Some(hud_json(&state))))
            }
            // Private news (the broadcast reaches everybody; `to` names who it is for).
            "dialog" if addressed_to(data, me) => {
                self.aiming = data.get("title").and_then(|t| t.as_str()).is_some_and(is_aiming_dialog);
                hud("dialog", Some(data.to_string()))
            }
            "dialog_closed" if addressed_to(data, me) => {
                self.aiming = false;
                hud("closeDialog", None)
            }
            "interact_focus" if addressed_to(data, me) => {
                self.interact_focus = parse_interact_focus(data);
                Vec::new()
            }
            "ovens" => {
                self.ovens = parse_power(data).into_iter().collect();
                self.ovens.sort_unstable();
                Vec::new()
            }
            "launchers" => {
                self.launchers = parse_launchers(data);
                Vec::new()
            }
            "lift_offer" if addressed_to(data, me) => hud("liftOffer", Some(data.to_string())),
            "power" => {
                self.powered = parse_power(data);
                Vec::new()
            }
            "events" => Self::effects(parse_events(data, me), me, ours),
            _ => Vec::new(),
        }
    }

    fn riding(&self) -> bool {
        self.riding
    }

    fn is_hidden(&self, player: u32) -> bool {
        self.hidden.contains(&player)
    }

    /// Caveland's own walking, jump and jetpack rules, the same as the server runs.
    fn apply_input(&mut self, entities: &mut Entities, world: &World, id: EntityId, input: PlayerInput) {
        self.caveland.set_controls(entities, world, id, controls(input));
    }

    /// Only our own player lives here, so the rules have nothing to hit or pick up; the server
    /// decides all of that and tells us.
    fn tick(&mut self, entities: &mut Entities, world: &mut World, dt: f32) -> Vec<Event> {
        self.caveland.tick(entities, world, dt);
        self.caveland.engine_events().to_vec()
    }

    /// Caveland's rules replay on a scratch player, so the live jetpack is not burned twice.
    fn replay(&mut self, world: &mut World, pos: Vec3, vel: Vec3, plan: &[(PlayerInput, u32)]) -> Option<(Vec3, Vec3)> {
        replay(world, pos, vel, plan)
    }

    fn key_action(&self, key: &str, pressed: bool) -> Option<(&'static str, i32)> {
        key_action(key, pressed)
    }

    fn mouse_action(&self, button: i16, pressed: bool) -> Option<(&'static str, i32)> {
        mouse_action(button, pressed)
    }

    fn exhaust(&self, id: EntityId) -> bool {
        self.caveland.player(id).is_some_and(|p| p.jetpack_on)
    }

    fn exhausting(&self) -> Vec<u32> {
        self.jetpacks.iter().copied().collect()
    }

    fn carriers_burning(&self) -> Vec<u32> {
        self.burning.iter().copied().collect()
    }

    fn burning_cells(&self) -> Vec<(i32, i32, i32)> {
        self.ovens.clone()
    }

    fn lights(&self, world: &World, things: &[ThingState]) -> Vec<PointLight> {
        lamps(world, &self.powered, things)
    }

    fn push_thing(&self, out: &mut Vec<Vertex>, thing: &ThingState) {
        push_thing(out, thing);
    }

    fn push_overlays(&mut self, out: &mut Vec<Vertex>, world: &World, local: Option<Vec3>, sprites: Option<&Sprites>) {
        // The dotted arc of the machine being aimed: only the start of the flight.
        if self.aiming {
            match local.and_then(|at| aimed_launcher(&self.launchers, at)).copied() {
                Some(info) => {
                    let key = preview_key(&info);
                    if self.preview.as_ref().is_none_or(|(k, _)| *k != key) {
                        self.preview = Some((key, preview_dots(world, &info)));
                    }
                    if let Some((_, dots)) = &self.preview {
                        push_preview(out, dots);
                    }
                }
                None => self.preview = None,
            }
        }
        if let (Some(sprites), Some(focus)) = (sprites, self.interact_focus) {
            push_interact_sign(out, sprites, focus);
        }
    }

    fn commands(&self) -> &'static [(&'static str, &'static str)] {
        &caveland_sim::commands::COMMANDS
    }
}

/// The action a key does. `pressed` is false for the key coming up. The keys avoid the ones the
/// camera and the console use. `None` if the key is not a Caveland key.
pub fn key_action(key: &str, pressed: bool) -> Option<(&'static str, i32)> {
    match (key, pressed) {
        ("f", true) => Some(("attack", 0)),
        ("f", false) => Some(("release_attack", 0)),
        ("t", true) => Some(("prepare_throw", 0)),
        ("t", false) => Some(("throw", 0)),
        ("e", true) => Some(("use", 0)),
        ("r", true) => Some(("interact", 0)),
        ("x", true) => Some(("drop", 0)),
        ("z", true) => Some(("switch_left", 0)),
        ("v", true) => Some(("switch_right", 0)),
        _ => None,
    }
}

/// The mouse does the same as the attack and throw keys (`MouseKeyboardListener.touchDown/Up`): the
/// left button (0) swings, holding it charges; the right button (2) winds up a throw while held and
/// throws when released, holding it long enough drops the item (the server times that).
pub fn mouse_action(button: i16, pressed: bool) -> Option<(&'static str, i32)> {
    match (button, pressed) {
        (0, true) => Some(("attack", 0)),
        (0, false) => Some(("release_attack", 0)),
        (2, true) => Some(("prepare_throw", 0)),
        (2, false) => Some(("throw", 0)),
        _ => None,
    }
}

/// How a thing is drawn: no sprites yet, so a coloured block of this size.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ThingStyle {
    pub color: [f32; 3],
    /// Half the side of the square it stands on, in blocks.
    pub half: f32,
    pub height: f32,
}

pub fn thing_style(kind: &str) -> ThingStyle {
    let item = |color: [f32; 3]| ThingStyle { color, half: 0.15, height: 0.3 };
    match kind {
        "robot" => ThingStyle { color: [0.85, 0.15, 0.15], half: 0.25, height: 1.1 },
        "friendly_robot" => ThingStyle { color: [0.2, 0.7, 0.75], half: 0.25, height: 1.1 },
        "money" => ThingStyle { color: [0.95, 0.8, 0.15], half: 0.1, height: 0.2 },
        "minecart" => ThingStyle { color: [0.45, 0.45, 0.5], half: 0.35, height: 0.5 },
        "spider_robot" => ThingStyle { color: [0.7, 0.2, 0.2], half: 0.3, height: 0.6 },
        "friendly_spider_robot" => ThingStyle { color: [0.2, 0.6, 0.65], half: 0.3, height: 0.6 },
        "drone" => ThingStyle { color: [0.75, 0.3, 0.3], half: 0.2, height: 0.3 },
        "friendly_drone" => ThingStyle { color: [0.3, 0.7, 0.75], half: 0.2, height: 0.3 },
        "vanya" => ThingStyle { color: [0.9, 0.55, 0.75], half: 0.25, height: 1.2 },
        "shopkeeper" => ThingStyle { color: [0.85, 0.7, 0.3], half: 0.25, height: 1.2 },
        "bird" => ThingStyle { color: [0.4, 0.55, 0.9], half: 0.12, height: 0.25 },
        "flag" | "flag_robots" | "flag_player" | "drop_space_flag" => ThingStyle { color: [0.9, 0.9, 0.9], half: 0.08, height: 1.2 },
        "lift_basket" => ThingStyle { color: [0.5, 0.35, 0.2], half: 0.4, height: 0.5 },
        "spaceship" => ThingStyle { color: [0.6, 0.65, 0.75], half: 0.9, height: 1.4 },
        "Wood" => item([0.55, 0.36, 0.18]),
        "Coal" => item([0.12, 0.12, 0.14]),
        "Torch" => item([1.0, 0.6, 0.1]),
        "Explosives" => item([0.8, 0.1, 0.1]),
        "Gunpowder" => item([0.3, 0.3, 0.25]),
        "Iron" => item([0.78, 0.78, 0.82]),
        "Ironore" => item([0.6, 0.35, 0.25]),
        "Cristall" => item([0.4, 0.9, 0.95]),
        "Sulfur" => item([0.9, 0.9, 0.2]),
        "Stone" => item([0.55, 0.55, 0.55]),
        "Toolkit" => item([0.25, 0.4, 0.85]),
        "Rails" => item([0.3, 0.3, 0.35]),
        "Powercable" => item([0.6, 0.3, 0.8]),
        _ => item([0.9, 0.5, 0.9]), // something new the client does not know yet
    }
}

/// Draw a thing as a small block standing on its position.
pub fn push_thing(out: &mut Vec<Vertex>, thing: &ThingState) {
    let style = thing_style(&thing.kind);
    let [x, y, z] = thing.pos;
    mesh::cuboid(out, style.color, [x - style.half, x + style.half, y - style.half, y + style.half], [z, z + style.height]);
}

/// What the local player's HUD shows (`Rules` `state`, our own entry).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Hud {
    pub health: f32,
    pub jetpack: f32,
    pub items: Vec<String>,
    /// In the server's fixed order; see [`craft_menu`] for the order the player sees.
    pub recipes: Vec<RecipeInfo>,
    /// What the shop takes; the money is the party's, not a player's.
    pub money: u32,
}

/// One recipe of the crafting menu.
#[derive(Debug, Clone, PartialEq)]
pub struct RecipeInfo {
    /// Position in the server's fixed recipe list: what the `craft` action takes.
    pub index: usize,
    pub name: String,
    /// The pack holds every ingredient.
    pub can: bool,
    pub ingredients: Vec<Ingredient>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Ingredient {
    pub name: String,
    /// The pack holds an item of this name at all (the menu dims the ones that are missing).
    pub have: bool,
}

/// The recipes as the crafting menu lists them: the craftable ones first, the rest after, each
/// group in the server's list order. Every recipe keeps its fixed `index`.
pub fn craft_menu(hud: &Hud) -> Vec<&RecipeInfo> {
    let (mut menu, rest): (Vec<_>, Vec<_>) = hud.recipes.iter().partition(|r| r.can);
    menu.extend(rest);
    menu
}

/// Our entry of a `state` message: `{"<id>": {health, jetpack, items, recipes}, ...}`.
pub fn parse_state(data: &Value, my_id: u32) -> Option<Hud> {
    let mine = data.get(my_id.to_string())?;
    let items: Vec<String> = mine.get("items")?.as_array()?.iter().filter_map(|v| v.as_str().map(str::to_string)).collect();
    let recipes = mine
        .get("recipes")?
        .as_array()?
        .iter()
        .enumerate()
        .filter_map(|(index, r)| {
            // `[name, can, [ingredient, ...]]`; the older `[name, can]` has no ingredients.
            let ingredients = r
                .get(2)
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .map(|name| Ingredient { name: name.to_string(), have: items.iter().any(|i| i == name) })
                .collect();
            Some(RecipeInfo { index, name: r.get(0)?.as_str()?.to_string(), can: r.get(1)?.as_bool()?, ingredients })
        })
        .collect();
    Some(Hud {
        health: mine.get("health")?.as_f64()? as f32,
        jetpack: mine.get("jetpack")?.as_f64()? as f32,
        items,
        recipes,
        money: mine.get("money").and_then(Value::as_u64).unwrap_or(0) as u32,
    })
}

/// What the server says about how a player is moved.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PlayerFlags {
    /// Not drawn: inside the spaceship.
    pub hidden: bool,
    /// Carried by a vehicle or ship: the server moves them, the client must not predict.
    pub riding: bool,
    /// The jetpack burns.
    pub jetpack: bool,
    /// A lit explosive in the pack: the player throws sparks.
    pub burning: bool,
}

/// `hidden` and `riding` of every player in a `state` message.
pub fn parse_flags(data: &Value) -> HashMap<u32, PlayerFlags> {
    let mut out = HashMap::new();
    for (id, entry) in data.as_object().into_iter().flatten() {
        let Ok(id) = id.parse::<u32>() else { continue };
        let flag = |key: &str| entry.get(key).and_then(Value::as_bool).unwrap_or(false);
        out.insert(id, PlayerFlags { hidden: flag("hidden"), riding: flag("riding"), jetpack: flag("jetpack_on"), burning: flag("burning") });
    }
    out
}

/// Was a `Rules` message meant for us? (The server has one broadcast; private news names its
/// receiver in `to`.)
pub fn addressed_to(data: &Value, me: u32) -> bool {
    data.get("to").and_then(Value::as_u64) == Some(u64::from(me))
}

/// The sprite value of the interaction sign: the trigger button (Java `Interactable.RT`, entity
/// sprite category `i`, id 23).
const SIGN_VALUE: u32 = 11;
/// How far above the usable thing the sign floats, in blocks (Java: one `GAME_EDGELENGTH`).
const SIGN_LIFT: f32 = 1.0;

/// Where the `interact_focus` message puts the sign, or `None` when it says there is nothing to use.
pub fn parse_interact_focus(data: &Value) -> Option<glam::Vec3> {
    let a = data.get("pos")?.as_array()?;
    Some(glam::Vec3::new(a.first()?.as_f64()? as f32, a.get(1)?.as_f64()? as f32, a.get(2)?.as_f64()? as f32))
}

/// Draw the interaction sign above `focus`, the thing the use button would act on.
pub fn push_interact_sign(out: &mut Vec<crate::mesh::Vertex>, sprites: &crate::sprites::Sprites, focus: glam::Vec3) {
    if let Some(region) = sprites.atlas.region(&format!("i23-{SIGN_VALUE}")) {
        crate::sprites::billboard(out, &sprites.atlas, region, focus + glam::Vec3::Z * SIGN_LIFT, crate::sprites::FOOTPRINT_TIP, false, [1.0; 3]);
    }
}

/// The cells of a `power` message.
pub fn parse_power(data: &Value) -> HashSet<(i32, i32, i32)> {
    let cell = |v: &Value| {
        let a = v.as_array()?;
        Some((a.first()?.as_i64()? as i32, a.get(1)?.as_i64()? as i32, a.get(2)?.as_i64()? as i32))
    };
    data.get("cells").and_then(Value::as_array).into_iter().flatten().filter_map(cell).collect()
}

/// The lights of the world that are not the sun: torches that have power (`PowerTorch` only
/// shines when a station feeds it) and the lamps of the carts.
pub fn lamps(world: &World, powered: &HashSet<(i32, i32, i32)>, things: &[ThingState]) -> Vec<PointLight> {
    let mut lights: Vec<PointLight> = powered
        .iter()
        .filter(|&&(x, y, z)| world.get(x, y, z).id() == ids::TORCH)
        .map(|&(x, y, z)| {
            let (gx, gy) = to_iso(x, y);
            PointLight::new(Vec3::new(gx, gy, z as f32 + 0.7), Vec3::new(1.0, 0.8, 0.45), 6.0, 2.0)
        })
        .collect();
    // The booster rails next to a powered cable glow magenta (`BoosterLogic`: radius 1, brightness 12).
    let mut boosters = HashSet::new();
    for &(x, y, z) in powered {
        if world.get(x, y, z).id() != ids::POWER_CABLE {
            continue;
        }
        for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
            if world.get(x + dx, y + dy, z).id() == ids::BOOSTER_RAILS {
                boosters.insert((x + dx, y + dy, z));
            }
        }
    }
    lights.extend(boosters.into_iter().map(|(x, y, z)| {
        let (gx, gy) = to_iso(x, y);
        PointLight::new(Vec3::new(gx, gy, z as f32 + 0.25), Vec3::new(0.8, 0.0, 0.3), 1.0, 12.0)
    }));
    lights.extend(things.iter().filter(|t| t.kind == "minecart").map(|t| {
        PointLight::new(Vec3::from(t.pos) + Vec3::new(0.0, 0.0, 0.8), Vec3::new(1.0, 0.9, 0.6), 4.0, 1.0)
    }));
    lights
}

/// The HUD as the page's `wurfelHud.update` takes it.
pub fn hud_json(hud: &Hud) -> String {
    serde_json::json!({
        "health": hud.health,
        "jetpack": hud.jetpack,
        "items": hud.items,
        // Already in menu order; `index` is what `craft` takes.
        "recipes": craft_menu(hud).iter().map(|r| serde_json::json!({
            "index": r.index,
            "name": r.name,
            "can": r.can,
            "ingredients": r.ingredients.iter().map(|i| serde_json::json!({"name": i.name, "have": i.have})).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
        "money": hud.money,
    })
    .to_string()
}

/// A happening of the `events` message, as far as the client reacts to it.
#[derive(Debug, Clone, PartialEq)]
pub enum Happening {
    Sound { name: String, pos: Vec3 },
    Dust { pos: Vec3 },
    /// A block that cannot be broken by hand was hit: dirt flies.
    HardHit { pos: Vec3 },
    /// A turret fired from one point to another.
    Shot { from: Vec3, to: Vec3 },
    /// A robot was destroyed.
    RobotDestroyed { pos: Vec3 },
    /// A block was hit and still stands: the cell and its health left, for the cracks over it.
    BlockDamaged { cell: (i32, i32, i32), health: u8 },
    Explosion { pos: Vec3, radius: i32 },
    /// A line for the HUD to show.
    Toast(String),
    /// We took damage.
    Hurt,
    /// We died and are back at the start.
    Died,
    /// Something was moved (portal, lift, the console's `tpplayer`): the owner of a predicted
    /// entity jumps there instead of blending.
    Teleported { entity: u32, pos: Vec3 },
    /// A sound that was started on something and ends now (a cart's rolling).
    SoundStopped { name: String },
    /// The intro ship came down: it burns.
    ShipCrashed { pos: Vec3 },
    /// A player did one of the moves that are animated (`attack`, `release_attack`, `prepare_throw`,
    /// `throw`, `drop`); `ok` is whether the rules accepted it. Starts the animation of other
    /// players and corrects ours when the server refused.
    Action { player: u32, name: String, ok: bool },
    /// A player was thrown by a catapult or cannon: where from and how fast.
    Launched { player: u32, pos: Vec3, vel: Vec3 },
}

fn position(v: &Value) -> Option<Vec3> {
    let a = v.as_array()?;
    Some(Vec3::new(a.first()?.as_f64()? as f32, a.get(1)?.as_f64()? as f32, a.get(2)?.as_f64()? as f32))
}

/// Read an `events` message. Things the client does not understand are skipped, so a newer server
/// does not break an older client.
pub fn parse_events(data: &Value, my_id: u32) -> Vec<Happening> {
    let mut out = Vec::new();
    for event in data.as_array().into_iter().flatten() {
        let mine = event.get("player").and_then(Value::as_u64) == Some(u64::from(my_id));
        let text = |key: &str| event.get(key).and_then(Value::as_str).unwrap_or_default();
        let happening = match event.get("t").and_then(Value::as_str).unwrap_or_default() {
            "sound" => position(&event["pos"]).map(|pos| Happening::Sound { name: text("name").to_string(), pos }),
            "dust" if event.get("health").is_none() => position(&event["pos"]).map(|pos| Happening::HardHit { pos }),
            "dust" => position(&event["pos"]).map(|pos| Happening::Dust { pos }),
            "shot" => match (position(&event["from"]), position(&event["to"])) {
                (Some(from), Some(to)) => Some(Happening::Shot { from, to }),
                _ => None,
            },
            "robot_destroyed" => position(&event["pos"]).map(|pos| Happening::RobotDestroyed { pos }),
            "explosion" => position(&event["pos"]).map(|pos| Happening::Explosion { pos, radius: event.get("radius").and_then(Value::as_i64).unwrap_or(3) as i32 }),
            "damaged" if mine => Some(Happening::Hurt),
            "picked" if mine => Some(Happening::Toast(format!("Picked up {}", text("item")))),
            "crafted" if mine => Some(Happening::Toast(format!("Crafted {}", text("item")))),
            "money" if mine => Some(Happening::Toast(format!("Money: {}", event.get("total").and_then(Value::as_u64).unwrap_or(0)))),
            "died" if mine => Some(Happening::Died),
            "built" => Some(Happening::Toast("Built".into())),
            "bought" if mine => Some(Happening::Toast(format!("Bought {} for {}", text("item"), event.get("price").and_then(Value::as_u64).unwrap_or(0)))),
            "flag" if event.get("team").and_then(Value::as_u64) == Some(2) => Some(Happening::Toast("Flag captured".into())),
            "respawn_set" => Some(Happening::Toast("Respawn point set".into())),
            "robot_built" => Some(Happening::Toast(format!("A {} was built", text("variant")))),
            "end_fight" => Some(Happening::Toast("The robots are attacking!".into())),
            "teleported" => match (event.get("entity").and_then(Value::as_u64), position(&event["pos"])) {
                (Some(entity), Some(pos)) => Some(Happening::Teleported { entity: entity as u32, pos }),
                _ => None,
            },
            "sound_stop" => Some(Happening::SoundStopped { name: text("name").to_string() }),
            "action" => event.get("player").and_then(Value::as_u64).map(|player| Happening::Action {
                player: player as u32,
                name: text("name").to_string(),
                ok: event.get("ok").and_then(Value::as_bool).unwrap_or(true),
            }),
            "ship_crashed" => position(&event["pos"]).map(|pos| Happening::ShipCrashed { pos }),
            "launched" => match (event.get("player").and_then(Value::as_u64), position(&event["pos"]), position(&event["vel"])) {
                (Some(player), Some(pos), Some(vel)) => Some(Happening::Launched { player: player as u32, pos, vel }),
                _ => None,
            },
            _ => None,
        };
        out.extend(happening);
        if event.get("t").and_then(Value::as_str) == Some("dust") {
            let cell = event.get("cell").and_then(Value::as_array).filter(|a| a.len() == 3);
            let health = event.get("health").and_then(Value::as_u64);
            if let (Some(cell), Some(health)) = (cell, health) {
                let n = |i: usize| cell[i].as_i64().map(|v| v as i32);
                if let (Some(x), Some(y), Some(z)) = (n(0), n(1), n(2)) {
                    out.push(Happening::BlockDamaged { cell: (x, y, z), health: health.min(100) as u8 });
                }
            }
        }
    }
    out
}

// ---- catapults and cannons -----------------------------------------------------------------

/// A catapult or cannon as the server last described it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LauncherInfo {
    pub cell: (i32, i32, i32),
    pub launcher: Launcher,
}

/// The cells of a `launchers` message: `{"launchers": [{cell, kind, heading, elevation, power, loaded}]}`.
pub fn parse_launchers(data: &Value) -> Vec<LauncherInfo> {
    let number = |v: &Value, key: &str| v.get(key).and_then(Value::as_u64).map(|n| n.min(255) as u8);
    data.get("launchers")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|v| {
            let cell = v.get("cell")?.as_array()?;
            let cell = (cell.first()?.as_i64()? as i32, cell.get(1)?.as_i64()? as i32, cell.get(2)?.as_i64()? as i32);
            let kind = match v.get("kind")?.as_str()? {
                "Catapult" => LauncherKind::Catapult,
                "Cannon" => LauncherKind::Cannon,
                _ => return None,
            };
            let mut launcher = Launcher::new(kind);
            launcher.heading = number(v, "heading")?;
            launcher.elevation = number(v, "elevation")?;
            launcher.power = number(v, "power")?;
            launcher.loaded = number(v, "loaded").unwrap_or(0);
            Some(LauncherInfo { cell, launcher })
        })
        .collect()
}

/// Is this dialog the aiming menu of a catapult or cannon? (The preview is shown while it is open.)
pub fn is_aiming_dialog(title: &str) -> bool {
    title == LauncherKind::Catapult.name() || title == LauncherKind::Cannon.name()
}

/// The launcher the player is aiming: the nearest one within reach of the menu.
pub fn aimed_launcher(list: &[LauncherInfo], at: Vec3) -> Option<&LauncherInfo> {
    list.iter()
        .map(|l| (cell_distance(l.cell, at), l))
        .filter(|(d, _)| *d <= 4.0)
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, l)| l)
}

fn cell_distance(cell: (i32, i32, i32), at: Vec3) -> f32 {
    let (gx, gy) = to_iso(cell.0, cell.1);
    Vec3::new(gx, gy, cell.2 as f32 + 0.5).distance(at)
}

/// What the aiming preview was computed for; it is only computed again when this changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PreviewKey {
    cell: (i32, i32, i32),
    heading: u8,
    elevation: u8,
    power: u8,
}

pub fn preview_key(info: &LauncherInfo) -> PreviewKey {
    PreviewKey { cell: info.cell, heading: info.launcher.heading, elevation: info.launcher.elevation, power: info.launcher.power }
}

/// The dots of the aiming preview, from the shared rules: only the first part of the flight.
pub fn preview_dots(world: &World, info: &LauncherInfo) -> Vec<PreviewDot> {
    preview_arc(world, Launcher::muzzle(info.cell), info.launcher.velocity())
}

/// Draw the preview as small blocks that shrink as the arc fades.
pub fn push_preview(out: &mut Vec<Vertex>, dots: &[PreviewDot]) {
    for dot in dots {
        let half = 0.04 + 0.08 * dot.strength.clamp(0.0, 1.0);
        let shade = 0.5 + 0.5 * dot.strength.clamp(0.0, 1.0);
        let [x, y, z] = dot.position.to_array();
        mesh::cuboid(out, [shade, shade, 0.4 * shade], [x - half, x + half, y - half, y + half], [z + 0.5 - half, z + 0.5 + half]);
    }
}

/// Where the server would have our player after `plan` (the inputs it has not acknowledged yet),
/// starting from the state it reported: the same rules, run on a scratch copy so the live player's
/// jetpack and attack state are not played twice. Returns position and velocity.
///
/// The scratch player starts with the first input already held, because that input was already
/// being applied when the server reported its state: a held jump is not a new jump.
pub fn replay(world: &mut World, server_pos: Vec3, server_vel: Vec3, plan: &[(PlayerInput, u32)]) -> Option<(Vec3, Vec3)> {
    let mut caveland = Caveland::new(Tuning::default(), 1);
    let mut entities = Entities::new();
    let id = caveland.spawn_player(&mut entities, 0, server_pos);
    entities.get_mut(id)?.body.as_mut()?.movement = server_vel;
    if let Some(&(first, _)) = plan.first() {
        caveland.assume_held(id, controls(first));
    }
    for &(input, steps) in plan {
        for _ in 0..steps {
            caveland.set_controls(&mut entities, world, id, controls(input));
            caveland.tick(&mut entities, world, TICK_DT);
        }
    }
    let entity = entities.get(id)?;
    Some((entity.position, entity.body.as_ref()?.movement))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use wurfel_sim::block::id;
    use wurfel_sim::{AirGenerator, Block};

    #[test]
    fn keys_map_to_actions_and_holding_keys_have_a_release() {
        assert_eq!(key_action("f", true), Some(("attack", 0)));
        assert_eq!(key_action("f", false), Some(("release_attack", 0)));
        assert_eq!(key_action("t", true), Some(("prepare_throw", 0)));
        assert_eq!(key_action("t", false), Some(("throw", 0)));
        assert_eq!(key_action("c", true), None, "c is the crafting popup, no longer the throw");
        assert_eq!(key_action("c", false), None);
        assert_eq!(key_action("e", true), Some(("use", 0)));
        assert_eq!(key_action("e", false), None, "using is a press, not a hold");
        assert_eq!(key_action("1", true), None, "crafting is the popup's job, not a digit key");
        assert_eq!(key_action("w", true), None, "walking is not an action");
        assert_eq!(mouse_action(0, true), Some(("attack", 0)));
        assert_eq!(mouse_action(0, false), Some(("release_attack", 0)));
        assert_eq!(mouse_action(2, true), Some(("prepare_throw", 0)), "right button: hold to wind up");
        assert_eq!(mouse_action(2, false), Some(("throw", 0)), "release throws");
        assert_eq!(mouse_action(1, true), None);
    }

    #[test]
    fn action_events_name_who_did_what_and_whether_the_rules_accepted_it() {
        let data = json!([
            {"t": "action", "player": 5, "name": "attack", "ok": true},
            {"t": "action", "player": 2, "name": "throw", "ok": false},
            {"t": "action", "player": 3, "name": "drop"},
            {"t": "action", "name": "attack"},
        ]);
        assert_eq!(
            parse_events(&data, 2),
            [
                Happening::Action { player: 5, name: "attack".into(), ok: true },
                Happening::Action { player: 2, name: "throw".into(), ok: false },
                Happening::Action { player: 3, name: "drop".into(), ok: true },
            ],
            "a missing ok counts as accepted; an event without a player is skipped"
        );
    }

    #[test]
    fn every_key_action_is_one_the_server_understands() {
        // The names the server's `CavelandMode::act` accepts.
        let known = ["attack", "release_attack", "prepare_throw", "throw", "drop", "use", "interact", "switch_left", "switch_right", "craft"];
        for key in ["f", "t", "e", "r", "x", "z", "v", "1", "5", "9"] {
            for pressed in [true, false] {
                if let Some((name, _)) = key_action(key, pressed) {
                    assert!(known.contains(&name), "{name}");
                }
            }
        }
    }

    #[test]
    fn things_have_distinct_looks_and_unknown_ones_still_draw() {
        assert_ne!(thing_style("robot").color, thing_style("friendly_robot").color);
        assert!(thing_style("robot").height > thing_style("Torch").height, "robots are taller than items");
        let mut out = Vec::new();
        push_thing(&mut out, &ThingState { id: 1, kind: "never-heard-of".into(), pos: [1.0, 2.0, 3.0], lit: false });
        assert_eq!(out.len(), 30, "top and four sides, two triangles each");
        let mut again = Vec::new();
        push_thing(&mut again, &ThingState { id: 1, kind: "never-heard-of".into(), pos: [1.0, 2.0, 3.0], lit: false });
        assert_eq!(out.len(), again.len());
    }

    #[test]
    fn our_entry_of_a_state_message_becomes_the_hud() {
        let data = json!({
            "4": {"health": 80.0, "jetpack": 0.5, "items": ["Wood", "Coal"], "recipes": [["Minecart", false, ["Iron", "Wood"]], ["Torch", true, ["Wood", "Coal"]], ["Old", false]]},
            "9": {"health": 10.0, "jetpack": 0.0, "items": [], "recipes": []},
        });
        let hud = parse_state(&data, 4).unwrap();
        assert_eq!(hud.health, 80.0);
        assert_eq!(hud.items, vec!["Wood".to_string(), "Coal".to_string()]);
        let ing = |name: &str, have| Ingredient { name: name.to_string(), have };
        assert_eq!(
            hud.recipes[0],
            RecipeInfo { index: 0, name: "Minecart".into(), can: false, ingredients: vec![ing("Iron", false), ing("Wood", true)] }
        );
        assert_eq!(hud.recipes[1].ingredients, vec![ing("Wood", true), ing("Coal", true)]);
        assert!(hud.recipes[2].ingredients.is_empty(), "the old [name, can] form still parses");
        assert!(parse_state(&data, 5).is_none(), "somebody else's state is not ours");
        assert!(parse_state(&json!({"4": {"health": 1}}), 4).is_none(), "a broken entry is ignored, not guessed");
        let round: Value = serde_json::from_str(&hud_json(&hud)).unwrap();
        assert_eq!(round["items"], json!(["Wood", "Coal"]));
        // The menu order: craftable first, each keeping its fixed index.
        let order: Vec<u64> = round["recipes"].as_array().unwrap().iter().map(|r| r["index"].as_u64().unwrap()).collect();
        assert_eq!(order, vec![1, 0, 2]);
        assert_eq!(round["recipes"][0]["name"], "Torch");
        assert_eq!(round["recipes"][0]["can"], true);
        assert_eq!(round["recipes"][1]["ingredients"], json!([{"name": "Iron", "have": false}, {"name": "Wood", "have": true}]));
    }

    #[test]
    fn the_craft_menu_lists_craftable_recipes_first_and_keeps_the_fixed_indices() {
        let r = |index, can| RecipeInfo { index, name: format!("r{index}"), can, ingredients: vec![] };
        let hud = Hud { recipes: vec![r(0, false), r(1, true), r(2, false), r(3, true)], ..Hud::default() };
        let order: Vec<usize> = craft_menu(&hud).iter().map(|r| r.index).collect();
        assert_eq!(order, vec![1, 3, 0, 2], "stable: list order within each group");
        assert!(craft_menu(&Hud::default()).is_empty());
    }

    #[test]
    fn a_hit_block_is_reported_with_its_cell_and_health_besides_the_dust() {
        let data = json!([{"t": "dust", "pos": [5.0, 5.0, 5.0], "cell": [3, -4, 7], "health": 62}]);
        assert_eq!(
            parse_events(&data, 1),
            vec![Happening::Dust { pos: Vec3::new(5.0, 5.0, 5.0) }, Happening::BlockDamaged { cell: (3, -4, 7), health: 62 }]
        );
    }

    #[test]
    fn events_are_read_and_only_our_own_news_becomes_a_toast() {
        let data = json!([
            {"t": "sound", "name": "collect", "pos": [1.0, 2.0, 3.0]},
            {"t": "picked", "player": 4, "item": "Torch"},
            {"t": "picked", "player": 9, "item": "Coal"},
            {"t": "crafted", "player": 4, "item": "Torch"},
            {"t": "explosion", "pos": [0.0, 0.0, 1.0], "radius": 3},
            {"t": "dust", "pos": [5.0, 5.0, 5.0]},
            {"t": "died", "player": 9},
            {"t": "died", "player": 4},
            {"t": "from-the-future", "pos": [0, 0, 0]},
            {"t": "sound", "name": "x"},
        ]);
        assert_eq!(
            parse_events(&data, 4),
            vec![
                Happening::Sound { name: "collect".into(), pos: Vec3::new(1.0, 2.0, 3.0) },
                Happening::Toast("Picked up Torch".into()),
                Happening::Toast("Crafted Torch".into()),
                Happening::Explosion { pos: Vec3::new(0.0, 0.0, 1.0), radius: 3 },
                Happening::HardHit { pos: Vec3::new(5.0, 5.0, 5.0) },
                Happening::Died,
            ]
        );
        assert!(parse_events(&json!("not a list"), 4).is_empty());
    }

    #[test]
    fn who_is_hidden_or_carried_comes_from_the_state_message() {
        let data = json!({
            "4": {"health": 80.0, "hidden": true, "riding": true},
            "5": {"health": 80.0, "riding": true},
            "6": {"health": 80.0},
            "x": {"hidden": true},
        });
        let flags = parse_flags(&data);
        assert_eq!(flags[&4], PlayerFlags { hidden: true, riding: true, jetpack: false, burning: false });
        assert_eq!(flags[&5], PlayerFlags { hidden: false, riding: true, jetpack: false, burning: false });
        assert_eq!(flags[&6], PlayerFlags::default(), "an older server says nothing: nobody is hidden");
        assert_eq!(flags.len(), 3, "a key that is not a player id is skipped");
    }

    #[test]
    fn launchers_and_launches_are_read_and_the_preview_is_only_a_start() {
        let data = json!({"launchers": [
            {"cell": [3, 4, 1], "kind": "Cannon", "heading": 2, "elevation": 6, "power": 8, "loaded": 3, "ready": true},
            {"cell": [9, 9, 1], "kind": "Trebuchet", "heading": 0, "elevation": 0, "power": 1},
            {"cell": [1, 1], "kind": "Catapult", "heading": 0, "elevation": 0, "power": 1},
        ]});
        let list = parse_launchers(&data);
        assert_eq!(list.len(), 1, "unknown kinds and broken entries are skipped");
        assert_eq!(list[0].launcher.kind, LauncherKind::Cannon);
        assert_eq!((list[0].launcher.heading, list[0].launcher.power, list[0].launcher.loaded), (2, 8, 3));
        assert!(is_aiming_dialog("Cannon") && is_aiming_dialog("Catapult") && !is_aiming_dialog("Turret"));

        let (gx, gy) = to_iso(3, 4);
        assert!(aimed_launcher(&list, Vec3::new(gx, gy, 1.0)).is_some());
        assert!(aimed_launcher(&list, Vec3::new(gx + 30.0, gy, 1.0)).is_none(), "too far away to be aiming it");

        let world = floor();
        let dots = preview_dots(&world, &list[0]);
        let flight = caveland_sim::launcher::simulate_flight(&world, Launcher::muzzle(list[0].cell), list[0].launcher.velocity());
        assert!(dots.last().unwrap().tick as usize * 2 <= flight.points.len(), "no more than about half the flight");
        let mut out = Vec::new();
        push_preview(&mut out, &dots);
        assert_eq!(out.len(), dots.len() * 30);

        let launched = parse_events(&json!([{"t": "launched", "player": 4, "pos": [1.0, 2.0, 3.0], "vel": [4.0, 5.0, 6.0]}, {"t": "launched", "player": 4}]), 4);
        assert_eq!(launched, vec![Happening::Launched { player: 4, pos: Vec3::new(1.0, 2.0, 3.0), vel: Vec3::new(4.0, 5.0, 6.0) }]);
    }

    #[test]
    fn private_messages_are_for_their_receiver_only() {
        let data = json!({"to": 4, "title": "Hello"});
        assert!(addressed_to(&data, 4));
        assert!(!addressed_to(&data, 5));
        assert!(!addressed_to(&json!({"title": "no receiver"}), 4), "no receiver: nobody acts on it");
    }

    #[test]
    fn the_money_is_part_of_the_hud() {
        let data = json!({"4": {"health": 1.0, "jetpack": 0.0, "items": [], "recipes": [], "money": 42}});
        let hud = parse_state(&data, 4).unwrap();
        assert_eq!(hud.money, 42);
        let round: Value = serde_json::from_str(&hud_json(&hud)).unwrap();
        assert_eq!(round["money"], 42);
    }

    #[test]
    fn the_powered_cells_are_read_and_bad_entries_skipped() {
        let cells = parse_power(&json!({"cells": [[1, 2, 3], [4, 5, 6], [7, 8], "no", [1, 2, 3]]}));
        assert_eq!(cells, HashSet::from([(1, 2, 3), (4, 5, 6)]));
        assert!(parse_power(&json!({})).is_empty());
    }

    #[test]
    fn only_powered_torches_and_carts_give_light() {
        let mut world = floor();
        for (x, y) in [(2, 3), (4, 3), (6, 3)] {
            world.set(x, y, 1, Block::new(ids::TORCH, 0));
        }
        let powered = HashSet::from([(2, 3, 1), (6, 3, 1), (9, 9, 1)]); // (9,9,1) is a cable or turret: air here
        let cart = ThingState { id: 1, kind: "minecart".into(), pos: [5.0, 5.0, 1.0], lit: false };
        let rock = ThingState { id: 2, kind: "Wood".into(), pos: [1.0, 1.0, 1.0], lit: false };
        let lights = lamps(&world, &powered, &[cart, rock]);
        assert_eq!(lights.len(), 3, "two powered torches and one cart: {lights:?}");
        let (gx, gy) = to_iso(2, 3);
        assert!(lights.iter().any(|l| l.position == Vec3::new(gx, gy, 1.7)), "a torch lights from just above its block");
        let unlit = lamps(&world, &HashSet::new(), &[]);
        assert!(unlit.is_empty(), "a torch nobody feeds stays dark");
    }

    #[test]
    fn the_rest_of_the_game_reports_through_events() {
        let data = json!([
            {"t": "teleported", "entity": 4, "pos": [1.0, 2.0, 3.0]},
            {"t": "teleported", "entity": 4},
            {"t": "sound_stop", "name": "wagon", "entity": 9},
            {"t": "ship_crashed", "pos": [5.0, 6.0, 7.0]},
            {"t": "built", "cell": [1, 2, 3], "block": 20},
            {"t": "bought", "player": 4, "item": "Torch", "price": 3},
            {"t": "bought", "player": 5, "item": "Coal", "price": 4},
            {"t": "flag", "flag": 1, "team": 2},
            {"t": "flag", "flag": 1, "team": 1},
            {"t": "respawn_set", "cell": [0, 0, 5]},
            {"t": "robot_built", "robot": 3, "variant": "spider"},
            {"t": "end_fight"},
        ]);
        assert_eq!(
            parse_events(&data, 4),
            vec![
                Happening::Teleported { entity: 4, pos: Vec3::new(1.0, 2.0, 3.0) },
                Happening::SoundStopped { name: "wagon".into() },
                Happening::ShipCrashed { pos: Vec3::new(5.0, 6.0, 7.0) },
                Happening::Toast("Built".into()),
                Happening::Toast("Bought Torch for 3".into()),
                Happening::Toast("Flag captured".into()),
                Happening::Toast("Respawn point set".into()),
                Happening::Toast("A spider was built".into()),
                Happening::Toast("The robots are attacking!".into()),
            ]
        );
    }

    #[test]
    fn every_kind_of_the_game_has_a_fallback_box_of_its_own() {
        use caveland_sim::{EntityKind, Team};
        let mut seen = HashSet::new();
        for kind in [
            EntityKind::Robot(Team::Robots), EntityKind::SpiderRobot(Team::Robots), EntityKind::Drone(Team::Robots),
            EntityKind::Vanya, EntityKind::Shopkeeper, EntityKind::Bird, EntityKind::Spaceship, EntityKind::LiftBasket,
            EntityKind::MineCart, EntityKind::DropSpaceFlag,
        ] {
            let style = thing_style(&kind.name());
            assert_ne!(style.color, [0.9, 0.5, 0.9], "{} falls through to the 'unknown' pink", kind.name());
            seen.insert(style.color.map(f32::to_bits));
        }
        assert_eq!(seen.len(), 10, "and they can be told apart");
    }

    /// A stone floor, the surface at height 1.
    fn floor() -> World {
        let mut world = World::new(AirGenerator);
        Caveland::install(&mut world);
        for x in -30..30 {
            for y in -60..60 {
                world.set(x, y, 0, Block::new(id::STONE, 0));
            }
        }
        world
    }

    #[test]
    fn replaying_walks_the_player_on_with_the_servers_rules() {
        let mut world = floor();
        let walk = PlayerInput { right: true, ..Default::default() };
        let (pos, vel) = replay(&mut world, Vec3::new(0.0, 0.0, 1.0), Vec3::ZERO, &[(walk, 30)]).unwrap();
        assert!(pos.distance(Vec3::new(0.0, 0.0, 1.0)) > 0.5, "half a second of walking moves: {pos:?}");
        assert!(vel.length() > 0.5);
        assert_eq!(pos.z, 1.0, "still on the ground");
        let (stay, _) = replay(&mut world, Vec3::new(0.0, 0.0, 1.0), Vec3::ZERO, &[]).unwrap();
        assert_eq!(stay, Vec3::new(0.0, 0.0, 1.0), "nothing to replay: the server state stands");
    }

    #[test]
    fn a_jump_that_was_already_held_when_the_server_reported_is_not_jumped_again() {
        let mut world = floor();
        let hold = PlayerInput { jump: true, ..Default::default() };
        // The server says we are on the ground with the jump key down for a while already.
        let (held, _) = replay(&mut world, Vec3::new(0.0, 0.0, 1.0), Vec3::ZERO, &[(hold, 6)]).unwrap();
        assert_eq!(held.z, 1.0, "no new jump from a key that was already down");
        // A jump pressed later in the plan is a real new one.
        let release = PlayerInput::default();
        let (jumped, vel) = replay(&mut world, Vec3::new(0.0, 0.0, 1.0), Vec3::ZERO, &[(release, 2), (hold, 6)]).unwrap();
        assert!(jumped.z > 1.0 && vel.z != 0.0, "{jumped:?} {vel:?}");
    }

    #[test]
    fn replay_does_not_touch_the_world() {
        let mut world = floor();
        let before = world.get(0, 0, 0);
        replay(&mut world, Vec3::new(0.0, 0.0, 1.0), Vec3::ZERO, &[(PlayerInput { up: true, jump: true, ..Default::default() }, 120)]).unwrap();
        assert_eq!(world.get(0, 0, 0), before);
    }
}

#[cfg(test)]
mod interact_sign_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_focus_message_gives_a_position_or_nothing() {
        assert_eq!(parse_interact_focus(&json!({"pos": [1.0, 2.5, 3.0]})), Some(glam::Vec3::new(1.0, 2.5, 3.0)));
        assert_eq!(parse_interact_focus(&json!({"pos": null})), None);
    }
}

#[cfg(test)]
mod mode_tests {
    use super::*;
    use serde_json::json;
    use wurfel_sim::AirGenerator;

    fn session() -> CavelandClient {
        CavelandClient::start(&mut World::new(AirGenerator))
    }

    #[test]
    fn the_state_message_updates_the_hud_and_says_who_rides_and_who_is_hidden() {
        let mut mode = session();
        let data = json!({"2": {"riding": true}, "7": {"hidden": true}});
        mode.on_rules("state", &data, 2, Vec3::ZERO);
        assert!(mode.riding());
        assert!(mode.is_hidden(7) && !mode.is_hidden(2));
        mode.on_rules("state", &json!({}), 2, Vec3::ZERO);
        assert!(!mode.riding() && !mode.is_hidden(7), "the next state replaces the flags");
    }

    #[test]
    fn private_news_for_somebody_else_does_nothing() {
        let mut mode = session();
        assert!(mode.on_rules("dialog", &json!({"to": 9, "title": "Oven"}), 2, Vec3::ZERO).is_empty());
        let ours = mode.on_rules("dialog", &json!({"to": 2, "title": "Oven"}), 2, Vec3::ZERO);
        assert!(matches!(&ours[..], [Effect::Hud { method: "dialog", arg: Some(_) }]));
        assert_eq!(mode.on_rules("dialog_closed", &json!({"to": 2}), 2, Vec3::ZERO), [Effect::Hud { method: "closeDialog", arg: None }]);
    }

    #[test]
    fn happenings_become_engine_effects() {
        let me = 2;
        let here = Vec3::new(1.0, 1.0, 1.0);
        let effects = CavelandClient::effects(
            vec![
                Happening::Sound { name: "urfJump".into(), pos: here },
                Happening::Sound { name: "urfJump".into(), pos: here + Vec3::X * 10.0 },
                Happening::Sound { name: "wagon".into(), pos: here },
                Happening::Teleported { entity: me, pos: here },
                Happening::Teleported { entity: 9, pos: here },
                Happening::Launched { player: me, pos: here, vel: Vec3::Z },
                Happening::Hurt,
                Happening::Died,
            ],
            me,
            here,
        );
        assert_eq!(
            effects,
            [
                Effect::Sound { name: "urfJump".into(), pos: here + Vec3::X * 10.0 },
                Effect::Loop { name: "wagon".into(), pos: here },
                Effect::PlaceLocalPlayer { pos: here, vel: Vec3::ZERO },
                Effect::PlaceLocalPlayer { pos: here, vel: Vec3::Z },
                Effect::Shake { amplitude: 14.0, millis: 220.0 },
                Effect::Respawned { message: "You died. Back at the start.".into() },
            ],
            "our own jump is not heard twice, the cart rolls on, only our player is placed"
        );
    }

    #[test]
    fn the_session_predicts_with_caveland_rules_and_offers_its_commands() {
        let mut mode = session();
        let mut world = World::new(AirGenerator);
        let mut entities = Entities::new();
        let id = mode.spawn_local_player(&mut entities, Vec3::new(0.0, 0.0, 5.0));
        mode.apply_input(&mut entities, &world, id, PlayerInput::default());
        mode.tick(&mut entities, &mut world, TICK_DT);
        assert!(entities.get(id).unwrap().position.z < 5.0, "it falls through the empty world");
        assert!(!mode.exhaust(id));
        assert!(mode.commands().iter().any(|(name, _)| *name == "give"));
        assert_eq!(mode.key_action("f", true), Some(("attack", 0)));
    }
}
