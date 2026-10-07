//! The cracks on a damaged block (Java `RenderCell`, `renderDamageOverlay`): below 100 health the
//! block wears the entity sprite 3, one picture per side, a stronger one at 50 and at 25 health, drawn
//! grey at 70% opacity over the block's own sides.
//!
//! The server tells the client when a block loses health (`BlockDamaged`); the client keeps those
//! blocks in [`Damaged`] and draws the cracks with the dynamic vertices, so the chunk meshes do not
//! have to be rebuilt with every hit.

use std::collections::HashMap;

use wurfel_sim::grid::{from_iso, to_iso};
use wurfel_sim::light::is_opaque_in;
use wurfel_sim::World;

use crate::mesh::{overlay_face, Vertex};
use crate::sprites::Sprites;

/// Blocks that were hit, with their health (1 to 99) as the server last said.
pub type Damaged = HashMap<(i32, i32, i32), u8>;

/// Java: 0.5 grey at 0.7 alpha.
const TINT: [f32; 3] = [0.5; 3];
const ALPHA: f32 = 0.7;
/// How far the cracks float off the block's side, in blocks. More than the depth peeling's margin,
/// so the side stays a surface of its own behind the see-through cracks.
const LIFT: f32 = 0.03;

/// Which of the three crack sets a health value shows: 0 up to 100 health left (hit once), 1 at 50 or
/// less, 2 at 25 or less.
pub fn step(health: u8) -> u32 {
    if health <= 25 {
        2
    } else if health <= 50 {
        1
    } else {
        0
    }
}

/// Add the cracks of every damaged block still standing. Forgets the blocks that are gone.
pub fn push(out: &mut Vec<Vertex>, sprites: &Sprites, world: &World, damaged: &mut Damaged) {
    damaged.retain(|&(x, y, z), health| *health > 0 && *health < 100 && !world.get(x, y, z).is_air());
    for (&(x, y, z), &health) in damaged.iter() {
        let base = 3 * step(health);
        let (gx, gy) = to_iso(x, y);
        let (x0, x1, y0, y1) = (gx - 0.5, gx + 0.5, gy - 0.5, gy + 0.5);
        let (z0, z1) = (z as f32, z as f32 + 1.0);
        let covered = |bx: i32, by: i32, bz: i32| is_opaque_in(world, bx, by, bz);
        let (lx, ly) = from_iso(gx, gy + 1.0);
        let (rx, ry) = from_iso(gx + 1.0, gy);
        // Left (+y), top, right (+x), the sprite values 0, 1 and 2 of a set.
        let sides = [
            (base, covered(lx, ly, z), [[x0, y1 + LIFT, z0], [x1, y1 + LIFT, z0], [x1, y1 + LIFT, z1], [x0, y1 + LIFT, z1]]),
            (base + 1, covered(x, y, z + 1), [[x0, y0, z1 + LIFT], [x1, y0, z1 + LIFT], [x1, y1, z1 + LIFT], [x0, y1, z1 + LIFT]]),
            (base + 2, covered(rx, ry, z), [[x1 + LIFT, y0, z0], [x1 + LIFT, y1, z0], [x1 + LIFT, y1, z1], [x1 + LIFT, y0, z1]]),
        ];
        for (value, hidden, corners) in sides {
            if hidden {
                continue;
            }
            if let Some(region) = sprites.entity(3, value) {
                overlay_face(out, TINT, corners, &sprites.atlas, region, ALPHA);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_cracks_get_stronger_at_50_and_25_health() {
        assert_eq!([99, 51, 50, 26, 25, 1].map(step), [0, 0, 1, 1, 2, 2]);
    }
}
