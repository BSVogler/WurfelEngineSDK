//! The stones of the ground cover (pebbles and rocks), scattered over dirt, sand, stone and the borders of
//! grass blocks. They belong to the grass: `grass.rs`'s `Grass` owns a [`Stones`], hands it the same
//! viewer, terrain and density as the blades and puts its vertices into the same buffer, so the grass
//! switch and the grass density are the only settings. What lies where is `wurfel_sim::detail`; this
//! module finds the blocks, keeps the placed instances and builds the vertices.
//!
//! The art is what the atlas has: the small stone `e7-1` (pebbles) and the brown rocks `e44-0` to
//! `e44-2`, all tinted per instance. The atlas has no flowers, mushrooms, ferns, leaves, twigs or
//! clover, so there are none (see the README).
//!
//! Like the blades, the list of blocks and the scattered instances are made again only when the viewer
//! changes column, the terrain changes, the layer limit moves or the density changes. Stones do not move.


use glam::Vec3;
use wurfel_sim::block::id;
use wurfel_sim::detail::{self, Cell, Instance, Species};
use wurfel_sim::grid::{from_iso, to_iso};
use wurfel_sim::World;

use crate::atlas::{Atlas, Region};
use crate::mesh::{Vertex, FACE_SPRITE};
use crate::sprites::Sprites;

/// Blocks around the viewer that get stones.
pub const RADIUS: f32 = 22.0;
/// Most stones drawn per frame; blocks nearer to the viewer come first.
pub const MAX_INSTANCES: usize = 4000;
/// Blocks above and below the viewer's feet that are searched for surfaces.
const SEARCH_UP: i32 = 8;
const SEARCH_DOWN: i32 = 14;
/// Depth bias towards the viewer, so an instance's foot is not cut by the block it stands on.
const BIAS: f32 = 0.08;
/// How much of the density a block at `distance` blocks from the viewer gets.
fn lod(distance: f32) -> f32 {
    if distance <= 9.0 {
        1.0
    } else if distance <= 16.0 {
        0.6
    } else {
        0.3
    }
}

/// An instance with where it stands.
#[derive(Debug, Clone, Copy)]
struct Placed {
    /// The foot in isometric blocks.
    anchor: Vec3,
    instance: Instance,
}

/// What the placed list was made for; it is made again when any of it changes.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Key {
    column: (i32, i32),
    z: i32,
    terrain: u64,
    max_z: Option<i32>,
    density: i32,
}

/// The scattered stones around the viewer.
#[derive(Default)]
pub struct Stones {
    placed: Vec<Placed>,
    key: Option<Key>,
}

impl Stones {
    /// Forget what was placed (the sprites changed).
    pub fn reset(&mut self) {
        self.key = None;
    }

    /// Push the vertices of the stones around `viewer` (feet position, isometric blocks) at the grass
    /// `density` (0 to 20) onto `out`. `max_z` is an upper limit of the surfaces' height (the editor's layer limit).
    pub fn update(&mut self, sprites: &Sprites, world: &World, terrain_version: u64, viewer: Vec3, max_z: Option<i32>, density: i32, out: &mut Vec<Vertex>) {
        if density <= 0 {
            return;
        }
        let key = Key { column: from_iso(viewer.x, viewer.y), z: viewer.z.floor() as i32, terrain: terrain_version, max_z, density };
        if self.key != Some(key) {
            self.key = Some(key);
            self.placed = place(world, viewer, key.z, max_z, density);
        }
        let Some(pebble) = sprites.entity(7, 1) else { return };
        for placed in &self.placed {
            let i = &placed.instance;
            let art = match i.species {
                Species::Pebble => pebble,
                Species::Rock => match sprites.entity(44, i.variant as u32) {
                    Some(region) => region,
                    None => continue,
                },
            };
            push_sprite(out, &sprites.atlas, art, placed.anchor, i.tint, i.scale, i.rotation);
        }
    }
}

