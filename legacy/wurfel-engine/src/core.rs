//! Core engine functionality

use std::sync::Arc;
use std::cell::RefCell;

/// Engine error types
#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error("Graphics initialization failed: {0}")]
    GraphicsInitError(String),
    #[error("Asset loading failed: {0}")]
    AssetLoadError(String),
    #[error("Chunk operation failed: {0}")]
    ChunkError(String),
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
}

/// Result type for engine operations
pub type EngineResult<T> = Result<T, EngineError>;

/// Engine configuration
#[derive(Debug, Clone)]
pub struct EngineConfig {
    /// Window width
    pub width: u32,
    /// Window height  
    pub height: u32,
    /// Window title
    pub title: String,
    /// Enable VSync
    pub vsync: bool,
    /// Maximum FPS (0 = unlimited)
    pub max_fps: u32,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            width: 1024,
            height: 768,
            title: "Wurfel Engine".to_string(),
            vsync: true,
            max_fps: 60,
        }
    }
}

/// Main engine context
pub struct Engine {
    config: EngineConfig,
    renderer: Option<Arc<RefCell<crate::graphics::Renderer>>>,
    chunk_manager: crate::map::ChunkManager,
}

impl Engine {
    /// Create a new engine instance
    pub fn new(config: EngineConfig) -> Self {
        Self {
            config,
            renderer: None,
            chunk_manager: crate::map::ChunkManager::new(),
        }
    }
    
    /// Initialize the engine (sets up graphics context)
    pub async fn initialize(&mut self) -> EngineResult<()> {
        log::info!("Initializing Wurfel Engine v{}", crate::VERSION);
        
        // Create wgpu instance
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            ..Default::default()
        });
        
        // Initialize graphics system
        let renderer = crate::graphics::Renderer::new(&self.config, &instance, None).await
            .map_err(|e| EngineError::GraphicsInitError(e.to_string()))?;
        
        self.renderer = Some(Arc::new(RefCell::new(renderer)));
        
        log::info!("Engine initialized successfully");
        Ok(())
    }
    
    /// Initialize the engine with a surface (sets up graphics context)
    pub async fn initialize_with_surface(&mut self, instance: &wgpu::Instance, surface: &wgpu::Surface<'_>) -> EngineResult<()> {
        log::info!("Initializing Wurfel Engine v{}", crate::VERSION);
        
        // Initialize graphics system with surface
        let renderer = crate::graphics::Renderer::new(&self.config, instance, Some(surface)).await
            .map_err(|e| EngineError::GraphicsInitError(e.to_string()))?;
        
        self.renderer = Some(Arc::new(RefCell::new(renderer)));
        
        log::info!("Engine initialized successfully");
        Ok(())
    }
    
    /// Get the renderer
    pub fn renderer(&self) -> Option<&Arc<RefCell<crate::graphics::Renderer>>> {
        self.renderer.as_ref()
    }
    
    /// Load sprite atlas with definition file
    pub fn load_sprite_atlas(&mut self, sprite_bytes: &[u8], def_content: &str) -> EngineResult<()> {
        if let Some(renderer) = &self.renderer {
            renderer.borrow_mut().load_sprite_atlas(sprite_bytes, def_content)?;
            Ok(())
        } else {
            Err(EngineError::GraphicsInitError("No renderer available".to_string()))
        }
    }

    /// Load sprite atlas (fallback without definition)
    pub fn load_sprite_atlas_fallback(&mut self, sprite_bytes: &[u8]) -> EngineResult<()> {
        if let Some(renderer) = &self.renderer {
            renderer.borrow_mut().load_sprite_atlas_fallback(sprite_bytes)?;
            Ok(())
        } else {
            Err(EngineError::GraphicsInitError("No renderer available".to_string()))
        }
    }
    
    /// Configure a surface
    pub fn configure_surface(&self, surface: &wgpu::Surface, width: u32, height: u32) -> EngineResult<()> {
        if let Some(renderer) = &self.renderer {
            renderer.borrow().configure_surface(surface, width, height)?;
            Ok(())
        } else {
            Err(EngineError::GraphicsInitError("No renderer available".to_string()))
        }
    }
    
    /// Render a chunk
    pub fn render_chunk(
        &self,
        chunk: &crate::map::Chunk,
        camera: &mut crate::graphics::Camera,
        surface: &wgpu::Surface,
    ) -> EngineResult<()> {
        if let Some(renderer) = &self.renderer {
            renderer.borrow_mut().render_chunk(chunk, camera, surface)?;
            Ok(())
        } else {
            Err(EngineError::GraphicsInitError("No renderer available".to_string()))
        }
    }
    
    /// Get the chunk manager
    pub fn chunk_manager(&mut self) -> &mut crate::map::ChunkManager {
        &mut self.chunk_manager
    }
    
    /// Render chunk to file for debugging
    pub fn render_chunk_to_file(
        &self,
        chunk: &crate::map::Chunk,
        camera: &mut crate::graphics::Camera,
        filename: &str,
    ) -> EngineResult<()> {
        if let Some(renderer) = &self.renderer {
            renderer.borrow_mut().render_chunk_to_file(chunk, camera, filename)?;
            Ok(())
        } else {
            Err(EngineError::GraphicsInitError("No renderer available".to_string()))
        }
    }

    /// Render chunk using authentic Wurfel Engine block face system
    pub fn render_chunk_to_file_authentic(
        &self,
        chunk: &crate::map::Chunk,
        camera: &mut crate::graphics::Camera,
        filename: &str,
    ) -> EngineResult<()> {
        if let Some(renderer) = &self.renderer {
            renderer.borrow_mut().render_chunk_to_file_authentic(chunk, camera, filename)?;
            Ok(())
        } else {
            Err(EngineError::GraphicsInitError("No renderer available".to_string()))
        }
    }

    /// Create debug render for testing screenshot system
    pub fn create_debug_render(&self) -> EngineResult<()> {
        if let Some(renderer) = &self.renderer {
            renderer.borrow_mut().create_debug_render()?;
            Ok(())
        } else {
            Err(EngineError::GraphicsInitError("No renderer available".to_string()))
        }
    }

    /// Get engine configuration
    pub fn config(&self) -> &EngineConfig {
        &self.config
    }
}