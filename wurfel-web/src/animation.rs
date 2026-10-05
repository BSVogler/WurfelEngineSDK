//! Ejira's animation: which sprite of the player sheet shows, from what the player is doing. A pure
//! port of `Ejira.playAnimation`, `updateSprite` and `getAnimationStep`.
//!
//! Animation is presentation, so the client owns it and the server neither holds nor sends it. The
//! rules (charge, release, throw, drop and their timings) are the server's, in `caveland-sim`. The
//! [`Performer`] here mirrors just enough of them (from the same constants) to animate: the local
//! player's starts from the local input at once, a remote player's from the one-shot `action`
//! events of the server, and the walking and jumping of both from the movement seen. Where the
//! server's outcome differs (a refused swing, a throw with nothing to throw) the actor's
//! performer is told with [`Performer::refused`].
//!
//! The sheet (`playerSheet`, atlas names `diff/<glyph>/<n>`) has one animation per [`Move`], each
//! with a run of frames per viewing direction: `w` walk, `h` first swing, `l` loaded (the swing
//! held), `i` power attack, `t` throw, `j` jump, and the overlays `s` (the charge) and `o` (the
//! power attack's glow).
//!
//! Java's `animationCycle` runs 0 to 1000 in `dt * 5` per millisecond, i.e. one cycle per 200 ms
//! for everything but walking. Here the cycle is 0 to 1 and [`CYCLES_PER_SECOND`] is 5.
//!
//! Walking is not simulated here: its speed follows how far the player moves, which the client sees
//! better than the server (it is the interpolated position). [`Move::Walk`] only says "nothing special
//! is happening", the client then plays the walk cycle itself.

#![cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]

use glam::Vec2;

use caveland_sim::player::{LOAD_ATTACK_TIME, LOAD_THRESHOLD};

/// Speed of the attack, throw and jump animations (Java: 1000 units per 200 ms).
pub const CYCLES_PER_SECOND: f32 = 5.0;

/// The animations of the sheet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Move {
    Walk,
    /// The swing started by a tap (`h`).
    Hit,
    /// The swing held: the stance the character keeps while charging (`l`).
    Loaded,
    /// The released power attack (`i`).
    Power,
    Throw,
    Jump,
}

impl Move {
    /// The character of the atlas names: `diff/<glyph>/<n>`.
    pub fn glyph(self) -> u8 {
        match self {
            Move::Walk => b'w',
            Move::Hit => b'h',
            Move::Loaded => b'l',
            Move::Power => b'i',
            Move::Throw => b't',
            Move::Jump => b'j',
        }
    }

    /// Frames of one direction: the throw and the power attack have 6, the others 8.
    pub fn frames_per_direction(self) -> u32 {
        match self {
            Move::Throw | Move::Power => 6,
            _ => 8,
        }
    }
}

/// What the animation needs to know about the player besides time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Context {
    /// The throwing pose is held until the throw.
    pub prepare_throw: bool,
    pub on_ground: bool,
}

/// The state of the animation: which move, how far through it, whether it runs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Animation {
    action: Move,
    /// 0 to 1 through the move.
    cycle: f32,
    /// False means paused on the current frame.
    playing: bool,
    /// The frame within the direction's run, as of the last [`Animation::refresh`].
    step: u32,
}

impl Default for Animation {
    /// Standing in the walking stance, not playing.
    fn default() -> Self {
        Animation { action: Move::Walk, cycle: 0.0, playing: false, step: 0 }
    }
}

impl Animation {
    pub fn action(&self) -> Move {
        self.action
    }

    pub fn is_playing(&self) -> bool {
        self.playing
    }

    pub fn step(&self) -> u32 {
        self.step
    }

    /// Start a move from its first frame (`playAnimation`). The caller handles the flags that
    /// Java clears in the same place, see `PlayerState::play`.
    pub fn play(&mut self, action: Move, ctx: Context) {
        self.action = action;
        self.cycle = 0.0;
        self.playing = true;
        self.refresh(ctx);
    }

