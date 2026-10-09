//! Caveland's console commands (`GiveCommand`, `TeleportPlayerCommand`, `PortalTargetCommand`) as
//! plain functions on the game state. The console that reads the line and shows the answer is the
//! host's business: it calls [`Caveland::run_command`] for a line that starts with one of
//! [`COMMANDS`].

use wurfel_sim::entity::{Entities, EntityId};

use crate::blocks::ids;
use crate::collectible::{CollectibleType, Item};
use crate::game::{cell_floor, Caveland, Cell};

/// The commands and their manuals (`getCommandName`, `getManual`).
pub const COMMANDS: [(&str, &str); 5] = [
    ("give", "gives you a collectible\nParameters: [name of collectible]"),
    ("tpplayer", "teleports the player: <x> <y> <z> <id>"),
    ("portaltarget", "Sets the target of the portal.\nParameters: [x][y][z]"),
    ("place", "puts a finished machine down in front of you\nParameters: catapult | cannon"),
    ("forcewave", "sends a force wave out from you: it ripples the ground and shoves everything away\nParameters: [strength, 1 is a dynamite blast]"),
];

/// A command that was understood.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Command {
    Give(CollectibleType),
    /// Teleport the player with this number to a cell.
    TeleportPlayer { cell: Cell, player: u8 },
    /// Set the target of the selected portal.
    PortalTarget(Cell),
    /// Put a finished machine (a block id) down in front of the player.
    Place(u8),
    /// A force wave from the player, of this strength.
    ForceWave(f32),
}

/// What running a command did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandOutcome {
    /// Done; the text is for the console.
    Done(&'static str),
    /// The host has to apply it to the portal the user selected: it knows what is selected.
    PortalTarget(Cell),
    /// The host puts the block down where the player faces: it knows the world.
    Place(u8),
}

fn number(token: Option<&str>) -> Result<i32, String> {
    let token = token.ok_or_else(|| "a number is missing".to_string())?;
    token.parse::<i32>().map_err(|_| format!("'{token}' is not a whole number"))
}

/// Read a command line (without the leading slash or colon the console may use).
pub fn parse(line: &str) -> Result<Command, String> {
    let mut tokens = line.split_whitespace();
    let name = tokens.next().ok_or_else(|| "empty command".to_string())?;
    match name {
        "give" => {
            let item = tokens.next().ok_or_else(|| manual("give"))?;
            let kind = CollectibleType::ALL
                .into_iter()
                .find(|k| k.name().eq_ignore_ascii_case(item))
                .ok_or_else(|| "Collectible not found or game not running.".to_string())?;
            Ok(Command::Give(kind))
        }
        "tpplayer" => {
            let cell = (number(tokens.next())?, number(tokens.next())?, number(tokens.next())?);
            let player = match tokens.next() {
                Some(token) => token.parse::<u8>().map_err(|_| format!("'{token}' is not a player number"))?,
                None => 0,
            };
            Ok(Command::TeleportPlayer { cell, player })
        }
        "portaltarget" => Ok(Command::PortalTarget((number(tokens.next())?, number(tokens.next())?, number(tokens.next())?))),
        "place" => match tokens.next().map(str::to_ascii_lowercase).as_deref() {
            Some("catapult") => Ok(Command::Place(ids::CATAPULT)),
            Some("cannon") => Ok(Command::Place(ids::CANNON)),
            _ => Err(manual("place")),
        },
        "forcewave" => {
            let strength = match tokens.next() {
                Some(token) => token.parse::<f32>().ok().filter(|s| s.is_finite() && *s > 0.0 && *s <= 10.0).ok_or_else(|| manual("forcewave"))?,
                None => 1.5,
            };
            Ok(Command::ForceWave(strength))
        }
        other => Err(format!("unknown command '{other}'")),
    }
}

fn manual(name: &str) -> String {
    COMMANDS.iter().find(|(n, _)| *n == name).map(|(_, m)| (*m).to_string()).unwrap_or_default()
}

