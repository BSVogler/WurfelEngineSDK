//! Block-cell helpers shared by the logic blocks: the neighbour numbering of the Java engine.

use glam::Vec3;
use wurfel_sim::grid::from_iso;
use wurfel_sim::{CHUNK_SIZE_X, CHUNK_SIZE_Y, CHUNK_SIZE_Z, World};

use crate::game::Cell;

/// The four horizontal neighbours power and rails connect through: the diagonals of the staggered
/// grid (`Coordinate.goToNeighbour`: 1 north-east, 3 south-east, 5 south-west, 7 north-west).
pub const SIDES: [u8; 4] = [1, 3, 5, 7];

/// The cell on the given side of `cell` (`Coordinate.goToNeighbour`). Sides 0 to 7 run clockwise from
/// the top of the screen: 0 north, 1 north-east, 2 east, 3 south-east, 4 south, 5 south-west, 6 west,
/// 7 north-west. Anything else is the cell itself.
pub fn neighbour(cell: Cell, side: u8) -> Cell {
    let (x, y, z) = cell;
    let odd = y.rem_euclid(2);
    let even = 1 - odd;
    match side {
        0 => (x, y - 2, z),
        1 => (x + odd, y - 1, z),
        2 => (x + 1, y, z),
        3 => (x + odd, y + 1, z),
        4 => (x, y + 2, z),
        5 => (x - even, y + 1, z),
        6 => (x - 1, y, z),
        7 => (x - even, y - 1, z),
        _ => cell,
    }
}

/// The side facing the other way (`(id + 4) % 8`).
pub fn opposite(side: u8) -> u8 {
    (side + 4) % 8
}

/// The cell a point is in.
pub fn cell_at(p: Vec3) -> Cell {
    let (x, y) = from_iso(p.x, p.y);
    (x, y, p.z.floor() as i32)
}

/// Every cell of every loaded chunk whose block id is one of `ids`, in a fixed order.
pub fn find_blocks(world: &World, ids: &[u8]) -> Vec<(Cell, u8)> {
    let mut found = Vec::new();
    let mut chunks: Vec<_> = world.loaded_chunks().collect();
    chunks.sort_by_key(|c| c.pos());
    for chunk in chunks {
        let (x0, y0) = chunk.top_left();
        for lx in 0..CHUNK_SIZE_X {
            for ly in 0..CHUNK_SIZE_Y {
                for z in 0..CHUNK_SIZE_Z {
                    let id = chunk.get(lx, ly, z).id();
                    if ids.contains(&id) {
                        found.push(((x0 + lx, y0 + ly, z), id));
                    }
                }
            }
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use wurfel_sim::grid::{lower_left, lower_right, to_iso};

    #[test]
    fn sides_match_the_engines_grid_helpers() {
        for y in -3..4 {
            for x in -3..4 {
                assert_eq!(neighbour((x, y, 5), 3), (lower_right(x, y).0, lower_right(x, y).1, 5));
                assert_eq!(neighbour((x, y, 5), 5), (lower_left(x, y).0, lower_left(x, y).1, 5));
            }
        }
    }

    #[test]
    fn a_neighbour_of_the_opposite_side_is_the_way_back() {
        for y in -3..4 {
            for x in -3..4 {
                for side in 0..8 {
                    let there = neighbour((x, y, 0), side);
                    assert_eq!(neighbour(there, opposite(side)), (x, y, 0), "side {side} from ({x}, {y})");
                }
            }
        }
    }

    #[test]
    fn the_diagonal_neighbours_are_one_iso_unit_away() {
        for y in -3..4 {
            for x in -3..4 {
                let (gx, gy) = to_iso(x, y);
                for side in SIDES {
                    let n = neighbour((x, y, 0), side);
                    let (nx, ny) = to_iso(n.0, n.1);
                    let d = ((nx - gx).powi(2) + (ny - gy).powi(2)).sqrt();
                    assert!((d - 1.0).abs() < 1e-4, "side {side} is {d} away");
                }
            }
        }
    }
}
