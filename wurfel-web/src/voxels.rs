//! The block world as a 3D grid of opacities, for the sharp sun shadows (`ShadowMethod::Voxel`).
//!
//! A shadow map has texels, so the edge of a block's shadow is a staircase however fine the map is.
//! The world is made of axis-aligned cubes, though, so the question "does the sun reach this point?"
//! can be answered exactly: walk the ray from the point towards the sun cell by cell (the Amanatides
//! and Woo traversal) and see whether it meets a block. `shader.wgsl` (`voxel_visibility`) does that
//! per pixel against a 3D texture made from [`VoxelGrid`]; [`VoxelGrid::transmittance`] is the same
//! walk on the CPU, the reference the tests check.
//!
//! Cells are the blocks in isometric ground coordinates (see `wurfel_sim::grid`): block centres are
//! on the integer lattice, so the cell of a point is `floor(p.xy + 0.5)` and `floor(p.z)`. A cell holds
//! how much light it stops: 255 for a solid block, [`WATER`] for liquid, 0 for air and for everything
//! that is a picture standing on the ground (trees, torches), which casts through the sprite map instead.
//!
//! # Soft edges
//!
//! The sun is a disc, so a real shadow is sharp where the occluder touches the ground and blurs with
//! the distance from it. For that the grid has a second form, a *signed distance field*
//! ([`VoxelGrid::distance_field`]): every cell holds how far it is from the nearest block (negative
//! inside one). The ray then does not walk cell by cell but jumps by that distance (sphere tracing),
//! and on the way keeps how close to a block it came, relative to how far it has gone: a ray that
//! passes `d` from an edge after `t` has the sun's disc (`2 * soft * t` wide) covered
//! `0.5 - 0.5 * d / (soft * t)`. That is a smooth function of the receiver's position, so the penumbra
//! has no steps, and it is as wide as the disc: nothing at the contact and more with the distance.
//! [`VoxelGrid::soft_visibility`] is that on the CPU; `shader.wgsl` (`voxel_visibility`) the same on the GPU.

#![cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]

use glam::Vec3;
use wurfel_sim::grid::to_iso;
use wurfel_sim::{CHUNK_SIZE_X, CHUNK_SIZE_Y, CHUNK_SIZE_Z};

use crate::render_storage::RenderStorage;

/// Cells along x and y. The 5x5 window of the free camera is about 152 cells across in isometric
/// ground coordinates, the 3x3 one about 92.
pub const SIZE_XY: usize = 160;
/// Cells along z: the height of the world.
pub const SIZE_Z: usize = CHUNK_SIZE_Z as usize;
/// How much light water stops (of 255): a lake shades what is under it a little.
pub const WATER: u8 = 70;
/// Steps of the sphere tracing at most. It takes big steps in the open, so it is far fewer than the walk.
pub const SOFT_STEPS: u32 = 64;
/// How much of the distance field a step uses: the field is made from the centres of the cells and
/// overestimates a little at corners, so a full step could jump into a block.
pub const SPHERE_STEP: f32 = 0.7;
/// The shortest step, in cells.
pub const MIN_STEP: f32 = 0.05;
/// A surface's own blur must not reach its own block: the sun's `soft` is at most this times the
/// cosine of the angle between the surface's normal and the sun.
pub const SELF_CLEARANCE: f32 = 0.95;
/// Distances are stored up to this far (cells); beyond it nothing is near.
pub const MAX_DISTANCE: f32 = 32.0;
/// Steps of the ray walk the shader takes at most. A ray is done when it leaves the grid, which for a
/// high sun is after about the height of the world.
pub const MAX_STEPS: u32 = 128;

#[derive(Debug, Clone, PartialEq)]
pub struct VoxelGrid {
    /// The isometric ground cell that cell (0, 0) of the grid is.
    pub origin: (i32, i32),
    /// `SIZE_XY * SIZE_XY * SIZE_Z` opacities, x fastest, then y, then z: the layout of the 3D texture.
    pub data: Vec<u8>,
}

fn index(x: usize, y: usize, z: usize) -> usize {
    x + SIZE_XY * (y + SIZE_XY * z)
}

impl VoxelGrid {
    pub fn empty(origin: (i32, i32)) -> Self {
        VoxelGrid { origin, data: vec![0; SIZE_XY * SIZE_XY * SIZE_Z] }
    }

