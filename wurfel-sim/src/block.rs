//! A block is a packed `u16`: the low byte is the id, the high byte the value (variant).

/// Block ids shared with the original Java engine (`RenderCell.getName`).
pub mod id {
    pub const AIR: u8 = 0;
    pub const GRASS: u8 = 1;
    pub const DIRT: u8 = 2;
    pub const STONE: u8 = 3;
    pub const SAND: u8 = 8;
    pub const WATER: u8 = 9;
    /// A tree (Caveland's id, which the terrain generator grows too).
    pub const TREE: u8 = 72;
}

/// What a block id means for the simulation: the Java engine's `BlockConfig`. The engine knows the
/// ids 0 to 9 ([`DefaultBlocks`]); a game that defines more ids installs its own configuration with
/// [`crate::World::set_block_config`] and the physics follows it.
pub trait BlockConfig: Send + Sync {
    /// Blocks movement. Everything but air and water by default.
    fn is_obstacle(&self, block: Block) -> bool {
        !matches!(block.id(), id::AIR | id::WATER)
    }

    fn is_liquid(&self, block: Block) -> bool {
        block.id() == id::WATER
    }

    /// Lets light and sight through (the block does not hide what is behind it).
    fn is_transparent(&self, block: Block) -> bool {
        matches!(block.id(), id::AIR | id::WATER)
    }

    /// Digging cannot remove it.
    fn is_indestructible(&self, _block: Block) -> bool {
        false
    }
}

/// The engine's own blocks and nothing else.
#[derive(Debug, Clone, Copy, Default)]
pub struct DefaultBlocks;

impl BlockConfig for DefaultBlocks {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Block(u16);

impl Block {
    pub const AIR: Block = Block(0);

    pub const fn new(id: u8, value: u8) -> Self {
        Block(id as u16 | (value as u16) << 8)
    }

    pub const fn id(self) -> u8 {
        (self.0 & 0xFF) as u8
    }

    pub const fn value(self) -> u8 {
        (self.0 >> 8) as u8
    }

    pub const fn raw(self) -> u16 {
        self.0
    }

    pub const fn from_raw(raw: u16) -> Self {
        Block(raw)
    }

    pub const fn is_air(self) -> bool {
        self.id() == id::AIR
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Soft;
    impl BlockConfig for Soft {
        fn is_obstacle(&self, block: Block) -> bool {
            block.id() == 20 // only this; stone is walk-through here
        }
    }

    #[test]
    fn a_games_block_config_changes_what_physics_stands_on() {
        use crate::entity::physics::{ground_height, is_on_ground};
        use crate::{AirGenerator, World};
        use glam::Vec3;

        let mut world = World::new(AirGenerator);
        world.set(0, 0, 0, Block::new(id::STONE, 0));
        world.set(0, 0, 1, Block::new(20, 0));
        let (gx, gy) = crate::grid::to_iso(0, 0);
        let above_stone = Vec3::new(gx, gy, 1.0);
        let above_custom = Vec3::new(gx, gy, 2.0);

        // The engine's defaults: stone is solid, block 20 is solid too (everything but air and water).
        assert!(is_on_ground(&world, above_stone, 1.0));
        assert_eq!(ground_height(&world, 0, 0), 2.0);

        world.set_block_config(std::sync::Arc::new(Soft));
        assert!(!world.blocks().is_obstacle(Block::new(id::STONE, 0)));
        assert!(!is_on_ground(&world, above_stone, 1.0), "stone no longer holds");
        assert!(is_on_ground(&world, above_custom, 1.0), "block 20 does");
        assert_eq!(ground_height(&world, 0, 0), 2.0);
    }

    #[test]
    fn block_health_is_stored_per_block_in_loaded_chunks() {
        use crate::{AirGenerator, World};
        let mut world = World::new(AirGenerator);
        assert_eq!(world.block_health(3, 4, 5), 0, "not loaded");
        assert!(!world.set_block_health(3, 4, 5, 70));
        world.set(3, 4, 5, Block::new(id::STONE, 0));
        assert!(world.set_block_health(3, 4, 5, 70));
        assert_eq!(world.block_health(3, 4, 5), 70);
        assert_eq!(world.block_health(3, 4, 6), 0);
    }

    #[test]
    fn packs_id_and_value() {
        let b = Block::new(9, 3);
        assert_eq!((b.id(), b.value()), (9, 3));
        assert!(Block::AIR.is_air());
        assert!(!b.is_air());
    }
}
