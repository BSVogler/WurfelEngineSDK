use super::{splitmix64, Generator};
use crate::block::{id, Block};
use crate::{CHUNK_SIZE_X, CHUNK_SIZE_Y};

/// The Java chunk height. The pyramid is as tall as the world was there; the world is taller now.
const JAVA_HEIGHT: i32 = 10;

/// A sand floor, a two-block deep sea and one pyramid-shaped mountain, ported from the Java
/// `IslandGenerator`. The Java version picked the peak with `Math.random()`; here the seed does.
///
/// There is exactly one mountain: Java hands the generator absolute block coordinates (see
/// `Chunk.fill`) and the formula has a single peak, so the rest of an unbounded map is open sea.
pub struct IslandGenerator {
    peak_x: i32,
    peak_y: i32,
}

impl IslandGenerator {
    pub fn new(seed: u64) -> Self {
        let mut state = seed;
        let peak_x = (splitmix64(&mut state) % CHUNK_SIZE_X as u64) as i32;
        let peak_y = (splitmix64(&mut state) % CHUNK_SIZE_Y as u64) as i32;
        Self { peak_x, peak_y }
    }

    /// An island with the mountain at a chosen block column.
    pub fn with_peak(peak_x: i32, peak_y: i32) -> Self {
        Self { peak_x, peak_y }
    }

    /// Block column of the mountain peak.
    pub fn peak(&self) -> (i32, i32) {
        (self.peak_x, self.peak_y)
    }
}

impl Generator for IslandGenerator {
    fn generate(&self, x: i32, y: i32, z: i32) -> Block {
        if z == 0 {
            return Block::new(id::SAND, 0);
        }

        let height = JAVA_HEIGHT - 1 - (self.peak_y - y).abs() - (self.peak_x - x).abs();
        if height > 0 && z < height {
            return if height - 1 == z && z > 2 {
                Block::new(id::GRASS, 0)
            } else if z > 2 {
                Block::new(id::DIRT, 0)
            } else {
                Block::new(id::SAND, 0)
            };
        }

        if z == 1 || z == 2 {
            return Block::new(id::WATER, 0);
        }
        Block::AIR
    }

    fn spawn_point(&self) -> (i32, i32) {
        self.peak()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generator::fixture;

    #[test]
    fn island_layers() {
        let g = IslandGenerator::new(1);
        let (px, py) = g.peak();
        // Far away from the peak: sand floor, two layers of water, then air.
        let (fx, fy) = (px + 1000, py + 1000);
        assert_eq!(g.generate(fx, fy, 0).id(), id::SAND);
        assert_eq!(g.generate(fx, fy, 1).id(), id::WATER);
        assert_eq!(g.generate(fx, fy, 2).id(), id::WATER);
        assert!(g.generate(fx, fy, 3).is_air());
        // The peak column is solid up to height 8 with grass on top.
        assert_eq!(g.generate(px, py, 8).id(), id::GRASS);
        assert!(g.generate(px, py, 9).is_air());
        assert_eq!(g.generate(px, py, 5).id(), id::DIRT);
    }

    #[test]
    fn same_seed_same_world() {
        assert_eq!(IslandGenerator::new(7).peak(), IslandGenerator::new(7).peak());
    }

    #[test]
    fn seeded_peaks_are_inside_the_first_chunk() {
        for seed in 0..200 {
            let (x, y) = IslandGenerator::new(seed).peak();
            assert!((0..CHUNK_SIZE_X).contains(&x) && (0..CHUNK_SIZE_Y).contains(&y), "seed {seed}");
        }
    }

    /// The real Java generator, with its random peak pinned to three places (including the corner
    /// of the first chunk), over a window that reaches beyond the mountain on every side.
    #[test]
    fn matches_the_java_generator_for_three_peaks() {
        let mut peak = (0, 0);
        let mut section = String::new();
        let mut sections = 0;
        let check = |peak: (i32, i32), section: &str| {
            let columns = fixture::columns(section);
            assert_eq!(columns.len(), 18 * 34);
            fixture::assert_generator_matches(&IslandGenerator::with_peak(peak.0, peak.1), &columns);
        };
        for line in include_str!("../../fixtures/generators/island.txt").lines() {
            if let Some(rest) = line.strip_prefix("peak ") {
                if !section.is_empty() {
                    check(peak, &section);
                    sections += 1;
                    section.clear();
                }
                let mut parts = rest.split_whitespace().map(|p| p.parse::<i32>().unwrap());
                peak = (parts.next().unwrap(), parts.next().unwrap());
            } else {
                section.push_str(line);
                section.push('\n');
            }
        }
        check(peak, &section);
        assert_eq!(sections + 1, 3);
    }
}
