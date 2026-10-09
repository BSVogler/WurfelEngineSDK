//! The render side of the map: a window of chunks (3x3 around the camera) with everything the
//! renderer needs per cell, kept apart from the compact simulation chunks in `wurfel_sim`.
//!
//! This is the Rust version of the Java `RenderStorage` / `RenderChunk` / `RenderCell`:
//!
//! * Cells carry **clipping** flags: which of the three visible sides (left, top, right) are
//!   covered by a neighbouring block and need no drawing (occlusion culling, which looks across
//!   chunk borders).
//! * Cells carry a **top light level** from the Java `resetShadingFor`: a cheap shadow for tops that
//!   have a block two or three cells above.
//! * Each chunk caches its mesh and is only re-meshed when its clipping or blocks change.
//!
//! Differences from Java: the top face is only clipped by the block directly above it. Java also
//! clips it for a block in the top front (`x, y + 2, z + 1`), which is right for sprites that are
//! painted over each other but would hide visible geometry with the depth buffer used here. The
//! Java ambient occlusion pass is replaced by per-vertex occlusion computed while meshing (see `wurfel_sim::light`).

use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use wurfel_sim::block::id;
use glam::Vec3;
use wurfel_sim::grid::{chunk_of, lower_left, lower_right, to_iso};
use wurfel_sim::{Block, World, CHUNK_SIZE_X, CHUNK_SIZE_Y, CHUNK_SIZE_Z};

use wurfel_sim::light::PointLight;

use crate::mesh::{self, MeshContext, Vertex};
use crate::sprites::Sprites;

pub const CLIP_LEFT: u8 = 1;
pub const CLIP_TOP: u8 = 1 << 1;
pub const CLIP_RIGHT: u8 = 1 << 2;

/// Block id that is see-through but not a liquid (Java `BlockConfig.isTransparent`: 0, 9 and 4).
const INVISIBLE_OBSTACLE: u8 = 4;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RenderCell {
    pub block: Block,
    /// Bit set of `CLIP_*`: sides that are covered and need not be drawn.
    pub clipping: u8,
    /// Brightness factor for the top face, 1.0 unless shaded.
    pub top_light: f32,
    /// The game's `BlockConfig::is_transparent`: a torch or tree is a picture, not a cube.
    config_transparent: bool,
}

impl RenderCell {
    fn new(block: Block, config_transparent: bool) -> Self {
        RenderCell { block, clipping: 0, top_light: 1.0, config_transparent }
    }

    /// Does not hide what is behind it.
    pub fn is_transparent(&self) -> bool {
        self.config_transparent || matches!(self.block.id(), id::AIR | id::WATER | INVISIBLE_OBSTACLE)
    }

    pub fn is_liquid(&self) -> bool {
        self.block.id() == id::WATER
    }

    /// Solid enough to cover the side of a block next to it.
    pub fn hides_past_block(&self) -> bool {
        !self.block.is_air() && !self.is_transparent()
    }

    pub fn is_fully_clipped(&self) -> bool {
        self.clipping == CLIP_LEFT | CLIP_TOP | CLIP_RIGHT
    }
}

pub struct RenderChunk {
    pos: (i32, i32),
    cells: Vec<RenderCell>,
    mesh: Vec<Vertex>,
    mesh_dirty: bool,
    mesh_version: u32,
}

impl RenderChunk {
    fn index(lx: i32, ly: i32, z: i32) -> usize {
        ((lx * CHUNK_SIZE_Y + ly) * CHUNK_SIZE_Z + z) as usize
    }

    // Part of the window API (a camera that follows the player); the app currently renders a fixed area.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn pos(&self) -> (i32, i32) {
        self.pos
    }

    /// Absolute coordinates of the block column with local index `(0, 0)`.
    pub fn top_left(&self) -> (i32, i32) {
        (self.pos.0 * CHUNK_SIZE_X, self.pos.1 * CHUNK_SIZE_Y)
    }

    /// Cell at a local index.
    pub fn cell(&self, lx: i32, ly: i32, z: i32) -> Option<&RenderCell> {
        let inside = (0..CHUNK_SIZE_X).contains(&lx) && (0..CHUNK_SIZE_Y).contains(&ly) && (0..CHUNK_SIZE_Z).contains(&z);
        inside.then(|| &self.cells[Self::index(lx, ly, z)])
    }

    /// How many times this chunk's mesh was built. For tests and diagnostics.
    // Part of the window API (a camera that follows the player); the app currently renders a fixed area.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn mesh_version(&self) -> u32 {
        self.mesh_version
    }

    /// Copy the blocks from the simulation chunk and work out the shading. Clipping needs the
    /// neighbouring chunks too, so it is done separately.
    fn load(pos: (i32, i32), world: &World, top: i32) -> Self {
        let sim = world.chunk(pos.0, pos.1);
        let mut cells = Vec::with_capacity((CHUNK_SIZE_X * CHUNK_SIZE_Y * CHUNK_SIZE_Z) as usize);
        for lx in 0..CHUNK_SIZE_X {
            for ly in 0..CHUNK_SIZE_Y {
                for z in 0..CHUNK_SIZE_Z {
                    let block = sim.map_or(Block::AIR, |c| c.get(lx, ly, z));
                    cells.push(RenderCell::new(block, world.blocks().is_transparent(block)));
                }
            }
        }
        let mut chunk = RenderChunk { pos, cells, mesh: Vec::new(), mesh_dirty: true, mesh_version: 0 };
        chunk.apply_shading(top);
        chunk
    }

    /// Java `RenderChunk.resetShadingFor`: a top face under an overhang gets darker. Only looks
    /// within the column, so it never depends on other chunks. Layers above `top` (the editor's
    /// layer limit) are not drawn, so they cast no shadow.
    fn apply_shading(&mut self, top: i32) {
        for lx in 0..CHUNK_SIZE_X {
            for ly in 0..CHUNK_SIZE_Y {
                for z in 0..CHUNK_SIZE_Z {
                    let transparent = |dz: i32| z + dz > top || self.cells[Self::index(lx, ly, z + dz)].is_transparent();
                    let light = if z < CHUNK_SIZE_Z - 2 && transparent(1) {
                        if !transparent(2) {
                            0.8
                        } else if z < CHUNK_SIZE_Z - 3 && !transparent(3) {
                            0.92
                        } else {
                            1.0
                        }
                    } else {
                        1.0
                    };
                    self.cells[Self::index(lx, ly, z)].top_light = light;
                }
            }
        }
    }
}

