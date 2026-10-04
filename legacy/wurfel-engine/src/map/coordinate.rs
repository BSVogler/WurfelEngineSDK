//! 3D coordinate system for the map

use crate::math::{Vec2, Vec3};
use crate::map::Position;
use serde::{Deserialize, Serialize};

/// A coordinate represents a specific cell position in the 3D world
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Coordinate {
    /// The x coordinate (left to right)
    pub x: i32,
    /// The y coordinate (back to front) 
    pub y: i32,
    /// The z coordinate (ground to sky)
    pub z: i32,
}

impl Coordinate {
    /// Create a new coordinate at origin
    pub fn new() -> Self {
        Self { x: 0, y: 0, z: 0 }
    }
    
    /// Create a coordinate with specific values
    pub fn new_with_coords(x: i32, y: i32, z: i32) -> Self {
        Self { x, y, z }
    }
    
    /// Convert to chunk coordinates
    pub fn to_chunk(&self) -> (i32, i32) {
        let chunk_x = self.x / crate::constants::CHUNK_SIZE_X as i32;
        let chunk_y = self.y / crate::constants::CHUNK_SIZE_Y as i32;
        (chunk_x, chunk_y)
    }
    
    /// Convert to local coordinates within a chunk
    pub fn to_local(&self) -> (usize, usize, usize) {
        let local_x = (self.x % crate::constants::CHUNK_SIZE_X as i32) as usize;
        let local_y = (self.y % crate::constants::CHUNK_SIZE_Y as i32) as usize;
        let local_z = self.z as usize;
        (local_x, local_y, local_z)
    }
    
    /// Convert coordinate to view space (authentic Wurfel Engine logic)
    pub fn to_view_space(&self) -> Vec2 {
        use crate::constants::*;
        
        // Authentic Java conversion: getViewSpcX() and getViewSpcY()
        let view_spc_x = self.x * VIEW_WIDTH + 
            if self.y % 2 != 0 { VIEW_WIDTH2 } else { 0 };
        
        let view_spc_y = -self.y * VIEW_DEPTH2 + self.z * VIEW_HEIGHT;
        
        Vec2::new(view_spc_x as f32, view_spc_y as f32)
    }
    
    /// Convert to screen coordinates using isometric projection (backward compatibility)
    pub fn to_screen(&self) -> Vec2 {
        self.to_view_space()
    }
    
    /// Calculate depth for rendering order (authentic Wurfel Engine logic)
    pub fn depth(&self) -> f32 {
        // In isometric view, objects further back (higher y) and higher up (higher z) should be rendered first
        // This matches the original Java depth sorting
        -(self.x + self.y + self.z) as f32
    }
    
    /// Get neighboring coordinates
    pub fn neighbors(&self) -> [Coordinate; 6] {
        [
            Coordinate::new_with_coords(self.x + 1, self.y, self.z),
            Coordinate::new_with_coords(self.x - 1, self.y, self.z),
            Coordinate::new_with_coords(self.x, self.y + 1, self.z),
            Coordinate::new_with_coords(self.x, self.y - 1, self.z),
            Coordinate::new_with_coords(self.x, self.y, self.z + 1),
            Coordinate::new_with_coords(self.x, self.y, self.z - 1),
        ]
    }
    
    /// Calculate distance to another coordinate
    pub fn distance_to(&self, other: &Coordinate) -> f32 {
        let dx = (self.x - other.x) as f32;
        let dy = (self.y - other.y) as f32;
        let dz = (self.z - other.z) as f32;
        (dx * dx + dy * dy + dz * dz).sqrt()
    }
}

impl Position for Coordinate {
    fn x(&self) -> i32 { self.x }
    fn y(&self) -> i32 { self.y }
    fn z(&self) -> i32 { self.z }
    
    fn set_position(&mut self, x: i32, y: i32, z: i32) {
        self.x = x;
        self.y = y;
        self.z = z;
    }
}

impl Default for Coordinate {
    fn default() -> Self {
        Self::new()
    }
}

impl From<(i32, i32, i32)> for Coordinate {
    fn from((x, y, z): (i32, i32, i32)) -> Self {
        Self::new_with_coords(x, y, z)
    }
}

impl From<Vec3> for Coordinate {
    fn from(v: Vec3) -> Self {
        Self::new_with_coords(v.x as i32, v.y as i32, v.z as i32)
    }
}