    /// The grid of the blocks in the render window.
    pub fn build(render: &RenderStorage) -> Self {
        // The window is a band of chunks in the staggered lattice, which is a diagonal band in the
        // isometric one: its corners give the lowest cell.
        let (mut min_x, mut min_y) = (i32::MAX, i32::MAX);
        for chunk in render.chunks() {
            let (left, top) = chunk.top_left();
            for (x, y) in [(left, top), (left + CHUNK_SIZE_X - 1, top), (left, top + CHUNK_SIZE_Y - 1), (left + CHUNK_SIZE_X - 1, top + CHUNK_SIZE_Y - 1)] {
                let (gx, gy) = to_iso(x, y);
                min_x = min_x.min(gx.round() as i32);
                min_y = min_y.min(gy.round() as i32);
            }
        }
        if min_x == i32::MAX {
            return VoxelGrid::empty((0, 0));
        }
        let mut grid = VoxelGrid::empty((min_x, min_y));
        for chunk in render.chunks() {
            let (left, top) = chunk.top_left();
            for lx in 0..CHUNK_SIZE_X {
                for ly in 0..CHUNK_SIZE_Y {
                    let (gx, gy) = to_iso(left + lx, top + ly);
                    let (cx, cy) = (gx.round() as i32 - min_x, gy.round() as i32 - min_y);
                    if cx < 0 || cy < 0 || cx >= SIZE_XY as i32 || cy >= SIZE_XY as i32 {
                        continue;
                    }
                    for z in 0..CHUNK_SIZE_Z {
                        let Some(cell) = chunk.cell(lx, ly, z) else { continue };
                        let opacity = if cell.block.is_air() {
                            0
                        } else if cell.hides_past_block() {
                            255
                        } else if cell.is_liquid() {
                            WATER
                        } else {
                            0
                        };
                        if opacity > 0 {
                            grid.data[index(cx as usize, cy as usize, z as usize)] = opacity;
                        }
                    }
                }
            }
        }
        grid
    }

    /// The opacity of the cell with these grid coordinates; 0 outside the grid.
    fn at(&self, cell: [i32; 3]) -> u8 {
        let [x, y, z] = cell;
        if x < 0 || y < 0 || z < 0 || x >= SIZE_XY as i32 || y >= SIZE_XY as i32 || z >= SIZE_Z as i32 {
            return 0;
        }
        self.data[index(x as usize, y as usize, z as usize)]
    }

    /// How much light is left after the ray from `start` along the unit vector `to_sun`: 1 in the open,
    /// 0 behind a solid block, in between behind water. Mirrors `voxel_visibility` in `shader.wgsl`
    /// without the soft part.
    pub fn transmittance(&self, start: Vec3, to_sun: Vec3, steps: u32) -> f32 {
        self.walk(start, to_sun, steps, f32::INFINITY).0
    }

    /// The exact walk. It stops after `steps` cells, when the ray leaves the grid, or when it has gone
    /// `reach` cells; in that last case it also returns where the soft part has to go on from: the
    /// distance along the ray.
    fn walk(&self, start: Vec3, to_sun: Vec3, steps: u32, reach: f32) -> (f32, Option<f32>) {
        let q = start + Vec3::new(0.5, 0.5, 0.0) - Vec3::new(self.origin.0 as f32, self.origin.1 as f32, 0.0);
        let mut cell = [q.x.floor() as i32, q.y.floor() as i32, q.z.floor() as i32];
        let along = |q: f32, d: f32| -> (f32, f32, i32) {
            if d.abs() < 1e-6 {
                return (1e30, 1e30, 0);
            }
            let edge = if d > 0.0 { q.floor() + 1.0 } else { q.floor() };
            ((edge - q) / d, 1.0 / d.abs(), if d > 0.0 { 1 } else { -1 })
        };
        let (mut tx, dx, sx) = along(q.x, to_sun.x);
        let (mut ty, dy, sy) = along(q.y, to_sun.y);
        let (mut tz, dz, sz) = along(q.z, to_sun.z);
        let mut transmittance = 1.0;
        for _ in 0..steps {
            if cell[2] >= SIZE_Z as i32 || cell[2] < 0 || cell[0] < 0 || cell[1] < 0 || cell[0] >= SIZE_XY as i32 || cell[1] >= SIZE_XY as i32 {
                break;
            }
            transmittance *= 1.0 - self.at(cell) as f32 / 255.0;
            if transmittance < 0.02 {
                return (0.0, None);
            }
            let travelled;
            if tx < ty && tx < tz {
                travelled = tx;
                cell[0] += sx;
                tx += dx;
            } else if ty < tz {
                travelled = ty;
                cell[1] += sy;
                ty += dy;
            } else {
                travelled = tz;
                cell[2] += sz;
                tz += dz;
            }
            if travelled > reach {
                return (transmittance, Some(travelled));
            }
        }
        (transmittance, None)
    }

