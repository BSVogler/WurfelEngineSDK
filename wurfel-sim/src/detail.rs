//! The stones of the ground cover (pebbles and rocks), the part of the grass system that grows on
//! dirt, sand and stone: which one lies where, how many there are and what each looks like, no graphics.
//! The blades themselves are [`crate::grass`]; `wurfel-web`'s `grass.rs` draws both as one thing, with
//! one switch and one density.
//!
//! Everything is a pure function of the block's position and kind, so every client agrees and an
//! instance stays where it is when the viewer walks around. The recipe:
//!
//! * A low-frequency **zone** noise ([`zone`], patches about 12 blocks across) says how rocky the
//!   ground is, a **patch** noise ([`patch`], about 8 blocks) how barren it is and a finer **clump**
//!   noise ([`clump`]) breaks every species into groups instead of an even sprinkling.
//! * Each species has a density per block type ([`density`]): pebbles and rocks like stone and the rocky
//!   zones. The expected number per block
//!   is that density times the masks times the user's density; the whole part is placed and the
//!   fraction is a hashed chance for one more.
//! * Instances are placed inside the block's footprint, but about half of them are moved to a border
//!   of the block that shows a side face (`Cell::edges`: no block beside it), so they stand where the
//!   cube's edge would otherwise be a hard line.
//! * Every instance has its own brightness, hue, size and lean, and the colour also follows the
//!   patch noise.
//!
//! Units: positions are offsets in isometric ground blocks from the block's centre (the block's
//! footprint is the square `-0.5..0.5` on both axes, as in [`crate::grid`]), rotations are degrees
//! counter-clockwise, tints multiply the sprite (1 leaves it as it is).

use crate::block::id;
use crate::grass::{hash, mix, to_unit};
use crate::grid::to_iso;

/// What grows. Which picture each one uses is the renderer's business.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Species {
    /// A small stone.
    Pebble,
    /// A larger rock, one of [`ROCK_VARIANTS`] pictures.
    Rock,
}

impl Species {
    pub const ALL: [Species; 2] = [Species::Pebble, Species::Rock];
}

/// How many different pictures a rock has.
pub const ROCK_VARIANTS: u8 = 3;
/// Most instances one block gets over all species.
pub const MAX_PER_CELL: usize = 8;
/// Density setting (instances per block of the most crowded species) that the table is made for.
pub const NORMAL_DENSITY: i32 = 10;
/// Largest density setting.
pub const MAX_DENSITY: i32 = 20;
/// Share of the instances that go to a block border where one is open.
const EDGE_SHARE: f32 = 0.55;
/// How far from the border an edge instance can stand inwards, and how far it may overhang.
const EDGE_DEPTH: f32 = 0.22;
const EDGE_OVERHANG: f32 = 0.03;

/// Which neighbours of a block count as open (a side face is visible), indexed by [`Edge`].
pub type Edges = [bool; 4];

/// The four borders of a block's footprint, named by the isometric ground axis they are on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edge {
    PlusX = 0,
    MinusX = 1,
    PlusY = 2,
    MinusY = 3,
}

/// A block that can carry detail: a top surface with air above it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cell {
    pub x: i32,
    pub y: i32,
    pub z: i32,
    /// The block's id (`block::id`).
    pub block: u8,
    pub edges: Edges,
}

/// One placed piece of detail, everything a renderer needs except the picture.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Instance {
    pub species: Species,
    /// Which picture of the species (always 0 except for rocks).
    pub variant: u8,
    /// Offset from the block's centre in isometric ground blocks.
    pub offset: (f32, f32),
    /// Multiplied into the sprite, every channel in `0..=1`.
    pub tint: [f32; 3],
    /// Width and height factors of the sprite.
    pub scale: (f32, f32),
    /// Rotation in degrees about the foot, counter-clockwise on the screen.
    pub rotation: f32,
}

