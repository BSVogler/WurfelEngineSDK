use super::{block_from_java_int, Generator};
use crate::Block;

/// Number of block types the engine knows (`RenderCell.OBJECTTYPESNUM`).
pub const OBJECT_TYPES_NUM: i32 = 124;

/// A floor made of one row of every block type, to look at them all (`BlockTestGenerator`): at
/// z = 0 the block id is `|y| % 124`, everything above is air.
pub struct BlockTestGenerator;

impl Generator for BlockTestGenerator {
    fn generate(&self, _x: i32, y: i32, z: i32) -> Block {
        if z == 0 {
            // Java: `(byte) (Math.abs(y) % OBJECTTYPESNUM)`. `Math.abs(Integer.MIN_VALUE)` stays
            // negative there, which is what wrapping_abs does too.
            let id = y.wrapping_abs() % OBJECT_TYPES_NUM;
            block_from_java_int(id as i8 as i32)
        } else {
            Block::AIR
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generator::fixture;

    #[test]
    fn matches_the_java_generator_including_the_extreme_rows() {
        let columns = fixture::columns(include_str!("../../fixtures/generators/blocktest.txt"));
        assert_eq!(columns.len(), 391 + 2);
        fixture::assert_generator_matches(&BlockTestGenerator, &columns);
    }

    #[test]
    fn rows_cycle_through_the_block_ids() {
        let g = BlockTestGenerator;
        assert_eq!(g.generate(0, 5, 0).id(), 5);
        assert_eq!(g.generate(0, -5, 0).id(), 5);
        assert_eq!(g.generate(7, 123, 0).id(), 123);
        assert!(g.generate(0, 124, 0).is_air(), "id 0 is air, so every 124th row is empty");
        assert_eq!(g.generate(0, 130, 0).id(), 6);
        assert!(g.generate(0, 5, 1).is_air());
    }
}