    /// Go into the walking stance and stop (`idle`).
    pub fn idle(&mut self, ctx: Context) {
        self.play(Move::Walk, ctx);
        self.playing = false;
    }

    /// Resume a paused move without restarting it (the throw after the pose was held).
    pub fn resume(&mut self) {
        self.playing = true;
    }

    /// Advance by `dt` seconds.
    pub fn advance(&mut self, dt: f32, ctx: Context) {
        if self.playing && self.action != Move::Walk {
            self.cycle += dt * CYCLES_PER_SECOND;
        }
        self.refresh(ctx);
    }

    /// `updateSprite(false)`: wrap the cycle, chain the swing into the loaded stance, and work out
    /// the frame and whether to pause.
    fn refresh(&mut self, ctx: Context) {
        if self.cycle >= 1.0 {
            self.cycle %= 1.0;
            if self.action == Move::Hit {
                // The first swing plays once, then the loaded stance continues.
                self.action = Move::Loaded;
            }
        }
        let steps = self.action.frames_per_direction();
        let mut step = (self.cycle * steps as f32) as u32;
        match self.action {
            Move::Throw | Move::Power => {
                if ctx.prepare_throw && step > 0 {
                    // The throwing pose waits on frame 1 for the release.
                    self.playing = false;
                    step = 1;
                }
                if step >= 5 {
                    self.playing = false;
                }
            }
            _ => {
                if step >= 7 && self.action == Move::Loaded {
                    self.playing = false; // the stance is held on its last frame
                }
                if self.action == Move::Jump {
                    if !ctx.on_ground && step > 3 {
                        // Rising or falling: hold the last air frame until the landing.
                        step = 3;
                        self.playing = false;
                    }
                    if ctx.on_ground {
                        self.playing = true;
                    }
                    if step > 6 {
                        self.playing = false;
                    }
                }
            }
        }
        self.step = step;
    }
}

/// The direction of the sheet, 0 to 7, in the order of `Ejira.updateSprite`: south, south-east, east,
/// north-east, north, north-west, west, south-west. `screen` is the facing in the screen-aligned
/// game space (x right, y down).
pub fn direction(screen: Vec2) -> u8 {
    let sin60 = (std::f32::consts::PI / 3.0).sin();
    if screen.x < -sin60 {
        6
    } else if screen.x < -0.5 {
        if screen.y < 0.0 { 5 } else { 7 }
    } else if screen.x < 0.5 {
        if screen.y < 0.0 { 4 } else { 0 }
    } else if screen.x < sin60 {
        if screen.y < 0.0 { 3 } else { 1 }
    } else {
        2
    }
}

/// The number `n` of `diff/<glyph>/<n>` for a move, direction and frame.
pub fn frame_number(action: Move, dir: u8, step: u32) -> u32 {
    let mut start = dir as i32;
    // The swing and charge animations start one direction earlier on the sheet.
    if !matches!(action, Move::Walk | Move::Jump | Move::Throw) {
        start -= 1;
    }
    if start < 0 {
        start = 7;
    }
    start as u32 * action.frames_per_direction() + 1 + step
}

/// How far the charge overlay is (`getAnimationStep(.., overlay)`): one frame per sixth of the
/// charge time once the hold has lasted [`LOAD_THRESHOLD`], 7 at the end.
pub fn overlay_step(load: f32, steps: u32) -> u32 {
    let charged = load - LOAD_THRESHOLD;
    if charged >= LOAD_ATTACK_TIME {
        7
    } else {
        (charged / (LOAD_ATTACK_TIME / steps as f32)).max(0.0) as u32
    }
}

/// What gets drawn on top of the character.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Overlay {
    /// `diff/s/<n>`: the charge filling up.
    Charge(u8),
    /// `diff/o/<n>`: the glow of the power attack, same frame as the character.
    Power(u8),
}

