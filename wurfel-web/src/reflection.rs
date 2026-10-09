//! What the surface of the water mirrors: the scene drawn a second time, flipped about the water plane.
//!
//! The camera is orthographic, so a mirror image needs no special projection: every vertex keeps its
//! screen position except that its height `z` becomes `2 * level - z` (`vs_main` in `shader.wgsl`, when
//! the camera uniform's mirror flag is set). The result is drawn into [`Reflection::color`] with the
//! scene pipeline, from the same camera, so a water pixel finds its reflection at its own window
//! position. The tops of blocks face down once flipped and are left out (we look at them from above),
//! as is everything that lies under the plane, the water itself included; what is left is the sides of
//! what stands on or above the water, and the sprites.
//!
//! One plane serves the whole picture: the most common height of the water around the view
//! ([`level`]); water at another height only mirrors the sky.

#![cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]

use wurfel_sim::block::id;
use wurfel_sim::{World, CHUNK_SIZE_X, CHUNK_SIZE_Y, CHUNK_SIZE_Z};

/// The height of the water surface that most of the water around `chunk` (the 3x3 chunks that are
/// drawn) has, if there is any. A column counts with the top of its highest water block, so a lake
/// under a cliff counts as well.
pub fn level(world: &World, chunk: (i32, i32)) -> Option<f32> {
    let mut counts = [0u32; CHUNK_SIZE_Z as usize + 1];
    for cx in chunk.0 - 1..=chunk.0 + 1 {
        for cy in chunk.1 - 1..=chunk.1 + 1 {
            if !world.is_loaded(cx, cy) {
                continue;
            }
            // Every second column each way is plenty to find the mode.
            for lx in (0..CHUNK_SIZE_X).step_by(2) {
                for ly in (0..CHUNK_SIZE_Y).step_by(2) {
                    let (x, y) = (cx * CHUNK_SIZE_X + lx, cy * CHUNK_SIZE_Y + ly);
                    if let Some(z) = (0..CHUNK_SIZE_Z).rev().find(|&z| world.get(x, y, z).id() == id::WATER) {
                        counts[z as usize + 1] += 1;
                    }
                }
            }
        }
    }
    let (best, count) = counts.iter().enumerate().max_by_key(|&(height, &n)| (n, std::cmp::Reverse(height)))?;
    (*count > 0).then_some(best as f32)
}

#[cfg(target_arch = "wasm32")]
pub mod gpu {
    use crate::peel::gpu::DEPTH_FORMAT;

    /// The mirrored scene and the depth it is drawn with, the size of the canvas.
    pub struct Reflection {
        color: wgpu::TextureView,
        depth: wgpu::TextureView,
        /// A stand-in for `color` in the bind group of the pass that draws it (a texture can not be read
        /// while it is written).
        blank: wgpu::TextureView,
    }

    impl Reflection {
        pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat, width: u32, height: u32) -> Self {
            let texture = |label: &str, format: wgpu::TextureFormat, width: u32, height: u32| {
                device
                    .create_texture(&wgpu::TextureDescriptor {
                        label: Some(label),
                        size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
                        mip_level_count: 1,
                        sample_count: 1,
                        dimension: wgpu::TextureDimension::D2,
                        format,
                        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                        view_formats: &[],
                    })
                    .create_view(&Default::default())
            };
            Reflection {
                color: texture("reflection", format, width, height),
                depth: texture("reflection depth", DEPTH_FORMAT, width, height),
                blank: texture("reflection blank", format, 1, 1),
            }
        }

        /// What the water reads.
        pub fn color(&self) -> &wgpu::TextureView {
            &self.color
        }

        /// What the mirror pass's own bind group holds in its place.
        pub fn blank(&self) -> &wgpu::TextureView {
            &self.blank
        }

        /// Open the mirror pass: cleared to transparent (nothing there: the water shows the sky).
        /// `peel_group` is the scene pipeline's group 2 of the first layer (nothing peeled).
        pub fn begin<'a>(&'a self, encoder: &'a mut wgpu::CommandEncoder, peel_group: &wgpu::BindGroup, timer: Option<&'a crate::gputime::GpuTimer>) -> wgpu::RenderPass<'a> {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("reflection"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.color,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.depth,
                    depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(crate::peel::CLEAR_DEPTH), store: wgpu::StoreOp::Store }),
                    stencil_ops: None,
                }),
                timestamp_writes: crate::gputime::GpuTimer::writes(timer, "water reflection"),
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_bind_group(2, peel_group, &[]);
            pass
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wurfel_sim::{Block, Generator};

    /// Water up to height `top` (blocks, exclusive) wherever `x < 8`, with a deeper pool at `x == 0`.
    struct Lake(i32);
    impl Generator for Lake {
        fn generate(&self, x: i32, _y: i32, z: i32) -> Block {
            let top = if x == 0 { self.0 - 1 } else { self.0 };
            if x < 8 && z < top { Block::new(id::WATER, 0) } else { Block::AIR }
        }
    }

    #[test]
    fn the_level_is_the_top_of_the_water_most_columns_have() {
        let mut world = World::new(Lake(3));
        world.load_chunk(0, 0);
        assert_eq!(level(&world, (0, 0)), Some(3.0));
    }

    #[test]
    fn no_water_no_level() {
        let mut world = World::new(wurfel_sim::AirGenerator);
        world.load_chunk(0, 0);
        assert_eq!(level(&world, (0, 0)), None);
    }

    #[test]
    fn the_shader_flips_about_the_level_and_leaves_out_what_lies_below() {
        let source = include_str!("shader.wgsl");
        assert!(source.contains("p.z = 2.0 * camera.mirror_level - p.z;"));
        assert!(source.contains("in.ground.z < floor_z"));
    }
}
