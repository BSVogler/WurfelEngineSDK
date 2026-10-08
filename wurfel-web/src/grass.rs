//! Grass blades standing on grass blocks, drawn as sprites through the same billboard path as the
//! players and things (`FACE_SPRITE`: the shader lights them at their foot, the depth buffer sorts
//! them against the blocks). The maths (wind, placement, bending) is `wurfel_sim::grass`; this module
//! picks the blocks, builds the vertices and keeps the settings.
//!
//! Blades exist only for grass blocks with air above them, within [`RADIUS`] blocks of the viewer
//! (the local player), at most [`MAX_BLADES`] per frame, nearest first. The list of such blocks is
//! cached and only searched again when the viewer changes column, the terrain changes or the layer
//! limit moves; every frame only the blades' rotation changes. Every block carries its height `z`,
//! so a layer limit (`max_z`) is a simple filter.

use std::rc::Rc;

use glam::Vec3;
use wurfel_sim::block::id;
use wurfel_sim::grass::{self, Blade, Kind, Wind};
use wurfel_sim::grid::{from_iso, to_iso};
use wurfel_sim::World;

use crate::atlas::Region;
use crate::mesh::{Vertex, FACE_SPRITE};
use crate::sprites::Sprites;

/// Blocks around the viewer that get blades at all (see `grass::blades_at_distance`).
pub const RADIUS: f32 = 20.0;
/// Most blades drawn per frame; blocks nearer to the viewer come first.
pub const MAX_BLADES: usize = 3000;
/// Blocks above the viewer's feet that are searched for grass tops.
const SEARCH_UP: i32 = 8;
/// Blocks below the viewer's feet that are searched for grass tops.
const SEARCH_DOWN: i32 = 14;
/// A force centre (a player) only bends blades within this many blocks of height.
const FORCE_HEIGHT: f32 = 2.0;
/// Depth bias towards the viewer, so the blade's foot is not cut by the grass top it stands on.
const BIAS: f32 = 0.08;

/// Settings of the grass, from the menu (`grass`, `grassDensity`) or the page address.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Settings {
    pub enabled: bool,
    /// Blades per block near the viewer (0 to 20, Java drew 10).
    pub density: i32,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { enabled: true, density: grass::MAX_BLADES_PER_CELL }
    }
}

/// A block that carries grass: its coordinates and how far it is from the viewer (blocks).
#[derive(Debug, Clone, Copy, PartialEq)]
struct Cell {
    x: i32,
    y: i32,
    z: i32,
    distance: f32,
}

/// What the cell list was made for; it is searched again when any of it changes.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Key {
    column: (i32, i32),
    z: i32,
    terrain: u64,
    max_z: Option<i32>,
}

#[derive(Default)]
pub struct Grass {
    pub settings: Settings,
    sprites: Option<Rc<Sprites>>,
    wind: Wind,
    cells: Vec<Cell>,
    key: Option<Key>,
    /// The vertices of this frame.
    pub vertices: Vec<Vertex>,
}

impl Grass {
    pub fn set_sprites(&mut self, sprites: Option<Rc<Sprites>>) {
        self.sprites = sprites;
        self.key = None;
    }

