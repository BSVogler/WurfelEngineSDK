//! WASM bindings for the Wurfel Engine

use wasm_bindgen::prelude::*;
use web_sys::console;
use wurfel_engine::{
    core::{Engine, EngineConfig},
    graphics::Camera,
    map::{Chunk, IslandGenerator, Generator, ChunkManager},
    Vec3,
};

// Set up panic hook for better error messages in the browser
#[wasm_bindgen(start)]
pub fn main() {
    console_error_panic_hook::set_once();
    console::log_1(&"Wurfel Engine WASM module loaded".into());
}

/// WASM wrapper for the Wurfel Engine
#[wasm_bindgen]
pub struct WurfelEngine {
    engine: Engine,
    camera: Camera,
    chunk_manager: ChunkManager,
}

#[wasm_bindgen]
impl WurfelEngine {
    /// Create a new engine instance
    #[wasm_bindgen(constructor)]
    pub fn new(width: u32, height: u32) -> WurfelEngine {
        let config = EngineConfig {
            width,
            height,
            title: "Wurfel Engine WASM".to_string(),
            vsync: true,
            max_fps: 60,
        };
        
        let engine = Engine::new(config);
        let camera = Camera::new(width as f32 / height as f32);
        let chunk_manager = ChunkManager::new();
        
        WurfelEngine {
            engine,
            camera,
            chunk_manager,
        }
    }
    
    /// Initialize the engine (async)
    #[wasm_bindgen]
    pub async fn initialize(&mut self) -> Result<(), JsValue> {
        self.engine.initialize().await
            .map_err(|e| JsValue::from_str(&e.to_string()))?;
        
        console::log_1(&"Engine initialized successfully".into());
        Ok(())
    }
    
    /// Set camera position
    #[wasm_bindgen]
    pub fn set_camera_position(&mut self, x: f32, y: f32, z: f32) {
        self.camera.set_position(Vec3::new(x, y, z));
    }
    
    /// Set camera target
    #[wasm_bindgen]
    pub fn set_camera_target(&mut self, x: f32, y: f32, z: f32) {
        self.camera.set_target(Vec3::new(x, y, z));
    }
    
    /// Move camera by offset
    #[wasm_bindgen]
    pub fn move_camera(&mut self, dx: f32, dy: f32, dz: f32) {
        self.camera.translate(Vec3::new(dx, dy, dz));
    }
    
    /// Zoom camera
    #[wasm_bindgen]
    pub fn zoom_camera(&mut self, factor: f32) {
        self.camera.zoom(factor);
    }
    
    /// Generate a test chunk at given coordinates
    #[wasm_bindgen]
    pub fn generate_chunk(&mut self, chunk_x: i32, chunk_y: i32, seed: u32) {
        let mut chunk = Chunk::new((chunk_x, chunk_y));
        let generator = IslandGenerator::new(seed as u64).with_base_height(4);
        generator.generate(&mut chunk);
        
        // Store chunk in manager would go here
        console::log_1(&format!("Generated chunk at ({}, {})", chunk_x, chunk_y).into());
    }
    
    /// Get engine version
    #[wasm_bindgen(getter)]
    pub fn version(&self) -> String {
        wurfel_engine::VERSION.to_string()
    }
}

/// JavaScript utilities and bindings
#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = console)]
    fn log(s: &str);
    
    type HTMLCanvasElement;
    
    #[wasm_bindgen(method, getter)]
    fn width(this: &HTMLCanvasElement) -> u32;
    
    #[wasm_bindgen(method, getter)]  
    fn height(this: &HTMLCanvasElement) -> u32;
}

/// Simple logging macro for WASM
macro_rules! console_log {
    ($($t:tt)*) => (log(&format_args!($($t)*).to_string()))
}

/// Block manipulation functions for JavaScript
#[wasm_bindgen]
pub struct BlockUtils;

#[wasm_bindgen]
impl BlockUtils {
    /// Create a new block with ID and value
    #[wasm_bindgen]
    pub fn create_block(id: u8, value: u8) -> u16 {
        wurfel_engine::map::Block::new(id, value).into()
    }
    
    /// Get block ID from packed data
    #[wasm_bindgen]
    pub fn get_block_id(block_data: u16) -> u8 {
        wurfel_engine::map::Block::from(block_data).id()
    }
    
    /// Get block value from packed data
    #[wasm_bindgen]
    pub fn get_block_value(block_data: u16) -> u8 {
        wurfel_engine::map::Block::from(block_data).value()
    }
    
    /// Check if block is air
    #[wasm_bindgen]
    pub fn is_air(block_data: u16) -> bool {
        wurfel_engine::map::Block::from(block_data).is_air()
    }
}

/// Coordinate utilities for JavaScript
#[wasm_bindgen]
pub struct CoordUtils;

#[wasm_bindgen]
impl CoordUtils {
    /// Convert world coordinates to chunk coordinates
    #[wasm_bindgen]
    pub fn world_to_chunk(x: i32, y: i32) -> Vec<i32> {
        let coord = wurfel_engine::map::Coordinate::new_with_coords(x, y, 0);
        let (chunk_x, chunk_y) = coord.to_chunk();
        vec![chunk_x, chunk_y]
    }
    
    /// Convert world coordinates to screen coordinates
    #[wasm_bindgen]
    pub fn world_to_screen(x: f32, y: f32, z: f32) -> Vec<f32> {
        let screen_pos = wurfel_engine::math::isometric::world_to_screen(x, y, z);
        vec![screen_pos.x, screen_pos.y]
    }
    
    /// Convert screen coordinates to world coordinates
    #[wasm_bindgen]
    pub fn screen_to_world(screen_x: f32, screen_y: f32) -> Vec<f32> {
        let world_pos = wurfel_engine::math::isometric::screen_to_world(screen_x, screen_y);
        vec![world_pos.x, world_pos.y]
    }
}