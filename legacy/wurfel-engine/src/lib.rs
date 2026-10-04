//! # Wurfel Engine - Rust Port
//! 
//! A 2.5D isometric game engine ported from Java/libGDX to Rust with WASM support.
//! This engine focuses on chunk-based world rendering with an isometric projection.

pub mod core;
pub mod graphics;
pub mod map;
pub mod math;
pub mod utils;

// Re-export commonly used types
pub use core::*;
pub use graphics::{Camera, Renderer};
pub use map::{Chunk, Coordinate, Position, Block};
pub use math::{Vec2, Vec3, Mat4};

/// Engine version
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Core engine constants matching the original Java Wurfel Engine
pub mod constants {
    /// View space constants (screen/rendering dimensions)
    pub const VIEW_DEPTH: i32 = 100;
    pub const VIEW_DEPTH2: i32 = VIEW_DEPTH / 2;  // 50
    pub const VIEW_DEPTH4: i32 = VIEW_DEPTH / 4;  // 25 (for backward compatibility)
    pub const VIEW_WIDTH: i32 = 200;
    pub const VIEW_WIDTH2: i32 = VIEW_WIDTH / 2;  // 100
    pub const VIEW_WIDTH4: i32 = VIEW_WIDTH / 4;  // 50 (for backward compatibility)
    pub const VIEW_HEIGHT: i32 = 122;
    pub const VIEW_HEIGHT2: i32 = VIEW_HEIGHT / 2;  // 61

    /// Game space constants
    pub const GAME_DIAGLENGTH: i32 = VIEW_WIDTH;  // 200
    pub const GAME_DIAGLENGTH2: i32 = VIEW_WIDTH2;  // 100
    /// ~141 (sqrt(200^2/2))
    pub const GAME_EDGELENGTH: i32 = (GAME_DIAGLENGTH as f32 / 1.41421356237) as i32;
    pub const GAME_EDGELENGTH2: i32 = GAME_EDGELENGTH / 2;

    /// Projection factors from original Java engine
    /// VIEW_HEIGHT / GAME_EDGELENGTH ≈ 0.866
    pub const PROJECTIONFACTORZ: f32 = VIEW_HEIGHT as f32 / GAME_EDGELENGTH as f32;
    /// VIEW_DEPTH / GAME_DIAGLENGTH = 0.5
    pub const PROJECTIONFACTORY: f32 = VIEW_DEPTH as f32 / GAME_DIAGLENGTH as f32;
    
    /// Chunk dimensions - blocks in X direction
    pub const CHUNK_SIZE_X: usize = 10;
    /// Chunk dimensions - blocks in Y direction (must be even)
    pub const CHUNK_SIZE_Y: usize = 40;
    /// Chunk dimensions - blocks in Z direction
    pub const CHUNK_SIZE_Z: usize = 10;
    
    /// Maximum number of block types
    pub const OBJECT_TYPES_NUM: usize = 256;
    /// Maximum number of block values per type
    pub const VALUES_NUM: usize = 256;
}