impl Caveland {
    /// Run a command line for `caller` (the player who typed it).
    pub fn run_command(&mut self, entities: &mut Entities, caller: EntityId, line: &str) -> Result<CommandOutcome, String> {
        match parse(line)? {
            Command::Give(kind) => {
                let state = self.player_mut(caller).ok_or_else(|| "Collectible not found or game not running.".to_string())?;
                if state.inventory.add(Item::new(kind)) {
                    Ok(CommandOutcome::Done("given"))
                } else {
                    Err("the inventory is full".to_string())
                }
            }
            Command::TeleportPlayer { cell, player } => {
                let target = self
                    .kinds
                    .iter()
                    .find_map(|(&id, kind)| match kind {
                        crate::game::Kind::Player(p) if p.number == player => Some(id),
                        _ => None,
                    })
                    .ok_or_else(|| format!("no player {player}"))?;
                let entity = entities.get_mut(target).ok_or_else(|| format!("player {player} is gone"))?;
                entity.position = cell_floor(cell);
                if let Some(body) = entity.body.as_mut() {
                    body.movement = glam::Vec3::ZERO;
                }
                Ok(CommandOutcome::Done("teleported"))
            }
            Command::PortalTarget(cell) => Ok(CommandOutcome::PortalTarget(cell)),
            Command::Place(block) => Ok(CommandOutcome::Place(block)),
            Command::ForceWave(strength) => {
                let at = entities.get(caller).map(|e| e.position).ok_or_else(|| "you are not in the game".to_string())?;
                self.shockwave(at, 9.0 * strength.sqrt(), strength);
                Ok(CommandOutcome::Done("force wave"))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tuning::Tuning;
    use glam::Vec3;
    use wurfel_sim::AirGenerator;
    use wurfel_sim::World;

    #[test]
    fn give_reads_the_collectible_name() {
        assert_eq!(parse("give Torch"), Ok(Command::Give(CollectibleType::Torch)));
        assert_eq!(parse("give coal"), Ok(Command::Give(CollectibleType::Coal)), "any case");
        assert!(parse("give Banana").unwrap_err().contains("not found"));
        assert!(parse("give").unwrap_err().contains("gives you a collectible"));
    }

    #[test]
    fn tpplayer_takes_a_cell_and_an_optional_player() {
        assert_eq!(parse("tpplayer 1 2 3"), Ok(Command::TeleportPlayer { cell: (1, 2, 3), player: 0 }));
        assert_eq!(parse("tpplayer -4 5 6 1"), Ok(Command::TeleportPlayer { cell: (-4, 5, 6), player: 1 }));
        assert!(parse("tpplayer 1 2").is_err());
        assert!(parse("tpplayer 1 x 3").unwrap_err().contains("'x'"));
    }

    #[test]
    fn portaltarget_takes_a_cell() {
        assert_eq!(parse("portaltarget 0 0 5"), Ok(Command::PortalTarget((0, 0, 5))));
        assert!(parse("portaltarget 0 0").is_err());
    }

    #[test]
    fn place_takes_a_launcher() {
        assert_eq!(parse("place catapult"), Ok(Command::Place(ids::CATAPULT)));
        assert_eq!(parse("place Cannon"), Ok(Command::Place(ids::CANNON)));
        assert!(parse("place").unwrap_err().contains("catapult"));
        assert!(parse("place oven").is_err());
    }

    #[test]
    fn unknown_commands_are_refused() {
        assert!(parse("dance").unwrap_err().contains("unknown command"));
        assert!(parse("   ").is_err());
    }

    #[test]
    fn every_command_has_a_manual() {
        for (name, text) in COMMANDS {
            assert!(!text.is_empty(), "{name}");
            assert!(parse(&format!("{name} 1 2 3")).is_ok() || name == "give" || name == "place");
        }
    }

    fn game() -> (Caveland, Entities, EntityId) {
        let mut world = World::new(AirGenerator);
        Caveland::install(&mut world);
        let mut caveland = Caveland::new(Tuning::default(), 1);
        let mut entities = Entities::new();
        let player = caveland.spawn_player(&mut entities, 0, Vec3::new(5.0, 5.0, 5.0));
        (caveland, entities, player)
    }

    #[test]
    fn give_puts_the_item_in_the_pack_until_it_is_full() {
        let (mut c, mut entities, player) = game();
        for _ in 0..3 {
            assert_eq!(c.run_command(&mut entities, player, "give Torch"), Ok(CommandOutcome::Done("given")));
        }
        assert_eq!(c.player(player).unwrap().inventory.count(CollectibleType::Torch), 3);
        assert!(c.run_command(&mut entities, player, "give Torch").unwrap_err().contains("full"));
    }

    #[test]
    fn tpplayer_moves_the_player_to_the_floor_of_the_cell_and_stops_them() {
        let (mut c, mut entities, player) = game();
        entities.get_mut(player).unwrap().body.as_mut().unwrap().movement = Vec3::new(3.0, 2.0, 1.0);
        assert_eq!(c.run_command(&mut entities, player, "tpplayer 7 20 4"), Ok(CommandOutcome::Done("teleported")));
        let e = entities.get(player).unwrap();
        assert_eq!(e.position, cell_floor((7, 20, 4)));
        assert_eq!(e.body.as_ref().unwrap().movement, Vec3::ZERO);
    }

    #[test]
    fn tpplayer_for_a_player_that_does_not_exist_says_so() {
        let (mut c, mut entities, player) = game();
        assert!(c.run_command(&mut entities, player, "tpplayer 1 1 1 1").unwrap_err().contains("no player 1"));
    }

    #[test]
    fn portaltarget_is_handed_to_the_host() {
        let (mut c, mut entities, player) = game();
        assert_eq!(c.run_command(&mut entities, player, "portaltarget 3 4 5"), Ok(CommandOutcome::PortalTarget((3, 4, 5))));
    }
}
