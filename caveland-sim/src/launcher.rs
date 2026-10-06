//! The catapult and the cannon (not in the Java game): constructions that throw the player and
//! items along a ballistic arc.
//!
//! * The **catapult** needs no ammunition; it reloads over time ([`CATAPULT_RELOAD`]).
//! * The **cannon** fires one unit of [`Gunpowder`](CollectibleType::Gunpowder) per shot, which has
//!   to be carried to it and loaded ([`CANNON_CAPACITY`] at most). It can also fire an explosives
//!   item as a shell that goes off where it lands, with the same explosion as a lit explosive.
//!
//! A launch is only a starting position and velocity. The body then flies under the engine's own
//! gravity and collision at the fixed physics step, so the server and a client that is told the
//! same start run the same arc. There is no "out of control" state: how much the player can steer
//! while they fly follows from their speed alone ([`crate::movement`]). Nothing in a launch or a
//! landing hurts: the game has no fall damage and the launch does none.
//!
//! The aim is a heading, an angle and a power. They are small whole numbers that index tables of
//! sines and cosines written out below, so no `sin` or `cos` of the platform runs in the rules and
//! every machine computes the same launch velocity bit for bit.
//!
//! [`preview_arc`] is what the client draws while a player aims. It shows only the first part of
//! the flight and fades out, so where the shot comes down stays a guess.

use glam::Vec3;
use serde::{Deserialize, Serialize};
use wurfel_sim::entity::physics::is_on_ground;
use wurfel_sim::entity::{Entities, EntityId, Event};
use wurfel_sim::player::{new_player, TICK_DT, WALKING_SPEED};
use wurfel_sim::World;

use crate::barrier::ColumnBarrier;
use crate::blocks::ids;
use crate::collectible::{CollectibleType, Item};
use crate::dialog::{option, Dialog, Source};
use crate::extras::ExtraEvent;
use crate::movement;
use crate::game::{cell_center, cell_floor, clamp_to_cave_ceiling, Caveland, Cell, Kind};
use crate::player::PlayerState;

/// Seconds a catapult needs to wind up again after a throw.
pub const CATAPULT_RELOAD: f32 = 4.0;
/// Seconds between two cannon shots, on top of needing gunpowder.
pub const CANNON_COOLDOWN: f32 = 1.0;
/// How much gunpowder a cannon holds. Inventories are small, so this is a few trips, not a stockpile.
pub const CANNON_CAPACITY: u8 = 4;
/// Power settings, 1 to this.
pub const MAX_POWER: u8 = 10;
pub const DEFAULT_POWER: u8 = 5;
/// Turns of the heading: one step is 15 degrees.
pub const HEADINGS: u8 = 24;
/// Angles of the elevation table: 15 to 75 degrees in steps of 5.
pub const ELEVATIONS: u8 = 13;
pub const DEFAULT_ELEVATION: u8 = 6; // 45 degrees
/// A flight that has not landed after this many physics steps ends anyway (ten seconds).
pub const MAX_FLIGHT_TICKS: u32 = 600;
/// The preview shows this share of the flight time and no more, so the landing stays a guess.
pub const PREVIEW_FRACTION: f32 = 0.45;
/// One dot of the preview every this many physics steps.
pub const PREVIEW_DOT_EVERY: u32 = 4;
/// Loose items this close to the muzzle (blocks, horizontally) are thrown along with the player.
const LOAD_RADIUS: f32 = 1.0;
/// Seconds before a thrown item can be picked up by who threw it.
const THROWN_PICKUP_BLOCK: f32 = 0.8;
/// A shell goes off when it has lost this much of its sideways speed (it hit a wall).
const SHELL_WALL_SPEED: f32 = 0.25;
/// A shell goes off when an enemy robot is this close, in blocks.
const SHELL_PROXIMITY: f32 = 0.8;
/// A shell that is still flying after this long is gone (seconds, a safety net).
const SHELL_LIFETIME: f32 = 12.0;