/// The surfaces within [`RADIUS`] of the viewer that carry detail, nearest first, scattered.
fn place(world: &World, viewer: Vec3, z: i32, max_z: Option<i32>, density: i32) -> Vec<Placed> {
    let mut found: Vec<(f32, Cell)> = Vec::new();
    let r = RADIUS.ceil() as i32;
    let (cx, cy) = (viewer.x.round() as i32, viewer.y.round() as i32);
    let top = (z + SEARCH_UP).min(wurfel_sim::CHUNK_SIZE_Z - 1);
    let bottom = (z - SEARCH_DOWN).max(0);
    for dy in -r..=r {
        for dx in -r..=r {
            let (gx, gy) = ((cx + dx) as f32, (cy + dy) as f32);
            let distance = ((gx - viewer.x).powi(2) + (gy - viewer.y).powi(2)).sqrt();
            if distance > RADIUS {
                continue;
            }
            let (x, y) = from_iso(gx, gy);
            for zz in bottom..=top {
                if max_z.is_some_and(|limit| zz + 1 > limit) {
                    break;
                }
                let block = world.get(x, y, zz).id();
                if !detail::carries_detail(block) || world.get(x, y, zz + 1).id() != id::AIR {
                    continue;
                }
                found.push((distance, Cell { x, y, z: zz, block, edges: open_edges(world, (gx, gy), zz) }));
            }
        }
    }
    found.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut out = Vec::new();
    for (distance, cell) in found {
        let (gx, gy) = to_iso(cell.x, cell.y);
        for instance in detail::scatter(&cell, density, lod(distance)) {
            if out.len() >= MAX_INSTANCES {
                return out;
            }
            let anchor = Vec3::new(gx + instance.offset.0, gy + instance.offset.1, cell.z as f32 + 1.0);
            out.push(Placed { anchor, instance });
        }
    }
    out
}

/// The neighbours in `+x, -x, +y, -y` (isometric ground axes, the order of `detail::Edge`) that have
/// nothing at the block's height, so the block's side face shows there.
fn open_edges(world: &World, (gx, gy): (f32, f32), z: i32) -> detail::Edges {
    let mut edges = [false; 4];
    for (i, (dx, dy)) in [(1.0, 0.0), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0)].into_iter().enumerate() {
        let (nx, ny) = from_iso(gx + dx, gy + dy);
        edges[i] = world.get(nx, ny, z).id() == id::AIR && world.get(nx, ny, z + 1).id() == id::AIR;
    }
    edges
}

/// The two triangles of one stone: the sprite's rectangle (as `grass::push_blade`), scaled in
/// width and height and turned about the middle of its foot. `rotation` is counter-clockwise on the screen.
pub fn push_sprite(out: &mut Vec<Vertex>, atlas: &Atlas, region: &Region, anchor: Vec3, tint: [f32; 3], scale: (f32, f32), rotation: f32) {
    let page = &atlas.pages[region.page];
    // The picture's foot sits on the anchor: the box's bottom is `offset_y` below the art's bottom.
    let box_bottom = region.offset_y as f32;
    let left = -(region.orig_w as f32) / 2.0 + region.offset_x as f32;
    let top = box_bottom - region.orig_h as f32 + region.top_in_orig() as f32;
    let (right, bottom) = (left + region.w as f32, top + region.h as f32);
    let pivot = ((left + right) / 2.0, bottom);
    let (sin, cos) = rotation.to_radians().sin_cos();
    let (u0, u1) = (region.x as f32 / page.width as f32, (region.x + region.w) as f32 / page.width as f32);
    let (v0, v1) = (region.y as f32 / page.height as f32, (region.y + region.h) as f32 / page.height as f32);
    let vertex = |px: f32, py: f32, u: f32, v: f32| {
        let (rx, ry) = ((px - pivot.0) * scale.0, (py - pivot.1) * scale.1);
        // Counter-clockwise on a screen whose y points down.
        let (dx, dy) = (rx * cos + ry * sin, -rx * sin + ry * cos);
        Vertex {
            position: anchor.to_array(),
            color: tint,
            shade: [FACE_SPRITE, 0.0],
            point: [pivot.0 + dx, pivot.1 + dy, BIAS],
            uv: [u, v],
            layer: region.page as f32,
            occlusion: 0.0,
            cell: crate::mesh::NO_CELL,
        }
    };
    let (a, b, c, d) = (vertex(left, bottom, u0, v1), vertex(right, bottom, u1, v1), vertex(right, top, u1, v0), vertex(left, top, u0, v0));
    out.extend_from_slice(&[a, b, c, a, c, d]);
}

#[cfg(test)]
mod tests {
    use super::*;
    use wurfel_sim::{AirGenerator, Block};

    fn real() -> Sprites {
        Sprites::new(Atlas::parse(include_str!("../assets/sprites/sprites.atlas")).expect("atlas"))
    }

