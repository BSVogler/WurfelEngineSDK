//! Authentic Wurfel Engine block face batch renderer
//! Based on the original Java SpriteBatchWithZAxis and GameSpaceSprite

use crate::constants::*;
use crate::map::{Block, Coordinate, Chunk, Renderable};
use crate::graphics::SpriteVertex;

/// Block face sides (matching original Wurfel Engine)
#[derive(Debug, Clone, Copy)]
pub enum BlockSide {
    Left = 0,   // Side facing left
    Top = 1,    // Top face
    Right = 2,  // Side facing right
}

/// Block face batch renderer (authentic Wurfel Engine approach)
pub struct BlockFaceBatch {
    vertices: Vec<SpriteVertex>,
    indices: Vec<u16>,
    max_quads: usize,
}

impl BlockFaceBatch {
    pub fn new() -> Self {
        Self {
            vertices: Vec::new(),
            indices: Vec::new(),
            max_quads: 8191, // Same limit as original SpriteBatchWithZAxis
        }
    }

    /// Add a block face to the batch (authentic Wurfel Engine vertex generation)
    pub fn add_block_face(&mut self, block: Block, coordinate: &Coordinate, side: BlockSide) {
        if self.vertices.len() / 4 >= self.max_quads {
            return; // Batch full
        }

        // Generate authentic block face vertices using original Wurfel Engine logic
        let vertices = self.generate_face_vertices(block, coordinate, side);
        let base_index = self.vertices.len() as u16;
        
        // Add vertices (4 per quad)
        self.vertices.extend_from_slice(&vertices);
        
        // Add indices (6 per quad - two triangles)
        let indices = [
            base_index,     base_index + 1, base_index + 2,
            base_index,     base_index + 2, base_index + 3,
        ];
        self.indices.extend_from_slice(&indices);
    }

    /// Generate vertices for a block face using authentic Wurfel Engine positioning
    fn generate_face_vertices(&self, block: Block, coordinate: &Coordinate, side: BlockSide) -> [SpriteVertex; 4] {
        // Authentic GameSpaceSprite.java vertex generation (lines 635-740)
        
        // For block faces, use standard block dimensions as origin
        let origin_x = VIEW_WIDTH2 as f32; // Half block width
        let origin_y = VIEW_HEIGHT2 as f32; // Half block height
        
        // Starting local coordinates (lines 635-651): all vertices start at -originX, -originY
        let mut local_x = [-origin_x, -origin_x, -origin_x, -origin_x]; // localX1,2,3,4 = -originX
        let mut local_y = [-origin_y, -origin_y, -origin_y, -origin_y]; // localY1,2,3,4 = -originY  
        let mut local_z = [-origin_y, -origin_y, -origin_y, -origin_y]; // localZ1,2,3,4 = -originY
        
        // Apply side-specific offsets exactly as in GameSpaceSprite.java
        match side {
            BlockSide::Left => {
                // LEFT side vertices (lines 658-664)
                local_x[0] += -GAME_DIAGLENGTH2 as f32; // localX1 += -GAME_DIAGLENGTH2
                local_x[1] += -GAME_DIAGLENGTH2 as f32; // localX2 += -GAME_DIAGLENGTH2
                local_z[1] += GAME_EDGELENGTH as f32;   // localZ2 += GAME_EDGELENGTH
                local_y[2] += GAME_DIAGLENGTH2 as f32;  // localY3 += GAME_DIAGLENGTH2
                local_z[2] += GAME_EDGELENGTH as f32;   // localZ3 += GAME_EDGELENGTH
                local_y[3] += GAME_DIAGLENGTH2 as f32;  // localY4 += GAME_DIAGLENGTH2
            },
            BlockSide::Top => {
                // TOP side vertices (lines 665-673)
                local_x[0] -= GAME_DIAGLENGTH2 as f32;  // localX1 -= GAME_DIAGLENGTH2
                local_y[1] -= GAME_DIAGLENGTH2 as f32;  // localY2 -= GAME_DIAGLENGTH2
                local_x[2] += GAME_DIAGLENGTH2 as f32;  // localX3 += GAME_DIAGLENGTH2
                local_y[3] += GAME_DIAGLENGTH2 as f32;  // localY4 += GAME_DIAGLENGTH2
                // All Z coordinates raised (lines 669-672)
                local_z[0] += GAME_EDGELENGTH as f32;   // localZ1 += GAME_EDGELENGTH
                local_z[1] += GAME_EDGELENGTH as f32;   // localZ2 += GAME_EDGELENGTH
                local_z[2] += GAME_EDGELENGTH as f32;   // localZ3 += GAME_EDGELENGTH
                local_z[3] += GAME_EDGELENGTH as f32;   // localZ4 += GAME_EDGELENGTH
            },
            BlockSide::Right => {
                // RIGHT side vertices (lines 674-680)
                local_y[0] += GAME_DIAGLENGTH2 as f32;  // localY1 += GAME_DIAGLENGTH2
                local_y[1] += GAME_DIAGLENGTH2 as f32;  // localY2 += GAME_DIAGLENGTH2
                local_z[1] += GAME_EDGELENGTH as f32;   // localZ2 += GAME_EDGELENGTH
                local_x[2] += GAME_DIAGLENGTH2 as f32;  // localX3 += GAME_DIAGLENGTH2
                local_z[2] += GAME_EDGELENGTH as f32;   // localZ3 += GAME_EDGELENGTH
                local_x[3] += GAME_DIAGLENGTH2 as f32;  // localX4 += GAME_DIAGLENGTH2
            }
        }

        // Calculate world origin (lines 653-655)
        let world_origin_x = coordinate.x as f32 * GAME_DIAGLENGTH as f32 + 
            if coordinate.y % 2 != 0 { GAME_DIAGLENGTH2 as f32 } else { 0.0 } + origin_x;
        let world_origin_y = coordinate.y as f32 * GAME_DIAGLENGTH as f32 + origin_y;
        let world_origin_z = coordinate.z as f32 * GAME_EDGELENGTH as f32 + origin_y;

        // Convert to world coordinates (lines 728-740)
        let world_positions = [
            [local_x[0] + world_origin_x, local_y[0] + world_origin_y, local_z[0] + world_origin_z], // vertices[X1,Y1,Z1]
            [local_x[1] + world_origin_x, local_y[1] + world_origin_y, local_z[1] + world_origin_z], // vertices[X2,Y2,Z2]
            [local_x[2] + world_origin_x, local_y[2] + world_origin_y, local_z[2] + world_origin_z], // vertices[X3,Y3,Z3]
            [local_x[3] + world_origin_x, local_y[3] + world_origin_y, local_z[3] + world_origin_z], // vertices[X4,Y4,Z4]
        ];
        
        let uvs = [[0.0, 1.0], [0.0, 0.0], [1.0, 0.0], [1.0, 1.0]];

        // Get per-vertex colors based on block type and face
        let base_color = self.get_block_color(block, side);
        let vertex_colors = [
            base_color, // Could add per-vertex AO here like original
            base_color,
            base_color,
            base_color,
        ];

        // Create vertices
        [
            SpriteVertex::new(world_positions[0], uvs[0], vertex_colors[0]),
            SpriteVertex::new(world_positions[1], uvs[1], vertex_colors[1]),
            SpriteVertex::new(world_positions[2], uvs[2], vertex_colors[2]),
            SpriteVertex::new(world_positions[3], uvs[3], vertex_colors[3]),
        ]
    }