/// `(cos, sin)` of the heading in the ground frame, 15 degrees per step.
const HEADING: [(f32, f32); HEADINGS as usize] = [
    (1.0, 0.0),
    (0.965925813, 0.258819044),
    (0.866025388, 0.5),
    (0.707106769, 0.707106769),
    (0.5, 0.866025388),
    (0.258819044, 0.965925813),
    (0.0, 1.0),
    (-0.258819044, 0.965925813),
    (-0.5, 0.866025388),
    (-0.707106769, 0.707106769),
    (-0.866025388, 0.5),
    (-0.965925813, 0.258819044),
    (-1.0, 0.0),
    (-0.965925813, -0.258819044),
    (-0.866025388, -0.5),
    (-0.707106769, -0.707106769),
    (-0.5, -0.866025388),
    (-0.258819044, -0.965925813),
    (0.0, -1.0),
    (0.258819044, -0.965925813),
    (0.5, -0.866025388),
    (0.707106769, -0.707106769),
    (0.866025388, -0.5),
    (0.965925813, -0.258819044),
];

/// `(cos, sin)` of the elevation, 15 to 75 degrees in steps of 5.
const ELEVATION: [(f32, f32); ELEVATIONS as usize] = [
    (0.965925813, 0.258819044),
    (0.939692616, 0.342020154),
    (0.906307817, 0.42261827),
    (0.866025388, 0.5),
    (0.819152057, 0.57357645),
    (0.766044438, 0.642787635),
    (0.707106769, 0.707106769),
    (0.642787635, 0.766044438),
    (0.57357645, 0.819152057),
    (0.5, 0.866025388),
    (0.42261827, 0.906307817),
    (0.342020154, 0.939692616),
    (0.258819044, 0.965925813),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LauncherKind {
    Catapult,
    Cannon,
}

impl LauncherKind {
    pub fn from_block(id: u8) -> Option<LauncherKind> {
        match id {
            ids::CATAPULT => Some(LauncherKind::Catapult),
            ids::CANNON => Some(LauncherKind::Cannon),
            _ => None,
        }
    }

    pub fn block(self) -> u8 {
        match self {
            LauncherKind::Catapult => ids::CATAPULT,
            LauncherKind::Cannon => ids::CANNON,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            LauncherKind::Catapult => "Catapult",
            LauncherKind::Cannon => "Cannon",
        }
    }

    /// Launch speed in blocks per second at power 1 and at the highest power. On level ground a
    /// throw at 45 degrees goes `speed^2 / gravity` blocks: a few blocks to a dozen for the
    /// catapult, up to about forty for the cannon.
    fn speeds(self) -> (f32, f32) {
        match self {
            LauncherKind::Catapult => (5.0, 11.0),
            LauncherKind::Cannon => (8.0, 20.0),
        }
    }
}

/// Why a launcher did not fire.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FireError {
    /// Still winding up or cooling down.
    Reloading,
    /// A cannon with nothing loaded.
    NoGunpowder,
}

/// One catapult or cannon: how it is aimed and what state it is in.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Launcher {
    pub kind: LauncherKind,
    /// 0 to [`HEADINGS`] - 1, 15 degrees per step.
    pub heading: u8,
    /// Index into the elevation table: 0 is 15 degrees, [`ELEVATIONS`] - 1 is 75.
    pub elevation: u8,
    /// 1 to [`MAX_POWER`].
    pub power: u8,
    /// Gunpowder in the barrel (cannon only).
    pub loaded: u8,
    /// Seconds until it can fire again.
    pub cooldown: f32,
}

impl Launcher {
    pub fn new(kind: LauncherKind) -> Self {
        Launcher { kind, heading: 0, elevation: DEFAULT_ELEVATION, power: DEFAULT_POWER, loaded: 0, cooldown: 0.0 }
    }

    pub fn heading_degrees(&self) -> u32 {
        self.heading as u32 * 15
    }

    pub fn elevation_degrees(&self) -> u32 {
        15 + self.elevation as u32 * 5
    }

    /// Turn the heading by whole steps, around the circle.
    pub fn turn(&mut self, steps: i32) {
        self.heading = (self.heading as i32 + steps).rem_euclid(HEADINGS as i32) as u8;
    }

    /// Raise or lower the barrel, within 15 to 75 degrees.
    pub fn tilt(&mut self, steps: i32) {
        self.elevation = (self.elevation as i32 + steps).clamp(0, ELEVATIONS as i32 - 1) as u8;
    }

