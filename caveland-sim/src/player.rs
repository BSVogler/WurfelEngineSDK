//! The Caveland player (`Ejira`): state that is not part of the engine's entity.

use glam::Vec2;
use wurfel_sim::entity::EntityId;

use crate::container::Inventory;

/// Seconds a held attack must charge before the power attack can be released (`LOADATTACKTIME`).
pub const LOAD_ATTACK_TIME: f32 = 1.0;
/// Seconds until holding the attack key counts as charging (`LOAD_THRESHOLD`).
pub const LOAD_THRESHOLD: f32 = 0.3;
/// Damage of a normal swing and of a released power attack.
pub const ATTACK_DAMAGE: u8 = 50;
pub const POWER_ATTACK_DAMAGE: u8 = 100;
/// Without damage for this long, health comes back.
pub const REGEN_DELAY: f32 = 4.0;
/// Health points per second that come back. The Java code heals `dt / 2` per frame with `dt` in
/// milliseconds, which is a few hundred points per second, i.e. an instant full heal; this is a
/// deliberate, calmer value.
pub const REGEN_PER_SECOND: f32 = 30.0;
/// Speed of the dash a normal attack gives on the ground, blocks per second.
pub const ATTACK_LUNGE: f32 = 13.0;
/// The dash of a released power attack on the ground.
pub const POWER_LUNGE: f32 = 40.0;
/// Seconds a thrown item cannot be picked up again by its thrower, and a dropped one.
pub const THROW_PICKUP_BLOCK: f32 = 0.4;
pub const DROP_PICKUP_BLOCK: f32 = 0.8;
/// Throw speed on top of the player's own movement (blocks per second).
pub const THROW_SPEED: f32 = 3.0;
/// Reach of a swing in blocks: where the hit sphere sits in front of the player and its radius.
pub const HIT_REACH: f32 = 160.0 / 141.421_36;
pub const HIT_RADIUS: f32 = 120.0 / 141.421_36;
/// Reach to a block when digging.
pub const DIG_REACH: f32 = 80.0 / 141.421_36;
/// Interactable things within this distance (blocks) of the player can be used.
pub const INTERACT_RADIUS: f32 = 2.0;

/// What a player is holding down. Directions are screen directions like the engine's
/// [`PlayerInput`](wurfel_sim::player::PlayerInput).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Controls {
    pub up: bool,
    pub down: bool,
    pub left: bool,
    pub right: bool,
    /// Jump key. Pressing it on the ground jumps; pressing it in the air fires the jetpack, which
    /// burns while the key is held.
    pub jump: bool,
}

/// One-off things a player does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Swing. Holding it charges the power attack.
    Attack,
    /// Let go of the attack key: releases a charged power attack.
    ReleaseAttack,
    /// Take the item in hand into the throwing pose.
    PrepareThrow,
    /// Throw the item in hand.
    Throw,
    /// Put the item in hand down.
    Drop,
    /// Use the item in hand (light explosives, place a torch).
    UseItem,
    /// Use the nearest machine.
    Interact,
    /// Rotate the inventory slots.
    SwitchItems { left: bool },
    /// Craft the n-th recipe of the fixed list [`crate::crafting::recipes`] (out of range is ignored).
    Craft(usize),
    /// Answer the open dialog (an NPC's line, a construction site, a shop...): the option's id. A
    /// plain "next" or "yes" is 1, "no" is 0; see [`crate::dialog::Dialog`].
    Choose(u8),
    /// Close the open dialog without choosing.
    Cancel,
}

/// Everything about a player that the engine's [`Entity`](wurfel_sim::entity::Entity) does not hold.
#[derive(Debug, Clone)]
pub struct PlayerState {
    /// Which player (local co-op number in Java, 0 for the first).
    pub number: u8,
    pub inventory: Inventory,
    /// Seconds since the last damage.
    pub time_since_damage: f32,
    /// Seconds of jetpack fuel left.
    pub jetpack_time: f32,
    pub jetpack_on: bool,
    /// Seconds the attack key has been held, `None` when not charging.
    pub load_attack: Option<f32>,
    /// Seconds until a swing connects.
    pub time_till_impact: Option<f32>,
    pub attack_damage: u8,
    pub performing_power_attack: bool,
    pub used_load_attack_in_air: bool,
    pub prepare_throw: bool,
    /// Seconds the throw button has been held; long enough and the item is dropped instead.
    pub throw_held: Option<f32>,
    /// The next jump works even in mid-air (set by bouncing off something).
    pub bunny_hop_forced: bool,
    /// Where the character looks and swings; turns smoothly towards the walking direction.
    pub aim: Vec2,
    /// The key state of the previous step, to find presses and releases.
    pub(crate) last_controls: Controls,
    pub(crate) entity: EntityId,
}

impl PlayerState {
    pub(crate) fn new(entity: EntityId, number: u8, jetpack_fuel: f32, facing: Vec2) -> Self {
        PlayerState {
            number,
            inventory: Inventory::new(),
            time_since_damage: REGEN_DELAY + 1.0,
            jetpack_time: jetpack_fuel,
            jetpack_on: false,
            load_attack: None,
            time_till_impact: None,
            attack_damage: ATTACK_DAMAGE,
            performing_power_attack: false,
            used_load_attack_in_air: false,
            prepare_throw: false,
            throw_held: None,
            bunny_hop_forced: false,
            aim: facing,
            last_controls: Controls::default(),
            entity,
        }
    }

    pub fn entity(&self) -> EntityId {
        self.entity
    }

    /// Is the player busy in a way that stops walking?
    pub fn movement_locked(&self) -> bool {
        self.load_attack.is_some() || self.performing_power_attack || self.prepare_throw
    }
}

/// Turn `start` towards `end` by the fraction `percent` of the angle between them (both unit
/// vectors). The Java `slerp`, without mutating its arguments.
pub fn slerp(start: Vec2, end: Vec2, percent: f32) -> Vec2 {
    let dot = start.dot(end).clamp(-1.0, 1.0);
    let theta = dot.acos() * percent;
    let rel = (end - start * dot).normalize_or_zero();
    if rel == Vec2::ZERO {
        return start; // already facing the same (or exactly the opposite) way: nothing to turn
    }
    (start * theta.cos() + rel * theta.sin()).normalize_or_zero()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slerp_turns_by_the_given_fraction() {
        let a = Vec2::X;
        let b = Vec2::Y;
        let half = slerp(a, b, 0.5);
        assert!((half.length() - 1.0).abs() < 1e-5);
        assert!((half.angle_to(a).abs() - std::f32::consts::FRAC_PI_4).abs() < 1e-4, "{half:?}");
        assert_eq!(slerp(a, b, 0.0), a);
        let full = slerp(a, b, 1.0);
        assert!((full - b).length() < 1e-5);
    }

    #[test]
    fn slerp_without_a_direction_to_turn_to_keeps_the_start() {
        assert_eq!(slerp(Vec2::X, Vec2::X, 0.5), Vec2::X);
        assert_eq!(slerp(Vec2::X, -Vec2::X, 0.5), Vec2::X);
    }

    #[test]
    fn movement_is_locked_while_charging_or_throwing() {
        let mut p = PlayerState::new(1, 0, 0.4, Vec2::X);
        assert!(!p.movement_locked());
        p.load_attack = Some(0.0);
        assert!(p.movement_locked());
        p.load_attack = None;
        p.prepare_throw = true;
        assert!(p.movement_locked());
    }
}