#[derive(Default)]
pub struct RenderStorage {
    world_id: Option<u64>,
    chunks: HashMap<(i32, i32), RenderChunk>,
    /// Point lights whose light is baked into the meshes (see `set_static_lights`).
    static_lights: Vec<PointLight>,
    /// The sprite atlas the meshes are textured with, `None` for the flat colours.
    sprites: Option<Rc<Sprites>>,
    /// The free camera is on: the render set is the whole surface around the player, seen from any
    /// side (see [`RenderStorage::set_free_view`]), not what the fixed camera can see.
    all_faces: bool,
    /// The highest layer that is drawn (the editor's wheel, see [`RenderStorage::set_layer_limit`]);
    /// `None` draws them all.
    layer_limit: Option<i32>,
    /// Where force waves are moving blocks: the centre in the ground frame and how far (blocks). Inside,
    /// the blocks near the surface are meshed with all their sides (see [`MeshContext::exposed`]).
    wave_zones: Vec<(Vec3, f32)>,
}

/// How many layers below the surface of a column show their covered sides while a wave moves it (a wave
/// moves a column by less than one block, so what shows is a sliver of the top blocks).
const EXPOSED_DEPTH: i32 = 3;

/// Chunks from the middle to the edge of the render window of the fixed camera (3x3 chunks).
pub const FIXED_WINDOW_RADIUS: i32 = 1;
/// The same for the free camera (5x5 chunks, the radius the server sends around a player).
pub const FREE_WINDOW_RADIUS: i32 = 2;

/// Chunks whose cells depend on the cells of chunk `(cx, cy)`: they have to be clipped and meshed
/// again when it changes.
///
/// Clipping only looks one row ahead (lower left and lower right), but ambient occlusion and baked
/// light look one step along each lattice axis, which is up to two rows in the staggered grid, and
/// sideways. All eight neighbours can therefore see a change at a chunk border.
fn dependents((cx, cy): (i32, i32)) -> impl Iterator<Item = (i32, i32)> {
    (-1..=1).flat_map(move |dx| (-1..=1).map(move |dy| (cx + dx, cy + dy)))
}

impl RenderStorage {
    pub fn new() -> Self {
        Self::default()
    }