    /// More or less power, within 1 to [`MAX_POWER`].
    pub fn adjust_power(&mut self, steps: i32) {
        self.power = (self.power as i32 + steps).clamp(1, MAX_POWER as i32) as u8;
    }

    /// The launch speed of the current power, blocks per second.
    pub fn speed(&self) -> f32 {
        let (low, high) = self.kind.speeds();
        low + (high - low) * (self.power - 1) as f32 / (MAX_POWER - 1) as f32
    }

    /// The velocity of whatever is launched now.
    pub fn velocity(&self) -> Vec3 {
        let (hx, hy) = HEADING[self.heading as usize % HEADING.len()];
        let (cos, sin) = ELEVATION[self.elevation as usize % ELEVATION.len()];
        let v = self.speed();
        Vec3::new(hx * cos * v, hy * cos * v, sin * v)
    }

    /// Where launched things start: standing on top of the machine.
    pub fn muzzle(cell: Cell) -> Vec3 {
        cell_floor((cell.0, cell.1, cell.2 + 1))
    }

    /// Load one unit of gunpowder. False for a catapult or a full barrel.
    pub fn load_gunpowder(&mut self) -> bool {
        if self.kind == LauncherKind::Cannon && self.loaded < CANNON_CAPACITY {
            self.loaded += 1;
            true
        } else {
            false
        }
    }

    /// Can it fire right now?
    pub fn ready(&self) -> bool {
        self.cooldown <= 0.0 && (self.kind == LauncherKind::Catapult || self.loaded > 0)
    }

    /// Fire: used up gunpowder (cannon) or started winding up (catapult). Returns the launch
    /// velocity, or says why not and changes nothing.
    pub fn fire(&mut self) -> Result<Vec3, FireError> {
        if self.cooldown > 0.0 {
            return Err(FireError::Reloading);
        }
        match self.kind {
            LauncherKind::Catapult => self.cooldown = CATAPULT_RELOAD,
            LauncherKind::Cannon => {
                if self.loaded == 0 {
                    return Err(FireError::NoGunpowder);
                }
                self.loaded -= 1;
                self.cooldown = CANNON_COOLDOWN;
            }
        }
        Ok(self.velocity())
    }

    pub fn update(&mut self, dt: f32) {
        self.cooldown = (self.cooldown - dt).max(0.0);
    }
}

/// What the clients need to know about a launcher (the server sends these as they change).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LauncherView {
    pub cell: Cell,
    pub launcher: Launcher,
}

// ---- the flight ----------------------------------------------------------------------------

/// A flight run to the end: where the body was at every physics step.
#[derive(Debug, Clone, PartialEq)]
pub struct Flight {
    /// The position at step 0 (the start) and after each step.
    pub points: Vec<Vec3>,
    /// It came down within [`MAX_FLIGHT_TICKS`].
    pub landed: bool,
}

/// Fly a player-sized body from `origin` with `velocity` in `world`: the same engine physics at the
/// same fixed step as a launched player, so the points are the positions the real flight has.
pub fn simulate_flight(world: &World, origin: Vec3, velocity: Vec3) -> Flight {
    let mut entities = Entities::new();
    let mut body = new_player(origin);
    body.add_component(Box::new(ColumnBarrier));
    if let Some(b) = body.body.as_mut() {
        b.set_movement(velocity);
    }
    let id = entities.spawn(body);
    let mut points = vec![origin];
    let mut landed = false;
    for _ in 0..MAX_FLIGHT_TICKS {
        let bounced = entities.get_mut(id).is_some_and(|e| movement::bounce(world, e, WALKING_SPEED, TICK_DT));
        let events = entities.update(world, TICK_DT);
        let Some(entity) = entities.get_mut(id) else { break };
        // The same rules a player's step applies after the engine's: light drag in the air, the cave ceiling.
        if let Some(b) = entity.body.as_mut() {
            if !is_on_ground(world, entity.position, entity.dimension_z) {
                movement::air_drag(b, TICK_DT);
            }
        }
        clamp_to_cave_ceiling(entity);
        points.push(entity.position);
        // The flight ends at its first contact: the first bounce, or the landing.
        if bounced || events.contains(&Event::Landed(id)) {
            landed = true;
            break;
        }
    }
    Flight { points, landed }
}