    /// The signed distance field of the solid cells, in cells: for an empty cell the distance from its
    /// centre to the nearest block's surface, for a solid cell minus the depth inside. Made from
    /// the distance between the centres of cells (the exact Euclidean transform), shifted by half a cell,
    /// so the border between a block and the air next to it is exactly 0 and it is linear across it.
    /// Water does not count. Layout as [`VoxelGrid::data`].
    pub fn distance_field(&self) -> Vec<f32> {
        let solid: Vec<bool> = self.data.iter().map(|&v| v >= 128).collect();
        let to_solid = euclidean_distance(&solid, true);
        let to_air = euclidean_distance(&solid, false);
        solid
            .iter()
            .zip(to_solid.iter().zip(&to_air))
            .map(|(&is_solid, (&near_solid, &near_air))| {
                let d = if is_solid { 0.5 - near_air } else { near_solid - 0.5 };
                d.clamp(-MAX_DISTANCE, MAX_DISTANCE)
            })
            .collect()
    }

    /// The same, as the 16 bit floats of the 3D texture.
    pub fn distance_texels(&self) -> Vec<u16> {
        self.distance_field().into_iter().map(f32_to_f16).collect()
    }

    /// What the GPU's trilinear sampling gives for `field` ([`Self::distance_field`]) at `p` in cells (the
    /// corner of the grid is 0): the distance, clamped at the borders of the grid.
    pub fn sample_field(field: &[f32], p: Vec3) -> f32 {
        let q = p - Vec3::splat(0.5);
        let base = q.floor();
        let t = q - base;
        let fetch = |dx: i32, dy: i32, dz: i32| -> f32 {
            let x = (base.x as i32 + dx).clamp(0, SIZE_XY as i32 - 1) as usize;
            let y = (base.y as i32 + dy).clamp(0, SIZE_XY as i32 - 1) as usize;
            let z = (base.z as i32 + dz).clamp(0, SIZE_Z as i32 - 1) as usize;
            field[index(x, y, z)]
        };
        let mut sum = 0.0;
        for dz in 0..2 {
            for dy in 0..2 {
                for dx in 0..2 {
                    let weight = (if dx == 0 { 1.0 - t.x } else { t.x }) * (if dy == 0 { 1.0 - t.y } else { t.y }) * (if dz == 0 { 1.0 - t.z } else { t.z });
                    sum += weight * fetch(dx, dy, dz);
                }
            }
        }
        sum
    }

    /// How much of the sun's disc is visible from `start` (1: all, 0: none) when the sun is a disc of
    /// angular radius `atan(soft)` in the direction `to_sun`, and `normal` is the way the surface at `start`
    /// faces. `field` is [`Self::distance_field`]. Mirrors `voxel_visibility` in `shader.wgsl` (its soft part).
    pub fn soft_visibility(&self, field: &[f32], start: Vec3, to_sun: Vec3, normal: Vec3, soft: f32) -> f32 {
        // The blur of a surface that is hardly turned to the sun must not reach its own block.
        let soft = soft.min(SELF_CLEARANCE * normal.dot(to_sun));
        if soft <= 0.001 {
            return self.transmittance(start, to_sun, MAX_STEPS);
        }
        let q = start + Vec3::new(0.5, 0.5, 0.0) - Vec3::new(self.origin.0 as f32, self.origin.1 as f32, 0.0);
        let (mut visible, mut previous, mut t) = (1.0f32, 1.0e20f32, 0.0f32);
        for _ in 0..SOFT_STEPS {
            let p = q + to_sun * t;
            if p.z >= SIZE_Z as f32 || p.z < 0.0 || p.x < 0.0 || p.y < 0.0 || p.x >= SIZE_XY as f32 || p.y >= SIZE_XY as f32 {
                break;
            }
            let d = Self::sample_field(field, p);
            if d <= 0.0 {
                return 0.0;
            }
            // The closest the ray came to the block between this sample and the one before is a bit
            // nearer than `d` (the spheres of the two overlap): this removes the bands of a plain minimum.
            let y = d * d / (2.0 * previous);
            let closest = (d * d - y * y).max(0.0).sqrt();
            visible = visible.min((0.5 + 0.5 * closest / (soft * (t - y).max(0.001))).clamp(0.0, 1.0));
            previous = d;
            t += (d * SPHERE_STEP).max(MIN_STEP);
        }
        visible
    }
}

