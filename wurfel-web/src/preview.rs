//! Top-down preview image of a map generator, for the "Create map" screen.
//!
//! The generator runs right here in the browser (no server round trip): the topmost block of
//! every column around the spawn point, flat-coloured like the minimap and shaded by height.

use wurfel_sim::generator::create_generator;
use wurfel_sim::CHUNK_SIZE_Z;

use crate::minimap::block_color;

/// Columns shown, in both directions, centred on the generator's spawn point.
pub const PREVIEW_SIZE: u32 = 160;

/// RGBA pixels (`size` x `size`, row-major) of the generator `id` with `seed`, or `None` for an
/// unknown id. Columns without any block stay transparent.
pub fn render(id: &str, seed: u64, size: u32) -> Option<Vec<u8>> {
    let generator = create_generator(id, seed)?;
    let (sx, sy) = generator.spawn_point();
    let half = size as i32 / 2;
    let mut pixels = vec![0u8; (size * size * 4) as usize];
    for row in 0..size as i32 {
        for col in 0..size as i32 {
            let (x, y) = (sx - half + col, sy - half + row);
            let Some(z) = (0..CHUNK_SIZE_Z).rev().find(|&z| !generator.generate(x, y, z).is_air()) else { continue };
            let block = generator.generate(x, y, z);
            let shade = 0.55 + 0.7 * (z + 1) as f32 / CHUNK_SIZE_Z as f32;
            let i = ((row as u32 * size + col as u32) * 4) as usize;
            let [r, g, b] = block_color(block).map(|c| (c as f32 * shade).round().clamp(0.0, 255.0) as u8);
            pixels[i..i + 4].copy_from_slice(&[r, g, b, 255]);
        }
    }
    Some(pixels)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_generator_makes_an_image_of_the_right_size() {
        for info in wurfel_sim::generator::generators() {
            let pixels = render(info.id, 1, 16).expect(info.id);
            assert_eq!(pixels.len(), 16 * 16 * 4, "{}", info.id);
        }
        assert!(render("nope", 1, 16).is_none());
    }

    #[test]
    fn previews_show_land_and_follow_the_seed() {
        let island = render("island", 1, 64).unwrap();
        assert!(island.chunks_exact(4).any(|p| p[0] > 60 && p[2] < 80), "the mountain is in the middle");
        assert_ne!(render("terrain", 1, 64).unwrap(), render("terrain", 2, 64).unwrap());
    }
}