    /// Get color for block face with proper Wurfel Engine side-based shading
    fn get_block_color(&self, block: Block, side: BlockSide) -> [f32; 4] {
        let base_color = match block.sprite_id() {
            1 => [0.0, 0.8, 0.0], // Green for grass
            2 => [0.8, 0.5, 0.2], // Brown/orange for dirt  
            8 => [0.6, 0.6, 0.6], // Gray for stone
            _ => [1.0, 0.0, 1.0], // Magenta for unknown
        };

        // Apply authentic Wurfel Engine side-based shading
        let shade_factor = match side {
            BlockSide::Top => 1.0,     // Brightest (top face receives most light)
            BlockSide::Left => 0.8,    // Medium brightness
            BlockSide::Right => 0.6,   // Darkest (shadow side)
        };

        [
            base_color[0] * shade_factor,
            base_color[1] * shade_factor,
            base_color[2] * shade_factor,
            1.0,
        ]
    }

    /// Get vertex and index data for rendering
    pub fn get_mesh_data(&self) -> (&[SpriteVertex], &[u16]) {
        (&self.vertices, &self.indices)
    }

    /// Clear the batch for next frame
    pub fn clear(&mut self) {
        log::debug!("Clearing batch: had {} vertices, {} indices", self.vertices.len(), self.indices.len());
        self.vertices.clear();
        self.indices.clear();
    }

    /// Add test blocks with proper 3D faces (LEFT, TOP, RIGHT) and shading
    pub fn add_chunk(&mut self, _chunk: &Chunk) {
        // Create a few test blocks with proper 3D isometric faces
        let test_positions = [
            (0, 0, 0),    // Center block
            (-1, 0, 0),   // Left block  
            (1, 0, 0),    // Right block
            (0, 0, 1),    // Block above
        ];
        
        for (i, (x, y, z)) in test_positions.iter().enumerate() {
            let coord = Coordinate::new_with_coords(*x, *y, *z);
            
            // Create a block with different IDs for different colors
            let block_id = match i {
                0 => 1, // Green grass
                1 => 2, // Brown dirt  
                2 => 8, // Gray stone
                _ => 1, // Default grass
            };
            let block = crate::map::Block::new(block_id, 0);
            
            // Add all three faces like authentic Wurfel Engine
            self.add_block_face(block, &coord, BlockSide::Left);
            self.add_block_face(block, &coord, BlockSide::Top);
            self.add_block_face(block, &coord, BlockSide::Right);
        }
        
        // Log vertex info at debug level instead of info to reduce spam
        log::debug!("Added {} test blocks with 3D faces, {} vertices, {} indices", 
            test_positions.len(), self.vertices.len(), self.indices.len());
            
        // Only show vertex debug info once
        static mut SHOWN_VERTEX_DEBUG: bool = false;
        unsafe {
            if !SHOWN_VERTEX_DEBUG {
                SHOWN_VERTEX_DEBUG = true;
                
                // Debug: Print all 4 vertices for first face of each type
                let mut face_idx = 0;
                for i in (0..self.vertices.len()).step_by(4) {
                    if face_idx < 3 && i + 3 < self.vertices.len() { // Only first 3 faces (one of each type)
                        let face_name = match face_idx { 0 => "LEFT", 1 => "TOP", 2 => "RIGHT", _ => "?" };
                        log::info!("DEBUG: {} face vertices:", face_name);
                        for j in 0..4 {
                            let v = &self.vertices[i + j];
                            log::info!("  v{}: pos=({:.1}, {:.1}, {:.1}) color=({:.2}, {:.2}, {:.2}, {:.2})", 
                                j, v.position[0], v.position[1], v.position[2],
                                v.color[0], v.color[1], v.color[2], v.color[3]);
                        }
                        face_idx += 1;
                    }
                }
            }
        }
    }
}