/// One dot of the aiming preview.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PreviewDot {
    pub position: Vec3,
    /// 1 at the muzzle, fading towards 0 at the end of what is shown.
    pub strength: f32,
    /// Physics steps after the launch.
    pub tick: u32,
}

/// The dotted arc shown while aiming: only the first [`PREVIEW_FRACTION`] of the flight time, one
/// dot every [`PREVIEW_DOT_EVERY`] steps, fading out. It stops well before the landing, so the
/// player sees the direction and the rise but has to guess where the shot comes down.
pub fn preview_arc(world: &World, origin: Vec3, velocity: Vec3) -> Vec<PreviewDot> {
    let flight = simulate_flight(world, origin, velocity);
    let ticks = flight.points.len() - 1;
    let shown = ((ticks as f32 * PREVIEW_FRACTION) as usize).max(1);
    let fade = (shown as u32 + PREVIEW_DOT_EVERY) as f32;
    (0..=shown as u32)
        .step_by(PREVIEW_DOT_EVERY as usize)
        .filter_map(|tick| {
            let position = *flight.points.get(tick as usize)?;
            Some(PreviewDot { position, strength: 1.0 - tick as f32 / fade, tick })
        })
        .collect()
}

// ---- in the world -----------------------------------------------------------------------------

/// A cannon shell in the air: an explosives item that goes off where it comes down.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Shell {
    age: f32,
    /// Sideways speed at the start, to notice a wall.
    speed: f32,
}

const OPT_LAUNCH_ME: u8 = 0;
const OPT_LAUNCH_ITEM: u8 = 1;
const OPT_SHELL: u8 = 2;
const OPT_LOAD: u8 = 3;
const OPT_LEFT: u8 = 4;
const OPT_RIGHT: u8 = 5;
const OPT_UP: u8 = 6;
const OPT_DOWN: u8 = 7;
const OPT_POWER_UP: u8 = 8;
const OPT_POWER_DOWN: u8 = 9;

impl Caveland {
    // ---- reading state --------------------------------------------------------------------

    /// The catapult or cannon at a cell, once it has been seen.
    pub fn launcher(&self, cell: Cell) -> Option<&Launcher> {
        self.x.launchers.get(&cell)
    }

    /// Change a launcher (what the aiming dialog does; also for hosts and tests).
    pub fn launcher_mut(&mut self, cell: Cell) -> Option<&mut Launcher> {
        self.x.launchers.get_mut(&cell)
    }

    /// Every launcher, in cell order.
    pub fn launcher_views(&self) -> Vec<LauncherView> {
        let mut views: Vec<LauncherView> = self.x.launchers.iter().map(|(&cell, &launcher)| LauncherView { cell, launcher }).collect();
        views.sort_by_key(|v| v.cell);
        views
    }

    // ---- launching ------------------------------------------------------------------------

    /// Throw a player: from `position` (or where they are) with `velocity`. Only the position and
    /// the velocity are set; everything after is normal movement. The server does this when somebody
    /// fires a machine; a client does the same when it is told, which is what makes both fly the
    /// same arc.
    pub fn launch_player(&mut self, entities: &mut Entities, id: EntityId, position: Option<Vec3>, velocity: Vec3) -> bool {
        if self.player(id).is_none() || entities.get(id).is_none() {
            return false;
        }
        self.begin_launch(entities, id, position, velocity);
        true
    }

    fn begin_launch(&mut self, entities: &mut Entities, id: EntityId, position: Option<Vec3>, velocity: Vec3) {
        let Some(entity) = entities.get_mut(id) else { return };
        if let Some(position) = position {
            entity.position = position;
        }
        if let Some(body) = entity.body.as_mut() {
            body.set_movement(velocity);
        }
        let position = entity.position;
        self.x.events.push(ExtraEvent::Launched { entity: id, position, velocity });
    }

