//! Renderer-independent world simulation.
//!
//! Nothing in this crate knows about graphics, windows or the browser, so the same code can run
//! in a headless server and in a wasm client.

pub mod animation;
pub mod atmosphere;
pub mod block;
pub mod chunk;
pub mod console;
pub mod cvar;
pub mod detail;
pub mod entity;
pub mod generator;
pub mod grass;
pub mod grid;
pub mod light;
pub mod particle;
pub mod player;
pub mod protocol;
pub mod shockwave;
pub mod storage;
pub mod world;

pub use block::Block;
pub use generator::{AirGenerator, Generator, IslandGenerator};
pub use world::World;

/// Blocks per chunk along x.
pub const CHUNK_SIZE_X: i32 = 10;
/// Blocks per chunk along y. Must be even: odd and even rows are staggered, see [`grid`].
pub const CHUNK_SIZE_Y: i32 = 40;
/// Blocks per chunk along z (the world height).
pub const CHUNK_SIZE_Z: i32 = 32;

