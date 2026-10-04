//! Caveland's CVars (registered in `Caveland.configureEngine`) and the values the rules read.

use wurfel_sim::cvar::{CVarSystem, Flags, Value};

/// Gameplay numbers. The defaults are the Java CVar defaults; times are in seconds here (the Java
/// CVars are milliseconds, see [`register_cvars`]).
#[derive(Debug, Clone, PartialEq)]
pub struct Tuning {
    /// `godmode`: the player takes no damage.
    pub godmode: bool,
    /// `PlayerTimeTillImpact`: from swinging to hitting.
    pub time_till_impact: f32,
    /// `jetpackMaxTime`: how long the jetpack burns on one tank.
    pub jetpack_max_time: f32,
    /// `jetpackPower`: upward acceleration in blocks per second squared.
    pub jetpack_power: f32,
    /// `jetpackMaxSpeed`: the jetpack stops accelerating above this rise speed (blocks per second).
    pub jetpack_max_speed: f32,
    /// `playerfriction`.
    pub player_friction: f32,
    /// `playerWalkingSpeed`, blocks per second.
    pub walking_speed: f32,
}

impl Default for Tuning {
    fn default() -> Self {
        Tuning {
            godmode: false,
            time_till_impact: 0.15,
            jetpack_max_time: 0.4,
            jetpack_power: 30.0,
            jetpack_max_speed: 5.0,
            player_friction: wurfel_sim::player::PLAYER_FRICTION,
            walking_speed: wurfel_sim::player::WALKING_SPEED,
        }
    }
}

impl Tuning {
    /// Read the current values; a missing cvar keeps its default.
    pub fn from_cvars(cvars: &CVarSystem) -> Tuning {
        let d = Tuning::default();
        let f = |name: &str, default: f32| cvars.get_f32(name).unwrap_or(default);
        Tuning {
            godmode: cvars.get_bool("godmode").unwrap_or(d.godmode),
            time_till_impact: f("PlayerTimeTillImpact", d.time_till_impact * 1000.0) / 1000.0,
            jetpack_max_time: f("jetpackMaxTime", d.jetpack_max_time * 1000.0) / 1000.0,
            // Java: blocks per second gained per millisecond of burning.
            jetpack_power: f("jetpackPower", d.jetpack_power / 1000.0) * 1000.0,
            jetpack_max_speed: f("jetpackMaxSpeed", d.jetpack_max_speed),
            player_friction: f("playerfriction", d.player_friction),
            walking_speed: f("playerWalkingSpeed", d.walking_speed),
        }
    }
}

/// Register the Caveland CVars with their Java defaults and units, next to the engine's own.
/// `money` lives in the save slot system in Java; register it in a `CVarSystem::save_slot()`.
pub fn register_cvars(cvars: &mut CVarSystem) {
    cvars.register("PlayerTimeTillImpact", Value::Float(150.0), Flags::Archive);
    cvars.register("godmode", Value::Bool(false), Flags::Archive);
    cvars.register("jetpackMaxTime", Value::Float(400.0), Flags::Archive);
    cvars.register("jetpackPower", Value::Float(0.03), Flags::Archive);
    cvars.register("jetpackMaxSpeed", Value::Float(5.0), Flags::Archive);
}

/// Register the Caveland save-slot CVars (the player's wallet).
pub fn register_save_cvars(cvars: &mut CVarSystem) {
    cvars.register("money", Value::Int(0), Flags::Archive);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_the_registered_cvars() {
        let mut cvars = CVarSystem::root();
        register_cvars(&mut cvars);
        let t = Tuning::from_cvars(&cvars);
        let d = Tuning::default();
        assert_eq!(t.godmode, d.godmode);
        assert!((t.time_till_impact - d.time_till_impact).abs() < 1e-6);
        assert!((t.jetpack_max_time - d.jetpack_max_time).abs() < 1e-6);
        assert!((t.jetpack_power - d.jetpack_power).abs() < 1e-3);
        assert_eq!(t.jetpack_max_speed, d.jetpack_max_speed);
        assert_eq!(t.player_friction, d.player_friction);
        assert_eq!(t.walking_speed, d.walking_speed);
    }

    #[test]
    fn changed_cvars_reach_the_tuning() {
        let mut cvars = CVarSystem::root();
        register_cvars(&mut cvars);
        cvars.set("godmode", "true").unwrap();
        cvars.set("jetpackMaxTime", "1000").unwrap();
        let t = Tuning::from_cvars(&cvars);
        assert!(t.godmode);
        assert!((t.jetpack_max_time - 1.0).abs() < 1e-6);
    }

    #[test]
    fn without_registration_the_defaults_apply() {
        assert_eq!(Tuning::from_cvars(&CVarSystem::new()), Tuning::default());
    }
}