    /// A dirt and sand field at height 3.
    fn field() -> World {
        let mut world = World::new(AirGenerator);
        for x in -14..14 {
            for y in -14..14 {
                world.set(x, y, 3, Block::new(if (x + y) % 3 == 0 { id::SAND } else { id::DIRT }, 0));
            }
        }
        world
    }

    #[test]
    fn the_sprites_the_stones_use_exist_in_the_atlas() {
        let sprites = real();
        assert!(sprites.entity(7, 1).is_some());
        for variant in 0..wurfel_sim::detail::ROCK_VARIANTS as u32 {
            assert!(sprites.entity(44, variant).is_some(), "rock e44-{variant}");
        }
    }

    #[test]
    fn stones_are_built_capped_and_thinned_by_the_density() {
        let world = field();
        let sprites = real();
        let mut stones = Stones::default();
        let viewer = Vec3::new(0.0, 0.0, 4.0);
        let build = |stones: &mut Stones, density| {
            let mut out = Vec::new();
            stones.update(&sprites, &world, 1, viewer, None, density, &mut out);
            out
        };
        let full = build(&mut stones, 10);
        assert!(full.len() > 300 && full.len() % 6 == 0, "{}", full.len());
        assert!(full.len() <= MAX_INSTANCES * 6);
        assert!(build(&mut stones, 3).len() < full.len());
        assert!(build(&mut stones, 0).is_empty());
    }

    #[test]
    fn stones_stand_on_surfaces_with_air_above_and_the_layer_limit_filters_them() {
        let mut world = field();
        world.set(0, 0, 4, Block::new(id::STONE, 0));
        let viewer = Vec3::new(0.0, 0.0, 4.0);
        let placed = place(&world, viewer, 4, None, 10);
        assert!(!placed.is_empty());
        assert!(placed.iter().all(|p| p.anchor.z == 4.0 || p.anchor.z == 5.0), "on the field (3 + 1) or on the stone's top");
        assert!(placed.iter().all(|p| (p.anchor.truncate() - viewer.truncate()).length() <= RADIUS + 1.0));
        assert!(place(&world, viewer, 4, Some(3), 10).is_empty(), "the field's top at 4 is above a limit of 3");
        assert!(!place(&world, viewer, 4, Some(4), 10).is_empty());
        assert!(placed.iter().any(|p| p.instance.species == Species::Pebble));
    }

    #[test]
    fn open_borders_are_found_beside_a_drop() {
        let world = field();
        // the middle of the field has neighbours all round, the rim of the world's field has none
        let (gx, gy) = to_iso(0, 0);
        assert_eq!(open_edges(&world, (gx, gy), 3), [false; 4]);
        let (rx, ry) = to_iso(13, 13);
        assert!(open_edges(&world, (rx, ry), 3).iter().any(|&e| e));
    }

    #[test]
    fn a_turn_swings_the_top_of_a_stone_sideways_and_a_scale_changes_its_size() {
        let sprites = real();
        let art = sprites.entity(7, 1).unwrap();
        let screen = |v: &Vertex| crate::sprites::project(crate::sprites::billboard_corner(v));
        let extent = |scale: (f32, f32), rotation: f32| {
            let mut out = Vec::new();
            push_sprite(&mut out, &sprites.atlas, art, Vec3::ZERO, [1.0; 3], scale, rotation);
            assert_eq!(out.len(), 6);
            let pts: Vec<[f32; 2]> = out.iter().map(screen).collect();
            let lo = |i: usize| pts.iter().map(|p| p[i]).fold(f32::MAX, f32::min);
            let hi = |i: usize| pts.iter().map(|p| p[i]).fold(f32::MIN, f32::max);
            (lo(0), hi(0), lo(1), hi(1))
        };
        let base = extent((1.0, 1.0), 0.0);
        let tall = extent((0.5, 2.0), 0.0);
        assert!(((tall.3 - tall.2) - 2.0 * (base.3 - base.2)).abs() < 0.5, "twice as tall");
        assert!(((tall.1 - tall.0) - 0.5 * (base.1 - base.0)).abs() < 0.5, "half as wide");
        let turned = extent((1.0, 1.0), 45.0);
        assert!((turned.0 + turned.1) / 2.0 < (base.0 + base.1) / 2.0 - 1.0, "a counter-clockwise turn moves the top to the left");
    }
}