    /// Put a finished catapult or cannon down at `cell` without building it (a dev command).
    pub fn place_machine(&mut self, world: &mut World, cell: Cell, block: u8) -> bool {
        let Some(kind) = LauncherKind::from_block(block) else { return false };
        if !world.get(cell.0, cell.1, cell.2).is_air() || !world.set(cell.0, cell.1, cell.2, wurfel_sim::Block::new(block, 0)) {
            return false;
        }
        self.x.launchers.insert(cell, Launcher::new(kind));
        self.x.events.push(ExtraEvent::BlockChanged { cell, block: wurfel_sim::Block::new(block, 0) });
        self.x.dirty = true;
        true
    }

    // ---- the aiming dialog ----------------------------------------------------------------

    /// The machine's menu: aim, load and fire. `note` says what the last try did.
    pub(crate) fn open_launcher_dialog(&mut self, player: EntityId, state: &PlayerState, cell: Cell, note: &str) {
        let Some(l) = self.x.launchers.get(&cell).copied() else { return };
        let status = match l.kind {
            LauncherKind::Catapult if l.cooldown > 0.0 => format!("Reloading, {:.0} s", l.cooldown.ceil()),
            LauncherKind::Catapult => "Ready".to_string(),
            LauncherKind::Cannon => format!("Gunpowder {}/{}{}", l.loaded, CANNON_CAPACITY, if l.cooldown > 0.0 { ", cooling down" } else { "" }),
        };
        let text = format!(
            "Heading {} deg, angle {} deg, power {}/{}. {status}. {note}",
            l.heading_degrees(),
            l.elevation_degrees(),
            l.power,
            MAX_POWER
        );
        let in_hand = state.inventory.front().map_or("nothing".to_string(), |i| i.kind.name().to_string());
        let mut options = vec![option(OPT_LAUNCH_ME, "Launch me"), option(OPT_LAUNCH_ITEM, format!("Launch item in hand: {in_hand}"))];
        if l.kind == LauncherKind::Cannon {
            options.push(option(OPT_SHELL, "Fire explosive shell"));
            options.push(option(OPT_LOAD, "Load gunpowder"));
        }
        options.extend([
            option(OPT_LEFT, "Aim left"),
            option(OPT_RIGHT, "Aim right"),
            option(OPT_UP, "Raise"),
            option(OPT_DOWN, "Lower"),
            option(OPT_POWER_UP, "More power"),
            option(OPT_POWER_DOWN, "Less power"),
        ]);
        self.open(player, Dialog::selection(l.kind.name(), &text, options), Source::Launcher(cell));
    }

    /// An answer to the machine's menu.
    pub(crate) fn launcher_choice(&mut self, entities: &mut Entities, world: &mut World, player: EntityId, state: &mut PlayerState, cell: Cell, answer: u8) {
        let Some(launcher) = self.x.launchers.get_mut(&cell) else { return };
        let kind = launcher.kind;
        let note = match answer {
            OPT_LEFT => {
                launcher.turn(-1);
                String::new()
            }
            OPT_RIGHT => {
                launcher.turn(1);
                String::new()
            }
            OPT_UP => {
                launcher.tilt(1);
                String::new()
            }
            OPT_DOWN => {
                launcher.tilt(-1);
                String::new()
            }
            OPT_POWER_UP => {
                launcher.adjust_power(1);
                String::new()
            }
            OPT_POWER_DOWN => {
                launcher.adjust_power(-1);
                String::new()
            }
            OPT_LOAD if kind == LauncherKind::Cannon => {
                if launcher.loaded >= CANNON_CAPACITY {
                    self.fail_sound(cell);
                    "The cannon is full.".to_string()
                } else if state.inventory.retrieve_type(CollectibleType::Gunpowder).is_some() {
                    launcher.load_gunpowder();
                    self.sound("metallic", cell_center(cell));
                    String::new()
                } else {
                    self.fail_sound(cell);
                    "You have no gunpowder.".to_string()
                }
            }
            OPT_LAUNCH_ME | OPT_LAUNCH_ITEM => match self.fire_launcher(entities, world, player, state, cell, answer) {
                Ok(()) => return,
                Err(note) => note,
            },
            OPT_SHELL if kind == LauncherKind::Cannon => match self.fire_launcher(entities, world, player, state, cell, answer) {
                Ok(()) => return,
                Err(note) => note,
            },
            _ => return,
        };
        self.open_launcher_dialog(player, state, cell, &note);
    }