    /// Keep the chunks around `center` up to date: 3x3 for the fixed camera, 5x5 for the free one. Call this every frame (it does nothing when
    /// nothing changed): it loads the chunks that came into range and drops the ones that left,
    /// picks up block changes from the world, and recomputes clipping where it could have changed.
    // Part of the window API (a camera that follows the player); the app currently renders a fixed area.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn update(&mut self, world: &mut World, center: (i32, i32)) {
        let radius = self.window_radius();
        self.update_area(world, (center.0 - radius, center.1 - radius), (center.0 + radius, center.1 + radius));
    }

    /// Like [`RenderStorage::update`] for any rectangle of chunks, both corners inclusive.
    pub fn update_area(&mut self, world: &mut World, min: (i32, i32), max: (i32, i32)) {
        if self.world_id != Some(world.id()) {
            // A different world (e.g. after reconnecting): nothing cached applies any more.
            self.chunks.clear();
            self.world_id = Some(world.id());
        }

        let wanted: HashSet<(i32, i32)> =
            (min.0..=max.0).flat_map(|cx| (min.1..=max.1).map(move |cy| (cx, cy))).collect();
        let mut recompute: HashSet<(i32, i32)> = HashSet::new();

        // chunks that left the window: whoever looked at them now sees the outside
        let dropped: Vec<_> = self.chunks.keys().copied().filter(|pos| !wanted.contains(pos)).collect();
        for pos in dropped {
            self.chunks.remove(&pos);
            recompute.extend(dependents(pos));
        }

        for &(cx, cy) in &wanted {
            world.load_chunk(cx, cy);
        }
        let changed: HashSet<_> = world.take_changed_chunks().into_iter().collect();
        let mut reload: Vec<_> = wanted
            .iter()
            .copied()
            .filter(|pos| changed.contains(pos) || !self.chunks.contains_key(pos))
            .collect();
        reload.sort_unstable();
        for pos in reload {
            let mut fresh = RenderChunk::load(pos, world, self.top_layer());
            // the build counter counts over the whole life of the position, not of one object
            fresh.mesh_version = self.chunks.get(&pos).map_or(0, |old| old.mesh_version);
            self.chunks.insert(pos, fresh);
            recompute.extend(dependents(pos));
        }

        let mut recompute: Vec<_> = recompute.into_iter().filter(|pos| self.chunks.contains_key(pos)).collect();
        recompute.sort_unstable();
        for pos in recompute {
            let clipping = self.compute_clipping(pos);
            let chunk = self.chunks.get_mut(&pos).expect("filtered to existing chunks");
            for (cell, flags) in chunk.cells.iter_mut().zip(clipping) {
                cell.clipping = flags;
            }
            chunk.mesh_dirty = true;
        }
    }

    // Part of the window API (a camera that follows the player); the app currently renders a fixed area.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn chunk(&self, cx: i32, cy: i32) -> Option<&RenderChunk> {
        self.chunks.get(&(cx, cy))
    }

    // Part of the window API (a camera that follows the player); the app currently renders a fixed area.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn chunks(&self) -> impl Iterator<Item = &RenderChunk> {
        self.chunks.values()
    }

    /// The cell at absolute coordinates, `None` outside the window or the world height.
    pub fn cell(&self, x: i32, y: i32, z: i32) -> Option<&RenderCell> {
        let chunk = self.chunks.get(&chunk_of(x, y))?;
        chunk.cell(x.rem_euclid(CHUNK_SIZE_X), y.rem_euclid(CHUNK_SIZE_Y), z)
    }

    /// Occlusion culling for one chunk (Java `RenderStorage.occlusionCulling`).
    fn compute_clipping(&self, pos: (i32, i32)) -> Vec<u8> {
        let chunk = &self.chunks[&pos];
        let (left, top) = chunk.top_left();
        // A neighbour covers a side if it is solid, or if both are liquid (a lake has no inner walls).
        let hides = |neighbour: Option<&RenderCell>, current: &RenderCell| {
            neighbour.is_some_and(|n| n.hides_past_block() || (n.is_liquid() && current.is_liquid()))
        };

        let top_layer = self.top_layer();
        let mut flags = Vec::with_capacity(chunk.cells.len());
        for lx in 0..CHUNK_SIZE_X {
            for ly in 0..CHUNK_SIZE_Y {
                let (x, y) = (left + lx, top + ly);
                let (llx, lly) = lower_left(x, y);
                let (lrx, lry) = lower_right(x, y);
                for z in 0..CHUNK_SIZE_Z {
                    let current = &chunk.cells[RenderChunk::index(lx, ly, z)];
                    let mut clipping = 0;
                    if !current.block.is_air() {
                        if hides(self.cell(llx, lly, z), current) {
                            clipping |= CLIP_LEFT;
                        }
                        if hides(self.cell(lrx, lry, z), current) {
                            clipping |= CLIP_RIGHT;
                        }
                        if z < top_layer && hides(chunk.cell(lx, ly, z + 1), current) {
                            clipping |= CLIP_TOP;
                        }
                    }
                    flags.push(clipping);
                }
            }
        }
        flags
    }

    /// Replace the lights that are baked into the meshes (torches built into the world, for
    /// example). Every chunk is meshed again if the set changed. Lights that move every frame
    /// belong in the lighting uniform instead, see `lighting.rs`.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn set_static_lights(&mut self, lights: Vec<PointLight>) {
        if lights != self.static_lights {
            self.static_lights = lights;
            for chunk in self.chunks.values_mut() {
                chunk.mesh_dirty = true;
            }
        }
    }

    /// Texture the meshes with these sprites (or go back to flat colours with `None`). Every chunk
    /// is meshed again.
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))] // the browser build loads the atlas
    /// Whether the meshes are textured: only then does a block's value (an animation frame) show.
    pub fn sprites(&self) -> Option<&Rc<Sprites>> {
        self.sprites.as_ref()
    }

    pub fn has_sprites(&self) -> bool {
        self.sprites.is_some()
    }

    pub fn set_sprites(&mut self, sprites: Option<Rc<Sprites>>) {
        self.sprites = sprites;
        for chunk in self.chunks.values_mut() {
            chunk.mesh_dirty = true;
        }
    }

    /// Chunks from the centre to the edge of the window.
    pub fn window_radius(&self) -> i32 {
        if self.all_faces { FREE_WINDOW_RADIUS } else { FIXED_WINDOW_RADIUS }
    }

    /// Switch between the two render sets. The 2.5D one is what the fixed camera can see: a window
    /// that covers the screen of the one fixed view, and the three sides of a block that face the
    /// camera (a block whose three sides are covered is skipped). The free camera can look from any
    /// side, so its set is the whole surface: a bigger window (`FREE_WINDOW_RADIUS`, what the server
    /// sends) and every side of every block that no neighbour covers, left to the shader to cull
    /// the ones that turn away. Every chunk is meshed again when this changes, and the window is
    /// adjusted by the next [`update`](Self::update).
    pub fn is_free_view(&self) -> bool {
        self.all_faces
    }

    pub fn set_free_view(&mut self, free: bool) {
        if self.all_faces != free {
            self.all_faces = free;
            for chunk in self.chunks.values_mut() {
                chunk.mesh_dirty = true;
            }
        }
    }

    /// Is the block at these block coordinates opaque? Cells outside the window count as open, and
    /// so do the layers above the layer limit.
    fn is_opaque(&self, x: i32, y: i32, z: i32) -> bool {
        z <= self.top_layer() && self.cell(x, y, z).is_some_and(|cell| cell.hides_past_block())
    }

    /// The editor's layer limit, if one is set.
    pub fn layer_limit(&self) -> Option<i32> {
        self.layer_limit
    }

    /// The highest layer that is drawn: the layer limit, or the top of the world.
    fn top_layer(&self) -> i32 {
        self.layer_limit.unwrap_or(CHUNK_SIZE_Z - 1)
    }

    /// Draw only the layers up to `limit` (`None`: all), like the Java editor's Z rendering limit:
    /// the layers above are left out of the meshes, so the ones below can be looked at and edited
    /// from above. A limit at or above the top layer counts as none. Returns whether it changed
    /// (the caller then meshes again with [`vertices`](Self::vertices)).
    pub fn set_layer_limit(&mut self, limit: Option<i32>) -> bool {
        let limit = limit.map(|l| l.max(0)).filter(|&l| l < CHUNK_SIZE_Z - 1);
        if self.layer_limit == limit {
            return false;
        }
        self.layer_limit = limit;
        let top = self.top_layer();
        // The shading and the clipping of the top layer look at the layers above it.
        for chunk in self.chunks.values_mut() {
            chunk.apply_shading(top);
        }
        let mut positions: Vec<_> = self.chunks.keys().copied().collect();
        positions.sort_unstable();
        for pos in positions {
            let clipping = self.compute_clipping(pos);
            let chunk = self.chunks.get_mut(&pos).expect("listed above");
            for (cell, flags) in chunk.cells.iter_mut().zip(clipping) {
                cell.clipping = flags;
            }
            chunk.mesh_dirty = true;
        }
        true
    }

    /// Tell which areas force waves are moving blocks in. The chunks that are or were in such an area are
    /// meshed again with all the sides of their surface blocks, or without once it is over. Returns whether
    /// anything changed.
    pub fn set_wave_zones(&mut self, zones: Vec<(Vec3, f32)>) -> bool {
        if zones == self.wave_zones {
            return false;
        }
        let old = std::mem::replace(&mut self.wave_zones, zones);
        let near = |zones: &[(Vec3, f32)], chunk: (i32, i32)| {
            // The middle of the chunk in the ground frame; a chunk reaches some 25 blocks from it.
            let (gx, gy) = to_iso(chunk.0 * CHUNK_SIZE_X + CHUNK_SIZE_X / 2, chunk.1 * CHUNK_SIZE_Y + CHUNK_SIZE_Y / 2);
            zones.iter().any(|(c, r)| ((c.x - gx).powi(2) + (c.y - gy).powi(2)).sqrt() <= r + 25.0)
        };
        for (&pos, chunk) in self.chunks.iter_mut() {
            if near(&old, pos) || near(&self.wave_zones, pos) {
                chunk.mesh_dirty = true;
            }
        }
        true
    }

    /// Is the block at `(x, y, z)` in a force wave's area and near the surface of its column?
    fn is_exposed(&self, x: i32, y: i32, z: i32) -> bool {
        if self.wave_zones.is_empty() {
            return false;
        }
        let (gx, gy) = to_iso(x, y);
        let inside = self.wave_zones.iter().any(|(c, r)| ((c.x - gx).powi(2) + (c.y - gy).powi(2)).sqrt() <= *r);
        inside && (1..=EXPOSED_DEPTH).any(|k| !self.is_opaque(x, y, z + k))
    }

    /// All vertices of the window. Only chunks that changed are meshed again.
    pub fn vertices(&mut self) -> Vec<Vertex> {
        let dirty: Vec<(i32, i32)> = self.chunks.iter().filter(|(_, c)| c.mesh_dirty).map(|(&pos, _)| pos).collect();
        for pos in dirty {
            let mesh = {
                let this = &*self;
                let opaque = |x: i32, y: i32, z: i32| this.is_opaque(x, y, z);
                let exposed = |x: i32, y: i32, z: i32| this.is_exposed(x, y, z);
                let ctx = MeshContext { opaque: &opaque, lights: &this.static_lights, sprites: this.sprites.as_deref(), all_faces: this.all_faces, top: this.top_layer(), exposed: &exposed };
                mesh::build_chunk(&this.chunks[&pos], &ctx)
            };
            let chunk = self.chunks.get_mut(&pos).expect("listed above");
            chunk.mesh = mesh;
            chunk.mesh_dirty = false;
            chunk.mesh_version += 1;
        }
        let mut positions: Vec<_> = self.chunks.keys().copied().collect();
        positions.sort_unstable();
        positions.into_iter().flat_map(|pos| self.chunks[&pos].mesh.iter().copied()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wurfel_sim::{AirGenerator, Generator, IslandGenerator};

    /// Blocks at a fixed list of positions, otherwise air.
    struct Blocks(Vec<((i32, i32, i32), u8)>);
    impl Generator for Blocks {
        fn generate(&self, x: i32, y: i32, z: i32) -> Block {
            self.0.iter().find(|(p, _)| *p == (x, y, z)).map_or(Block::AIR, |&(_, b)| Block::new(b, 0))
        }
    }

    fn flags(storage: &RenderStorage, x: i32, y: i32, z: i32) -> u8 {
        storage.cell(x, y, z).expect("cell in window").clipping
    }

    fn positions(storage: &RenderStorage) -> Vec<(i32, i32)> {
        let mut p: Vec<_> = storage.chunks().map(|c| c.pos()).collect();
        p.sort_unstable();
        p
    }

    #[test]
    fn in_a_wave_zone_the_covered_sides_of_the_surface_blocks_are_meshed_and_afterwards_not_any_more() {
        // A solid 12 x 12 block of stone, four layers deep: nearly all sides are covered.
        let mut cells = Vec::new();
        for x in 0..12 {
            for y in 0..12 {
                for z in 0..4 {
                    cells.push(((x, y, z), id::STONE));
                }
            }
        }
        let mut world = World::new(Blocks(cells));
        let mut storage = RenderStorage::new();
        storage.update(&mut world, (0, 0));
        let plain = storage.vertices().len();
        let (gx, gy) = to_iso(6, 6);
        assert!(storage.set_wave_zones(vec![(Vec3::new(gx, gy, 1.0), 6.0)]));
        assert!(!storage.set_wave_zones(storage.wave_zones.clone()), "the same zone changes nothing");
        let during = storage.vertices().len();
        assert!(during > plain, "the covered sides show while the wave moves the blocks: {plain} -> {during}");
        assert!(storage.set_wave_zones(Vec::new()));
        assert_eq!(storage.vertices().len(), plain, "back to the cheap mesh once it is over");
    }

    #[test]
    fn the_free_camera_renders_a_5x5_window_and_the_fixed_one_a_3x3_window() {
        let mut world = World::new(IslandGenerator::new(1));
        let mut storage = RenderStorage::new();
        storage.update(&mut world, (0, 0));
        assert_eq!(positions(&storage).len(), 9);
        storage.set_free_view(true);
        storage.update(&mut world, (0, 0));
        assert_eq!(positions(&storage).len(), 25);
        assert_eq!((positions(&storage)[0], positions(&storage)[24]), ((-2, -2), (2, 2)));
        storage.set_free_view(false);
        storage.update(&mut world, (0, 0));
        assert_eq!(positions(&storage).len(), 9, "back to what the fixed camera sees");
    }

    #[test]
    fn the_window_is_3x3_and_follows_the_centre() {
        let mut world = World::new(IslandGenerator::new(1));
        let mut storage = RenderStorage::new();
        storage.update(&mut world, (0, 0));
        assert_eq!(positions(&storage).len(), 9);
        assert_eq!(positions(&storage)[0], (-1, -1));
        assert_eq!(positions(&storage)[8], (1, 1));

        storage.update(&mut world, (1, 0));
        let p = positions(&storage);
        assert_eq!(p.len(), 9);
        assert!(p.iter().all(|&(cx, cy)| (0..=2).contains(&cx) && (-1..=1).contains(&cy)), "{p:?}");
        assert!(storage.cell(-1, 0, 0).is_none(), "left the window");
        assert!(storage.cell(25, 0, 0).is_some(), "came into range");
    }

    #[test]
    fn cells_mirror_the_world_across_chunks() {
        let mut world = World::new(IslandGenerator::new(1));
        let mut storage = RenderStorage::new();
        storage.update(&mut world, (0, 0));
        for (x, y) in [(-10, -40), (-1, -1), (0, 0), (9, 39), (10, 40), (19, 79), (5, 12)] {
            for z in 0..CHUNK_SIZE_Z {
                assert_eq!(storage.cell(x, y, z).unwrap().block, world.get(x, y, z), "at {x},{y},{z}");
            }
        }
        assert!(storage.cell(20, 0, 0).is_none(), "outside the window");
        assert!(storage.cell(0, 0, CHUNK_SIZE_Z).is_none(), "above the world");
        assert!(storage.cell(0, 0, -1).is_none(), "below the world");
    }

    #[test]
    fn solid_neighbours_clip_the_sides_they_cover() {
        // A is at (5,5,0). B sits in front of its left side, C in front of its right side and D on top.
        let a = (5, 5, 0);
        let (llx, lly) = lower_left(5, 5);
        let (lrx, lry) = lower_right(5, 5);
        let mut world = World::new(Blocks(vec![
            (a, id::STONE),
            ((llx, lly, 0), id::STONE),
            ((lrx, lry, 0), id::STONE),
            ((5, 5, 1), id::STONE),
        ]));
        let mut storage = RenderStorage::new();
        storage.update(&mut world, (0, 0));
        assert_eq!(flags(&storage, 5, 5, 0), CLIP_LEFT | CLIP_RIGHT | CLIP_TOP);
        // the blocks in front are not covered by anything on their visible sides
        assert_eq!(flags(&storage, llx, lly, 0), 0);
        assert_eq!(flags(&storage, lrx, lry, 0), 0);
        assert_eq!(flags(&storage, 5, 5, 1), 0);
    }

    #[test]
    fn a_block_with_its_front_covered_still_shows_its_back_to_the_free_camera() {
        use crate::mesh::{FACE_BACK_X, FACE_BACK_Y};
        let (llx, lly) = lower_left(5, 5);
        let (lrx, lry) = lower_right(5, 5);
        let mut world = World::new(Blocks(vec![
            ((5, 5, 0), id::STONE),
            ((llx, lly, 0), id::STONE),
            ((lrx, lry, 0), id::STONE),
            ((5, 5, 1), id::STONE),
        ]));
        let back_faces = |storage: &mut RenderStorage| {
            storage.vertices().iter().filter(|v| v.shade[0] == FACE_BACK_X || v.shade[0] == FACE_BACK_Y).count()
        };
        let mut storage = RenderStorage::new();
        storage.update(&mut world, (0, 0));
        assert_eq!(back_faces(&mut storage), 0, "the fixed camera never meshes them");
        storage.set_free_view(true);
        // The block at (5, 5, 0) is fully clipped (its front is covered) but its two back sides are open.
        assert!(back_faces(&mut storage) >= 12, "{} back vertices", back_faces(&mut storage));
        storage.set_free_view(false);
        assert_eq!(back_faces(&mut storage), 0, "and switching back removes them");
    }

    #[test]
    fn air_covers_nothing_and_air_cells_have_no_flags() {
        let mut world = World::new(Blocks(vec![((5, 5, 0), id::STONE)]));
        let mut storage = RenderStorage::new();
        storage.update(&mut world, (0, 0));
        assert_eq!(flags(&storage, 5, 5, 0), 0);
        assert_eq!(flags(&storage, 6, 6, 0), 0);
    }

    #[test]
    fn water_hides_nothing_but_hides_water() {
        let (wx, wy) = lower_left(5, 5);
        let (w2x, w2y) = lower_left(8, 8);
        let mut world = World::new(Blocks(vec![
            ((5, 5, 0), id::STONE), // stone with water in front of its left side
            ((wx, wy, 0), id::WATER),
            ((8, 8, 0), id::WATER), // water with water in front of its left side
            ((w2x, w2y, 0), id::WATER),
        ]));
        let mut storage = RenderStorage::new();
        storage.update(&mut world, (0, 0));
        assert_eq!(flags(&storage, 5, 5, 0) & CLIP_LEFT, 0, "stone is visible through water");
        assert_eq!(flags(&storage, 8, 8, 0) & CLIP_LEFT, CLIP_LEFT, "no wall between two water blocks");
        assert_eq!(flags(&storage, w2x, w2y, 0) & CLIP_LEFT, 0, "the front water block still shows its side");
    }

    #[test]
    fn clipping_works_across_chunk_borders_and_follows_edits() {
        // Row 39 is the last row of chunk (0, 0); its lower neighbours are in chunk (0, 1).
        let (ax, ay) = (4, 39);
        let (bx, by) = lower_left(ax, ay);
        assert_eq!(chunk_of(bx, by), (0, 1), "the test needs the neighbour in another chunk");
        let mut world = World::new(Blocks(vec![((ax, ay, 0), id::STONE)]));
        let mut storage = RenderStorage::new();
        storage.update(&mut world, (0, 0));
        assert_eq!(flags(&storage, ax, ay, 0), 0);

        world.set(bx, by, 0, Block::new(id::STONE, 0));
        storage.update(&mut world, (0, 0));
        assert_eq!(flags(&storage, ax, ay, 0), CLIP_LEFT, "covered by a block in the next chunk");

        world.set(bx, by, 0, Block::AIR);
        storage.update(&mut world, (0, 0));
        assert_eq!(flags(&storage, ax, ay, 0), 0, "visible again after the block is removed");
    }

    #[test]
    fn dropping_a_neighbour_chunk_uncovers_the_border() {
        let (ax, ay) = (4, 39);
        let (bx, by) = lower_left(ax, ay);
        let mut world = World::new(Blocks(vec![((ax, ay, 0), id::STONE), ((bx, by, 0), id::STONE)]));
        let mut storage = RenderStorage::new();
        storage.update(&mut world, (0, 0));
        assert_eq!(flags(&storage, ax, ay, 0), CLIP_LEFT);

        storage.update(&mut world, (0, -1)); // window now covers chunk rows -2..=0, chunk (0, 1) is gone
        assert!(storage.chunk(0, 1).is_none());
        assert_eq!(flags(&storage, ax, ay, 0), 0, "Java treats cells outside the window as open");
    }

    #[test]
    fn overhangs_shade_the_top_like_java() {
        let mut world = World::new(Blocks(vec![
            ((1, 1, 0), id::STONE), // air above, stone at z=2: shade 0.8
            ((1, 1, 2), id::STONE),
            ((3, 1, 0), id::STONE), // air above, stone at z=3: shade 0.92
            ((3, 1, 3), id::STONE),
            ((5, 1, 0), id::STONE), // stone directly above: no extra shading rule applies
            ((5, 1, 1), id::STONE),
            ((7, 1, 0), id::STONE), // stone at z=4 is too far away
            ((7, 1, 4), id::STONE),
            ((9, 1, 8), id::STONE), // top of the world: nothing to look at
            ((9, 1, 9), id::STONE),
        ]));
        let mut storage = RenderStorage::new();
        storage.update(&mut world, (0, 0));
        let light = |x, z| storage.cell(x, 1, z).unwrap().top_light;
        assert_eq!(light(1, 0), 0.8);
        assert_eq!(light(3, 0), 0.92);
        assert_eq!(light(5, 0), 1.0);
        assert_eq!(light(7, 0), 1.0);
        assert_eq!(light(9, 8), 1.0);
        assert_eq!(light(9, 9), 1.0);
    }

    #[test]
    fn the_layer_limit_leaves_out_the_layers_above_and_uncovers_the_top_one() {
        // A tower of three stones, and a stone with an overhang two layers above it.
        let mut world = World::new(Blocks(vec![
            ((1, 1, 0), id::STONE),
            ((1, 1, 1), id::STONE),
            ((1, 1, 2), id::STONE),
            ((5, 1, 0), id::STONE),
            ((5, 1, 2), id::STONE),
        ]));
        let mut storage = RenderStorage::new();
        storage.update(&mut world, (0, 0));
        let all = storage.vertices();
        assert_eq!(storage.cell(5, 1, 0).unwrap().top_light, 0.8);
        assert_eq!(flags(&storage, 1, 1, 1) & CLIP_TOP, CLIP_TOP, "covered by the stone above");

        assert!(storage.set_layer_limit(Some(1)));
        assert!(!storage.set_layer_limit(Some(1)), "nothing changed");
        let limited = storage.vertices();
        assert!(limited.len() < all.len(), "the layer 2 stones are gone");
        assert!(limited.iter().all(|v| v.position[2] <= 2.0), "nothing above the top of layer 1");
        assert_eq!(flags(&storage, 1, 1, 1) & CLIP_TOP, 0, "the top layer shows its top");
        assert_eq!(storage.cell(5, 1, 0).unwrap().top_light, 1.0, "an overhang that is not drawn casts no shadow");
        assert!(!storage.is_opaque(1, 1, 2), "and does not occlude");
        assert!(storage.is_opaque(1, 1, 1));

        // A limit at the top of the world, or none, is the whole world again.
        assert!(storage.set_layer_limit(Some(CHUNK_SIZE_Z + 3)));
        assert_eq!(storage.vertices(), all);
        assert!(!storage.set_layer_limit(None));
        // Chunks that come into the window afterwards honour the limit.
        storage.set_layer_limit(Some(0));
        storage.update(&mut world, (1, 0));
        assert!(storage.vertices().iter().all(|v| v.position[2] <= 1.0));
    }

    #[test]
    fn a_replaced_world_does_not_leak_old_cells() {
        let mut first = World::new(Blocks(vec![((5, 5, 0), id::STONE)]));
        let mut storage = RenderStorage::new();
        storage.update(&mut first, (0, 0));
        assert_eq!(storage.cell(5, 5, 0).unwrap().block.id(), id::STONE);

        let mut second = World::new(AirGenerator);
        storage.update(&mut second, (0, 0));
        assert!(storage.cell(5, 5, 0).unwrap().block.is_air());
    }

    #[test]
    fn an_edit_remeshes_only_the_chunks_that_can_be_affected() {
        let mut world = World::new(IslandGenerator::new(1));
        let mut storage = RenderStorage::new();
        storage.update_area(&mut world, (-2, -2), (2, 2));
        let before = storage.vertices();
        let versions = |s: &RenderStorage| -> Vec<((i32, i32), u32)> {
            let mut v: Vec<_> = s.chunks().map(|c| (c.pos(), c.mesh_version())).collect();
            v.sort_unstable();
            v
        };
        let v0 = versions(&storage);
        assert_eq!(v0.len(), 25);
        assert!(v0.iter().all(|&(_, n)| n == 1));

        storage.update_area(&mut world, (-2, -2), (2, 2));
        assert_eq!(storage.vertices(), before, "nothing changed, nothing is rebuilt");
        assert_eq!(versions(&storage), v0);

        // An edit in the middle of the middle chunk. Clipping and ambient occlusion reach across the
        // chunk border, so the chunk and its eight neighbours are rebuilt, and nothing farther away.
        world.set(5, 20, 9, Block::new(id::STONE, 0));
        storage.update_area(&mut world, (-2, -2), (2, 2));
        let after = storage.vertices();
        assert_ne!(after, before);
        let rebuilt: Vec<_> = versions(&storage).iter().filter(|&&(_, n)| n == 2).map(|&(p, _)| p).collect();
        let expected: Vec<_> = (-1..=1).flat_map(|cx| (-1..=1).map(move |cy| (cx, cy))).collect();
        assert_eq!(rebuilt, expected);
    }

    /// A stone block on a stone floor, in one chunk.
    fn stone_on_floor() -> World {
        let mut blocks = vec![((5, 20, 1), id::STONE)];
        for x in 0..10 {
            for y in 10..30 {
                blocks.push(((x, y, 0), id::STONE));
            }
        }
        World::new(Blocks(blocks))
    }

    #[test]
    fn every_lit_face_carries_the_occlusion_of_all_its_corners_for_the_shader() {
        let mut world = stone_on_floor();
        let mut storage = RenderStorage::new();
        storage.update_area(&mut world, (0, 0), (0, 0));
        let vertices = storage.vertices();
        let lit: Vec<_> = vertices.iter().filter(|v| v.shade[0] == mesh::FACE_TOP || v.shade[0] == mesh::FACE_LEFT || v.shade[0] == mesh::FACE_RIGHT).collect();
        assert!(!lit.is_empty());
        let decode = |v: &Vertex| {
            assert!(v.occlusion >= mesh::OCCLUSION_FLAG, "a lit face has no corners' occlusion: {v:?}");
            let packed = (v.occlusion - mesh::OCCLUSION_FLAG) as u32;
            ([0, 1, 2, 3].map(|i| ((packed >> (2 * i)) & 3) as u8), (packed >> 8) as usize)
        };
        let mut seen_dark = false;
        // A face is two triangles, six vertices in a row, which share the corners' counts and show every corner.
        for quad in vertices.chunks(6).filter(|q| mesh::FACE_TOP == q[0].shade[0] || mesh::FACE_LEFT == q[0].shade[0] || mesh::FACE_RIGHT == q[0].shade[0]) {
            let (counts, _) = decode(&quad[0]);
            let mut corners: Vec<usize> = quad.iter().map(|v| decode(v).1).collect();
            for v in quad {
                let (c, corner) = decode(v);
                assert_eq!(c, counts, "the corners' counts differ inside one face");
                assert!((v.shade[1] * 3.0 - counts[corner] as f32).abs() < 1e-4, "the vertex's own occlusion is its corner's count");
            }
            corners.sort_unstable();
            corners.dedup();
            assert_eq!(corners, vec![0, 1, 2, 3]);
            seen_dark |= counts.iter().any(|&c| c > 0);
        }
        assert!(seen_dark, "the floor next to the block is occluded");
    }

    #[test]
    fn meshes_carry_ambient_occlusion_next_to_walls_and_none_in_the_open() {
        let mut world = stone_on_floor();
        let mut storage = RenderStorage::new();
        storage.update_area(&mut world, (0, 0), (0, 0));
        let vertices = storage.vertices();
        assert!(vertices.iter().all(|v| (0.0..=1.0).contains(&v.shade[1])));
        let top_of_floor: Vec<_> = vertices.iter().filter(|v| v.shade[0] == mesh::FACE_TOP && v.position[2] == 1.0).collect();
        assert!(!top_of_floor.is_empty());
        assert!(top_of_floor.iter().any(|v| v.shade[1] > 0.0), "the floor next to the block is occluded");
        assert!(top_of_floor.iter().any(|v| v.shade[1] == 0.0), "and open farther away");
        // Every darkened floor vertex lies within one block of the stone (lattice distance 1).
        let (gx, gy) = wurfel_sim::grid::to_iso(5, 20);
        for v in top_of_floor.iter().filter(|v| v.shade[1] > 0.0) {
            assert!((v.position[0] - gx).abs() <= 1.5 && (v.position[1] - gy).abs() <= 1.5, "{:?}", v.position);
        }
    }

    #[test]
    fn static_point_lights_are_baked_and_changing_them_remeshes() {
        let mut world = stone_on_floor();
        let mut storage = RenderStorage::new();
        storage.update_area(&mut world, (0, 0), (0, 0));
        let dark = storage.vertices();
        assert!(dark.iter().all(|v| v.point == [0.0; 3]));
        let version = storage.chunk(0, 0).unwrap().mesh_version();

        let (gx, gy) = wurfel_sim::grid::to_iso(2, 14);
        let lamp = PointLight::new(glam::Vec3::new(gx, gy, 4.0), glam::Vec3::new(1.0, 0.8, 0.4), 6.0, 1.0);
        storage.set_static_lights(vec![lamp]);
        let lit = storage.vertices();
        assert_eq!(storage.chunk(0, 0).unwrap().mesh_version(), version + 1);
        assert!(lit.iter().any(|v| v.point[0] > 0.0), "some vertices are lit");
        assert!(lit.iter().all(|v| v.point[0] >= v.point[2]), "the warm colour is kept");
        for v in &lit {
            let d = (glam::Vec3::from(v.position) - lamp.position).length();
            if d >= lamp.radius {
                assert_eq!(v.point, [0.0; 3], "nothing beyond the radius is lit");
            }
        }

        storage.set_static_lights(vec![lamp]);
        storage.vertices();
        assert_eq!(storage.chunk(0, 0).unwrap().mesh_version(), version + 1, "the same lights change nothing");
        storage.set_static_lights(vec![]);
        assert!(storage.vertices().iter().all(|v| v.point == [0.0; 3]));
    }

    #[test]
    fn vertices_are_the_chunk_meshes_in_a_stable_order() {
        let mut world = World::new(IslandGenerator::new(1));
        let mut a = RenderStorage::new();
        let mut b = RenderStorage::new();
        a.update(&mut world, (0, 0));
        b.update_area(&mut world, (-1, -1), (1, 1));
        assert_eq!(a.vertices(), b.vertices());
    }

    /// Timing of meshing with lighting. Run with:
    /// `cargo test --release -p wurfel-web remesh_cost -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn remesh_cost() {
        use std::time::Instant;
        let mut world = World::new(IslandGenerator::new(1));
        let mut storage = RenderStorage::new();
        let t = Instant::now();
        storage.update_area(&mut world, (-1, -1), (1, 1));
        let load = t.elapsed();
        let t = Instant::now();
        let vertices = storage.vertices();
        let first = t.elapsed();
        println!("load + clipping of 9 chunks: {load:?}");
        println!("first mesh of 9 chunks (AO, no lights): {first:?} -> {} vertices ({:?} per chunk)", vertices.len(), first / 9);

        // One block edit: the chunk and its eight neighbours are meshed again.
        world.set(5, 20, 9, Block::new(id::STONE, 0));
        storage.update_area(&mut world, (-1, -1), (1, 1));
        let t = Instant::now();
        storage.vertices();
        println!("re-mesh after an edit (9 chunks): {:?}", t.elapsed());

        let (gx, gy) = wurfel_sim::grid::to_iso(5, 20);
        let lamp = PointLight::new(glam::Vec3::new(gx, gy, 9.0), glam::Vec3::ONE, 9.0, 2.0);
        storage.set_static_lights(vec![lamp]);
        let t = Instant::now();
        storage.vertices();
        println!("re-mesh of 9 chunks with one baked point light: {:?}", t.elapsed());
    }
}