/// The picture of a player: what the sheet shows. 
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pose {
    pub action: Move,
    pub dir: u8,
    pub step: u8,
    pub overlay: Option<Overlay>,
}

impl Pose {
    /// `diff/<glyph>/<n>` of the character.
    pub fn frame(&self) -> u32 {
        frame_number(self.action, self.dir, self.step as u32)
    }

    /// The overlay as its atlas name's parts: glyph and number.
    pub fn overlay_frame(&self) -> Option<(u8, u32)> {
        self.overlay.map(|o| match o {
            Overlay::Charge(n) => (b's', n as u32),
            Overlay::Power(n) => (b'o', n as u32),
        })
    }
}

/// Work out the pose from the animation, the facing (screen-aligned game space) and the charge.
/// `load` is the seconds the attack has been held, `power` whether a power attack is under way.
pub fn pose(animation: &Animation, facing: Vec2, load: Option<f32>, power: bool) -> Pose {
    let dir = direction(facing);
    let step = animation.step() as u8;
    let charging = load.filter(|&t| t > LOAD_THRESHOLD);
    let overlay = if charging.is_some() || power {
        if animation.action() == Move::Power {
            Some(Overlay::Power(frame_number(Move::Power, dir, step as u32) as u8))
        } else {
            // The charge overlay uses the frame counting of the current move, with its own step.
            let ostep = charging.map_or(0, |t| overlay_step(t, animation.action().frames_per_direction()));
            Some(Overlay::Charge(frame_number(animation.action(), dir, ostep) as u8))
        }
    } else {
        None
    };
    Pose { action: animation.action(), dir, step, overlay }
}

/// The moves of the player that the animation reacts to: the actions the server also gets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Act {
    Attack,
    ReleaseAttack,
    PrepareThrow,
    Throw,
    Drop,
}

impl Act {
    /// The action names of `ClientMsg::Action` and of the server's `action` events.
    pub fn from_name(name: &str) -> Option<Act> {
        Some(match name {
            "attack" => Act::Attack,
            "release_attack" => Act::ReleaseAttack,
            "prepare_throw" => Act::PrepareThrow,
            "throw" => Act::Throw,
            "drop" => Act::Drop,
            _ => return None,
        })
    }
}

/// What the world around the player is doing this step.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Env {
    pub on_ground: bool,
    /// Horizontal speed in blocks per second.
    pub speed: f32,
}

/// The animation of one player with the little of the rules it needs: how long the attack has been
/// held, whether a swing is under way, whether a throw is prepared and for how long the button has
/// been down. Same timings as the server's `Tuning`.
#[derive(Debug, Clone, PartialEq)]
pub struct Performer {
    anim: Animation,
    load: Option<f32>,
    swing: Option<f32>,
    power: bool,
    used_load_in_air: bool,
    prepare_throw: bool,
    throw_held: Option<f32>,
    on_ground: bool,
    time_till_impact: f32,
    drop_time: f32,
}

impl Default for Performer {
    fn default() -> Self {
        let tuning = caveland_sim::Tuning::default();
        Performer {
            anim: Animation::default(),
            load: None,
            swing: None,
            power: false,
            used_load_in_air: false,
            prepare_throw: false,
            throw_held: None,
            on_ground: true,
            time_till_impact: tuning.time_till_impact,
            drop_time: tuning.item_drop_time,
        }
    }
}

impl Performer {
    fn ctx(&self) -> Context {
        Context { prepare_throw: self.prepare_throw, on_ground: self.on_ground }
    }

    /// `Ejira.playAnimation`: anything but the throw cancels a prepared throw, walking ends a
    /// power attack.
    fn play(&mut self, action: Move) {
        if action != Move::Throw {
            self.prepare_throw = false;
        }
        if action == Move::Walk {
            self.power = false;
        }
        let ctx = self.ctx();
        self.anim.play(action, ctx);
    }

    fn locked(&self) -> bool {
        self.load.is_some() || self.power || self.prepare_throw
    }

