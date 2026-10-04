//! Geometry of the staggered block grid used by the Java engine.
//!
//! A block `(x, y, z)` is a cube whose footprint is a 2:1 diamond on screen. Each step in `y` moves
//! half a block down the screen and shifts odd rows half a block to the right, so a rectangular map
//! shows up as a rectangle. This is why the chunk height in `y` must be even.
//!
//! For rendering and depth it is convenient to describe the same block in plain isometric ground
//! coordinates `(gx, gy)`, where one unit is one block edge and the screen position is
//! `sx = (gx - gy) * W/2`, `sy = (gx + gy) * D/2`. [`to_iso`] converts between the two.

use crate::{CHUNK_SIZE_X, CHUNK_SIZE_Y};

/// 1 if the row is odd (shifted half a block right), 0 otherwise. Correct for negative `y`.
pub fn row_offset(y: i32) -> i32 {
    y.rem_euclid(2)
}

/// The neighbour that touches the lower-left (front-left) side of the block.
pub fn lower_left(x: i32, y: i32) -> (i32, i32) {
    (x - 1 + row_offset(y), y + 1)
}

/// The neighbour that touches the lower-right (front-right) side of the block.
pub fn lower_right(x: i32, y: i32) -> (i32, i32) {
    (x + row_offset(y), y + 1)
}

/// Centre of the block's footprint in isometric ground coordinates.
pub fn to_iso(x: i32, y: i32) -> (f32, f32) {
    let diff = (2 * x + row_offset(y)) as f32; // gx - gy
    let sum = y as f32; // gx + gy
    ((sum + diff) / 2.0, (sum - diff) / 2.0)
}

/// The block column whose footprint contains the isometric ground point `(gx, gy)`.
///
/// Block centres sit on the integer lattice in `(gx, gy)` and every footprint is the unit square
/// around its centre, so this is just rounding followed by undoing [`to_iso`].
pub fn from_iso(gx: f32, gy: f32) -> (i32, i32) {
    let (cx, cy) = (gx.round() as i32, gy.round() as i32);
    let y = cx + cy; // gx + gy
    let x = (cx - cy - row_offset(y)) / 2; // gx - gy = 2x + offset, always even after the subtraction
    (x, y)
}

/// Chunk containing the block column `(x, y)`. Rounds towards negative infinity.
pub fn chunk_of(x: i32, y: i32) -> (i32, i32) {
    (x.div_euclid(CHUNK_SIZE_X), y.div_euclid(CHUNK_SIZE_Y))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lower_neighbours_are_one_iso_unit_away() {
        for y in -3..4 {
            for x in -3..4 {
                let (gx, gy) = to_iso(x, y);
                let (rx, ry) = lower_right(x, y);
                assert_eq!(to_iso(rx, ry), (gx + 1.0, gy));
                let (lx, ly) = lower_left(x, y);
                assert_eq!(to_iso(lx, ly), (gx, gy + 1.0));
            }
        }
    }

    #[test]
    fn from_iso_inverts_to_iso_and_covers_the_whole_footprint() {
        for y in -5..6 {
            for x in -5..6 {
                let (gx, gy) = to_iso(x, y);
                assert_eq!(from_iso(gx, gy), (x, y));
                for (dx, dy) in [(0.49, 0.49), (-0.49, 0.49), (0.49, -0.49), (-0.49, -0.49)] {
                    assert_eq!(from_iso(gx + dx, gy + dy), (x, y), "corner of {x},{y}");
                }
            }
        }
    }

    #[test]
    fn negative_coordinates_land_in_negative_chunks() {
        assert_eq!(chunk_of(-1, -1), (-1, -1));
        assert_eq!(chunk_of(0, 0), (0, 0));
        assert_eq!(chunk_of(CHUNK_SIZE_X, CHUNK_SIZE_Y - 1), (1, 0));
        assert_eq!(row_offset(-1), 1);
    }
}