    fn fail_sound(&mut self, cell: Cell) {
        self.sound("interactionFail", cell_center(cell));
    }

    /// Fire the machine at `cell` with what `what` (a menu option) says. `Err` is what to tell the
    /// player; nothing was used then.
    fn fire_launcher(&mut self, entities: &mut Entities, world: &World, player: EntityId, state: &mut PlayerState, cell: Cell, what: u8) -> Result<(), String> {
        let muzzle = Launcher::muzzle(cell);
        let top = (cell.0, cell.1, cell.2 + 1);
        if !world.get(top.0, top.1, top.2).is_air() {
            self.fail_sound(cell);
            return Err("Something is in the way above it.".to_string());
        }
        // Check what the shot needs before the machine is used.
        match what {
            OPT_LAUNCH_ITEM if state.inventory.is_empty() => {
                self.fail_sound(cell);
                return Err("You hold nothing to launch.".to_string());
            }
            OPT_SHELL if state.inventory.count(CollectibleType::Explosives) == 0 => {
                self.fail_sound(cell);
                return Err("A shell needs explosives in your pack.".to_string());
            }
            _ => {}
        }
        let launcher = self.x.launchers.get_mut(&cell).ok_or_else(|| "The machine is gone.".to_string())?;
        let velocity = match launcher.fire() {
            Ok(v) => v,
            Err(e) => {
                self.fail_sound(cell);
                return Err(match e {
                    FireError::Reloading => "It is still reloading.".to_string(),
                    FireError::NoGunpowder => "It has no gunpowder.".to_string(),
                });
            }
        };
        let kind = launcher.kind;
        match what {
            OPT_LAUNCH_ME => {
                self.begin_launch(entities, player, Some(muzzle), velocity);
                self.throw_loose_items(entities, player, muzzle, velocity);
            }
            OPT_LAUNCH_ITEM => {
                if let Some(item) = state.inventory.retrieve(0) {
                    let eid = self.spawn_collectible(entities, item, muzzle + Vec3::Z * 0.5);
                    if let Some(body) = entities.get_mut(eid).and_then(|e| e.body.as_mut()) {
                        body.set_movement(velocity);
                    }
                    self.block_pickup(eid, player, THROWN_PICKUP_BLOCK);
                }
            }
            _ => {
                state.inventory.retrieve_type(CollectibleType::Explosives);
                let eid = self.spawn_collectible(entities, Item::new(CollectibleType::Explosives), muzzle + Vec3::Z * 0.5);
                if let Some(body) = entities.get_mut(eid).and_then(|e| e.body.as_mut()) {
                    body.set_movement(velocity);
                }
                self.set_pickup_allowed(eid, false);
                self.x.shells.insert(eid, Shell { age: 0.0, speed: velocity.truncate().length() });
            }
        }
        self.sound(if kind == LauncherKind::Cannon { "turret" } else { "release" }, muzzle);
        Ok(())
    }

    /// Items lying on the machine go with the player (a load of stones, a crate of goods).
    fn throw_loose_items(&mut self, entities: &mut Entities, thrower: EntityId, muzzle: Vec3, velocity: Vec3) {
        let mut loose: Vec<EntityId> = self
            .kinds
            .iter()
            .filter_map(|(&id, kind)| match kind {
                Kind::Collectible(c) if !c.no_pickup => Some(id),
                _ => None,
            })
            .filter(|&id| {
                entities.get(id).is_some_and(|e| {
                    let d = e.position - muzzle;
                    d.truncate().length() <= LOAD_RADIUS && (-0.3..1.5).contains(&d.z)
                })
            })
            .collect();
        loose.sort_unstable();
        for id in loose {
            if let Some(entity) = entities.get_mut(id) {
                entity.position = muzzle + Vec3::Z * 0.3;
                if let Some(body) = entity.body.as_mut() {
                    body.set_movement(velocity);
                }
            }
            self.block_pickup(id, thrower, THROWN_PICKUP_BLOCK);
        }
    }

    // ---- the step -------------------------------------------------------------------------

