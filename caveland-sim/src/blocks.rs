//! Caveland's block definitions (`CavelandBlocks`), installed into a world as a
//! [`BlockConfig`](wurfel_sim::block::BlockConfig).

use wurfel_sim::block::{Block, BlockConfig, DefaultBlocks};

use crate::collectible::CollectibleType;

/// Block ids. They live in the engine crate next to the Caveland map generator, which has to place
/// them; this is the single list.
pub use wurfel_sim::caveland::blocks as ids;

/// What a block with behaviour does (the Java `AbstractBlockLogicExtension` subclasses). Only
/// [`LogicKind::Oven`] is ported so far, see the crate docs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogicKind {
    ConstructionSite,
    Oven,
    Torch,
    PowerStation,
    Lift,
    CaveEntry,
    LiftGround,
    Turret,
    RobotFactory,
    PowerCable,
    BoosterRails,
    FlagPole,
    Catapult,
    Cannon,
}

/// The Java `CLBlocks` enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ClBlock {
    ConstructionSite,
    Oven,
    Torch,
    PowerStation,
    Lift,
    Entry,
    IndestructibleObstacle,
    LiftGround,
    Crystal,
    Sulfur,
    IronOre,
    Coal,
    Turret,
    RobotFactory,
    PowerCable,
    Rails,
    BoosterRails,
    FlagPole,
    Catapult,
    Cannon,
    Tree,
    /// Everything else, including the engine's blocks 0 to 9 (Java quirk: `valueOf` does not know them).
    Undefined,
}

impl ClBlock {
    pub const ALL: [ClBlock; 21] = [
        ClBlock::ConstructionSite,
        ClBlock::Oven,
        ClBlock::Torch,
        ClBlock::PowerStation,
        ClBlock::Lift,
        ClBlock::Entry,
        ClBlock::IndestructibleObstacle,
        ClBlock::LiftGround,
        ClBlock::Crystal,
        ClBlock::Sulfur,
        ClBlock::IronOre,
        ClBlock::Coal,
        ClBlock::Turret,
        ClBlock::RobotFactory,
        ClBlock::PowerCable,
        ClBlock::Rails,
        ClBlock::BoosterRails,
        ClBlock::FlagPole,
        ClBlock::Catapult,
        ClBlock::Cannon,
        ClBlock::Tree,
    ];

    pub fn id(self) -> u8 {
        match self {
            ClBlock::ConstructionSite => ids::CONSTRUCTION_SITE,
            ClBlock::Oven => ids::OVEN,
            ClBlock::Torch => ids::TORCH,
            ClBlock::PowerStation => ids::POWER_STATION,
            ClBlock::Lift => ids::LIFT,
            ClBlock::Entry => ids::ENTRY,
            ClBlock::IndestructibleObstacle => ids::INDESTRUCTIBLE_OBSTACLE,
            ClBlock::LiftGround => ids::LIFT_GROUND,
            ClBlock::Crystal => ids::CRYSTAL,
            ClBlock::Sulfur => ids::SULFUR,
            ClBlock::IronOre => ids::IRON_ORE,
            ClBlock::Coal => ids::COAL,
            ClBlock::Turret => ids::TURRET,
            ClBlock::RobotFactory => ids::ROBOT_FACTORY,
            ClBlock::PowerCable => ids::POWER_CABLE,
            ClBlock::Rails => ids::RAILS,
            ClBlock::BoosterRails => ids::BOOSTER_RAILS,
            ClBlock::FlagPole => ids::FLAG_POLE,
            ClBlock::Catapult => ids::CATAPULT,
            ClBlock::Cannon => ids::CANNON,
            ClBlock::Tree => ids::TREE,
            ClBlock::Undefined => ids::UNDEFINED,
        }
    }

    /// Reverse lookup (`CLBlocks.valueOf(byte)`).
    pub fn from_id(id: u8) -> ClBlock {
        ClBlock::ALL.into_iter().find(|b| b.id() == id).unwrap_or(ClBlock::Undefined)
    }