fn lattice(seed: i32, ix: i32, iy: i32) -> f32 {
    to_unit(mix(&[seed, ix, iy]))
}

/// Smooth value noise in `[0, 1)` at `(x, y)`, features about `scale` long, a different field per `seed`.
pub fn noise(x: f32, y: f32, scale: f32, seed: i32) -> f32 {
    let (x, y) = (x / scale, y / scale);
    let (fx, fy) = (x.floor(), y.floor());
    let (ix, iy) = (fx as i32, fy as i32);
    let smooth = |t: f32| t * t * (3.0 - 2.0 * t);
    let (tx, ty) = (smooth(x - fx), smooth(y - fy));
    let (a, b) = (lattice(seed, ix, iy), lattice(seed, ix + 1, iy));
    let (c, d) = (lattice(seed, ix, iy + 1), lattice(seed, ix + 1, iy + 1));
    let top = a + (b - a) * tx;
    let bottom = c + (d - c) * tx;
    top + (bottom - top) * ty
}

fn smoothstep(lo: f32, hi: f32, v: f32) -> f32 {
    let t = ((v - lo) / (hi - lo)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// How rocky the ground is at the isometric ground point `(gx, gy)`, 0 (soft ground) to 1 (rocky).
pub fn zone(gx: f32, gy: f32) -> f32 {
    smoothstep(0.45, 0.7, noise(gx, gy, 12.0, 0x2011))
}

/// How lush the ground is, 0 (barren, dry) to 1 (lush). Drives the colour of the stones.
pub fn patch(gx: f32, gy: f32) -> f32 {
    noise(gx, gy, 8.0, 0x7a7c)
}

/// Grouping of the stones, 0 to 1: the species only lie where this is high, so they come in clumps.
pub fn clump(gx: f32, gy: f32, species: Species) -> f32 {
    let seed = 0x5c00 + species as i32;
    let n = noise(gx, gy, 3.0, seed) * 0.7 + noise(gx, gy, 1.3, seed + 7) * 0.3;
    smoothstep(0.3, 0.6, n)
}

/// Instances per block of `species` on a block of kind `block` before the masks, at
/// [`NORMAL_DENSITY`]. `edge_only` ones only grow along an open border. Zero for blocks that carry
/// nothing of that species.
fn density(species: Species, block: u8) -> (f32, bool) {
    match (species, block) {
        (Species::Pebble, id::STONE) => (2.2, false),
        (Species::Pebble, id::DIRT) => (1.4, false),
        (Species::Pebble, id::SAND) => (1.6, false),
        (Species::Rock, id::STONE) => (0.7, false),
        (Species::Rock, id::DIRT) => (0.35, false),
        (Species::Rock, id::SAND) => (0.2, false),
        (Species::Rock, id::GRASS) => (0.15, true),
        _ => (0.0, false),
    }
}

/// Does this kind of block carry any detail at all?
pub fn carries_detail(block: u8) -> bool {
    Species::ALL.iter().any(|&s| density(s, block).0 > 0.0)
}

/// The mask of a species at a place: how much the zones and patches favour it, 0 to 1.
fn mask(species: Species, gx: f32, gy: f32) -> f32 {
    let rocky = zone(gx, gy);
    let grouped = clump(gx, gy, species);
    match species {
        Species::Pebble => (0.3 + 0.7 * rocky) * (0.45 + 0.55 * grouped),
        Species::Rock => rocky * (0.3 + 0.7 * grouped),
    }
}

/// The instances of one block at the user's density `density_setting` (0 to [`MAX_DENSITY`],
/// [`NORMAL_DENSITY`] is the table's own) scaled by `lod` (1 near the viewer, less far away).
/// Deterministic: the same cell always gives the same instances in the same order.
pub fn scatter(cell: &Cell, density_setting: i32, lod: f32) -> Vec<Instance> {
    let mut out = Vec::new();
    let scale = density_setting.clamp(0, MAX_DENSITY) as f32 / NORMAL_DENSITY as f32 * lod.max(0.0);
    if scale <= 0.0 {
        return out;
    }
    let (gx, gy) = to_iso(cell.x, cell.y);
    let seed = mix(&[cell.x, cell.y, cell.z, 0x0de7]);
    let any_open = cell.edges.iter().any(|&e| e);
    for species in Species::ALL {
        let (per_block, edge_only) = density(species, cell.block);
        if per_block <= 0.0 || (edge_only && !any_open) {
            continue;
        }
        let expected = per_block * mask(species, gx, gy) * scale * if edge_only { 0.6 } else { 1.0 };
        let chance = to_unit(hash(seed ^ (species as u32 + 1).wrapping_mul(0x9e37_79b9)));
        let count = (expected + chance).floor() as usize;
        for i in 0..count.min(MAX_PER_CELL) {
            if out.len() >= MAX_PER_CELL {
                return out;
            }
            out.push(instance(cell, species, edge_only, i as u32, (gx, gy)));
        }
    }
    out
}

/// Instance `i` of `species` on `cell`.
fn instance(cell: &Cell, species: Species, edge_only: bool, i: u32, ground: (f32, f32)) -> Instance {
    let key = mix(&[cell.x, cell.y, cell.z, species as i32, i as i32, 0x1157]);
    // A stream of independent unit values from the key.
    let draw = |n: u32| to_unit(hash(key ^ n.wrapping_mul(0x85eb_ca6b)));

    // Where: at a border that is open, or anywhere inside the block.
    let open: Vec<usize> = (0..4).filter(|&e| cell.edges[e]).collect();
    let on_edge = !open.is_empty() && (edge_only || draw(1) < EDGE_SHARE);
    let offset = if on_edge {
        let edge = open[(draw(2) * open.len() as f32) as usize % open.len()];
        let along = draw(3) - 0.5;
        let inwards = 0.5 + EDGE_OVERHANG - draw(4) * (EDGE_DEPTH + EDGE_OVERHANG);
        match edge {
            0 => (inwards, along),
            1 => (-inwards, along),
            2 => (along, inwards),
            _ => (along, -inwards),
        }
    } else {
        (draw(5) - 0.5, draw(6) - 0.5)
    };

    // Size and lean.
    let (scale, lean) = match species {
        Species::Pebble => {
            let s = 0.6 + 0.7 * draw(7);
            ((s, s), (draw(9) - 0.5) * 30.0)
        }
        Species::Rock => {
            let s = 0.3 + 0.35 * draw(7);
            ((s, s * (0.85 + 0.3 * draw(8))), (draw(9) - 0.5) * 20.0)
        }
    };
    let variant = if species == Species::Rock { (draw(11) * ROCK_VARIANTS as f32) as u8 % ROCK_VARIANTS } else { 0 };
    let tint = tint(species, cell.block, ground, offset, [draw(13), draw(14), draw(15), draw(16)]);
    Instance { species, variant, offset, tint, scale, rotation: lean }
}

/// The colour of an instance: the species' base colour for this block type, moved along the lush
/// to barren patch noise, then each instance's own brightness and slight hue (`dice`: four unit
/// values).
pub fn tint(species: Species, block: u8, ground: (f32, f32), offset: (f32, f32), dice: [f32; 4]) -> [f32; 3] {
    // The patch is sampled where the instance stands, so neighbours across a block border agree.
    let lush = patch(ground.0 + offset.0, ground.1 + offset.1);
    let mix3 = |dry: [f32; 3], lush_color: [f32; 3], t: f32| [0, 1, 2].map(|c| dry[c] + (lush_color[c] - dry[c]) * t);
    let base = match species {
        Species::Pebble => match block {
            id::SAND => [1.0, 0.88, 0.72],
            id::DIRT => [1.0, 0.82, 0.7],
            _ => mix3([0.92, 0.88, 0.86], [0.82, 0.88, 0.92], lush),
        },
        // The rock sprites are brown: pull them towards grey on stone, towards moss on grass.
        Species::Rock => match block {
            id::GRASS => mix3([0.85, 0.9, 0.8], [0.7, 0.95, 0.65], lush),
            id::STONE => mix3([0.8, 0.82, 0.9], [0.72, 0.9, 0.82], lush),
            _ => [0.95, 0.9, 0.85],
        },
    };
    // Brightness only darkens (1 is the sprite's own colour), the hue wobble pulls channels apart a little.
    let brightness = 0.76 + 0.24 * dice[0];
    [0, 1, 2].map(|c| (base[c] * brightness * (1.0 - 0.12 * dice[c + 1])).clamp(0.0, 1.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cell(x: i32, y: i32, block: u8) -> Cell {
        Cell { x, y, z: 3, block, edges: [false; 4] }
    }

    fn all(block: u8, edges: Edges, n: i32) -> Vec<Instance> {
        let mut out = Vec::new();
        for x in -n..n {
            for y in -n..n {
                out.extend(scatter(&Cell { edges, ..cell(x, y, block) }, NORMAL_DENSITY, 1.0));
            }
        }
        out
    }

    #[test]
    fn the_same_block_always_gets_the_same_stones() {
        let c = cell(7, -4, id::STONE);
        assert_eq!(scatter(&c, 10, 1.0), scatter(&c, 10, 1.0));
        assert_ne!(scatter(&c, 10, 1.0), scatter(&cell(8, -4, id::STONE), 10, 1.0));
    }

    #[test]
    fn density_zero_and_distance_thin_it_out() {
        let full = all(id::STONE, [false; 4], 20).len();
        assert!(full > 400, "stone has plenty: {full}");
        assert!(scatter(&cell(1, 1, id::STONE), 0, 1.0).is_empty());
        let sum = |density, lod| -> usize { (-20..20).flat_map(|x| (-20..20).map(move |y| (x, y))).map(|(x, y)| scatter(&cell(x, y, id::STONE), density, lod).len()).sum() };
        assert!(sum(10, 0.3) < full / 2);
        assert!(sum(20, 1.0) > full * 3 / 2);
    }

    #[test]
    fn blocks_that_carry_nothing_get_nothing_and_a_block_never_gets_too_many() {
        assert!(!carries_detail(id::WATER) && !carries_detail(id::AIR) && carries_detail(id::SAND));
        assert!(all(id::WATER, [true; 4], 10).is_empty());
        for x in -15..15 {
            for y in -15..15 {
                assert!(scatter(&Cell { edges: [true; 4], ..cell(x, y, id::STONE) }, MAX_DENSITY, 1.0).len() <= MAX_PER_CELL);
            }
        }
    }

    #[test]
    fn pebbles_prefer_stone_and_grass_blocks_keep_to_their_blades() {
        let on = |block| all(block, [false; 4], 20).iter().filter(|i| i.species == Species::Pebble).count();
        assert!(on(id::STONE) > on(id::SAND), "{} vs {}", on(id::STONE), on(id::SAND));
        assert_eq!(on(id::WATER), 0);
        // grass blocks have their blades: no pebbles, and a rock only along an open border
        assert_eq!(on(id::GRASS), 0);
        assert!(all(id::GRASS, [false; 4], 20).is_empty());
    }

    #[test]
    fn species_come_in_clumps_not_evenly() {
        let mut counts = Vec::new();
        for bx in 0..16 {
            let mut n = 0;
            for x in 0..3 {
                for y in 0..3 {
                    n += scatter(&cell(bx * 5 + x, y, id::STONE), 10, 1.0).len();
                }
            }
            counts.push(n);
        }
        let (lo, hi) = (counts.iter().min().unwrap(), counts.iter().max().unwrap());
        assert!(*hi > lo * 2 + 3, "{counts:?}");
    }

    #[test]
    fn the_noise_is_smooth_and_in_range() {
        let mut worst = 0.0f32;
        for i in 0..2000 {
            let (x, y) = (i as f32 * 0.37 - 300.0, i as f32 * 0.21 + 17.0);
            let (a, b) = (noise(x, y, 8.0, 5), noise(x + 0.02, y + 0.02, 8.0, 5));
            assert!((0.0..1.0).contains(&a));
            worst = worst.max((a - b).abs());
        }
        assert!(worst < 0.05, "a step of 0.03 blocks changes the noise by {worst}");
        assert_ne!(noise(3.3, 4.4, 8.0, 1), noise(3.3, 4.4, 8.0, 2), "seeds give different fields");
    }

    #[test]
    fn open_borders_collect_instances_inside_the_footprint() {
        let edges = [true, false, false, false]; // only +x is open
        let placed = all(id::STONE, edges, 25);
        let at_edge = placed.iter().filter(|i| i.offset.0 > 0.5 - EDGE_DEPTH - 1e-4).count();
        let closed = all(id::STONE, [false; 4], 25);
        let at_edge_closed = closed.iter().filter(|i| i.offset.0 > 0.5 - EDGE_DEPTH - 1e-4).count();
        assert!(at_edge * 10 > placed.len() * 5, "more than half stand at the open border: {at_edge} of {}", placed.len());
        assert!(at_edge_closed * 10 < closed.len() * 4, "{at_edge_closed} of {}", closed.len());
        let reach = 0.5 + EDGE_OVERHANG + 1e-4;
        assert!(placed.iter().all(|i| i.offset.0.abs() <= reach && i.offset.1.abs() <= reach));
        // a grass block with a closed border gets nothing, with an open one a few rocks on that border
        let lawn_edge = all(id::GRASS, [false, false, true, false], 40);
        assert!(!lawn_edge.is_empty() && lawn_edge.iter().all(|i| i.species == Species::Rock && i.offset.1 > 0.5 - EDGE_DEPTH - 1e-4));
    }

    #[test]
    fn every_instance_differs_and_stays_inside_its_ranges() {
        let placed = all(id::STONE, [true; 4], 25);
        assert!(placed.len() > 1000);
        for i in &placed {
            assert!(i.tint.iter().all(|c| (0.0..=1.0).contains(c)), "{i:?}");
            assert!(i.tint.iter().cloned().fold(0.0, f32::max) > 0.45, "not black: {i:?}");
            assert!(i.scale.0 > 0.1 && i.scale.1 > 0.1 && i.scale.1 < 2.5);
            assert!(i.variant < ROCK_VARIANTS && (i.species == Species::Rock || i.variant == 0));
        }
        let spread = |f: &dyn Fn(&Instance) -> f32| {
            let (lo, hi) = placed.iter().map(|i| f(i)).fold((f32::MAX, f32::MIN), |(l, h), v| (l.min(v), h.max(v)));
            hi - lo
        };
        assert!(spread(&|i| i.scale.0) > 0.35, "sizes vary");
        assert!(spread(&|i| i.tint[1]) > 0.1, "brightness varies");
        assert!(spread(&|i| i.rotation) > 15.0, "lean varies");
    }

    #[test]
    fn the_patch_tint_follows_the_ground_over_a_distance_not_the_single_instance() {
        let ground = |x: f32, y: f32| tint(Species::Pebble, id::STONE, (x, y), (0.0, 0.0), [1.0, 0.0, 0.0, 0.0]);
        let near = (ground(10.0, 10.0)[2] - ground(10.5, 10.0)[2]).abs();
        let mut far_max = 0.0f32;
        for i in 0..40 {
            far_max = far_max.max((ground(10.0, 10.0)[2] - ground(10.0 + i as f32 * 5.0, 10.0)[2]).abs());
        }
        assert!(near < 0.02 && far_max > 0.015, "{near} {far_max}");
    }
}
