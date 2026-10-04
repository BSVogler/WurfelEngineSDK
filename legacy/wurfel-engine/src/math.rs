//! Math utilities and types

// Re-export glam types for convenience
pub use glam::*;

/// 2D vector type
pub type Vec2 = glam::Vec2;

/// 3D vector type  
pub type Vec3 = glam::Vec3;

/// 4D vector type
pub type Vec4 = glam::Vec4;

/// 4x4 matrix type
pub type Mat4 = glam::Mat4;

/// 2D integer vector
pub type IVec2 = glam::IVec2;

/// 3D integer vector
pub type IVec3 = glam::IVec3;

/// Utility functions for isometric math
pub mod isometric {
    use super::*;
    use crate::constants::*;
    
    /// Convert world coordinates to screen coordinates using isometric projection
    pub fn world_to_screen(x: f32, y: f32, z: f32) -> Vec2 {
        let screen_x = (x - y) * (VIEW_WIDTH2 as f32);
        let screen_y = (x + y) * (VIEW_DEPTH4 as f32) - z * (VIEW_DEPTH2 as f32);
        Vec2::new(screen_x, screen_y)
    }
    
    /// Convert screen coordinates back to world coordinates (z = 0 plane)
    pub fn screen_to_world(screen_x: f32, screen_y: f32) -> Vec2 {
        let world_x = (screen_x / (VIEW_WIDTH2 as f32) + screen_y / (VIEW_DEPTH4 as f32)) * 0.5;
        let world_y = (screen_y / (VIEW_DEPTH4 as f32) - screen_x / (VIEW_WIDTH2 as f32)) * 0.5;
        Vec2::new(world_x, world_y)
    }
    
    /// Calculate the depth value for depth sorting
    pub fn calculate_depth(x: f32, y: f32, z: f32) -> f32 {
        // In isometric view, objects further back and higher up should be rendered first
        -(x + y + z * 0.1)
    }
}