    pub fn name(self) -> &'static str {
        match self {
            ClBlock::ConstructionSite => "Construction Site",
            ClBlock::Oven => "Oven",
            ClBlock::Torch => "Torch",
            ClBlock::PowerStation => "Power Station",
            ClBlock::Lift => "Lift",
            ClBlock::Entry => "Cave Entry",
            ClBlock::IndestructibleObstacle => "Indestructible Obstacle",
            ClBlock::LiftGround => "Lift (Ground)",
            ClBlock::Crystal => "Crystal Block",
            ClBlock::Sulfur => "Sulfur Block",
            ClBlock::IronOre => "Iron Ore Block",
            ClBlock::Coal => "Coal Block",
            ClBlock::Turret => "Turret",
            ClBlock::RobotFactory => "robot factory",
            ClBlock::PowerCable => "power cable",
            ClBlock::Rails => "rails",
            ClBlock::BoosterRails => "booster rails",
            ClBlock::FlagPole => "flag pole",
            ClBlock::Catapult => "Catapult",
            ClBlock::Cannon => "Cannon",
            ClBlock::Tree => "tree",
            ClBlock::Undefined => "undefined",
        }
    }

    /// Rendered with sides (as a cube) rather than as a sprite.
    pub fn has_sides(self) -> bool {
        matches!(
            self,
            ClBlock::Entry
                | ClBlock::LiftGround
                | ClBlock::Crystal
                | ClBlock::Sulfur
                | ClBlock::IronOre
                | ClBlock::Coal
                | ClBlock::Undefined
        )
    }

    pub fn logic(self) -> Option<LogicKind> {
        Some(match self {
            ClBlock::ConstructionSite => LogicKind::ConstructionSite,
            ClBlock::Oven => LogicKind::Oven,
            ClBlock::Torch => LogicKind::Torch,
            ClBlock::PowerStation => LogicKind::PowerStation,
            ClBlock::Lift => LogicKind::Lift,
            ClBlock::Entry => LogicKind::CaveEntry,
            ClBlock::LiftGround => LogicKind::LiftGround,
            ClBlock::Turret => LogicKind::Turret,
            ClBlock::RobotFactory => LogicKind::RobotFactory,
            ClBlock::PowerCable => LogicKind::PowerCable,
            ClBlock::BoosterRails => LogicKind::BoosterRails,
            ClBlock::FlagPole => LogicKind::FlagPole,
            ClBlock::Catapult => LogicKind::Catapult,
            ClBlock::Cannon => LogicKind::Cannon,
            _ => return None,
        })
    }
}

/// Digging cannot break it (`hardMaterial`); the player's attack only makes dust.
pub fn hard_material(id: u8) -> bool {
    matches!(
        id,
        ids::INDESTRUCTIBLE_OBSTACLE | ids::CRYSTAL | ids::IRON_ORE | ids::FLAG_POLE | ids::STONE
    )
}

/// What breaking a block drops (`getLoot`).
pub fn loot(id: u8) -> Option<CollectibleType> {
    match id {
        ids::STONE => Some(CollectibleType::Stone),
        ids::CRYSTAL => Some(CollectibleType::Cristall),
        ids::SULFUR => Some(CollectibleType::Sulfur),
        ids::IRON_ORE => Some(CollectibleType::Ironore),
        ids::COAL => Some(CollectibleType::Coal),
        ids::TREE => Some(CollectibleType::Wood),
        _ => None,
    }
}

/// The block rules of Caveland. Blocks below id 10 are the engine's and follow its defaults.
#[derive(Debug, Clone, Copy, Default)]
pub struct CavelandBlocks;

impl BlockConfig for CavelandBlocks {
    fn is_obstacle(&self, block: Block) -> bool {
        let (id, value) = (block.id(), block.value());
        if id < 10 {
            return DefaultBlocks.is_obstacle(block);
        }
        match id {
            ids::OVEN | ids::POWER_STATION | ids::INDESTRUCTIBLE_OBSTACLE | ids::LIFT_GROUND => true,
            ids::ENTRY => value == 1,
            ids::CRYSTAL | ids::SULFUR | ids::IRON_ORE | ids::COAL => true,
            ids::TREE | 46 | ids::TURRET | ids::FLAG_POLE | ids::CATAPULT | ids::CANNON => true,
            _ => false,
        }
    }

