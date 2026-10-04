//! Chunk system for organizing the world into manageable sections

use crate::constants::*;
use crate::map::{Block, Coordinate};
use crate::utils::Pool;
use std::sync::Arc;

/// A chunk contains a 3D array of blocks
#[derive(Debug, Clone)]
pub struct Chunk {
    /// Chunk position in chunk coordinates
    pub position: (i32, i32),
    /// 3D array of blocks [x][y][z]
    blocks: Box<[[[Block; CHUNK_SIZE_Z]; CHUNK_SIZE_Y]; CHUNK_SIZE_X]>,
    /// Whether this chunk has been modified and needs saving
    dirty: bool,
    /// Whether this chunk is currently being rendered
    render_dirty: bool,
}

impl Chunk {
    /// Create a new empty chunk (filled with air)
    pub fn new(position: (i32, i32)) -> Self {
        Self {
            position,
            blocks: Box::new([[[Block::AIR; CHUNK_SIZE_Z]; CHUNK_SIZE_Y]; CHUNK_SIZE_X]),
            dirty: false,
            render_dirty: true,
        }
    }
    
    /// Get the top-left world coordinate of this chunk
    pub fn top_left_coordinate(&self) -> Coordinate {
        Coordinate::new_with_coords(
            self.position.0 * CHUNK_SIZE_X as i32,
            self.position.1 * CHUNK_SIZE_Y as i32,
            0
        )
    }
    
    /// Get a block by local chunk coordinates
    pub fn get_block(&self, x: usize, y: usize, z: usize) -> Option<Block> {
        if x < CHUNK_SIZE_X && y < CHUNK_SIZE_Y && z < CHUNK_SIZE_Z {
            Some(self.blocks[x][y][z])
        } else {
            None
        }
    }
    
    /// Set a block by local chunk coordinates
    pub fn set_block(&mut self, x: usize, y: usize, z: usize, block: Block) -> bool {
        if x < CHUNK_SIZE_X && y < CHUNK_SIZE_Y && z < CHUNK_SIZE_Z {
            if self.blocks[x][y][z] != block {
                self.blocks[x][y][z] = block;
                self.dirty = true;
                self.render_dirty = true;
            }
            true
        } else {
            false
        }
    }
    
    /// Get a block by world coordinate (if it's within this chunk)
    pub fn get_block_at(&self, coord: &Coordinate) -> Option<Block> {
        let (chunk_x, chunk_y) = coord.to_chunk();
        if chunk_x != self.position.0 || chunk_y != self.position.1 {
            return None;
        }
        
        let (local_x, local_y, local_z) = coord.to_local();
        self.get_block(local_x, local_y, local_z)
    }
    
    /// Set a block by world coordinate (if it's within this chunk)
    pub fn set_block_at(&mut self, coord: &Coordinate, block: Block) -> bool {
        let (chunk_x, chunk_y) = coord.to_chunk();
        if chunk_x != self.position.0 || chunk_y != self.position.1 {
            return false;
        }
        
        let (local_x, local_y, local_z) = coord.to_local();
        if local_z >= CHUNK_SIZE_Z {
            return false;
        }
        
        self.set_block(local_x, local_y, local_z, block)
    }
    
    /// Fill the chunk with a specific block
    pub fn fill(&mut self, block: Block) {
        for x in 0..CHUNK_SIZE_X {
            for y in 0..CHUNK_SIZE_Y {
                for z in 0..CHUNK_SIZE_Z {
                    self.blocks[x][y][z] = block;
                }
            }
        }
        self.dirty = true;
        self.render_dirty = true;
    }
    
    /// Check if the chunk has been modified
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }
    
    /// Mark the chunk as saved
    pub fn mark_saved(&mut self) {
        self.dirty = false;
    }
    
    /// Check if the chunk needs re-rendering
    pub fn is_render_dirty(&self) -> bool {
        self.render_dirty
    }
    
    /// Mark the chunk as rendered
    pub fn mark_rendered(&mut self) {
        self.render_dirty = false;
    }
    
    /// Get an iterator over all blocks in the chunk
    pub fn blocks(&self) -> ChunkBlockIterator {
        ChunkBlockIterator::new(self)
    }
    
    /// Get the raw block data (for serialization or bulk operations)
    pub fn raw_blocks(&self) -> &[[[Block; CHUNK_SIZE_Z]; CHUNK_SIZE_Y]; CHUNK_SIZE_X] {
        &self.blocks
    }
}

/// Iterator over all blocks in a chunk with their local coordinates
pub struct ChunkBlockIterator<'a> {
    chunk: &'a Chunk,
    x: usize,
    y: usize,
    z: usize,
}

impl<'a> ChunkBlockIterator<'a> {
    fn new(chunk: &'a Chunk) -> Self {
        Self { chunk, x: 0, y: 0, z: 0 }
    }
}

impl<'a> Iterator for ChunkBlockIterator<'a> {
    type Item = ((usize, usize, usize), Block);
    
    fn next(&mut self) -> Option<Self::Item> {
        if self.x >= CHUNK_SIZE_X {
            return None;
        }
        
        let coord = (self.x, self.y, self.z);
        let block = self.chunk.blocks[self.x][self.y][self.z];
        
        // Advance to next position
        self.z += 1;
        if self.z >= CHUNK_SIZE_Z {
            self.z = 0;
            self.y += 1;
            if self.y >= CHUNK_SIZE_Y {
                self.y = 0;
                self.x += 1;
            }
        }
        
        Some((coord, block))
    }
}

/// Chunk manager for loading, unloading, and caching chunks
#[derive(Debug)]
pub struct ChunkManager {
    chunks: std::collections::HashMap<(i32, i32), Arc<std::sync::RwLock<Chunk>>>,
    chunk_pool: Pool<Chunk>,
}

impl ChunkManager {
    pub fn new() -> Self {
        Self {
            chunks: std::collections::HashMap::new(),
            chunk_pool: Pool::new(|| Chunk::new((0, 0)), 16),
        }
    }
    
    /// Get or create a chunk at the given position
    pub fn get_chunk(&mut self, position: (i32, i32)) -> Arc<std::sync::RwLock<Chunk>> {
        self.chunks.entry(position)
            .or_insert_with(|| {
                let mut chunk = self.chunk_pool.get();
                chunk.position = position;
                Arc::new(std::sync::RwLock::new(chunk))
            })
            .clone()
    }
    
    /// Unload a chunk (moves it back to the pool if possible)
    pub fn unload_chunk(&mut self, position: (i32, i32)) {
        if let Some(chunk_arc) = self.chunks.remove(&position) {
            if let Ok(chunk) = Arc::try_unwrap(chunk_arc) {
                let chunk = chunk.into_inner().unwrap();
                if !chunk.is_dirty() {
                    self.chunk_pool.put(chunk);
                }
            }
        }
    }
    
    /// Get block at world coordinate, loading chunk if necessary
    pub fn get_block_at(&mut self, coord: &Coordinate) -> Option<Block> {
        let (chunk_x, chunk_y) = coord.to_chunk();
        let chunk = self.get_chunk((chunk_x, chunk_y));
        let result = chunk.read().unwrap().get_block_at(coord);
        result
    }
    
    /// Set block at world coordinate, loading chunk if necessary
    pub fn set_block_at(&mut self, coord: &Coordinate, block: Block) -> bool {
        let (chunk_x, chunk_y) = coord.to_chunk();
        let chunk = self.get_chunk((chunk_x, chunk_y));
        let result = chunk.write().unwrap().set_block_at(coord, block);
        result
    }
}

impl Default for ChunkManager {
    fn default() -> Self {
        Self::new()
    }
}