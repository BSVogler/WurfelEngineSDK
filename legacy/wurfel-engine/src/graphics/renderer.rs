//! Main rendering system

use crate::core::{EngineConfig, EngineError, EngineResult};
use crate::graphics::{Camera, SpriteAtlas, SpriteRenderer, SpriteBatch, SpriteVertex, Texture};
use crate::map::{Chunk, Coordinate, Renderable};

/// Main renderer for the engine
pub struct Renderer {
    /// Graphics adapter
    adapter: wgpu::Adapter,
    /// Graphics device
    device: wgpu::Device,
    /// Command queue
    queue: wgpu::Queue,
    /// Surface configuration
    config: wgpu::SurfaceConfiguration,
    /// Render pipeline for sprites
    sprite_pipeline: wgpu::RenderPipeline,
    /// Sprite renderer
    sprite_renderer: SpriteRenderer,
    /// Sprite atlas
    sprite_atlas: Option<SpriteAtlas>,
    /// Uniform buffer for camera matrices
    uniform_buffer: wgpu::Buffer,
    /// Bind group for uniforms and textures
    bind_group: Option<wgpu::BindGroup>,
    /// Bind group layout
    bind_group_layout: wgpu::BindGroupLayout,
}

/// Uniform data sent to shaders
#[repr(C)]
#[derive(Debug, Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct Uniforms {
    /// View-projection matrix
    view_proj: [[f32; 4]; 4],
    /// Screen dimensions
    screen_size: [f32; 2],
    /// Padding
    _padding: [f32; 2],
}

impl Uniforms {
    #[allow(dead_code)]
    fn new() -> Self {
        Self {
            view_proj: crate::math::Mat4::IDENTITY.to_cols_array_2d(),
            screen_size: [1024.0, 768.0],
            _padding: [0.0; 2],
        }
    }
}

