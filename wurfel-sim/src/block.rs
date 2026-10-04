//! A block is a packed `u16`: the low byte is the id, the high byte the value (variant).

/// Block ids shared with the original Java engine (`RenderCell.getName`).
pub mod id {
    pub const AIR: u8 = 0;
    pub const GRASS: u8 = 1;
    pub const DIRT: u8 = 2;
    pub const STONE: u8 = 3;
    pub const SAND: u8 = 8;
    pub const WATER: u8 = 9;
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

    #[test]
    fn packs_id_and_value() {
        let b = Block::new(9, 3);
        assert_eq!((b.id(), b.value()), (9, 3));
        assert!(Block::AIR.is_air());
        assert!(!b.is_air());
    }
}