    /// The player did something (`Ejira.attack`, `attackLoadingStopped`, `prepareThrow`, `throwItem`,
    /// `dropItem`).
    pub fn act(&mut self, act: Act) {
        match act {
            Act::Attack => self.attack(),
            Act::ReleaseAttack => {
                if self.load.is_some_and(|t| t >= LOAD_ATTACK_TIME) {
                    self.play(Move::Power);
                    self.swing = None;
                    self.attack();
                    self.power = true;
                    self.used_load_in_air = true;
                }
                self.load = None;
            }
            Act::PrepareThrow => {
                self.throw_held = Some(0.0);
                self.play(Move::Throw);
                self.prepare_throw = true;
            }
            Act::Throw => {
                self.throw_held = None;
                if self.prepare_throw {
                    if self.anim.action() != Move::Throw {
                        self.play(Move::Throw);
                    }
                    self.anim.resume();
                    self.prepare_throw = false;
                }
            }
            Act::Drop => {
                self.throw_held = None;
                self.prepare_throw = false;
                self.play(Move::Walk);
            }
        }
    }

    fn attack(&mut self) {
        if self.swing.is_some() {
            return;
        }
        self.power = false;
        if self.anim.action() == Move::Loaded {
            self.play(Move::Power);
        } else if self.load.is_none_or(|t| t < LOAD_ATTACK_TIME) || self.anim.action() != Move::Power {
            self.play(Move::Hit);
        }
        self.swing = Some(self.time_till_impact);
        if !self.used_load_in_air {
            self.load = Some(0.0);
        }
    }

    /// The server did not do what was started here: a swing that did not begin, a throw with nothing
    /// to throw. Back to standing.
    pub fn refused(&mut self, act: Act) {
        match act {
            Act::Attack => {
                self.swing = None;
                self.load = None;
            }
            Act::PrepareThrow | Act::Throw => {
                self.throw_held = None;
                self.prepare_throw = false;
            }
            _ => return,
        }
        let ctx = self.ctx();
        self.anim.idle(ctx);
    }

    /// The player left the ground by jumping (`Ejira.jump`).
    pub fn jump(&mut self) {
        self.play(Move::Jump);
    }

    /// Advance by `dt` seconds.
    pub fn tick(&mut self, dt: f32, env: Env) {
        self.on_ground = env.on_ground;
        if env.on_ground {
            self.used_load_in_air = false;
        }
        let ctx = self.ctx();
        self.anim.advance(dt, ctx);
        if let Some(held) = self.throw_held.as_mut() {
            *held += dt;
            if *held >= self.drop_time {
                self.act(Act::Drop);
            }
        }
        if let Some(left) = self.swing.as_mut() {
            *left -= dt;
            if *left <= 0.0 {
                self.swing = None;
            }
        }
        if self.power && env.speed < 0.1 {
            self.power = false;
        }
        if let Some(load) = self.load.as_mut() {
            *load += dt;
            let load = *load;
            if load > LOAD_THRESHOLD && !matches!(self.anim.action(), Move::Loaded | Move::Power) && !self.anim.is_playing() {
                self.play(Move::Loaded);
            }
            if load >= LOAD_ATTACK_TIME {
                self.act(Act::ReleaseAttack);
            }
        }
        // Walking starts the walking stance unless something still plays (`Ejira.walk`).
        if env.speed > 0.1 && env.on_ground && !self.locked() && !self.anim.is_playing() {
            self.play(Move::Walk);
        }
    }

