//! Teams (`HasTeam`): who fights whom.

/// Java ids: 0 neutral, 1 robots (enemies), 2 player.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Team {
    #[default]
    Neutral,
    Robots,
    Player,
}

impl Team {
    pub fn id(self) -> u8 {
        match self {
            Team::Neutral => 0,
            Team::Robots => 1,
            Team::Player => 2,
        }
    }

    pub fn from_id(id: u8) -> Team {
        match id {
            1 => Team::Robots,
            2 => Team::Player,
            _ => Team::Neutral,
        }
    }

    /// Would members of the two teams attack each other? Neutral robots only defend themselves
    /// and never pick a fight.
    pub fn is_hostile_to(self, other: Team) -> bool {
        self != Team::Neutral && self != other
    }
}

/// Tint of an enemy and of a friend (`HasTeam.COLORENEMY`, `COLORTEAM`), RGBA.
pub const COLOR_ENEMY: [f32; 4] = [0.25, 0.5, 0.25, 1.0];
pub const COLOR_TEAM: [f32; 4] = [0.2, 0.5, 0.95, 1.0];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_match_java() {
        for t in [Team::Neutral, Team::Robots, Team::Player] {
            assert_eq!(Team::from_id(t.id()), t);
        }
        assert_eq!(Team::from_id(9), Team::Neutral);
    }

    #[test]
    fn hostility() {
        assert!(Team::Robots.is_hostile_to(Team::Player));
        assert!(Team::Player.is_hostile_to(Team::Robots));
        assert!(!Team::Robots.is_hostile_to(Team::Robots));
        assert!(!Team::Neutral.is_hostile_to(Team::Player));
        assert!(Team::Robots.is_hostile_to(Team::Neutral), "enemies attack neutral things too");
    }
}
