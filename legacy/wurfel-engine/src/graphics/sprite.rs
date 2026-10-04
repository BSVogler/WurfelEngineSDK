//! Sprite rendering system for 2.5D isometric blocks

use crate::graphics::{SpriteVertex, SpriteAtlas};
use crate::map::{Block, Coordinate, Renderable, Position};
use crate::math::{Vec2, Vec3};
use crate::constants::*;

/// A renderable sprite instance
#[derive(Debug, Clone)]
pub struct Sprite {
    /// World position
    pub position: Vec3,
    /// Sprite UV coordinates
    pub uv_coords: [f32; 4],
    /// Color tint
    pub color: [f32; 4],
    /// Depth for sorting
    pub depth: f32,
    /// Size scale
    pub scale: Vec2,
    /// Sprite dimensions (width, height) in pixels
    pub size: Vec2,
}

impl Sprite {
    /// Create a new sprite
    pub fn new(position: Vec3, uv_coords: [f32; 4]) -> Self {
        Self {
            position,
            uv_coords,
            color: [1.0, 1.0, 1.0, 1.0], // White (no tint)
            depth: crate::math::isometric::calculate_depth(position.x, position.y, position.z),
            scale: Vec2::ONE,
            size: Vec2::new(80.0, 120.0), // Default block size
        }
    }
    
    /// Create a new sprite with specific size
    pub fn new_with_size(position: Vec3, uv_coords: [f32; 4], size: Vec2) -> Self {
        Self {
            position,
            uv_coords,
            color: [1.0, 1.0, 1.0, 1.0], // White (no tint)
            depth: crate::math::isometric::calculate_depth(position.x, position.y, position.z),
            scale: Vec2::ONE,
            size,
        }
    }
    
    /// Create a sprite from a block using texture atlas
    pub fn from_block(block: Block, coordinate: &Coordinate, atlas: &SpriteAtlas) -> Option<Self> {
        if !block.is_visible() {
            return None;
        }
        
        // Use authentic Wurfel Engine view space coordinates
        let view_space_pos = coordinate.to_view_space();
        
        // Apply scale and offset to center the chunk on screen (1024x768)
        // The original coordinate system is much larger, so we need to scale it down
        let scale = 2.0; // Make sprites larger to see them better
        let offset_x = 512.0; // Center horizontally on 1024px screen
        let offset_y = 400.0; // Position vertically on 768px screen
        
        let screen_pos = Vec2::new(
            view_space_pos.x / scale + offset_x,
            view_space_pos.y / scale + offset_y,
        );
        
        let position = Vec3::new(screen_pos.x, screen_pos.y, coordinate.depth());
        let uv_coords = atlas.get_block_sprite_uv(block.sprite_id(), block.sprite_value(), 0);
        
        Some(Self::new(position, uv_coords))
    }
    
    /// Create block faces using authentic Wurfel Engine batch rendering approach
    pub fn from_block_colored(block: Block, coordinate: &Coordinate) -> Option<Self> {
        if !block.is_visible() {
            return None;
        }
        
        // This is just a placeholder - we'll implement proper block face generation
        // in the batch renderer instead of individual sprites
        
        // For now, create a simple quad at the block position for testing
        let world_x = coordinate.x as f32 * GAME_DIAGLENGTH as f32;
        let world_y = coordinate.y as f32 * GAME_DIAGLENGTH as f32;
        let world_z = coordinate.z as f32 * GAME_EDGELENGTH as f32;
        
        // Apply isometric projection (simplified for testing)
        let screen_x = world_x - world_y;
        let screen_y = (world_x + world_y) * 0.5 - world_z;
        
        // Scale and center
        let scale = 10.0;
        let final_pos = Vec3::new(
            screen_x / scale + 512.0,
            screen_y / scale + 400.0,
            coordinate.depth() / 1000.0,
        );
        
        // Distinct colors
        let color = match block.sprite_id() {
            1 => [0.0, 1.0, 0.0, 1.0], // Green for grass
            2 => [0.6, 0.4, 0.2, 1.0], // Brown for dirt
            8 => [0.5, 0.5, 0.5, 1.0], // Gray for stone
            _ => [1.0, 0.0, 1.0, 1.0], // Magenta
        };
        
        let mut sprite = Self::new(final_pos, [0.0, 0.0, 1.0, 1.0]);
        sprite.color = color;
        sprite.size = Vec2::new(50.0, 30.0); // Visible size for testing
        Some(sprite)
    }
    
    /// Set color tint
    pub fn with_color(mut self, color: [f32; 4]) -> Self {
        self.color = color;
        self
    }
    
    /// Set scale
    pub fn with_scale(mut self, scale: Vec2) -> Self {
        self.scale = scale;
        self
    }
    