/// The distance from every cell to the nearest cell whose `solid` flag is `target` (0 for those cells
/// themselves), by the exact Euclidean transform (Felzenszwalb and Huttenlocher) along the three axes. Cells
/// of the grid only count: beyond it is unknown, so a grid without such a cell gets [`MAX_DISTANCE`] everywhere.
fn euclidean_distance(solid: &[bool], target: bool) -> Vec<f32> {
    const FAR: f32 = 1.0e9;
    let mut squared: Vec<f32> = solid.iter().map(|&s| if s == target { 0.0 } else { FAR }).collect();
    let n = SIZE_XY.max(SIZE_Z);
    let (mut line, mut out, mut v, mut z) = (vec![0.0f32; n], vec![0.0f32; n], vec![0usize; n], vec![0.0f32; n + 1]);
    // One pass along an axis: `len` cells with `stride` between them, for every line starting at `starts`.
    let mut pass = |squared: &mut [f32], len: usize, stride: usize, starts: &mut dyn Iterator<Item = usize>| {
        for start in starts {
            for i in 0..len {
                line[i] = squared[start + i * stride];
            }
            transform_line(&line[..len], &mut out[..len], &mut v, &mut z);
            for i in 0..len {
                squared[start + i * stride] = out[i];
            }
        }
    };
    pass(&mut squared, SIZE_XY, 1, &mut (0..SIZE_Z * SIZE_XY).map(|row| row * SIZE_XY));
    pass(&mut squared, SIZE_XY, SIZE_XY, &mut (0..SIZE_Z).flat_map(|z| (0..SIZE_XY).map(move |x| x + SIZE_XY * SIZE_XY * z)));
    pass(&mut squared, SIZE_Z, SIZE_XY * SIZE_XY, &mut (0..SIZE_XY * SIZE_XY));
    squared.into_iter().map(|d| if d >= FAR * 0.5 { MAX_DISTANCE * 2.0 } else { d.sqrt() }).collect()
}

/// The 1D squared distance transform of `f` (the lower envelope of the parabolas `f[q] + (x - q)^2`).
fn transform_line(f: &[f32], out: &mut [f32], v: &mut [usize], z: &mut [f32]) {
    let n = f.len();
    let mut k = 0usize;
    v[0] = 0;
    z[0] = f32::NEG_INFINITY;
    z[1] = f32::INFINITY;
    for q in 1..n {
        loop {
            let p = v[k];
            let s = ((f[q] + (q * q) as f32) - (f[p] + (p * p) as f32)) / (2.0 * (q as f32 - p as f32));
            if s <= z[k] && k > 0 {
                k -= 1;
            } else {
                k += 1;
                v[k] = q;
                z[k] = s;
                z[k + 1] = f32::INFINITY;
                break;
            }
        }
        // `k` was already increased in the else branch; the first iteration with k = 0 never goes down.
    }
    let mut k = 0usize;
    for q in 0..n {
        while z[k + 1] < q as f32 {
            k += 1;
        }
        let d = q as f32 - v[k] as f32;
        out[q] = d * d + f[v[k]];
    }
}

/// `x` as the bits of an IEEE half float (round to nearest; values too small become 0, too large the
/// largest half). Enough for distances of a few dozen cells.
pub fn f32_to_f16(x: f32) -> u16 {
    if !x.is_finite() {
        return if x.is_nan() { 0 } else if x > 0.0 { 0x7bff } else { 0xfbff };
    }
    let bits = x.to_bits();
    let sign = ((bits >> 16) & 0x8000) as u16;
    let exponent = ((bits >> 23) & 0xff) as i32 - 127 + 15;
    let mantissa = bits & 0x007f_ffff;
    if exponent <= 0 {
        return sign;
    }
    if exponent >= 31 {
        return sign | 0x7bff;
    }
    let half = ((exponent as u32) << 10) | (mantissa >> 13);
    // Round to nearest: the dropped 13 bits decide, a carry may roll into the exponent (which is right).
    let rounded = half + ((mantissa >> 12) & 1);
    if rounded >= 0x7c00 {
        return sign | 0x7bff;
    }
    sign | rounded as u16
}

