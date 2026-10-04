//! Block system - the basic building blocks of the world

use crate::map::Renderable;
use bytemuck::{Pod, Zeroable};

/// A block represents a single voxel in the world
/// Uses a packed representation similar to the Java version
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Pod, Zeroable)]
#[repr(transparent)]
pub struct Block(u16);

impl Block {
    /// Air block (empty space)
    pub const AIR: Block = Block(0);
    
    /// Create a new block with ID and value
    pub fn new(id: u8, value: u8) -> Self {
        Block((id as u16) | ((value as u16) << 8))
    }
    
    /// Get the block ID (type)
    pub fn id(&self) -> u8 {
        (self.0 & 0xFF) as u8
    }
    
    /// Get the block value (variant/metadata)
    pub fn value(&self) -> u8 {
        ((self.0 >> 8) & 0xFF) as u8
    }
    
    /// Check if this is an air block
    pub fn is_air(&self) -> bool {
        self.id() == 0
    }
    
    /// Check if this block is solid (blocks movement/light)
    pub fn is_solid(&self) -> bool {
        !self.is_air()
    }
    
    /// Check if this block is transparent (allows light through)
    pub fn is_transparent(&self) -> bool {
        self.is_air() // For now, only air is transparent
    }
}

impl Default for Block {
    fn default() -> Self {
        Self::AIR
    }
}

impl Renderable for Block {
    fn sprite_id(&self) -> u8 {
        self.id()
    }
    
    fn sprite_value(&self) -> u8 {
        self.value()
    }
    
    fn is_visible(&self) -> bool {
        !self.is_air()
    }
}

impl From<u16> for Block {
    fn from(data: u16) -> Self {
        Block(data)
    }
}

impl From<Block> for u16 {
    fn from(block: Block) -> Self {
        block.0
    }
}

/// Block configuration and properties
#[derive(Debug, Clone)]
pub struct BlockConfig {
    /// Block ID
    pub id: u8,
    /// Display name
    pub name: String,
    /// Is this block solid?
    pub solid: bool,
    /// Is this block transparent?
    pub transparent: bool,
    /// Light level emitted (0-15)
    pub light_emission: u8,
    /// Light absorption (0-15)
    pub light_absorption: u8,
}

impl BlockConfig {
    pub fn new(id: u8, name: &str) -> Self {
        Self {
            id,
            name: name.to_string(),
            solid: true,
            transparent: false,
            light_emission: 0,
            light_absorption: 15,
        }
    }
    
    pub fn solid(mut self, solid: bool) -> Self {
        self.solid = solid;
        self
    }
    
    pub fn transparent(mut self, transparent: bool) -> Self {
        self.transparent = transparent;
        self
    }
    
    pub fn light_emission(mut self, emission: u8) -> Self {
        self.light_emission = emission.min(15);
        self
    }
    
    pub fn light_absorption(mut self, absorption: u8) -> Self {
        self.light_absorption = absorption.min(15);
        self
    }
}

/// Block registry to manage all block types
#[derive(Debug)]
pub struct BlockRegistry {
    configs: Vec<Option<BlockConfig>>,
}

impl BlockRegistry {
    pub fn new() -> Self {
        let mut configs = vec![None; crate::constants::OBJECT_TYPES_NUM];
        
        // Register air block
        configs[0] = Some(BlockConfig::new(0, "Air")
            .solid(false)
            .transparent(true)
            .light_absorption(0));
        
        Self { configs }
    }
    
    pub fn register(&mut self, config: BlockConfig) {
        let id = config.id;
        if (id as usize) < self.configs.len() {
            self.configs[id as usize] = Some(config);
        }
    }
    
    pub fn get(&self, id: u8) -> Option<&BlockConfig> {
        self.configs.get(id as usize).and_then(|c| c.as_ref())
    }
    
    pub fn is_solid(&self, block: Block) -> bool {
        self.get(block.id()).map_or(false, |c| c.solid)
    }
    
    pub fn is_transparent(&self, block: Block) -> bool {
        self.get(block.id()).map_or(true, |c| c.transparent)
    }
}

impl Default for BlockRegistry {
    fn default() -> Self {
        Self::new()
    }
}