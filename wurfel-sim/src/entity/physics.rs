//! World queries used by entity physics, ported from `MovableEntity` and `Point.getBlock`.
//!
//! # Units and axes
//!
//! Positions and velocities are in **blocks** (and blocks per second) in the isometric ground frame
//! of [`crate::grid`]: `x = gx`, `y = gy`, `z` up. The Java engine used "game units" (141.42 per
//! block edge) along screen-aligned axes. The two frames differ by a rotation of 45 degrees and a
//! uniform scale, so distances, speeds and angles between vectors are the same in both; only the
//! meaning of "x" and "y" is rotated. One game unit is [`UNIT`] blocks.

use glam::Vec3;

use crate::block::{id, BlockConfig, DefaultBlocks};
use crate::grid::from_iso;
use crate::{Block, World, CHUNK_SIZE_Z};

/// One Java game unit in blocks (`GAME_EDGELENGTH` is 141.42 units).
pub const UNIT: f32 = 1.0 / 141.421_36;
/// Gravity in blocks per second squared (CVar `gravity`).
pub const GRAVITY: f32 = 9.81;
/// Default friction per millisecond (CVar `friction`).
pub const DEFAULT_FRICTION: f32 = 0.001;
/// Height of the world in blocks.
pub const WORLD_HEIGHT: f32 = CHUNK_SIZE_Z as f32;

/// Half the side of the square an entity occupies on the ground, in blocks. The Java engine probes
/// the four diamond tips at 50 game units from the centre, which in this frame are the corners
/// `(+-0.25, +-0.25)`.
const PROBE: f32 = 0.25;
const PROBES: [(f32, f32); 4] = [(-PROBE, -PROBE), (PROBE, PROBE), (-PROBE, PROBE), (PROBE, -PROBE)];

/// What is below the map: the Java engine returns its `groundBlockID` (2, dirt).
const GROUND: Block = Block::new(id::DIRT, 0);
/// A chunk a remote world has not received yet. Like the Java engine, which only lets entities move
/// where chunks are in memory, the unknown is a wall: id 4 is the Java engine's "invisible obstacle".
const BARRIER: Block = Block::new(4, 0);

/// Water (id 9) can be walked into; everything else except air blocks movement. This is the
/// engine's default rule; [`World::blocks`] decides for a world whose game defines more blocks.
pub fn is_obstacle(block: Block) -> bool {
    DefaultBlocks.is_obstacle(block)
}

pub fn is_liquid(block: Block) -> bool {
    DefaultBlocks.is_liquid(block)
}

/// The block at a point in space (`Point.getBlock`).
pub fn block_at(world: &World, p: Vec3) -> Block {
    if p.z >= WORLD_HEIGHT {
        return Block::AIR;
    }
    if p.z < 0.0 {
        return GROUND;
    }
    let (x, y) = from_iso(p.x, p.y);
    if !world.has_generator() && !world.is_loaded_at(x, y) {
        return BARRIER;
    }
    world.get(x, y, p.z.floor() as i32)
}

/// Is any of the four corners of the entity's footprint inside an obstacle at this height?
/// (`checkCollisionCorners`)
fn corners_blocked(world: &World, pos: Vec3) -> bool {
    PROBES.iter().any(|&(dx, dy)| world.blocks().is_obstacle(block_at(world, Vec3::new(pos.x + dx, pos.y + dy, pos.z))))
}

/// Horizontal collision at `pos` (`collidesWithWorld`). `dimension_z` is the entity's height.
///
/// The Java method also tried to test the head, but resets `z` before doing so, so in effect it
/// only checks the feet and, for entities taller than a block, the middle. That behaviour is kept:
/// a tall entity is blocked sideways only by obstacles at its feet and waist.
pub fn collides_with_world(world: &World, pos: Vec3, dimension_z: f32) -> bool {
    if pos.z > WORLD_HEIGHT {
        return false;
    }
    if pos.z < 0.0 {
        return true;
    }
    if corners_blocked(world, pos) {
        return true;
    }
    dimension_z > 1.0 && corners_blocked(world, pos + Vec3::new(0.0, 0.0, dimension_z / 2.0))
}

/// Is the top of the entity touching a block above it? (`isOnCeil`)
pub fn is_on_ceil(world: &World, pos: Vec3, dimension_z: f32) -> bool {
    if pos.z <= 0.0 || pos.z > WORLD_HEIGHT {
        return false;
    }
    corners_blocked(world, pos + Vec3::new(0.0, 0.0, dimension_z))
}

/// Is something solid within one game unit below the feet? (`MovableEntity.isOnGround`)
pub fn is_on_ground(world: &World, pos: Vec3, dimension_z: f32) -> bool {
    if pos.z <= 0.0 {
        return true;
    }
    if pos.z > WORLD_HEIGHT {
        return false;
    }
    let below = pos - Vec3::new(0.0, 0.0, UNIT);
    world.blocks().is_obstacle(block_at(world, below)) || collides_with_world(world, below, dimension_z)
}

pub fn is_in_liquid(world: &World, pos: Vec3) -> bool {
    world.blocks().is_liquid(block_at(world, pos))
}

/// Height at which something standing in column `(x, y)` would rest: the top of the highest
/// obstacle. 0 for a column without ground.
pub fn ground_height(world: &World, x: i32, y: i32) -> f32 {
    (0..CHUNK_SIZE_Z)
        .rev()
        .find(|&z| world.blocks().is_obstacle(world.get(x, y, z)))
        .map_or(0.0, |z| (z + 1) as f32)
}

/// Every block cell that a body at `pos` with the given height overlaps. Used to refuse placing a
/// block inside someone.
pub fn occupied_cells(pos: Vec3, dimension_z: f32) -> Vec<(i32, i32, i32)> {
    let mut cells = Vec::new();
    let z0 = pos.z.floor() as i32;
    let z1 = (pos.z + dimension_z - 1e-4).floor() as i32;
    for &(dx, dy) in &PROBES {
        let (x, y) = from_iso(pos.x + dx, pos.y + dy);
        for z in z0..=z1 {
            if !cells.contains(&(x, y, z)) {
                cells.push((x, y, z));
            }
        }
    }
    cells
}
