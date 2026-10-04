//! World generators for creating procedural content

use crate::map::{Block, Chunk};
use crate::constants::*;

/// Trait for world generators
pub trait Generator {
    /// Generate blocks for a chunk
    fn generate(&self, chunk: &mut Chunk);
    
    /// Get the generator's name
    fn name(&self) -> &str;
}

/// Simple air generator - fills chunks with air (empty space)
#[derive(Debug, Default)]
pub struct AirGenerator;

impl Generator for AirGenerator {
    fn generate(&self, chunk: &mut Chunk) {
        chunk.fill(Block::AIR);
    }
    
    fn name(&self) -> &str {
        "Air Generator"
    }
}

/// Basic terrain generator similar to the Java IslandGenerator
#[derive(Debug)]
pub struct IslandGenerator {
    /// Height of the terrain base
    pub base_height: usize,
    /// Random seed
    pub seed: u64,
}

impl IslandGenerator {
    pub fn new(seed: u64) -> Self {
        Self {
            base_height: 5,
            seed,
        }
    }
    
    pub fn with_base_height(mut self, height: usize) -> Self {
        self.base_height = height.min(CHUNK_SIZE_Z - 1);
        self
    }
    
    /// Simple noise function for terrain generation
    fn noise(&self, x: i32, y: i32) -> f32 {
        let mut hash = (x as u64).wrapping_mul(374761393)
            .wrapping_add((y as u64).wrapping_mul(668265263))
            .wrapping_add(self.seed);
        hash = hash.wrapping_mul(1274126177);
        hash ^= hash >> 16;
        hash = hash.wrapping_mul(85734735);
        hash ^= hash >> 13;
        hash = hash.wrapping_mul(658871167);
        hash ^= hash >> 16;
        
        (hash as f32 / u64::MAX as f32) * 2.0 - 1.0
    }
    
    /// Calculate height at given world coordinates
    fn calculate_height(&self, world_x: i32, world_y: i32) -> usize {
        let noise_value = self.noise(world_x / 4, world_y / 4) * 3.0;
        let height = self.base_height as f32 + noise_value;
        height.max(0.0) as usize
    }
}

impl Generator for IslandGenerator {
    fn generate(&self, chunk: &mut Chunk) {
        let top_left = chunk.top_left_coordinate();
        
        for x in 0..CHUNK_SIZE_X {
            for y in 0..CHUNK_SIZE_Y {
                let world_x = top_left.x + x as i32;
                let world_y = top_left.y + y as i32;
                let height = self.calculate_height(world_x, world_y);
                
                for z in 0..CHUNK_SIZE_Z {
                    let block = if z < height {
                        if z < height - 1 {
                            Block::new(1, 0) // Stone
                        } else {
                            Block::new(2, 0) // Grass
                        }
                    } else {
                        Block::AIR
                    };
                    
                    chunk.set_block(x, y, z, block);
                }
            }
        }
    }
    
    fn name(&self) -> &str {
        "Island Generator"
    }
}

/// Test generator that creates a simple pattern for debugging
#[derive(Debug, Default)]
pub struct TestGenerator;

impl Generator for TestGenerator {
    fn generate(&self, chunk: &mut Chunk) {
        // Create a simple checkerboard pattern at ground level
        for x in 0..CHUNK_SIZE_X {
            for y in 0..CHUNK_SIZE_Y {
                let block = if (x + y) % 2 == 0 {
                    Block::new(1, 0) // Stone
                } else {
                    Block::new(8, 0) // Different block type
                };
                chunk.set_block(x, y, 0, block);
            }
        }
    }
    
    fn name(&self) -> &str {
        "Test Generator"
    }
}