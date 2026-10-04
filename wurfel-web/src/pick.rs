//! Mouse picking: which block is under a screen position?
//!
//! Every screen point corresponds to a line through the world along the camera direction. Walking
//! that line from the sky downwards, the first solid block is the one that is visible there.

use wurfel_sim::grid::from_iso;
use wurfel_sim::{World, CHUNK_SIZE_Z};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pick {
    /// The visible block under the pointer.
    pub hit: (i32, i32, i32),
    /// The empty cell the ray passed just before it, i.e. where a new block would go.
    pub place: (i32, i32, i32),
}

/// `(sx, sy)` is a position in screen space: pixels at zoom 1 with y pointing down, the same space
/// `shader.wgsl` projects into.
pub fn pick(world: &World, sx: f32, sy: f32) -> Option<Pick> {
    const STEP: f32 = 0.05;
    let mut previous = None;
    let mut gz = CHUNK_SIZE_Z as f32;
    while gz >= 0.0 {
        // Invert the projection for a fixed height: sx = (gx - gy) * 100, sy = (gx + gy) * 50 - gz * 122.
        let diff = sx / 100.0;
        let sum = (sy + gz * 122.0) / 50.0;
        let (x, y) = from_iso((sum + diff) / 2.0, (sum - diff) / 2.0);
        let z = gz.floor() as i32;
        // Terrain that has not arrived yet cannot be picked.
        if world.has_generator() || world.is_loaded_at(x, y) {
            if !world.get(x, y, z).is_air() {
                return previous.map(|place| Pick { hit: (x, y, z), place });
            }
            previous = Some((x, y, z));
        } else {
            previous = None;
        }
        gz -= STEP;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use wurfel_sim::grid::to_iso;
    use wurfel_sim::{Block, Generator};

    /// Air with a single stone block at (5, 5, 0), plus optionally one right behind and above it.
    struct Pillar(Vec<(i32, i32, i32)>);
    impl Generator for Pillar {
        fn generate(&self, x: i32, y: i32, z: i32) -> Block {
            if self.0.contains(&(x, y, z)) { Block::new(3, 0) } else { Block::AIR }
        }
    }

    fn screen(gx: f32, gy: f32, gz: f32) -> (f32, f32) {
        ((gx - gy) * 100.0, (gx + gy) * 50.0 - gz * 122.0)
    }

    #[test]
    fn clicking_the_top_face_places_on_top() {
        let world = World::new(Pillar(vec![(5, 5, 0)]));
        let (gx, gy) = to_iso(5, 5);
        let (sx, sy) = screen(gx, gy, 1.0);
        let pick = pick(&world, sx, sy).expect("block under pointer");
        assert_eq!(pick.hit, (5, 5, 0));
        assert_eq!(pick.place, (5, 5, 1));
    }

    #[test]
    fn clicking_the_side_faces_hits_the_same_block_from_the_front() {
        let world = World::new(Pillar(vec![(5, 5, 0)]));
        let (gx, gy) = to_iso(5, 5);
        for (fx, fy) in [(gx + 0.5, gy), (gx, gy + 0.5)] {
            // Middle of the +x face and of the +y face, half way up the block.
            let (sx, sy) = screen(fx, fy, 0.5);
            let pick = pick(&world, sx, sy).expect("block under pointer");
            assert_eq!(pick.hit, (5, 5, 0));
            assert!(world.get(pick.place.0, pick.place.1, pick.place.2).is_air());
            assert_ne!(pick.place, pick.hit);
        }
    }

    #[test]
    fn sky_and_out_of_world_are_misses() {
        let world = World::new(Pillar(vec![(5, 5, 0)]));
        assert_eq!(pick(&world, 3000.0, -500.0), None);
        assert_eq!(pick(&world, 0.0, 0.0), None);
    }

    #[test]
    fn the_nearer_block_wins_over_one_behind_it() {
        // A three-high column one block-diagonal in front of (5, 5, 0) hides it.
        let near = (6, 7, 0);
        let world = World::new(Pillar(vec![(5, 5, 0), near, (6, 7, 1), (6, 7, 2)]));
        let (gx, gy) = to_iso(near.0, near.1);
        let (sx, sy) = screen(gx, gy, 3.0); // top of the near column
        assert_eq!(pick(&world, sx, sy).unwrap().hit, (6, 7, 2));
    }
}
