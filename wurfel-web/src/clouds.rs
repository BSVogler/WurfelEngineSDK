//! The cloud layer: a tileable coverage texture that the shader projects over the world from above,
//! scrolling with the wind, so clouds seem to drift past overhead and shade the ground.
//!
//! The texture is made here (pure, tested natively): fractal value noise that wraps in both
//! directions, thresholded into puffy clouds with soft edges. One byte per texel: 0 is clear sky,
//! 255 is the heart of a cloud. `shader.wgsl` (`cloud_shade`) samples it at the point where the
//! sun's ray through the surface crosses the cloud height, so shadows slide with the sun.

/// Side of the (square) texture in texels.
pub const SIZE: usize = 256;
/// How many blocks one repeat of the texture covers.
pub const TILE_BLOCKS: f32 = 48.0;
/// Height of the cloud layer above the ground (blocks); sets how far a shadow shifts with the sun.
pub const HEIGHT: f32 = 24.0;
/// How much of the sun the heart of a cloud blocks (0 none, 1 all of it; the ambient light stays).
pub const DEFAULT_STRENGTH: f32 = 0.4;

/// A deterministic hash of a lattice point to 0..1.
fn hash(x: u32, y: u32, seed: u32) -> f32 {
    let mut h = x.wrapping_mul(0x9E37_79B1) ^ y.wrapping_mul(0x85EB_CA77) ^ seed.wrapping_mul(0xC2B2_AE3D);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    h = h.wrapping_mul(0x297A_2D39);
    h ^= h >> 15;
    (h & 0x00FF_FFFF) as f32 / 0x0100_0000 as f32
}

/// Value noise on a lattice of `period` cells that wraps, sampled at `(x, y)` in cell units.
fn value_noise(x: f32, y: f32, period: u32, seed: u32) -> f32 {
    let (x0, y0) = (x.floor(), y.floor());
    let (fx, fy) = (x - x0, y - y0);
    let smooth = |t: f32| t * t * (3.0 - 2.0 * t);
    let (sx, sy) = (smooth(fx), smooth(fy));
    let cell = |dx: u32, dy: u32| hash((x0 as u32).wrapping_add(dx) % period, (y0 as u32).wrapping_add(dy) % period, seed);
    let top = cell(0, 0) + (cell(1, 0) - cell(0, 0)) * sx;
    let bottom = cell(0, 1) + (cell(1, 1) - cell(0, 1)) * sx;
    top + (bottom - top) * sy
}

/// Fractal noise 0..1 that tiles over the unit square.
fn fbm(u: f32, v: f32) -> f32 {
    let (mut sum, mut amplitude, mut total) = (0.0, 1.0, 0.0);
    let mut period = 4u32;
    for octave in 0..5 {
        sum += amplitude * value_noise(u * period as f32, v * period as f32, period, 17 + octave);
        total += amplitude;
        amplitude *= 0.5;
        period *= 2;
    }
    sum / total
}

/// The coverage texture, `SIZE * SIZE` bytes, rows top to bottom.
pub fn generate() -> Vec<u8> {
    let mut texels = Vec::with_capacity(SIZE * SIZE);
    for y in 0..SIZE {
        for x in 0..SIZE {
            let n = fbm(x as f32 / SIZE as f32, y as f32 / SIZE as f32);
            // Below the threshold the sky is clear; above it the cloud thickens quickly, so there
            // are distinct banks with soft edges instead of a uniform haze.
            let t = ((n - 0.5) / 0.2).clamp(0.0, 1.0);
            let cloud = t * t * (3.0 - 2.0 * t);
            texels.push((cloud * 255.0 + 0.5) as u8);
        }
    }
    texels
}

#[cfg(target_arch = "wasm32")]
pub mod gpu {
    use super::{generate, SIZE};

    /// The view and the repeating, linear sampler for bindings 2 and 3 of group 0.
    pub fn create(device: &wgpu::Device, queue: &wgpu::Queue) -> (wgpu::TextureView, wgpu::Sampler) {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("clouds"),
            size: wgpu::Extent3d { width: SIZE as u32, height: SIZE as u32, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            wgpu::TexelCopyTextureInfo { texture: &texture, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            &generate(),
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(SIZE as u32), rows_per_image: Some(SIZE as u32) },
            wgpu::Extent3d { width: SIZE as u32, height: SIZE as u32, depth_or_array_layers: 1 },
        );
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("clouds sampler"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        (texture.create_view(&wgpu::TextureViewDescriptor::default()), sampler)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_texture_has_clear_sky_and_clouds() {
        let texels = generate();
        assert_eq!(texels.len(), SIZE * SIZE);
        let covered = texels.iter().filter(|&&t| t > 128).count() as f32 / texels.len() as f32;
        let clear = texels.iter().filter(|&&t| t == 0).count() as f32 / texels.len() as f32;
        assert!((0.1..0.6).contains(&covered), "cloud cover {covered}");
        assert!(clear > 0.2, "clear sky {clear}");
    }

    #[test]
    fn the_texture_tiles() {
        let texels = generate();
        let at = |x: usize, y: usize| texels[(y % SIZE) * SIZE + x % SIZE] as i32;
        for i in 0..SIZE {
            assert!((at(SIZE - 1, i) - at(SIZE, i)).abs() < 40, "seam in x at row {i}");
            assert!((at(i, SIZE - 1) - at(i, SIZE)).abs() < 40, "seam in y at column {i}");
        }
    }

    #[test]
    fn it_is_deterministic() {
        assert_eq!(generate(), generate());
    }
}