    /// What to draw for a player facing `facing` (screen-aligned game space).
    pub fn pose(&self, facing: Vec2) -> Pose {
        pose(&self.anim, facing, self.load, self.power)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GROUND: Context = Context { prepare_throw: false, on_ground: true };
    const AIR: Context = Context { prepare_throw: false, on_ground: false };

    fn run(a: &mut Animation, seconds: f32, ctx: Context) {
        let steps = (seconds * 600.0).round() as u32;
        for _ in 0..steps {
            a.advance(1.0 / 600.0, ctx);
        }
    }

    #[test]
    fn directions_are_numbered_like_the_java_sheet() {
        assert_eq!(direction(Vec2::new(0.0, 1.0)), 0, "south, towards the viewer");
        assert_eq!(direction(Vec2::new(0.8, 0.6)), 1);
        assert_eq!(direction(Vec2::new(1.0, 0.0)), 2, "east");
        assert_eq!(direction(Vec2::new(0.7, -0.7)), 3);
        assert_eq!(direction(Vec2::new(0.0, -1.0)), 4, "north");
        assert_eq!(direction(Vec2::new(-0.7, -0.7)), 5);
        assert_eq!(direction(Vec2::new(-1.0, 0.0)), 6, "west");
        assert_eq!(direction(Vec2::new(-0.7, 0.7)), 7);
    }

    #[test]
    fn frame_numbers_start_one_direction_earlier_for_swings() {
        // walking: south is the first run of 8
        assert_eq!(frame_number(Move::Walk, 0, 0), 1);
        assert_eq!(frame_number(Move::Walk, 6, 3), 6 * 8 + 1 + 3);
        // swings: the run before the direction number
        assert_eq!(frame_number(Move::Hit, 1, 0), 1);
        assert_eq!(frame_number(Move::Hit, 0, 0), 7 * 8 + 1, "south wraps to the last run");
        // the throw and the power attack have runs of 6
        assert_eq!(frame_number(Move::Throw, 2, 1), 2 * 6 + 1 + 1);
        assert_eq!(frame_number(Move::Power, 1, 2), 0 * 6 + 1 + 2);
        assert_eq!(frame_number(Move::Jump, 3, 0), 3 * 8 + 1);
    }

    #[test]
    fn a_swing_plays_eight_frames_in_200_ms_then_holds_the_loaded_stance_on_its_last_frame() {
        let mut a = Animation::default();
        a.play(Move::Hit, GROUND);
        assert_eq!((a.action(), a.step()), (Move::Hit, 0));
        run(&mut a, 0.11, GROUND);
        assert_eq!((a.action(), a.step()), (Move::Hit, 4), "half way after about 100 ms");
        run(&mut a, 0.1, GROUND);
        assert_eq!(a.action(), Move::Loaded, "chained into the loaded stance after one cycle");
        run(&mut a, 0.5, GROUND);
        assert_eq!((a.action(), a.step(), a.is_playing()), (Move::Loaded, 7, false), "held on the last frame");
    }

    #[test]
    fn the_throw_pose_waits_on_frame_one_until_the_throw_then_plays_to_the_end() {
        let mut a = Animation::default();
        let preparing = Context { prepare_throw: true, on_ground: true };
        a.play(Move::Throw, preparing);
        run(&mut a, 0.5, preparing);
        assert_eq!((a.action(), a.step(), a.is_playing()), (Move::Throw, 1, false));
        // the throw: resume without the flag
        a.resume();
        run(&mut a, 0.5, GROUND);
        assert_eq!((a.step(), a.is_playing()), (5, false), "the swing through ends on frame 5");
    }

    #[test]
    fn a_jump_holds_its_air_frame_until_the_landing() {
        let mut a = Animation::default();
        a.play(Move::Jump, GROUND);
        run(&mut a, 0.6, AIR);
        assert_eq!((a.action(), a.step(), a.is_playing()), (Move::Jump, 3, false), "airborne: frame 3 held");
        // landing resumes it and it ends on the last frame
        run(&mut a, 0.5, GROUND);
        assert_eq!((a.step(), a.is_playing()), (7, false));
    }

    #[test]
    fn walking_is_left_to_the_client() {
        let mut a = Animation::default();
        a.play(Move::Walk, GROUND);
        run(&mut a, 1.0, GROUND);
        assert_eq!((a.action(), a.step()), (Move::Walk, 0));
    }

    #[test]
    fn the_charge_overlay_fills_in_steps_once_the_hold_counts() {
        // the loaded stance has 8 frames per direction: one overlay step per 125 ms of charge
        assert_eq!(overlay_step(0.3, 8), 0);
        assert_eq!(overlay_step(0.3 + 0.13, 8), 1);
        assert_eq!(overlay_step(0.3 + 0.5, 8), 4);
        assert_eq!(overlay_step(0.3 + 1.0, 8), 7);
        assert_eq!(overlay_step(5.0, 8), 7);
        assert_eq!(overlay_step(0.3 + 0.5, 6), 3, "six steps for the throw's counting");
    }

    #[test]
    fn overlays_show_while_charging_or_during_the_power_attack() {
        let mut a = Animation::default();
        a.play(Move::Loaded, GROUND);
        let south = Vec2::new(0.0, 1.0);
        assert_eq!(pose(&a, south, Some(0.2), false).overlay, None, "a tap is no charge yet");
        let charging = pose(&a, south, Some(0.8), false);
        assert_eq!(charging.overlay_frame(), Some((b's', frame_number(Move::Loaded, 0, 4))));
        a.play(Move::Power, GROUND);
        let power = pose(&a, south, None, true);
        assert_eq!(power.overlay_frame(), Some((b'o', power.frame())));
        assert_eq!(pose(&a, south, None, false).overlay, None);
    }

    #[test]
    fn every_frame_number_stays_on_the_sheet() {
        // diff/h,l,j,s,w have 64 frames, diff/i,o,t have 48
        for action in [Move::Walk, Move::Hit, Move::Loaded, Move::Power, Move::Throw, Move::Jump] {
            let max = if action.frames_per_direction() == 6 { 48 } else { 64 };
            for dir in 0..8 {
                for step in 0..action.frames_per_direction() {
                    let n = frame_number(action, dir, step);
                    assert!((1..=max).contains(&n), "{action:?} dir {dir} step {step} -> {n}");
                }
            }
        }
    }

    fn env(on_ground: bool, speed: f32) -> Env {
        Env { on_ground, speed }
    }

    fn run_performer(p: &mut Performer, seconds: f32, env: Env) {
        for _ in 0..(seconds * 600.0).round() as u32 {
            p.tick(1.0 / 600.0, env);
        }
    }

    const SOUTH: Vec2 = Vec2::new(0.0, 1.0);

    #[test]
    fn an_attack_starts_the_swing_at_once_without_any_tick() {
        let mut p = Performer::default();
        assert_eq!(p.pose(SOUTH).action, Move::Walk);
        p.act(Act::Attack);
        let pose = p.pose(SOUTH);
        assert_eq!((pose.action, pose.step), (Move::Hit, 0));
        run_performer(&mut p, 0.1, env(true, 0.0));
        p.act(Act::ReleaseAttack);
        run_performer(&mut p, 0.5, env(true, 0.0));
        assert_eq!(p.pose(SOUTH).action, Move::Loaded, "held after the swing");
    }

    #[test]
    fn holding_charges_with_the_servers_thresholds_and_fires_by_itself() {
        let mut p = Performer::default();
        p.act(Act::Attack);
        run_performer(&mut p, 0.2, env(true, 0.0));
        assert_eq!(p.pose(SOUTH).overlay, None, "not yet a hold");
        run_performer(&mut p, 0.6, env(true, 0.0));
        assert!(matches!(p.pose(SOUTH).overlay, Some(Overlay::Charge(_))));
        // the release dashes forward (the server's lunge), so the glow lasts while it moves
        run_performer(&mut p, 0.4, env(true, 5.0));
        let pose = p.pose(SOUTH);
        assert_eq!(pose.action, Move::Power);
        assert!(matches!(pose.overlay, Some(Overlay::Power(_))));
        // it ends when the dash has stopped
        run_performer(&mut p, 0.1, env(true, 0.0));
        assert_eq!(p.pose(SOUTH).overlay, None);
    }

    #[test]
    fn a_second_swing_from_the_held_stance_is_the_long_one() {
        let mut p = Performer::default();
        p.act(Act::Attack);
        run_performer(&mut p, 0.1, env(true, 0.0));
        p.act(Act::ReleaseAttack);
        run_performer(&mut p, 0.5, env(true, 0.0));
        assert_eq!((p.pose(SOUTH).action, p.pose(SOUTH).step), (Move::Loaded, 7));
        p.act(Act::Attack);
        assert_eq!(p.pose(SOUTH).action, Move::Power);
    }

    #[test]
    fn the_throw_pose_is_held_then_played_through_and_a_long_hold_drops() {
        let mut p = Performer::default();
        p.act(Act::PrepareThrow);
        run_performer(&mut p, 0.4, env(true, 0.0));
        assert_eq!((p.pose(SOUTH).action, p.pose(SOUTH).step), (Move::Throw, 1));
        p.act(Act::Throw);
        run_performer(&mut p, 0.4, env(true, 0.0));
        assert_eq!(p.pose(SOUTH).step, 5);

        let mut q = Performer::default();
        q.act(Act::PrepareThrow);
        run_performer(&mut q, 0.7, env(true, 0.0));
        assert_eq!(q.pose(SOUTH).action, Move::Walk, "held past 0.6 s: dropped, back to standing");
        q.act(Act::Throw);
        assert_eq!(q.pose(SOUTH).action, Move::Walk, "the release finds nothing prepared");
    }

    #[test]
    fn an_attack_or_a_jump_cancels_a_prepared_throw() {
        let mut p = Performer::default();
        p.act(Act::PrepareThrow);
        p.act(Act::Attack);
        p.act(Act::Throw);
        assert_eq!(p.pose(SOUTH).action, Move::Hit);
        let mut q = Performer::default();
        q.act(Act::PrepareThrow);
        q.jump();
        assert_eq!(q.pose(SOUTH).action, Move::Jump);
        q.act(Act::Throw);
        assert_eq!(q.pose(SOUTH).action, Move::Jump, "no throw after the jump cancelled the pose");
    }

    #[test]
    fn a_jump_holds_the_air_frame_and_walking_resumes_after_it() {
        let mut p = Performer::default();
        p.jump();
        run_performer(&mut p, 0.6, env(false, 0.0));
        assert_eq!((p.pose(SOUTH).action, p.pose(SOUTH).step), (Move::Jump, 3));
        run_performer(&mut p, 0.5, env(true, 0.0));
        assert_eq!(p.pose(SOUTH).step, 7);
        run_performer(&mut p, 0.05, env(true, 3.0));
        assert_eq!(p.pose(SOUTH).action, Move::Walk);
    }

    #[test]
    fn a_refused_move_goes_back_to_standing() {
        let mut p = Performer::default();
        p.act(Act::PrepareThrow);
        p.refused(Act::PrepareThrow);
        assert_eq!(p.pose(SOUTH).action, Move::Walk);
        p.act(Act::Attack);
        p.refused(Act::Attack);
        assert_eq!(p.pose(SOUTH).action, Move::Walk);
        p.act(Act::Attack);
        assert_eq!(p.pose(SOUTH).action, Move::Hit, "the refused swing left nothing under way");
    }

    #[test]
    fn action_names_are_the_ones_of_the_protocol() {
        assert_eq!(Act::from_name("attack"), Some(Act::Attack));
        assert_eq!(Act::from_name("release_attack"), Some(Act::ReleaseAttack));
        assert_eq!(Act::from_name("prepare_throw"), Some(Act::PrepareThrow));
        assert_eq!(Act::from_name("throw"), Some(Act::Throw));
        assert_eq!(Act::from_name("drop"), Some(Act::Drop));
        assert_eq!(Act::from_name("use"), None);
    }
}