    fn is_transparent(&self, block: Block) -> bool {
        let (id, value) = (block.id(), block.value());
        if id < 10 {
            return DefaultBlocks.is_transparent(block);
        }
        match id {
            ids::INDESTRUCTIBLE_OBSTACLE => value == 0,
            ids::OVEN
            | ids::TORCH
            | ids::POWER_STATION
            | ids::LIFT
            | ids::CONSTRUCTION_SITE
            | ids::TURRET
            | ids::ROBOT_FACTORY
            | ids::POWER_CABLE
            | ids::RAILS
            | ids::BOOSTER_RAILS
            | ids::FLAG_POLE
            | ids::CATAPULT
            | ids::CANNON
            | ids::TREE => true,
            _ => false,
        }
    }

    /// Java quirk, kept: Caveland has no liquids at all, not even the engine's water.
    fn is_liquid(&self, _block: Block) -> bool {
        false
    }

    fn is_indestructible(&self, block: Block) -> bool {
        block.id() == ids::INDESTRUCTIBLE_OBSTACLE
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn b(id: u8) -> Block {
        Block::new(id, 0)
    }

    #[test]
    fn ids_round_trip_and_unknown_is_undefined() {
        for block in ClBlock::ALL {
            assert_eq!(ClBlock::from_id(block.id()), block);
        }
        assert_eq!(ClBlock::from_id(2), ClBlock::Undefined);
        assert_eq!(ClBlock::from_id(200), ClBlock::Undefined);
        assert_eq!(ClBlock::Undefined.id(), 255, "Java's byte -1");
    }

    #[test]
    fn what_blocks_movement() {
        let c = CavelandBlocks;
        // Engine blocks follow the engine's rules.
        assert!(!c.is_obstacle(b(ids::AIR)));
        assert!(!c.is_obstacle(b(ids::WATER)));
        assert!(c.is_obstacle(b(ids::STONE)));
        assert!(c.is_obstacle(b(ids::INVISIBLE_OBSTACLE)), "the unloaded-chunk barrier must stay solid");
        // Machines and ores.
        assert!(c.is_obstacle(b(ids::OVEN)) && c.is_obstacle(b(ids::COAL)) && c.is_obstacle(b(ids::TREE)));
        // Things you walk through.
        for id in [ids::TORCH, ids::CONSTRUCTION_SITE, ids::LIFT, ids::RAILS, ids::BOOSTER_RAILS, ids::POWER_CABLE, ids::ROBOT_FACTORY] {
            assert!(!c.is_obstacle(b(id)), "id {id}");
        }
        // The cave entry only blocks when its value is 1.
        assert!(!c.is_obstacle(Block::new(ids::ENTRY, 0)));
        assert!(c.is_obstacle(Block::new(ids::ENTRY, 1)));
    }

    #[test]
    fn indestructible_obstacle_is_see_through_only_with_value_zero() {
        let c = CavelandBlocks;
        assert!(c.is_transparent(Block::new(ids::INDESTRUCTIBLE_OBSTACLE, 0)));
        assert!(!c.is_transparent(Block::new(ids::INDESTRUCTIBLE_OBSTACLE, 1)));
        assert!(c.is_indestructible(b(ids::INDESTRUCTIBLE_OBSTACLE)));
        assert!(!c.is_indestructible(b(ids::STONE)));
    }

    #[test]
    fn digging_rules() {
        assert!(hard_material(ids::STONE) && hard_material(ids::CRYSTAL) && hard_material(ids::IRON_ORE));
        assert!(!hard_material(ids::COAL) && !hard_material(ids::DIRT) && !hard_material(ids::TREE));
        assert_eq!(loot(ids::COAL), Some(CollectibleType::Coal));
        assert_eq!(loot(ids::TREE), Some(CollectibleType::Wood));
        assert_eq!(loot(ids::DIRT), None);
    }

    #[test]
    fn only_logic_blocks_have_logic() {
        assert_eq!(ClBlock::Oven.logic(), Some(LogicKind::Oven));
        assert_eq!(ClBlock::Coal.logic(), None);
        assert_eq!(ClBlock::Rails.logic(), None);
        assert_eq!(ClBlock::BoosterRails.logic(), Some(LogicKind::BoosterRails));
    }
}
