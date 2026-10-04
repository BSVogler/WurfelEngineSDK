//! Texture and sprite atlas management

use crate::core::EngineResult;
use std::collections::HashMap;

/// A texture resource
#[derive(Debug)]
pub struct Texture {
    pub texture: wgpu::Texture,
    pub view: wgpu::TextureView,
    pub sampler: wgpu::Sampler,
    pub width: u32,
    pub height: u32,
}

impl Texture {
    /// Create a new texture from raw image data
    pub fn from_bytes(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        bytes: &[u8],
        label: Option<&str>,
    ) -> EngineResult<Self> {
        let img = image::load_from_memory(bytes)
            .map_err(|e| crate::core::EngineError::AssetLoadError(e.to_string()))?;
        Self::from_image(device, queue, &img, label)
    }
    
    /// Create a new texture from an image
    pub fn from_image(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        img: &image::DynamicImage,
        label: Option<&str>,
    ) -> EngineResult<Self> {
        let img = img.to_rgba8();
        let (width, height) = img.dimensions();
        
        let texture_size = wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        };
        
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label,
            size: texture_size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        
        queue.write_texture(
            wgpu::ImageCopyTexture {
                aspect: wgpu::TextureAspect::All,
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
            },
            &img,
            wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(4 * width),
                rows_per_image: Some(height),
            },
            texture_size,
        );
        
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Nearest, // Pixel art style
            min_filter: wgpu::FilterMode::Nearest,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });
        
        Ok(Self {
            texture,
            view,
            sampler,
            width,
            height,
        })
    }
    
    /// Create a 1x1 white texture for colored rendering
    pub fn create_white_texture(device: &wgpu::Device, queue: &wgpu::Queue) -> EngineResult<Self> {
        let img = image::DynamicImage::ImageRgba8(
            image::RgbaImage::from_pixel(1, 1, image::Rgba([255, 255, 255, 255]))
        );
        Self::from_image(device, queue, &img, Some("white_texture"))
    }
}

/// Sprite definition parsed from .def file
#[derive(Debug)]
struct SpriteDefinition {
    name: String,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
}

/// Sprite atlas for managing multiple sprites in a single texture
#[derive(Debug)]
pub struct SpriteAtlas {
    pub texture: Texture,
    sprites: HashMap<String, SpriteInfo>,
}

/// Information about a sprite in the atlas
#[derive(Debug, Clone)]
pub struct SpriteInfo {
    /// UV coordinates in the atlas [min_u, min_v, max_u, max_v]
    pub uv_coords: [f32; 4],
    /// Original pixel coordinates
    pub pixel_coords: [u32; 4], // [x, y, width, height]
}

impl SpriteAtlas {
    /// Create a new sprite atlas from a texture and sprite definitions
    pub fn new(texture: Texture) -> Self {
        Self {
            texture,
            sprites: HashMap::new(),
        }
    }
    
    /// Add a sprite to the atlas
    pub fn add_sprite(&mut self, name: String, x: u32, y: u32, width: u32, height: u32) {
        let min_u = x as f32 / self.texture.width as f32;
        let min_v = y as f32 / self.texture.height as f32;
        let max_u = (x + width) as f32 / self.texture.width as f32;
        let max_v = (y + height) as f32 / self.texture.height as f32;
        
        let sprite_info = SpriteInfo {
            uv_coords: [min_u, min_v, max_u, max_v],
            pixel_coords: [x, y, width, height],
        };
        
        self.sprites.insert(name, sprite_info);
    }
    
    /// Get sprite information by name
    pub fn get_sprite(&self, name: &str) -> Option<&SpriteInfo> {
        self.sprites.get(name)
    }
    
    /// Create sprite atlas by parsing the spritesheet definition file
    pub fn create_block_atlas(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        sprite_bytes: &[u8],
        def_content: &str,
    ) -> EngineResult<Self> {
        let texture = Texture::from_bytes(device, queue, sprite_bytes, Some("block_atlas"))?;
        let mut atlas = Self::new(texture);
        
        // Parse sprite definitions from .def file
        let sprites = Self::parse_sprite_definitions(def_content)?;
        
        for sprite_def in sprites {
            atlas.add_sprite(sprite_def.name, sprite_def.x, sprite_def.y, sprite_def.width, sprite_def.height);
        }
        
        log::info!("Loaded {} sprites from definition file", atlas.sprites.len());
        Ok(atlas)
    }
    
    /// Parse sprite definitions from the .def file format
    fn parse_sprite_definitions(def_content: &str) -> EngineResult<Vec<SpriteDefinition>> {
        let mut sprites = Vec::new();
        let lines: Vec<&str> = def_content.lines().collect();
        
        let mut i = 0;
        while i < lines.len() {
            let line = lines[i].trim();
            
            // Skip empty lines and the header
            if line.is_empty() || line.starts_with("Spritesheet.png") {
                i += 1;
                continue;
            }
            
            // Look for sprite definition blocks starting with '{'
            if line == "{" {
                if i + 8 < lines.len() {
                    let name = lines[i + 1].trim().to_string();
                    
                    // Parse coordinates (lines are: name, x, y, width, height, ...)
                    if let (Ok(x), Ok(y), Ok(width), Ok(height)) = (
                        lines[i + 2].trim().parse::<u32>(),
                        lines[i + 3].trim().parse::<u32>(),
                        lines[i + 4].trim().parse::<u32>(),
                        lines[i + 5].trim().parse::<u32>(),
                    ) {
                        sprites.push(SpriteDefinition { name, x, y, width, height });
                    }
                }
                
                // Skip to the next '}' 
                while i < lines.len() && lines[i].trim() != "}" {
                    i += 1;
                }
            }
            i += 1;
        }
        
        Ok(sprites)
    }
    
    /// Create a basic sprite atlas with hardcoded block textures (fallback)
    pub fn create_block_atlas_fallback(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        sprite_bytes: &[u8],
    ) -> EngineResult<Self> {
        let texture = Texture::from_bytes(device, queue, sprite_bytes, Some("block_atlas"))?;
        let mut atlas = Self::new(texture);
        
        // Fallback hardcoded sprite coordinates
        let block_definitions = [
            ("b0-0-0", 0, 0, 80, 120),     // Air block
            ("b1-0-0", 323, 0, 80, 120),   // Stone block
            ("b2-0-0", 242, 0, 80, 120),   // Grass block  
            ("b8-0-0", 404, 0, 80, 120),   // Another block type
        ];
        
        for (name, x, y, width, height) in block_definitions {
            atlas.add_sprite(name.to_string(), x, y, width, height);
        }
        
        Ok(atlas)
    }
    
    /// Get UV coordinates for a block sprite
    pub fn get_block_sprite_uv(&self, id: u8, value: u8, side: u8) -> [f32; 4] {
        let sprite_name = format!("b{}-{}-{}", id, value, side);
        
        if let Some(sprite) = self.get_sprite(&sprite_name) {
            sprite.uv_coords
        } else {
            // Default to first sprite if not found
            [0.0, 0.0, 1.0, 1.0]
        }
    }
}