    /// Wind the machines up again and let the shells in the air find what they hit.
    pub(crate) fn update_launchers(&mut self, entities: &mut Entities, dt: f32) {
        for launcher in self.x.launchers.values_mut() {
            launcher.update(dt);
        }
        let mut shells: Vec<EntityId> = self.x.shells.keys().copied().collect();
        shells.sort_unstable();
        let robots: Vec<Vec3> = self
            .kinds
            .iter()
            .filter_map(|(&id, kind)| match kind {
                Kind::Robot(r) if r.team == crate::team::Team::Robots => entities.get(id).map(|e| e.position),
                _ => None,
            })
            .collect();
        for id in shells {
            let Some(mut shell) = self.x.shells.remove(&id) else { continue };
            let Some(entity) = entities.get_mut(id).filter(|e| !e.is_disposed()) else { continue };
            shell.age += dt;
            let position = entity.position;
            let sideways = entity.body.as_ref().map_or(0.0, |b| b.speed_hor());
            // The first few steps still start from the machine.
            let flying = shell.age > 4.0 * dt;
            let landed = self.engine_events.contains(&Event::Landed(id)) || self.bounced.contains(&id);
            let hit_wall = flying && sideways < shell.speed * SHELL_WALL_SPEED;
            let near_enemy = flying && robots.iter().any(|r| r.distance(position) < SHELL_PROXIMITY);
            if flying && (landed || hit_wall || near_enemy) || shell.age > SHELL_LIFETIME {
                entity.dispose();
                self.kinds.remove(&id);
                self.shell_blasts.push(position);
            } else {
                self.x.shells.insert(id, shell);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wurfel_sim::{AirGenerator, Block};

    fn floor() -> World {
        let mut world = World::new(AirGenerator);
        for x in -40..40 {
            for y in -80..80 {
                world.set(x, y, 0, Block::new(wurfel_sim::block::id::STONE, 0));
            }
        }
        world
    }

    #[test]
    fn the_tables_are_sines_and_cosines() {
        for (k, (c, s)) in HEADING.iter().enumerate() {
            let a = (k as f64 * 15.0).to_radians();
            assert!((*c as f64 - a.cos()).abs() < 1e-6 && (*s as f64 - a.sin()).abs() < 1e-6, "heading {k}");
        }
        for (k, (c, s)) in ELEVATION.iter().enumerate() {
            let a = (15.0 + k as f64 * 5.0).to_radians();
            assert!((*c as f64 - a.cos()).abs() < 1e-6 && (*s as f64 - a.sin()).abs() < 1e-6, "elevation {k}");
        }
    }

    #[test]
    fn aiming_wraps_the_heading_and_clamps_the_rest() {
        let mut l = Launcher::new(LauncherKind::Catapult);
        l.turn(-1);
        assert_eq!(l.heading, HEADINGS - 1);
        l.turn(1);
        assert_eq!(l.heading, 0);
        l.tilt(100);
        assert_eq!(l.elevation_degrees(), 75);
        l.tilt(-100);
        assert_eq!(l.elevation_degrees(), 15);
        l.adjust_power(100);
        assert_eq!(l.power, MAX_POWER);
        l.adjust_power(-100);
        assert_eq!(l.power, 1);
    }

    #[test]
    fn the_launch_velocity_follows_the_aim_and_the_power() {
        let mut l = Launcher::new(LauncherKind::Cannon);
        l.heading = 6; // 90 degrees: along +y
        l.elevation = 6; // 45 degrees
        l.power = MAX_POWER;
        let v = l.velocity();
        assert!(v.x.abs() < 1e-5 && v.y > 0.0 && v.z > 0.0);
        assert!((v.length() - 20.0).abs() < 1e-3, "the top power of a cannon is 20 blocks per second: {v}");
        l.power = 1;
        assert!((l.velocity().length() - 8.0).abs() < 1e-3);
        let catapult = Launcher { kind: LauncherKind::Catapult, ..l };
        assert!(catapult.speed() < Launcher { power: MAX_POWER, ..l }.speed(), "a catapult throws shorter than a cannon");
    }

    #[test]
    fn a_catapult_needs_no_ammunition_and_reloads_over_time() {
        let mut l = Launcher::new(LauncherKind::Catapult);
        assert!(l.ready());
        assert!(l.fire().is_ok());
        assert_eq!(l.fire(), Err(FireError::Reloading));
        assert!(!l.ready());
        l.update(CATAPULT_RELOAD - 0.5);
        assert_eq!(l.fire(), Err(FireError::Reloading), "not yet");
        l.update(0.6);
        assert!(l.ready());
        assert!(l.fire().is_ok(), "ready again without anything loaded");
        assert!(!l.load_gunpowder(), "a catapult takes no gunpowder");
    }

    #[test]
    fn a_cannon_burns_one_gunpowder_per_shot_and_refuses_without() {
        let mut l = Launcher::new(LauncherKind::Cannon);
        assert_eq!(l.fire(), Err(FireError::NoGunpowder));
        assert!(l.load_gunpowder() && l.load_gunpowder());
        assert!(l.fire().is_ok());
        assert_eq!(l.loaded, 1);
        assert_eq!(l.fire(), Err(FireError::Reloading), "it cools down between shots");
        l.update(CANNON_COOLDOWN);
        assert!(l.fire().is_ok());
        assert_eq!(l.loaded, 0);
        l.update(CANNON_COOLDOWN);
        assert_eq!(l.fire(), Err(FireError::NoGunpowder), "and it is empty again");
        for _ in 0..CANNON_CAPACITY {
            assert!(l.load_gunpowder());
        }
        assert!(!l.load_gunpowder(), "the barrel holds {CANNON_CAPACITY}");
    }

    #[test]
    fn the_same_launch_flies_the_same_arc_every_time() {
        let world = floor();
        let mut l = Launcher::new(LauncherKind::Cannon);
        l.heading = 3;
        l.power = 7;
        let a = simulate_flight(&world, Launcher::muzzle((0, 0, 0)), l.velocity());
        let b = simulate_flight(&world, Launcher::muzzle((0, 0, 0)), l.velocity());
        assert_eq!(a, b, "bit for bit");
        assert!(a.landed && a.points.len() > 30);
        let end = *a.points.last().unwrap();
        assert!(end.z < 1.5, "it came down on the floor (and bounced): {end}");
        assert!(a.points.iter().map(|p| p.z).fold(0.0, f32::max) > 2.0, "and it went up first");
    }

    #[test]
    fn a_flight_is_a_gravity_arc() {
        let world = floor();
        let flight = simulate_flight(&world, Vec3::new(0.0, 0.0, 1.0), Vec3::new(6.0, 0.0, 6.0));
        // The height follows gravity: z(t) = z0 + vz t - g t^2 / 2 (the engine steps with the new
        // velocity, so allow one step of difference); the sideways speed is only lightly dragged.
        let t = 30.0 * TICK_DT;
        let p = flight.points[30];
        assert!(p.x < 6.0 * t && p.x > 6.0 * t * 0.9, "{}", p.x);
        let expected = 1.0 + 6.0 * t - 0.5 * 9.81 * t * t;
        assert!((p.z - expected).abs() < 0.2, "{} vs {expected}", p.z);
    }

    #[test]
    fn the_preview_shows_only_the_start_of_the_flight_and_fades() {
        let world = floor();
        let mut l = Launcher::new(LauncherKind::Cannon);
        l.power = 9;
        let origin = Launcher::muzzle((0, 0, 0));
        let flight = simulate_flight(&world, origin, l.velocity());
        let dots = preview_arc(&world, origin, l.velocity());
        let ticks = flight.points.len() as u32 - 1;
        assert!(dots.len() >= 4, "enough to read the direction: {}", dots.len());
        let last = dots.last().unwrap();
        assert!(last.tick as f32 <= ticks as f32 * 0.5, "no more than half the flight: {} of {ticks}", last.tick);
        assert!(last.tick as f32 >= ticks as f32 * 0.3, "but not a stub");
        assert!(dots.windows(2).all(|w| w[0].strength > w[1].strength), "fading out");
        assert!(dots[0].strength > 0.9 && last.strength > 0.0);
        let landing = *flight.points.last().unwrap();
        let nearest = dots.iter().map(|d| d.position.distance(landing)).fold(f32::MAX, f32::min);
        assert!(nearest > landing.distance(origin) * 0.3, "no dot near the landing: {nearest}");
        assert_eq!(dots[0].position, origin);
    }
}
