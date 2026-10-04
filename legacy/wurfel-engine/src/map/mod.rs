//! Map system including chunks, coordinates, and blocks

mod chunk;
mod coordinate;
mod block;
mod generators;

pub use chunk::*;
pub use coordinate::*;
pub use block::*;
pub use generators::*;

/// Position trait for objects that have a 3D position
pub trait Position {
    /// Get the x coordinate
    fn x(&self) -> i32;
    /// Get the y coordinate  
    fn y(&self) -> i32;
    /// Get the z coordinate
    fn z(&self) -> i32;
    
    /// Set the position
    fn set_position(&mut self, x: i32, y: i32, z: i32);
    
    /// Convert to world space coordinates
    fn to_world(&self) -> crate::math::Vec3 {
        crate::math::Vec3::new(self.x() as f32, self.y() as f32, self.z() as f32)
    }
}

/// Trait for objects that can be rendered
pub trait Renderable {
    /// Get the sprite ID for rendering
    fn sprite_id(&self) -> u8;
    /// Get the sprite value/variant
    fn sprite_value(&self) -> u8;
    /// Check if this object should be rendered
    fn is_visible(&self) -> bool;
}