impl Renderer {
    /// Create a new renderer  
    pub async fn new(config: &EngineConfig, instance: &wgpu::Instance, surface: Option<&wgpu::Surface<'_>>) -> EngineResult<Self> {
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::default(),
                compatible_surface: surface,
                force_fallback_adapter: false,
            })
            .await
            .ok_or_else(|| EngineError::GraphicsInitError("Failed to find adapter".to_string()))?;
        
        // Get device and queue
        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    label: None,
                    required_features: wgpu::Features::empty(),
                    required_limits: wgpu::Limits::default(),
                },
                None,
            )
            .await
            .map_err(|e| EngineError::GraphicsInitError(e.to_string()))?;
        
        // Create surface configuration
        let surface_config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: wgpu::TextureFormat::Bgra8UnormSrgb,
            width: config.width,
            height: config.height,
            present_mode: if config.vsync {
                wgpu::PresentMode::Fifo
            } else {
                wgpu::PresentMode::Immediate
            },
            alpha_mode: wgpu::CompositeAlphaMode::Auto,
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        
        // Create uniform buffer
        let uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Uniform Buffer"),
            size: std::mem::size_of::<Uniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        
        // Create bind group layout
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            entries: &[
                // Uniforms
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                // Texture
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        multisampled: false,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    },
                    count: None,
                },
                // Sampler
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
            label: Some("Sprite Bind Group Layout"),
        });
        
        // Create render pipeline
        let sprite_pipeline = Self::create_sprite_pipeline(&device, &bind_group_layout, surface_config.format)?;
        
        // Create sprite renderer
        let sprite_renderer = SpriteRenderer::new(&device, 10000); // Support up to 10k sprites
        
        Ok(Self {
            adapter,
            device,
            queue,
            config: surface_config,
            sprite_pipeline,
            sprite_renderer,
            sprite_atlas: None,
            uniform_buffer,
            bind_group: None,
            bind_group_layout,
        })
    }
    
    /// Create the sprite rendering pipeline
    fn create_sprite_pipeline(
        device: &wgpu::Device,
        bind_group_layout: &wgpu::BindGroupLayout,
        format: wgpu::TextureFormat,
    ) -> EngineResult<wgpu::RenderPipeline> {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Sprite Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/sprite.wgsl").into()),
        });
        
        let render_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Sprite Render Pipeline Layout"),
            bind_group_layouts: &[bind_group_layout],
            push_constant_ranges: &[],
        });
        
        let render_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Sprite Render Pipeline"),
            layout: Some(&render_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_main",
                buffers: &[SpriteVertex::desc()],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs_main",
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                unclipped_depth: false,
                polygon_mode: wgpu::PolygonMode::Fill,
                conservative: false,
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::Less,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState {
                count: 1,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            multiview: None,
        });
        
        Ok(render_pipeline)
    }
    
    /// Load sprite atlas from bytes and definition
    pub fn load_sprite_atlas(&mut self, sprite_bytes: &[u8], def_content: &str) -> EngineResult<()> {
        let atlas = SpriteAtlas::create_block_atlas(&self.device, &self.queue, sprite_bytes, def_content)?;
        self.create_bind_group(&atlas.texture);
        self.sprite_atlas = Some(atlas);
        Ok(())
    }
    
    /// Load sprite atlas from bytes only (fallback)
    pub fn load_sprite_atlas_fallback(&mut self, sprite_bytes: &[u8]) -> EngineResult<()> {
        let atlas = SpriteAtlas::create_block_atlas_fallback(&self.device, &self.queue, sprite_bytes)?;
        self.create_bind_group(&atlas.texture);
        self.sprite_atlas = Some(atlas);
        Ok(())
    }
    
    /// Create bind group with texture
    fn create_bind_group(&mut self, texture: &Texture) {
        self.bind_group = Some(self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            layout: &self.bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.uniform_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&texture.view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&texture.sampler),
                },
            ],
            label: Some("Sprite Bind Group"),
        }));
    }
    
    /// Render a chunk
    pub fn render_chunk(
        &mut self,
        chunk: &Chunk,
        camera: &mut Camera,
        surface: &wgpu::Surface,
    ) -> EngineResult<()> {
        // Configure surface on first use
        let surface_caps = surface.get_capabilities(&self.adapter);
        let surface_format = surface_caps
            .formats
            .iter()
            .find(|f| f.is_srgb())
            .copied()
            .unwrap_or(surface_caps.formats[0]);

        let surface_config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface_format,
            width: self.config.width,
            height: self.config.height,
            present_mode: wgpu::PresentMode::Fifo,
            alpha_mode: surface_caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };

        surface.configure(&self.device, &surface_config);
        
        // Check if we have a sprite atlas loaded, if not create white texture for colored blocks
        if self.sprite_atlas.is_none() {
            let white_texture = crate::graphics::Texture::create_white_texture(&self.device, &self.queue)?;
            self.create_bind_group(&white_texture);
            log::debug!("Using white texture for colored block rendering");
        }

        // Create authentic block face batch instead of sprites
        let mut block_batch = crate::graphics::BlockFaceBatch::new();
        block_batch.add_chunk(chunk);
        
        let (vertices, indices) = block_batch.get_mesh_data();
        log::debug!("Generated {} vertices ({} faces) for main window rendering", 
            vertices.len(), vertices.len() / 4);
        
        if vertices.is_empty() {
            log::warn!("No block faces generated for main window");
            return Ok(());
        }

        // Render using the authentic block face system
        self.render_block_faces_to_surface(vertices, indices, camera, surface)
    }

    /// Render block faces to a surface (main window)
    fn render_block_faces_to_surface(
        &mut self,
        vertices: &[SpriteVertex],
        indices: &[u16],
        camera: &mut Camera,
        surface: &wgpu::Surface,
    ) -> EngineResult<()> {
        // Upload block face vertices to GPU
        if !self.sprite_renderer.upload_batch(&self.queue, vertices, indices) {
            return Err(EngineError::GraphicsInitError("Failed to upload block faces".to_string()));
        }

        // Update camera uniforms
        let uniforms = Uniforms {
            view_proj: camera.view_proj_matrix().to_cols_array_2d(),
            screen_size: [self.config.width as f32, self.config.height as f32],
            _padding: [0.0; 2],
        };
        
        self.queue.write_buffer(
            &self.uniform_buffer,
            0,
            bytemuck::cast_slice(&[uniforms]),
        );

        // Get surface texture
        let output = surface.get_current_texture()
            .map_err(|e| EngineError::GraphicsInitError(e.to_string()))?;
        
        let view = output.texture.create_view(&wgpu::TextureViewDescriptor::default());
        
        // Create depth texture
        let depth_texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Main Window Depth Texture"),
            size: wgpu::Extent3d {
                width: self.config.width,
                height: self.config.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Depth32Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        
        let depth_view = depth_texture.create_view(&wgpu::TextureViewDescriptor::default());

        // Render block faces
        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Main Window Render Encoder"),
        });

        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Main Window Render Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.0,
                            g: 0.0,
                            b: 0.0,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            render_pass.set_pipeline(&self.sprite_pipeline);
            render_pass.set_bind_group(0, self.bind_group.as_ref().unwrap(), &[]);
            render_pass.set_vertex_buffer(0, self.sprite_renderer.vertex_buffer().slice(..));
            render_pass.set_index_buffer(
                self.sprite_renderer.index_buffer().slice(..),
                wgpu::IndexFormat::Uint16,
            );
            render_pass.draw_indexed(0..indices.len() as u32, 0, 0..1);
        }

        self.queue.submit(std::iter::once(encoder.finish()));
        output.present();
        
        Ok(())
    }
    
    /// Render a sprite batch
    pub fn render_sprite_batch(
        &mut self,
        batch: &mut SpriteBatch,
        camera: &mut Camera,
        surface: &wgpu::Surface,
    ) -> EngineResult<()> {
        if self.bind_group.is_none() {
            return Err(EngineError::GraphicsInitError("No bind group created".to_string()));
        }
        
        // Update uniforms
        let uniforms = Uniforms {
            view_proj: camera.view_proj_matrix().to_cols_array_2d(),
            screen_size: [self.config.width as f32, self.config.height as f32],
            _padding: [0.0; 2],
        };
        
        self.queue.write_buffer(
            &self.uniform_buffer,
            0,
            bytemuck::cast_slice(&[uniforms]),
        );
        
        // Generate mesh from batch
        let (vertices, indices) = batch.generate_mesh(camera);
        
        if vertices.is_empty() {
            return Ok(()); // Nothing to render
        }
        
        // Upload to GPU
        if !self.sprite_renderer.upload_batch(&self.queue, vertices, indices) {
            return Err(EngineError::GraphicsInitError("Failed to upload sprite batch".to_string()));
        }
        
        // Get surface texture
        let output = surface.get_current_texture()
            .map_err(|e| EngineError::GraphicsInitError(e.to_string()))?;
        
        let view = output.texture.create_view(&wgpu::TextureViewDescriptor::default());
        
        // Create depth texture
        let depth_texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Depth Texture"),
            size: wgpu::Extent3d {
                width: self.config.width,
                height: self.config.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Depth32Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        
        let depth_view = depth_texture.create_view(&wgpu::TextureViewDescriptor::default());
        
        // Create command encoder
        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Render Encoder"),
        });
        
        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Render Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.0,
                            g: 0.0,
                            b: 0.0,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            
            render_pass.set_pipeline(&self.sprite_pipeline);
            render_pass.set_bind_group(0, self.bind_group.as_ref().unwrap(), &[]);
            render_pass.set_vertex_buffer(0, self.sprite_renderer.vertex_buffer().slice(..));
            render_pass.set_index_buffer(
                self.sprite_renderer.index_buffer().slice(..),
                wgpu::IndexFormat::Uint16,
            );
            render_pass.draw_indexed(0..indices.len() as u32, 0, 0..1);
        }
        
        // Submit commands
        self.queue.submit(std::iter::once(encoder.finish()));
        
        // TODO: Fix screenshot functionality - texture needs COPY_SRC usage
        // static SCREENSHOT_TAKEN: std::sync::Once = std::sync::Once::new();
        // SCREENSHOT_TAKEN.call_once(|| {
        //     self.take_screenshot(&output.texture);
        // });
        
        output.present();
        
        Ok(())
    }
    
    /// Get device reference
    pub fn device(&self) -> &wgpu::Device {
        &self.device
    }
    
    /// Get queue reference
    pub fn queue(&self) -> &wgpu::Queue {
        &self.queue
    }
    
    /// Configure a surface with this renderer
    pub fn configure_surface(&self, surface: &wgpu::Surface, width: u32, height: u32) -> EngineResult<()> {
        let surface_caps = surface.get_capabilities(&self.adapter);
        let surface_format = surface_caps
            .formats
            .iter()
            .find(|f| f.is_srgb())
            .copied()
            .unwrap_or(surface_caps.formats[0]);

        let surface_config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface_format,
            width,
            height,
            present_mode: wgpu::PresentMode::Fifo,
            alpha_mode: surface_caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };

        surface.configure(&self.device, &surface_config);
        Ok(())
    }

    /// Render the chunk to a debug texture and save it
    /// Render chunk using authentic Wurfel Engine block face batch rendering
    pub fn render_chunk_to_file_authentic(
        &mut self,
        chunk: &Chunk,
        camera: &mut crate::graphics::Camera,
        filename: &str,
    ) -> EngineResult<()> {
        // Create white texture if needed
        if self.sprite_atlas.is_none() {
            let white_texture = crate::graphics::Texture::create_white_texture(&self.device, &self.queue)?;
            self.create_bind_group(&white_texture);
            log::info!("Using white texture for block face rendering");
        }

        // Create authentic block face batch
        let mut block_batch = crate::graphics::BlockFaceBatch::new();
        block_batch.add_chunk(chunk);
        
        let (vertices, indices) = block_batch.get_mesh_data();
        log::info!("Generated {} vertices ({} faces) for authentic block rendering", 
            vertices.len(), vertices.len() / 4);
        
        if vertices.is_empty() {
            log::warn!("No block faces generated");
            return Ok(());
        }

        // Render using the block face vertices instead of sprites
        self.render_block_faces(vertices, indices, camera, filename)
    }

    /// Render block faces directly (bypassing sprite system)
    fn render_block_faces(
        &mut self,
        vertices: &[SpriteVertex],
        indices: &[u16],
        camera: &mut crate::graphics::Camera,
        filename: &str,
    ) -> EngineResult<()> {
        // Create render texture
        let texture_extent = wgpu::Extent3d {
            width: 1024,
            height: 768,
            depth_or_array_layers: 1,
        };
        
        let render_texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Block Face Render Texture"),
            size: texture_extent,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Bgra8UnormSrgb,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        
        let render_view = render_texture.create_view(&wgpu::TextureViewDescriptor::default());
        
        // Create depth buffer (required by sprite pipeline)
        let depth_texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Block Face Depth Texture"),
            size: texture_extent,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Depth32Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        
        let depth_view = depth_texture.create_view(&wgpu::TextureViewDescriptor::default());

        // Upload block face vertices to GPU
        if !self.sprite_renderer.upload_batch(&self.queue, vertices, indices) {
            return Err(EngineError::GraphicsInitError("Failed to upload block faces".to_string()));
        }

        // Update camera uniforms
        let uniforms = Uniforms {
            view_proj: camera.view_proj_matrix().to_cols_array_2d(),
            screen_size: [texture_extent.width as f32, texture_extent.height as f32],
            _padding: [0.0; 2],
        };
        
        self.queue.write_buffer(
            &self.uniform_buffer,
            0,
            bytemuck::cast_slice(&[uniforms]),
        );

        // Render block faces
        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Block Face Render Encoder"),
        });

        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Block Face Render Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &render_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.0,
                            g: 0.0,
                            b: 0.0,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            render_pass.set_pipeline(&self.sprite_pipeline);
            render_pass.set_bind_group(0, self.bind_group.as_ref().unwrap(), &[]);
            render_pass.set_vertex_buffer(0, self.sprite_renderer.vertex_buffer().slice(..));
            render_pass.set_index_buffer(
                self.sprite_renderer.index_buffer().slice(..),
                wgpu::IndexFormat::Uint16,
            );
            render_pass.draw_indexed(0..indices.len() as u32, 0, 0..1);
        }

        // Copy to CPU and save as PNG
        let buffer_size = (texture_extent.width * texture_extent.height * 4) as u64;
        let output_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Block Face Output Buffer"),
            size: buffer_size,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        encoder.copy_texture_to_buffer(
            wgpu::ImageCopyTexture {
                texture: &render_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::ImageCopyBuffer {
                buffer: &output_buffer,
                layout: wgpu::ImageDataLayout {
                    offset: 0,
                    bytes_per_row: Some(texture_extent.width * 4),
                    rows_per_image: Some(texture_extent.height),
                },
            },
            texture_extent,
        );

        self.queue.submit(std::iter::once(encoder.finish()));

        // Map buffer and save (simplified approach)
        let buffer_slice = output_buffer.slice(..);
        let (sender, receiver) = std::sync::mpsc::channel();
        buffer_slice.map_async(wgpu::MapMode::Read, move |v| {
            sender.send(v).unwrap();
        });

        self.device.poll(wgpu::Maintain::wait()).panic_on_timeout();
        if let Ok(Ok(_)) = receiver.recv() {
            let data = buffer_slice.get_mapped_range();
            let buffer: &[u8] = &data;
            
            if let Some(img) = image::RgbaImage::from_raw(
                texture_extent.width, 
                texture_extent.height, 
                buffer.to_vec()
            ) {
                let img = image::DynamicImage::ImageRgba8(img).flipv();
                img.save(filename).map_err(|e| EngineError::IoError(
                    std::io::Error::new(std::io::ErrorKind::Other, e.to_string())
                ))?;
                log::info!("Authentic block face render saved to {} ({}x{})", 
                    filename, texture_extent.width, texture_extent.height);
            }
        }

        Ok(())
    }

    pub fn render_chunk_to_file(
        &mut self,
        chunk: &Chunk,
        camera: &mut crate::graphics::Camera,
        filename: &str,
    ) -> EngineResult<()> {
        // Create a simple white texture for colored rendering if no atlas is loaded
        if self.sprite_atlas.is_none() {
            let white_texture = crate::graphics::Texture::create_white_texture(&self.device, &self.queue)?;
            self.create_bind_group(&white_texture);
            log::info!("Using white texture for colored block rendering");
        }

        // Create a texture we can copy from
        let texture_extent = wgpu::Extent3d {
            width: 1024,
            height: 768,
            depth_or_array_layers: 1,
        };
        
        let render_texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Chunk Render Texture"),
            size: texture_extent,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Bgra8UnormSrgb,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        
        // Create depth texture too
        let depth_texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Chunk Depth Texture"),
            size: texture_extent,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Depth32Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        
        let render_view = render_texture.create_view(&wgpu::TextureViewDescriptor::default());
        let depth_view = depth_texture.create_view(&wgpu::TextureViewDescriptor::default());
        
        // Create sprite batch from chunk
        let mut batch = crate::graphics::SpriteBatch::new();
        
        let mut sprite_count = 0;
        for ((x, y, z), block) in chunk.blocks() {
            // Render all blocks with authentic coordinates
            
            if !block.is_visible() {
                continue;
            }
            
            let world_coord = chunk.top_left_coordinate();
            let block_coord = crate::map::Coordinate::new_with_coords(
                world_coord.x + x as i32,
                world_coord.y + y as i32,
                world_coord.z + z as i32,
            );
            
            // Use colored blocks with authentic Wurfel Engine coordinates
            if let Some(sprite) = crate::graphics::Sprite::from_block_colored(block, &block_coord) {
                batch.add_sprite(sprite);
                sprite_count += 1;
            }
        }
        
        log::info!("Rendering {} sprites to {}", sprite_count, filename);
        
        // Update uniforms
        let uniforms = Uniforms {
            view_proj: camera.view_proj_matrix().to_cols_array_2d(),
            screen_size: [texture_extent.width as f32, texture_extent.height as f32],
            _padding: [0.0; 2],
        };
        
        self.queue.write_buffer(
            &self.uniform_buffer,
            0,
            bytemuck::cast_slice(&[uniforms]),
        );
        
        // Generate mesh from batch
        let (vertices, indices) = batch.generate_mesh(camera);
        
        if vertices.is_empty() {
            log::warn!("No vertices generated for chunk render");
            return Ok(());
        }
        
        // Upload to GPU
        if !self.sprite_renderer.upload_batch(&self.queue, vertices, indices) {
            return Err(EngineError::GraphicsInitError("Failed to upload sprite batch".to_string()));
        }
        
        // Render
        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Chunk Render Encoder"),
        });

        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Chunk Render Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &render_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.0,
                            g: 0.0,
                            b: 0.0,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            
            if self.bind_group.is_some() {
                render_pass.set_pipeline(&self.sprite_pipeline);
                render_pass.set_bind_group(0, self.bind_group.as_ref().unwrap(), &[]);
                render_pass.set_vertex_buffer(0, self.sprite_renderer.vertex_buffer().slice(..));
                render_pass.set_index_buffer(
                    self.sprite_renderer.index_buffer().slice(..),
                    wgpu::IndexFormat::Uint16,
                );
                render_pass.draw_indexed(0..indices.len() as u32, 0, 0..1);
            }
        }
        
        // Copy to buffer for saving
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Chunk Screenshot Buffer"),
            size: (texture_extent.width * texture_extent.height * 4) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        encoder.copy_texture_to_buffer(
            wgpu::ImageCopyTexture {
                texture: &render_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::ImageCopyBuffer {
                buffer: &buffer,
                layout: wgpu::ImageDataLayout {
                    offset: 0,
                    bytes_per_row: Some(texture_extent.width * 4),
                    rows_per_image: Some(texture_extent.height),
                },
            },
            texture_extent,
        );

        self.queue.submit(std::iter::once(encoder.finish()));
        
        // Save the buffer
        self.save_buffer_to_file(&buffer, texture_extent.width, texture_extent.height, filename);
        
        Ok(())
    }
    
    /// Create a simple debug render to file
    pub fn create_debug_render(&mut self) -> EngineResult<()> {
        // Create a texture we can copy from
        let texture_extent = wgpu::Extent3d {
            width: 512,  // Smaller size for debugging
            height: 512,
            depth_or_array_layers: 1,
        };
        
        let render_texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Debug Render Texture"),
            size: texture_extent,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        
        let render_view = render_texture.create_view(&wgpu::TextureViewDescriptor::default());
        
        // Create a simple red square to test the screenshot system
        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Debug Render Encoder"),
        });

        {
            let _render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Debug Render Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &render_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.8,
                            g: 0.2,
                            b: 0.2,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
        }
        
        // Copy to buffer for saving
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Debug Screenshot Buffer"),
            size: (512 * 512 * 4) as u64, // RGBA = 4 bytes per pixel
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        encoder.copy_texture_to_buffer(
            wgpu::ImageCopyTexture {
                texture: &render_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::ImageCopyBuffer {
                buffer: &buffer,
                layout: wgpu::ImageDataLayout {
                    offset: 0,
                    bytes_per_row: Some(512 * 4),
                    rows_per_image: Some(512),
                },
            },
            texture_extent,
        );

        self.queue.submit(std::iter::once(encoder.finish()));
        
        // Save the buffer
        self.save_buffer_to_file(&buffer, 512, 512, "debug_render.png");
        
        Ok(())
    }
    
    /// Save a buffer to file as PNG
    fn save_buffer_to_file(&self, buffer: &wgpu::Buffer, width: u32, height: u32, filename: &str) {
        let buffer_slice = buffer.slice(..);
        let (sender, receiver) = std::sync::mpsc::channel();
        
        buffer_slice.map_async(wgpu::MapMode::Read, move |result| {
            sender.send(result).unwrap();
        });
        
        self.device.poll(wgpu::Maintain::Wait);
        
        if let Ok(Ok(())) = receiver.recv() {
            let data = buffer_slice.get_mapped_range();
            
            // Save as PNG using image crate
            if let Some(img) = image::RgbaImage::from_raw(width, height, data.to_vec()) {
                if let Err(e) = img.save(filename) {
                    log::error!("Failed to save {}: {}", filename, e);
                } else {
                    log::info!("Debug render saved to {} ({}x{})", filename, width, height);
                }
            } else {
                log::error!("Failed to create image from buffer data for {}", filename);
            }
        } else {
            log::error!("Failed to map buffer for {}", filename);
        }
    }

    /// Take a screenshot of the current frame (old method - needs fixing)
    fn take_screenshot(&self, texture: &wgpu::Texture) {
        // Create a buffer to read the texture data
        let texture_extent = wgpu::Extent3d {
            width: self.config.width,
            height: self.config.height,
            depth_or_array_layers: 1,
        };
        
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Screenshot Buffer"),
            size: (self.config.width * self.config.height * 4) as u64, // RGBA = 4 bytes per pixel
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Screenshot Encoder"),
        });

        encoder.copy_texture_to_buffer(
            wgpu::ImageCopyTexture {
                texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::ImageCopyBuffer {
                buffer: &buffer,
                layout: wgpu::ImageDataLayout {
                    offset: 0,
                    bytes_per_row: Some(self.config.width * 4),
                    rows_per_image: Some(self.config.height),
                },
            },
            texture_extent,
        );

        self.queue.submit(std::iter::once(encoder.finish()));

        // Map the buffer and save to file
        let buffer_slice = buffer.slice(..);
        let (sender, receiver) = std::sync::mpsc::channel();
        
        buffer_slice.map_async(wgpu::MapMode::Read, move |result| {
            sender.send(result).unwrap();
        });
        
        self.device.poll(wgpu::Maintain::Wait);
        
        if let Ok(Ok(())) = receiver.recv() {
            let data = buffer_slice.get_mapped_range();
            
            // Save as PNG using image crate
            if let Some(img) = image::RgbaImage::from_raw(self.config.width, self.config.height, data.to_vec()) {
                if let Err(e) = img.save("screenshot.png") {
                    log::error!("Failed to save screenshot: {}", e);
                } else {
                    log::info!("Screenshot saved to screenshot.png ({}x{})", self.config.width, self.config.height);
                }
            } else {
                log::error!("Failed to create image from buffer data");
            }
        } else {
            log::error!("Failed to map screenshot buffer");
        }
    }

    /// Resize surface
    pub fn resize(&mut self, width: u32, height: u32) {
        self.config.width = width;
        self.config.height = height;
    }
}