/// Browser only: the 3D textures the shader reads.
#[cfg(target_arch = "wasm32")]
pub mod gpu {
    use super::*;

    /// The opacities (the exact walk of the hard shadows).
    pub const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R8Unorm;
    /// The signed distance field (the soft shadows): half floats are filterable everywhere.
    pub const FIELD_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R16Float;

    pub struct VoxelTexture {
        texture: wgpu::Texture,
        view: wgpu::TextureView,
        field_texture: wgpu::Texture,
        field_view: wgpu::TextureView,
        /// Linear between cells: the soft part reads the distance between the centres from it.
        sampler: wgpu::Sampler,
        /// What the textures hold, to skip uploads of an unchanged grid.
        grid: VoxelGrid,
        field_valid: bool,
    }

    impl VoxelTexture {
        pub fn new(device: &wgpu::Device) -> Self {
            let texture = |label: &str, format| {
                device.create_texture(&wgpu::TextureDescriptor {
                    label: Some(label),
                    size: wgpu::Extent3d { width: SIZE_XY as u32, height: SIZE_XY as u32, depth_or_array_layers: SIZE_Z as u32 },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D3,
                    format,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                    view_formats: &[],
                })
            };
            let (texture, field_texture) = (texture("voxels", FORMAT), texture("voxel distances", FIELD_FORMAT));
            let (view, field_view) = (texture.create_view(&Default::default()), field_texture.create_view(&Default::default()));
            let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("voxels"),
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                ..Default::default()
            });
            VoxelTexture { texture, view, field_texture, field_view, sampler, grid: VoxelGrid::empty((i32::MIN, i32::MIN)), field_valid: false }
        }

        pub fn view(&self) -> &wgpu::TextureView {
            &self.view
        }

        pub fn field_view(&self) -> &wgpu::TextureView {
            &self.field_view
        }

        pub fn sampler(&self) -> &wgpu::Sampler {
            &self.sampler
        }

        /// The cell that cell (0, 0) of the texture is.
        pub fn origin(&self) -> (i32, i32) {
            self.grid.origin
        }

        fn write<T: bytemuck::Pod>(&self, texture: &wgpu::Texture, queue: &wgpu::Queue, data: &[T]) {
            let texel = std::mem::size_of::<T>() as u32;
            queue.write_texture(
                wgpu::TexelCopyTextureInfo { texture, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
                bytemuck::cast_slice(data),
                wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(SIZE_XY as u32 * texel), rows_per_image: Some(SIZE_XY as u32) },
                wgpu::Extent3d { width: SIZE_XY as u32, height: SIZE_XY as u32, depth_or_array_layers: SIZE_Z as u32 },
            );
        }

        /// Make the textures hold `grid`. The distance field costs a few milliseconds to make, so it is only
        /// made (`with_field`) while the soft shadows are on. Nothing is sent that is already there.
        pub fn set(&mut self, queue: &wgpu::Queue, grid: VoxelGrid, with_field: bool) {
            let changed = grid != self.grid;
            if !changed && (!with_field || self.field_valid) {
                return;
            }
            if changed {
                self.write(&self.texture, queue, &grid.data);
                self.field_valid = false;
            }
            if with_field {
                self.write(&self.field_texture, queue, &grid.distance_texels());
                self.field_valid = true;
            }
            self.grid = grid;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wurfel_sim::block::id;
    use wurfel_sim::{Block, Generator, World};

    struct Blocks(Vec<((i32, i32, i32), u8)>);
    impl Generator for Blocks {
        fn generate(&self, x: i32, y: i32, z: i32) -> Block {
            self.0.iter().find(|(p, _)| *p == (x, y, z)).map_or(Block::AIR, |&(_, b)| Block::new(b, 0))
        }
    }

    fn storage(blocks: Vec<((i32, i32, i32), u8)>) -> RenderStorage {
        let mut world = World::new(Blocks(blocks));
        let mut render = RenderStorage::new();
        render.update(&mut world, (0, 0));
        render
    }

    /// The grid cell of the block at staggered coordinates `(x, y, z)`.
    fn cell_of(grid: &VoxelGrid, x: i32, y: i32, z: i32) -> [usize; 3] {
        let (gx, gy) = to_iso(x, y);
        [(gx.round() as i32 - grid.origin.0) as usize, (gy.round() as i32 - grid.origin.1) as usize, z as usize]
    }

    #[test]
    fn a_solid_block_is_a_cell_at_its_isometric_position_and_air_is_empty() {
        let render = storage(vec![((5, 5, 3), id::STONE), ((6, 5, 0), id::WATER)]);
        let grid = VoxelGrid::build(&render);
        let [x, y, z] = cell_of(&grid, 5, 5, 3);
        assert_eq!(grid.data[index(x, y, z)], 255);
        let [x, y, z] = cell_of(&grid, 6, 5, 0);
        assert_eq!(grid.data[index(x, y, z)], WATER);
        assert_eq!(grid.data.iter().filter(|&&v| v == 255).count(), 1, "nothing else is solid");
        assert_eq!(grid.data.iter().filter(|&&v| v == WATER).count(), 1);
    }

    #[test]
    fn every_loaded_cell_fits_into_the_grid_for_both_windows() {
        for free in [false, true] {
            let mut world = World::new(Blocks(Vec::new()));
            let mut render = RenderStorage::new();
            render.set_free_view(free);
            render.update(&mut world, (3, -2));
            let grid = VoxelGrid::build(&render);
            for chunk in render.chunks() {
                let (left, top) = chunk.top_left();
                for (x, y) in [(left, top), (left + CHUNK_SIZE_X - 1, top + CHUNK_SIZE_Y - 1), (left, top + CHUNK_SIZE_Y - 1), (left + CHUNK_SIZE_X - 1, top)] {
                    let (gx, gy) = to_iso(x, y);
                    let (cx, cy) = (gx.round() as i32 - grid.origin.0, gy.round() as i32 - grid.origin.1);
                    assert!((0..SIZE_XY as i32).contains(&cx) && (0..SIZE_XY as i32).contains(&cy), "free {free}: cell ({cx}, {cy})");
                }
            }
        }
    }

    #[test]
    fn a_wall_shades_the_ground_behind_it_exactly_and_the_open_ground_is_lit() {
        let mut grid = VoxelGrid::empty((0, 0));
        // A wall: the cells x = 50 for all y, z 0..4 (isometric ground coordinates).
        for y in 0..SIZE_XY {
            for z in 0..4 {
                grid.data[index(50, y, z)] = 255;
            }
        }
        let sun = Vec3::new(-1.0, 0.0, 1.0).normalize(); // towards -x and up: the shadow falls towards +x
        // A point on the ground (z 0 plane of cell z = 0 is the floor; ground is at z = 4, on top of the wall? no:
        // the ground here is the plane z = 0 and the wall stands from 0 to 4).
        // Behind the wall (x larger) at height 0.5: the ray towards the sun crosses the wall.
        let behind = Vec3::new(52.0 + 0.0 - 0.5, 10.0, 0.5);
        assert_eq!(grid.transmittance(behind, sun, MAX_STEPS), 0.0);
        // Far behind it the ray passes above the wall: lit.
        let far = Vec3::new(60.0, 10.0, 0.5);
        assert_eq!(grid.transmittance(far, sun, MAX_STEPS), 1.0);
        // In front of the wall (towards the sun) it is lit.
        let front = Vec3::new(40.0, 10.0, 0.5);
        assert_eq!(grid.transmittance(front, sun, MAX_STEPS), 1.0);
    }

    #[test]
    fn the_edge_of_a_shadow_is_where_the_wall_top_is_not_at_a_texel() {
        // The wall is 4 high and the sun is 45 degrees: the shadow reaches 4 cells behind the wall's far side.
        let mut grid = VoxelGrid::empty((0, 0));
        for y in 0..SIZE_XY {
            for z in 0..4 {
                grid.data[index(50, y, z)] = 255;
            }
        }
        let sun = Vec3::new(-1.0, 0.0, 1.0).normalize();
        // The wall fills x from 49.5 to 50.5 (cell 50 spans 49.5..50.5 in world x because centres are on integers).
        // At z = 0.1 the ray rises as it goes back: it clears the top (z = 4) after 3.9 along x.
        assert_eq!(grid.transmittance(Vec3::new(50.5 + 3.8, 10.0, 0.1), sun, MAX_STEPS), 0.0);
        assert_eq!(grid.transmittance(Vec3::new(50.5 + 4.1, 10.0, 0.1), sun, MAX_STEPS), 1.0);
    }

    #[test]
    fn water_dims_without_blocking_and_stacks() {
        let mut grid = VoxelGrid::empty((0, 0));
        for z in 1..3 {
            grid.data[index(30, 30, z)] = WATER;
        }
        let one = grid.transmittance(Vec3::new(29.5 + 0.5, 29.5 + 0.5, 0.5), Vec3::Z, MAX_STEPS);
        let expected = (1.0 - WATER as f32 / 255.0).powi(2);
        assert!((one - expected).abs() < 1e-5, "{one} vs {expected}");
        assert!(one > 0.0 && one < 1.0);
    }

    #[test]
    fn a_ray_that_leaves_the_grid_is_lit_and_a_flat_ray_does_not_hang() {
        let grid = VoxelGrid::empty((0, 0));
        assert_eq!(grid.transmittance(Vec3::new(1.0, 1.0, 1.0), Vec3::new(-1.0, 0.0, 0.0), MAX_STEPS), 1.0);
        assert_eq!(grid.transmittance(Vec3::new(80.0, 80.0, 1.0), Vec3::Z, MAX_STEPS), 1.0);
        assert_eq!(grid.transmittance(Vec3::new(80.0, 80.0, 1.0), Vec3::new(0.7, 0.7, 0.0).normalize(), 5), 1.0);
    }

    fn wall(height: usize) -> VoxelGrid {
        let mut grid = VoxelGrid::empty((0, 0));
        for y in 0..SIZE_XY {
            for z in 0..height {
                grid.data[index(50, y, z)] = 255;
            }
        }
        grid
    }

    /// How far along x (in cells) the light goes from at most 10 % to at least 90 % behind a wall of this
    /// height, receivers on the ground at z = 0.1, the sun 45 degrees high towards -x.
    fn penumbra_width(height: usize, soft: f32) -> f32 {
        let grid = wall(height);
        let mips = grid.mips();
        let sun = Vec3::new(-1.0, 0.0, 1.0).normalize();
        let (mut dark, mut light) = (None, None);
        let mut x = 50.6;
        while x < 80.0 {
            let v = grid.soft_transmittance(&mips, Vec3::new(x, 10.0, 0.1), sun, Vec3::Z, soft);
            if v <= 0.1 {
                dark = Some(x);
            }
            if v >= 0.9 && light.is_none() && dark.is_some() {
                light = Some(x);
            }
            x += 0.02;
        }
        light.expect("the light comes back") - dark.expect("there is shadow")
    }

    #[test]
    fn the_mip_chain_averages_two_cubes_and_keeps_a_filled_grid_filled() {
        let mut grid = VoxelGrid::empty((0, 0));
        grid.data[index(0, 0, 0)] = 255;
        let mips = grid.mips();
        assert_eq!(mips.len(), MIP_LEVELS);
        assert_eq!(mips[1][0], 32, "one solid cell in eight");
        assert_eq!(mips[5].len(), 5 * 5);
        assert_eq!(VoxelGrid::mip_size(5), (5, 5, 1));
        let full = VoxelGrid { origin: (0, 0), data: vec![255; SIZE_XY * SIZE_XY * SIZE_Z] };
        assert!(full.mips().iter().all(|level| level.iter().all(|&v| v == 255)));
    }

    #[test]
    fn sampling_a_level_reads_the_cell_at_its_centre_and_blends_between_levels() {
        let mut grid = VoxelGrid::empty((0, 0));
        grid.data[index(10, 10, 5)] = 255;
        let mips = grid.mips();
        assert!((VoxelGrid::sample(&mips, Vec3::new(10.5, 10.5, 5.5), 0.0) - 1.0).abs() < 1e-5);
        assert!(VoxelGrid::sample(&mips, Vec3::new(12.5, 10.5, 5.5), 0.0) < 1e-5);
        let blend = VoxelGrid::sample(&mips, Vec3::new(10.5, 10.5, 5.5), 0.5);
        assert!(blend > VoxelGrid::sample(&mips, Vec3::new(10.5, 10.5, 5.5), 1.0) && blend < 1.0);
    }

    #[test]
    fn without_a_disc_the_soft_walk_is_the_exact_one() {
        let grid = wall(4);
        let mips = grid.mips();
        let sun = Vec3::new(-1.0, 0.0, 1.0).normalize();
        for x in [51.5, 54.3, 54.6, 60.0] {
            let at = Vec3::new(x, 10.0, 0.1);
            assert_eq!(grid.soft_transmittance(&mips, at, sun, Vec3::Z, 0.0), grid.transmittance(at, sun, MAX_STEPS));
        }
    }

    #[test]
    fn a_shadow_edge_is_blurrier_the_further_it_is_from_what_casts_it() {
        // The shadow tip of a tall wall comes from an edge further up and away than that of a low wall.
        let (low, tall) = (penumbra_width(2, 0.1), penumbra_width(8, 0.1));
        assert!(tall > 1.5 * low, "tall {tall} vs low {low}");
        // A wider sun blurs more, a point sun not at all (the edge is one step of the scan).
        assert!(penumbra_width(8, 0.2) > penumbra_width(8, 0.05));
        assert!(penumbra_width(8, 0.0) < 0.1, "{}", penumbra_width(8, 0.0));
    }

    #[test]
    fn even_the_short_shadow_of_a_low_wall_has_a_visible_edge_at_the_default_softness() {
        // The default of the menu: half of the softest (a quarter of a block of blur per block of distance).
        let soft = 0.5 * crate::sunshadow::MAX_SOFT;
        let (hard, soft_width) = (penumbra_width(2, 0.0), penumbra_width(2, soft));
        assert!(hard < 0.1, "{hard}");
        assert!(soft_width > 0.4, "a 2 block wall's shadow edge is only {soft_width} cells wide");
        assert!(soft_width < 3.0, "but not a smear: {soft_width}");
    }

    #[test]
    fn open_ground_is_not_shaded_by_its_own_blur_at_any_sun_height() {
        let mut grid = VoxelGrid::empty((0, 0));
        for z in 0..3 {
            for y in 0..SIZE_XY {
                for x in 0..SIZE_XY {
                    grid.data[index(x, y, z)] = 255;
                }
            }
        }
        let mips = grid.mips();
        for height in [0.12f32, 0.2, 0.35, 0.6, 0.9] {
            let sun = Vec3::new(-(1.0 - height * height).sqrt(), 0.0, height);
            for soft in [0.05, 0.15, crate::sunshadow::MAX_SOFT] {
                for x in [40.0, 80.3, 101.7] {
                    let v = grid.soft_transmittance(&mips, Vec3::new(x, 60.0, 3.0), sun, Vec3::Z, soft);
                    assert!(v > 0.97, "ground is {v} lit with the sun {height} high and softness {soft}");
                }
            }
        }
    }

    #[test]
    fn a_wall_does_not_shade_itself_on_its_sunny_side() {
        // The wall faces +x here; the sun is on that side.
        let grid = wall(8);
        let mips = grid.mips();
        let sun = Vec3::new(0.7, 0.0, 0.7).normalize();
        for z in [0.5, 3.5, 7.2] {
            let v = grid.soft_transmittance(&mips, Vec3::new(50.5, 10.0, z), sun, Vec3::X, 0.15);
            assert!(v > 0.95, "wall face at z {z}: {v}");
        }
    }

    #[test]
    fn the_blur_leaves_the_middle_of_a_shadow_dark_and_the_open_light() {
        let grid = wall(8);
        let mips = grid.mips();
        let sun = Vec3::new(-1.0, 0.0, 1.0).normalize();
        assert!(grid.soft_transmittance(&mips, Vec3::new(51.5, 10.0, 0.5), sun, Vec3::Z, 0.1) < 0.05, "right behind the wall");
        assert!(grid.soft_transmittance(&mips, Vec3::new(75.0, 10.0, 0.5), sun, Vec3::Z, 0.1) > 0.95, "far from it");
        assert!(grid.soft_transmittance(&mips, Vec3::new(30.0, 10.0, 0.5), sun, Vec3::Z, 0.1) > 0.99, "on the sun's side");
    }

    #[test]
    fn the_shader_walk_has_the_same_shape_as_the_reference() {
        let source = include_str!("shader.wgsl");
        for needle in ["fn voxel_visibility(", "textureLoad(voxels", "textureSampleLevel(voxels", "0.02", "* (1.0 - ", "const SOFT_STEPS = 48;", "const CONE_START = 0.25;", "const SELF_CLEARANCE = 0.3;", "const NOISE_FLOOR = 0.04;"] {
            assert!(source.contains(needle), "{needle} missing in shader.wgsl");
        }
        assert_eq!(MAX_STEPS, 128, "the uniform carries the same step budget as the reference");
        assert_eq!((SOFT_STEPS, CONE_START, SELF_CLEARANCE), (48, 0.25, 0.3));
    }
}