    /// Generate vertices for this sprite
    pub fn generate_vertices(&self, screen_pos: Vec2) -> [SpriteVertex; 4] {
        let width = self.size.x * self.scale.x;
        let height = self.size.y * self.scale.y;
        
        let half_width = width * 0.5;
        let half_height = height * 0.5;
        
        let [min_u, min_v, max_u, max_v] = self.uv_coords;
        
        [
            // Top-left
            SpriteVertex::new(
                [screen_pos.x - half_width, screen_pos.y - half_height, self.depth],
                [min_u, min_v],
                self.color,
            ),
            // Top-right
            SpriteVertex::new(
                [screen_pos.x + half_width, screen_pos.y - half_height, self.depth],
                [max_u, min_v],
                self.color,
            ),
            // Bottom-right
            SpriteVertex::new(
                [screen_pos.x + half_width, screen_pos.y + half_height, self.depth],
                [max_u, max_v],
                self.color,
            ),
            // Bottom-left
            SpriteVertex::new(
                [screen_pos.x - half_width, screen_pos.y + half_height, self.depth],
                [min_u, max_v],
                self.color,
            ),
        ]
    }
}

/// Sprite batch for efficient rendering of many sprites
#[derive(Debug)]
pub struct SpriteBatch {
    sprites: Vec<Sprite>,
    vertices: Vec<SpriteVertex>,
    indices: Vec<u16>,
    sorted: bool,
}

impl SpriteBatch {
    /// Create a new sprite batch
    pub fn new() -> Self {
        Self {
            sprites: Vec::new(),
            vertices: Vec::new(),
            indices: Vec::new(),
            sorted: false,
        }
    }
    
    /// Add a sprite to the batch
    pub fn add_sprite(&mut self, sprite: Sprite) {
        self.sprites.push(sprite);
        self.sorted = false;
    }
    
    /// Clear all sprites
    pub fn clear(&mut self) {
        self.sprites.clear();
        self.vertices.clear();
        self.indices.clear();
        self.sorted = false;
    }
    
    /// Sort sprites by depth for proper rendering order
    pub fn sort(&mut self) {
        if self.sorted {
            return;
        }
        
        // Sort by depth (furthest first for proper alpha blending)
        self.sprites.sort_by(|a, b| a.depth.partial_cmp(&b.depth).unwrap());
        self.sorted = true;
    }
    
    /// Generate vertices and indices for all sprites
    pub fn generate_mesh(&mut self, camera: &mut crate::graphics::Camera) -> (&[SpriteVertex], &[u16]) {
        if !self.sorted {
            self.sort();
        }
        
        self.vertices.clear();
        self.indices.clear();
        
        for (_sprite_index, sprite) in self.sprites.iter().enumerate() {
            // The sprite position is already in screen coordinates from the isometric conversion
            // Just use the x,y components directly
            let screen_pos = Vec2::new(sprite.position.x, sprite.position.y);
            
            // Generate vertices for this sprite
            let sprite_vertices = sprite.generate_vertices(screen_pos);
            let vertex_offset = self.vertices.len() as u16;
            
            // Add vertices
            self.vertices.extend_from_slice(&sprite_vertices);
            
            // Add indices (two triangles per sprite)
            let indices = [
                vertex_offset, vertex_offset + 1, vertex_offset + 2,
                vertex_offset, vertex_offset + 2, vertex_offset + 3,
            ];
            self.indices.extend_from_slice(&indices);
        }
        
        (&self.vertices, &self.indices)
    }
    
    /// Get sprite count
    pub fn len(&self) -> usize {
        self.sprites.len()
    }
    
    /// Check if batch is empty
    pub fn is_empty(&self) -> bool {
        self.sprites.is_empty()
    }
}

impl Default for SpriteBatch {
    fn default() -> Self {
        Self::new()
    }
}

/// Sprite renderer for handling batched sprite rendering
#[derive(Debug)]
pub struct SpriteRenderer {
    vertex_buffer: wgpu::Buffer,
    index_buffer: wgpu::Buffer,
    max_sprites: usize,
}

impl SpriteRenderer {
    /// Create a new sprite renderer
    pub fn new(device: &wgpu::Device, max_sprites: usize) -> Self {
        let max_vertices = max_sprites * 4; // 4 vertices per sprite
        let max_indices = max_sprites * 6;  // 6 indices per sprite
        
        let vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Sprite Vertex Buffer"),
            size: (max_vertices * std::mem::size_of::<SpriteVertex>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        
        let index_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Sprite Index Buffer"),
            size: (max_indices * std::mem::size_of::<u16>()) as u64,
            usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        
        Self {
            vertex_buffer,
            index_buffer,
            max_sprites,
        }
    }
    
    /// Upload sprite batch data to GPU buffers
    pub fn upload_batch(
        &self,
        queue: &wgpu::Queue,
        vertices: &[SpriteVertex],
        indices: &[u16],
    ) -> bool {
        if vertices.len() > self.max_sprites * 4 || indices.len() > self.max_sprites * 6 {
            log::warn!("Sprite batch exceeds maximum size, skipping upload");
            return false;
        }
        
        // Upload vertex data
        queue.write_buffer(
            &self.vertex_buffer,
            0,
            bytemuck::cast_slice(vertices),
        );
        
        // Upload index data
        queue.write_buffer(
            &self.index_buffer,
            0,
            bytemuck::cast_slice(indices),
        );
        
        true
    }
    
    /// Get vertex buffer
    pub fn vertex_buffer(&self) -> &wgpu::Buffer {
        &self.vertex_buffer
    }
    
    /// Get index buffer
    pub fn index_buffer(&self) -> &wgpu::Buffer {
        &self.index_buffer
    }
}