    /// Everything the next frame needs: advance the wind by `dt` seconds and build the blades around
    /// `viewer` (feet position, isometric blocks). `forces` are the places something stands that the
    /// blades bend away from. `max_z` is an upper limit of the cells' height (the editor's layer limit).
    pub fn update(&mut self, dt: f32, world: &World, terrain_version: u64, viewer: Vec3, forces: &[Vec3], max_z: Option<i32>) {
        self.wind.update(dt);
        self.vertices.clear();
        let Some(sprites) = self.sprites.clone() else { return };
        if !self.settings.enabled || self.settings.density <= 0 {
            return;
        }
        let key = Key { column: from_iso(viewer.x, viewer.y), z: viewer.z.floor() as i32, terrain: terrain_version, max_z };
        if self.key != Some(key) {
            self.key = Some(key);
            self.cells = find_cells(world, viewer, key.z, max_z);
        }
        let (Some(blade_art), Some(stone_art)) = (sprites.entity(7, 0), sprites.entity(7, 1)) else { return };
        let forces: Vec<(f32, f32, f32)> = forces.iter().map(|f| (game(f.x, f.y).0, game(f.x, f.y).1, f.z)).collect();
        let mut drawn = 0;
        for cell in &self.cells {
            let count = grass::blades_at_distance(self.settings.density, cell.distance);
            if count == 0 {
                continue;
            }
            let (gx, gy) = to_iso(cell.x, cell.y);
            let force = nearest_force(&forces, cell.z as f32 + 1.0, (gx, gy));
            for blade in grass::blades(cell.x, cell.y, cell.z, count, &self.wind, force) {
                if drawn >= MAX_BLADES {
                    return;
                }
                drawn += 1;
                let anchor = Vec3::new(gx + blade.iso_offset.0, gy + blade.iso_offset.1, cell.z as f32 + 1.0);
                let art = if blade.kind == Kind::Stone { stone_art } else { blade_art };
                push_blade(&mut self.vertices, &sprites.atlas, art, anchor, &blade);
            }
        }
    }
}

/// The game-space `(x, y)` of the isometric ground point `(gx, gy)`.
fn game(gx: f32, gy: f32) -> (f32, f32) {
    ((gx - gy) * 100.0, (gx + gy) * 100.0)
}

/// The force centre, in game units, that pushes a block at `(gx, gy)` and height `top` most, if any
/// is near enough in height. Several players: the closest wins.
fn nearest_force(forces: &[(f32, f32, f32)], top: f32, (gx, gy): (f32, f32)) -> Option<(f32, f32)> {
    let here = game(gx, gy);
    forces
        .iter()
        .filter(|f| (f.2 - top).abs() <= FORCE_HEIGHT)
        .map(|f| (f.0, f.1))
        .min_by(|a, b| {
            let d = |p: &(f32, f32)| (p.0 - here.0).powi(2) + (p.1 - here.1).powi(2);
            d(a).total_cmp(&d(b))
        })
}

/// Grass blocks with air above within [`RADIUS`] of the viewer, nearest first.
fn find_cells(world: &World, viewer: Vec3, z: i32, max_z: Option<i32>) -> Vec<Cell> {
    let mut cells = Vec::new();
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
                if world.get(x, y, zz).id() == id::GRASS && world.get(x, y, zz + 1).id() == id::AIR {
                    cells.push(Cell { x, y, z: zz, distance });
                }
            }
        }
    }
    cells.sort_by(|a, b| a.distance.total_cmp(&b.distance));
    cells
}

/// The two triangles of one blade: the sprite's rectangle (as `sprites::billboard`), scaled and
/// turned about the middle of its foot. `rotation` is counter-clockwise on the screen.
pub fn push_blade(out: &mut Vec<Vertex>, atlas: &crate::atlas::Atlas, region: &Region, anchor: Vec3, blade: &Blade) {
    let page = &atlas.pages[region.page];
    // The picture's foot sits on the anchor: the box's bottom is `offset_y` below the art's bottom.
    let box_bottom = region.offset_y as f32;
    let left = -(region.orig_w as f32) / 2.0 + region.offset_x as f32;
    let top = box_bottom - region.orig_h as f32 + region.top_in_orig() as f32;
    let (right, bottom) = (left + region.w as f32, top + region.h as f32);
    let pivot = ((left + right) / 2.0, bottom);
    let (sin, cos) = blade.rotation.to_radians().sin_cos();
    let (u0, u1) = (region.x as f32 / page.width as f32, (region.x + region.w) as f32 / page.width as f32);
    let (v0, v1) = (region.y as f32 / page.height as f32, (region.y + region.h) as f32 / page.height as f32);
    let vertex = |px: f32, py: f32, u: f32, v: f32| {
        let (rx, ry) = ((px - pivot.0) * blade.scale, (py - pivot.1) * blade.scale);
        // Counter-clockwise on a screen whose y points down.
        let (dx, dy) = (rx * cos + ry * sin, -rx * sin + ry * cos);
        Vertex {
            position: anchor.to_array(),
            color: blade.tint,
            shade: [FACE_SPRITE, 0.0],
            point: [pivot.0 + dx, pivot.1 + dy, BIAS],
            uv: [u, v],
            layer: region.page as f32,
            occlusion: 0.0,
        }
    };
    let (a, b, c, d) = (vertex(left, bottom, u0, v1), vertex(right, bottom, u1, v1), vertex(right, top, u1, v0), vertex(left, top, u0, v0));
    out.extend_from_slice(&[a, b, c, a, c, d]);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::atlas::Atlas;
    use wurfel_sim::{AirGenerator, Block};

    fn real() -> Sprites {
        Sprites::new(Atlas::parse(include_str!("../assets/sprites/sprites.atlas")).expect("atlas"))
    }

    fn lawn() -> World {
        let mut world = World::new(AirGenerator);
        for x in -12..12 {
            for y in -12..12 {
                world.set(x, y, 3, Block::new(id::GRASS, 0));
            }
        }
        world
    }

    #[test]
    fn the_blade_sprites_exist_in_the_atlas() {
        let sprites = real();
        let blade = sprites.entity(7, 0).expect("e7-0");
        assert_eq!((blade.w, blade.h, blade.orig_w, blade.orig_h), (19, 59, 200, 223));
        let stone = sprites.entity(7, 1).expect("e7-1");
        assert_eq!((stone.w, stone.h), (13, 13));
    }

    #[test]
    fn a_blade_stands_on_its_anchor_and_a_turn_swings_the_top_sideways() {
        let sprites = real();
        let art = sprites.entity(7, 0).unwrap();
        let mk = |rotation: f32, scale: f32| Blade { kind: Kind::Blade, offset: (0, 0), iso_offset: (0.0, 0.0), tint: [1.0; 3], scale, rotation };
        let screen = |v: &Vertex| crate::sprites::project(crate::sprites::billboard_corner(v));
        let extent = |blade: &Blade| {
            let mut out = Vec::new();
            push_blade(&mut out, &sprites.atlas, art, Vec3::ZERO, blade);
            assert_eq!(out.len(), 6);
            let pts: Vec<[f32; 2]> = out.iter().map(|v| screen(v)).collect();
            let lo = |i: usize| pts.iter().map(|p| p[i]).fold(f32::MAX, f32::min);
            let hi = |i: usize| pts.iter().map(|p| p[i]).fold(f32::MIN, f32::max);
            (lo(0), hi(0), lo(1), hi(1))
        };
        let upright = extent(&mk(0.0, 1.0));
        // wall height of the foot: ground at the anchor, within a few pixels (the wall is a tip plane)
        assert!(upright.3.abs() < 51.0, "{upright:?}");
        let mid_x = (upright.0 + upright.1) / 2.0;
        // a counter-clockwise turn moves the blade's top to the left
        let turned = extent(&mk(45.0, 1.0));
        assert!((turned.0 + turned.1) / 2.0 < mid_x - 5.0, "{turned:?} vs {upright:?}");
        let small = extent(&mk(0.0, 0.5));
        assert!(((small.3 - small.2) - (upright.3 - upright.2) / 2.0).abs() < 0.5, "half scale is half as tall");
    }

    #[test]
    fn only_grass_with_air_above_gets_blades_and_the_layer_limit_filters_them() {
        let mut world = lawn();
        world.set(0, 0, 4, Block::new(id::STONE, 0)); // covers a grass block
        let viewer = Vec3::new(0.0, 0.0, 4.0);
        let cells = find_cells(&world, viewer, 4, None);
        assert!(!cells.is_empty());
        assert!(cells.iter().all(|c| c.z == 3 && c.distance <= RADIUS));
        assert!(!cells.iter().any(|c| (c.x, c.y) == (0, 0)), "the covered block has none");
        assert!(cells.windows(2).all(|w| w[0].distance <= w[1].distance), "nearest first");
        assert!(find_cells(&world, viewer, 4, Some(3)).is_empty(), "grass top at 4 is above a limit of 3");
        assert!(!find_cells(&world, viewer, 4, Some(4)).is_empty());
    }

    #[test]
    fn grass_is_built_capped_and_can_be_switched_off() {
        let world = lawn();
        let mut grass = Grass::default();
        grass.set_sprites(Some(Rc::new(real())));
        let viewer = Vec3::new(0.0, 0.0, 4.0);
        grass.update(0.016, &world, 1, viewer, &[], None);
        assert!(grass.vertices.len() > 600 && grass.vertices.len() % 6 == 0);
        assert!(grass.vertices.len() <= MAX_BLADES * 6);
        // density 0 and disabled draw nothing
        grass.settings.density = 0;
        grass.update(0.016, &world, 1, viewer, &[], None);
        assert!(grass.vertices.is_empty());
        grass.settings = Settings { enabled: false, density: 10 };
        grass.update(0.016, &world, 1, viewer, &[], None);
        assert!(grass.vertices.is_empty());
        // fewer blades with a lower density
        grass.settings = Settings { enabled: true, density: 10 };
        grass.update(0.016, &world, 1, viewer, &[], None);
        let full = grass.vertices.len();
        grass.settings.density = 3;
        grass.update(0.016, &world, 1, viewer, &[], None);
        assert!(grass.vertices.len() < full);
    }

    #[test]
    fn a_player_standing_on_the_lawn_bends_the_blades_beside_them() {
        let world = lawn();
        let mut grass = Grass::default();
        grass.set_sprites(Some(Rc::new(real())));
        let viewer = Vec3::new(0.0, 0.0, 4.0);
        grass.update(0.0, &world, 1, viewer, &[], None);
        let calm = grass.vertices.clone();
        grass.update(0.0, &world, 1, viewer, &[viewer], None);
        let pushed = grass.vertices.clone();
        assert_eq!(calm.len(), pushed.len());
        assert!(calm.iter().zip(&pushed).any(|(a, b)| a.point != b.point), "some blades moved");
        // a player on another floor does not push
        grass.update(0.0, &world, 1, viewer, &[Vec3::new(0.0, 0.0, 12.0)], None);
        assert_eq!(grass.vertices, calm);
    }

    #[test]
    fn the_wind_moves_the_blades_between_frames() {
        let world = lawn();
        let mut grass = Grass::default();
        grass.set_sprites(Some(Rc::new(real())));
        let viewer = Vec3::new(0.0, 0.0, 4.0);
        grass.update(0.1, &world, 1, viewer, &[], None);
        let before = grass.vertices.clone();
        grass.update(0.1, &world, 1, viewer, &[], None);
        assert_ne!(before, grass.vertices);
        assert_eq!(before.len(), grass.vertices.len());
    }

    /// Not a test of behaviour: `cargo test -p wurfel-web --release grass_cost -- --ignored --nocapture`
    /// prints what one frame of grass costs on the CPU (the browser runs the same code, slower).
    #[test]
    #[ignore]
    fn grass_cost() {
        let mut world = World::new(AirGenerator);
        for x in -25..25 {
            for y in -25..25 {
                world.set(x, y, 3, Block::new(id::GRASS, 0));
            }
        }
        let mut grass = Grass::default();
        grass.set_sprites(Some(Rc::new(real())));
        let viewer = Vec3::new(0.0, 0.0, 4.0);
        let forces = [viewer];
        grass.update(0.016, &world, 1, viewer, &forces, None);
        let frames = 300;
        let start = std::time::Instant::now();
        for _ in 0..frames {
            grass.update(0.016, &world, 1, viewer, &forces, None);
        }
        let per_frame = start.elapsed().as_secs_f64() * 1000.0 / frames as f64;
        let search = std::time::Instant::now();
        let cells = find_cells(&world, viewer, 4, None).len();
        println!("grass: {} vertices ({} blades), {per_frame:.3} ms per frame; cell search {:.3} ms for {cells} cells",
            grass.vertices.len(), grass.vertices.len() / 6, search.elapsed().as_secs_f64() * 1000.0);
    }
}
