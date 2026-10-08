//! Turns the cells of a [`RenderChunk`] into coloured triangles. Pure CPU code with no graphics
//! dependency.
//!
//! Every block is a unit cube in isometric ground coordinates (see `wurfel_sim::grid`). Only the
//! three faces that face the camera are emitted, and only those that the render storage has not
//! clipped because a neighbouring block covers them.
//!
//! # Lighting
//!
//! A vertex carries the block's flat colour (`color`) and what the light engine
//! (`wurfel_sim::light`) needs to light it: which face it is on, its ambient occlusion and the
//! light baked in from static point lights. The colour itself is computed by the vertex shader
//! (`shader.wgsl`) every frame, so the time of day and moving lights cost nothing at meshing time,
//! and meshing only has to run when blocks change.

use bytemuck::{Pod, Zeroable};
use glam::Vec3;
use wurfel_sim::block::id;
use wurfel_sim::grid::{from_iso, to_iso};
use wurfel_sim::light::{bake_point_lights_with, face_vertex_ao_with, Face, PointLight};
use wurfel_sim::{Block, CHUNK_SIZE_X, CHUNK_SIZE_Y, CHUNK_SIZE_Z};

use crate::render_storage::{RenderChunk, CLIP_LEFT, CLIP_RIGHT, CLIP_TOP};
use crate::sprites::{self, BlockLook, Sprites};

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub struct Vertex {
    /// Isometric ground x, ground y and height, in block units.
    pub position: [f32; 3],
    /// The block's flat colour (before any lighting).
    pub color: [f32; 3],
    /// `[face, occlusion]`: the face id (`FACE_*`) and the ambient occlusion from 0 (open) to 1.
    pub shade: [f32; 2],
    /// Light from static point lights, baked in while meshing, colour included.
    pub point: [f32; 3],
    /// Texture coordinates in the sprite atlas (0..1 across a page).
    pub uv: [f32; 2],
    /// Atlas page (the layer of the texture array) the sprite is on, or [`NO_SPRITE`] for a
    /// vertex that is shown in its flat `color`. With a sprite, `color` is a tint that multiplies it.
    pub layer: f32,
    /// The ambient occlusion of all four corners of the face, so the shader can interpolate it
    /// bilinearly per pixel: 0 for none (then `shade[1]` is used per vertex), else
    /// [`OCCLUSION_FLAG`] + 256 * this vertex's corner (0..=3, in the corner order of the face) + the
    /// four counts (0..=3) as 2 bits each, corner 0 in the lowest bits.
    pub occlusion: f32,
}

impl Vertex {
    /// A vertex without a sprite.
    pub const fn flat(position: [f32; 3], color: [f32; 3], shade: [f32; 2], point: [f32; 3]) -> Self {
        Vertex { position, color, shade, point, uv: [0.0; 2], layer: NO_SPRITE, occlusion: 0.0 }
    }
}

/// Added to `Vertex::occlusion` of a face that has its corners' occlusion in it (a plain 0 means none).
pub const OCCLUSION_FLAG: f32 = 1024.0;

/// `Vertex::occlusion` of the vertex at `corner` (0..=3) of a face with these corner counts (0..=3 each).
pub fn pack_occlusion(counts: [u8; 4], corner: usize) -> f32 {
    let bits = counts.iter().enumerate().fold(0u32, |bits, (i, &c)| bits | ((c.min(3) as u32) << (2 * i)));
    OCCLUSION_FLAG + 256.0 * corner as f32 + bits as f32
}

/// `Vertex::layer` of a vertex that has no sprite.
pub const NO_SPRITE: f32 = -1.0;

/// Values of `Vertex::shade[0]`, the same numbering as `wurfel_sim::light::Face::index`.
pub const FACE_LEFT: f32 = 0.0;
pub const FACE_TOP: f32 = 1.0;
pub const FACE_RIGHT: f32 = 2.0;
/// Not lit by the light engine: shown in its own colour (the hover marker).
pub const FACE_UNLIT: f32 = 3.0;
/// A sprite standing in the world (entities, trees...): lit by the average of the three faces.
/// Its vertices are all the anchor point (`position`) with the screen offset in `point`
/// (`[dx, dy, bias]`, see `sprites::billboard_point`): the shader builds the picture's rectangle
/// facing the camera, however the free camera is turned.
pub const FACE_SPRITE: f32 = 4.0;
/// The side of a block that looks towards `-y` (opposite of the left face). Only meshed for the free camera.
pub const FACE_BACK_Y: f32 = 5.0;
/// The side of a block that looks towards `-x` (opposite of the right face). Only meshed for the free camera.
pub const FACE_BACK_X: f32 = 6.0;
/// A flat coloured square facing the camera, not lit (particles): positioned like [`FACE_SPRITE`].
pub const FACE_BILLBOARD: f32 = 7.0;
/// Added to the face id of the top of a water block: the shader rounds the id to the nearest whole
/// number, so this fraction only marks the surface for the reflection (`shader.wgsl`, `water_flag`).
pub const WATER_SURFACE: f32 = 0.25;

/// Brightness per face of the old flat look, used when lighting is switched off (left, top, right).
pub const FLAT_SHADES: [f32; 3] = [0.78, 1.0, 0.58];

#[cfg(target_arch = "wasm32")]
impl Vertex {
    /// The vertex buffer layout matching `shader.wgsl` (`@location(0..=6)`).
    pub fn layout() -> wgpu::VertexBufferLayout<'static> {
        const ATTRIBUTES: [wgpu::VertexAttribute; 7] = wgpu::vertex_attr_array![
            0 => Float32x3, 1 => Float32x3, 2 => Float32x2, 3 => Float32x3, 4 => Float32x2, 5 => Float32, 6 => Float32
        ];
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Vertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &ATTRIBUTES,
        }
    }
}

/// What meshing needs to know beyond the chunk itself.
pub struct MeshContext<'a> {
    /// Is the block at these block coordinates opaque? Used for ambient occlusion and for the
    /// shadows of baked point lights. Blocks outside the render window count as open.
    pub opaque: &'a dyn Fn(i32, i32, i32) -> bool,
    /// Lights whose contribution is baked into the vertices.
    pub lights: &'a [PointLight],
    /// The sprite atlas, once loaded. Blocks without a sprite, and everything while this is `None`
    /// (the flat look, `?flat=1`), show their flat colour.
    pub sprites: Option<&'a Sprites>,
    /// The highest layer that is meshed (the editor's layer limit); `CHUNK_SIZE_Z - 1` for all.
    pub top: i32,
    /// Also mesh the sides that look away from the fixed camera (`-x`, `-y`), so the free camera
    /// can look at the world from behind. Costs about a third more triangles.
    pub all_faces: bool,
}

/// Colour of a block, for things that break off it.
#[cfg(target_arch = "wasm32")]
pub fn block_color(block: Block) -> [f32; 3] {
    base_color(block)
}

fn base_color(block: Block) -> [f32; 3] {
    use caveland_sim::blocks::ids as cl;
    match block.id() {
        id::GRASS => [0.36, 0.64, 0.25],
        id::DIRT => [0.53, 0.37, 0.24],
        id::STONE => [0.52, 0.52, 0.55],
        id::SAND => [0.88, 0.80, 0.53],
        id::WATER => [0.20, 0.45, 0.80],
        // Caveland
        cl::INVISIBLE_OBSTACLE => [0.28, 0.30, 0.38],
        cl::CONSTRUCTION_SITE => [0.80, 0.55, 0.20],
        cl::OVEN => [0.65, 0.30, 0.22],
        cl::TORCH => [1.00, 0.78, 0.25],
        cl::POWER_STATION => [0.35, 0.50, 0.70],
        cl::LIFT => [0.50, 0.62, 0.62],
        cl::ENTRY => [0.55, 0.35, 0.70],
        cl::INDESTRUCTIBLE_OBSTACLE => [0.22, 0.22, 0.26],
        cl::LIFT_GROUND => [0.62, 0.62, 0.55],
        cl::CRYSTAL => [0.45, 0.85, 0.95],
        cl::SULFUR => [0.92, 0.85, 0.20],
        cl::IRON_ORE => [0.62, 0.48, 0.42],
        cl::COAL => [0.14, 0.14, 0.16],
        cl::TURRET => [0.60, 0.25, 0.25],
        cl::ROBOT_FACTORY => [0.45, 0.45, 0.55],
        cl::POWER_CABLE => [0.30, 0.26, 0.20],
        cl::RAILS => [0.55, 0.50, 0.45],
        cl::BOOSTER_RAILS => [0.85, 0.60, 0.25],
        // No art yet for the two launchers: plain colours (wood, and dark iron).
        cl::CATAPULT => [0.62, 0.45, 0.28],
        cl::CANNON => [0.25, 0.27, 0.32],
        cl::FLAG_POLE => [0.75, 0.75, 0.78],
        cl::TREE => [0.20, 0.45, 0.20],
        // Every other id the engine knows (the "block test" and "solid block" maps show them all)
        // gets a stable colour of its own, so different blocks can be told apart.
        other if (other as i32) < wurfel_sim::generator::OBJECT_TYPES_NUM => id_color(other),
        _ => [1.0, 0.0, 1.0], // not a block id at all: obvious magenta
    }
}

/// A distinct, stable colour for an id: the hue steps by the golden ratio so that neighbouring ids
/// are far apart.
fn id_color(block_id: u8) -> [f32; 3] {
    let hue = (block_id as f32 * 0.618_034).fract() * 6.0;
    let (saturation, value) = (0.55, 0.85);
    let sector = hue.floor() as i32;
    let f = hue - sector as f32;
    let (p, q, t) = (value * (1.0 - saturation), value * (1.0 - saturation * f), value * (1.0 - saturation * (1.0 - f)));
    match sector % 6 {
        0 => [value, t, p],
        1 => [q, value, p],
        2 => [p, value, t],
        3 => [p, q, value],
        4 => [t, p, value],
        _ => [value, p, q],
    }
}

/// Mesh one chunk from its render cells.
pub fn build_chunk(chunk: &RenderChunk, ctx: &MeshContext) -> Vec<Vertex> {
    let (left, top) = chunk.top_left();
    let mut vertices = Vec::new();
    for lx in 0..CHUNK_SIZE_X {
        for ly in 0..CHUNK_SIZE_Y {
            let (x, y) = (left + lx, top + ly);
            let (gx, gy) = to_iso(x, y);
            let (x0, x1, y0, y1) = (gx - 0.5, gx + 0.5, gy - 0.5, gy + 0.5);
            for z in 0..=ctx.top.min(CHUNK_SIZE_Z - 1) {
                let cell = chunk.cell(lx, ly, z).expect("index is inside the chunk");
                // A cell whose three front sides are covered can still show its back sides to the
                // free camera (the far slope of a hill), so it is only skipped for the fixed one.
                // The upper half of a tree is an invisible obstacle (`CustomTree.TREETOPVALUE`).
                let tree_top = cell.block.id() == wurfel_sim::block::id::TREE && cell.block.value() == 8;
                if cell.block.is_air() || tree_top || (cell.is_fully_clipped() && !ctx.all_faces) {
                    continue;
                }
                let look = ctx.sprites.and_then(|s| s.block(cell.block.id(), cell.block.value()).map(|look| (s, look)));
                let (z0, z1) = (z as f32, z as f32 + 1.0);
                let at = (x, y, z);
                if let Some((sprites, BlockLook::Single(index))) = look {
                    // A single picture (a tree, a torch...) standing on the cell.
                    if cell.is_fully_clipped() {
                        continue;
                    }
                    let region = sprites.region(index);
                    let anchor = Vec3::new(gx, gy, z0);
                    sprites::billboard(&mut vertices, &sprites.atlas, region, anchor, sprites::FOOTPRINT_TIP, false, [cell.top_light; 3]);
                    continue;
                }
                // With sprites the colour is a tint over the picture: white, so only the light shows.
                let textured = matches!(look, Some((_, BlockLook::Sided { .. })));
                let color = if textured { [1.0; 3] } else { base_color(cell.block) };
                let sprite_of = |face: Face| -> Option<Sprite<'_>> {
                    let (sprites, BlockLook::Sided { left, top, right }) = look? else { return None };
                    let index = match face {
                        Face::Left => left,
                        Face::Top => top,
                        Face::Right => right,
                    };
                    Some(Sprite { atlas: &sprites.atlas, region: sprites.region(index) })
                };
                if cell.clipping & CLIP_TOP == 0 {
                    // The Java cheap shadow under overhangs (`top_light`) stays part of the colour.
                    let shaded = color.map(|c| c * cell.top_light);
                    let corners = [[x0, y0, z1], [x1, y0, z1], [x1, y1, z1], [x0, y1, z1]];
                    let first = vertices.len();
                    lit_quad(&mut vertices, ctx, at, Face::Top, shaded, corners, sprite_of(Face::Top));
                    if cell.block.id() == id::WATER {
                        for vertex in &mut vertices[first..] {
                            vertex.shade[0] += WATER_SURFACE;
                        }
                    }
                }
                if cell.clipping & CLIP_LEFT == 0 {
                    let corners = [[x0, y1, z0], [x1, y1, z0], [x1, y1, z1], [x0, y1, z1]];
                    lit_quad(&mut vertices, ctx, at, Face::Left, color, corners, sprite_of(Face::Left));
                }
                if cell.clipping & CLIP_RIGHT == 0 {
                    let corners = [[x1, y0, z0], [x1, y1, z0], [x1, y1, z1], [x1, y0, z1]];
                    lit_quad(&mut vertices, ctx, at, Face::Right, color, corners, sprite_of(Face::Right));
                }
                if ctx.all_faces {
                    // The hidden sides: not covered by the next block towards -y or -x. They wear the
                    // pictures of their opposite sides (a face's picture only depends on its shape).
                    let covered = |dx: f32, dy: f32| {
                        let (nx, ny) = from_iso(gx + dx, gy + dy);
                        (ctx.opaque)(nx, ny, z)
                    };
                    if !covered(0.0, -1.0) {
                        let corners = [[x0, y0, z0], [x1, y0, z0], [x1, y0, z1], [x0, y0, z1]];
                        back_quad(&mut vertices, FACE_BACK_Y, color, corners, sprite_of(Face::Left));
                    }
                    if !covered(-1.0, 0.0) {
                        let corners = [[x0, y0, z0], [x0, y1, z0], [x0, y1, z1], [x0, y0, z1]];
                        back_quad(&mut vertices, FACE_BACK_X, color, corners, sprite_of(Face::Right));
                    }
                }
            }
        }
    }
    vertices
}

/// The corner order of every face is bottom/left, bottom/right, top/right, top/left in the face's
/// own axes; this is the sign of the offset from the face centre along them, the `du`, `dv` of
/// `face_vertex_ao`.
const CORNER_SIGNS: [(i32, i32); 4] = [(-1, -1), (1, -1), (1, 1), (-1, 1)];

/// The sprite that textures a face.
#[derive(Clone, Copy)]
struct Sprite<'a> {
    atlas: &'a crate::atlas::Atlas,
    region: &'a crate::atlas::Region,
}

/// A face of the block at `cell` with ambient occlusion and baked lights per corner.
fn lit_quad(
    out: &mut Vec<Vertex>,
    ctx: &MeshContext,
    cell: (i32, i32, i32),
    face: Face,
    color: [f32; 3],
    corners: [[f32; 3]; 4],
    sprite: Option<Sprite>,
) {
    let mut ao = [0.0f32; 4];
    let mut counts = [0u8; 4];
    let mut point = [[0.0f32; 3]; 4];
    for (i, &(du, dv)) in CORNER_SIGNS.iter().enumerate() {
        counts[i] = face_vertex_ao_with(ctx.opaque, cell, face, du, dv);
        ao[i] = counts[i] as f32 / 3.0;
        if !ctx.lights.is_empty() {
            point[i] = bake_point_lights_with(ctx.opaque, ctx.lights, Vec3::from(corners[i]), face).to_array();
        }
    }
    let first = out.len();
    quad_occluded(out, face_id(face), color, corners, ao, point, Some(counts));
    if let Some(Sprite { atlas, region }) = sprite {
        // `quad` chose the diagonal; its vertices are copies of the four corners, so find each
        // one's atlas coordinates by position.
        let uvs = sprites::face_uvs(atlas, region, corners);
        for vertex in &mut out[first..] {
            let corner = corners.iter().position(|c| *c == vertex.position).expect("a vertex is one of the corners");
            vertex.uv = uvs[corner];
            vertex.layer = region.page as f32;
        }
    }
}

/// A side that looks away from the fixed camera: no occlusion and no baked lights.
fn back_quad(out: &mut Vec<Vertex>, face: f32, color: [f32; 3], corners: [[f32; 3]; 4], sprite: Option<Sprite>) {
    let first = out.len();
    quad(out, face, color, corners, [0.0; 4], [[0.0; 3]; 4]);
    if let Some(Sprite { atlas, region }) = sprite {
        let uvs = sprites::face_uvs(atlas, region, corners);
        for vertex in &mut out[first..] {
            let corner = corners.iter().position(|c| *c == vertex.position).expect("a vertex is one of the corners");
            vertex.uv = uvs[corner];
            vertex.layer = region.page as f32;
        }
    }
}

fn face_id(face: Face) -> f32 {
    match face {
        Face::Left => FACE_LEFT,
        Face::Top => FACE_TOP,
        Face::Right => FACE_RIGHT,
    }
}

/// Face pointing up. `[x0, x1, y0, y1]` are isometric ground bounds.
pub fn top_face(out: &mut Vec<Vertex>, color: [f32; 3], [x0, x1, y0, y1]: [f32; 4], z: f32) {
    quad(out, FACE_TOP, color, [[x0, y0, z], [x1, y0, z], [x1, y1, z], [x0, y1, z]], [0.0; 4], [[0.0; 3]; 4]);
}

/// Like [`top_face`] but not touched by the light engine: it always shows `color` (for markers).
pub fn top_face_unlit(out: &mut Vec<Vertex>, color: [f32; 3], [x0, x1, y0, y1]: [f32; 4], z: f32) {
    quad(out, FACE_UNLIT, color, [[x0, y0, z], [x1, y0, z], [x1, y1, z], [x0, y1, z]], [0.0; 4], [[0.0; 3]; 4]);
}

/// A picture laid over a block face (the cracks of a damaged block): `corners` as for the block's own
/// face, unlit, `alpha` see-through.
pub fn overlay_face(out: &mut Vec<Vertex>, color: [f32; 3], corners: [[f32; 3]; 4], atlas: &crate::atlas::Atlas, region: &crate::atlas::Region, alpha: f32) {
    let first = out.len();
    quad(out, FACE_UNLIT, color, corners, [0.0; 4], [[0.0; 3]; 4]);
    let uvs = sprites::face_uvs(atlas, region, corners);
    for vertex in &mut out[first..] {
        let corner = corners.iter().position(|c| *c == vertex.position).expect("a vertex is one of the corners");
        vertex.uv = uvs[corner];
        vertex.layer = region.page as f32;
    }
    set_alpha(&mut out[first..], alpha);
}

/// Make the unlit faces ([`FACE_UNLIT`], [`FACE_BILLBOARD`]) among `vertices` see-through: `alpha` 1 is
/// opaque, 0 invisible. (Their occlusion slot carries it, see `vs_main`.)
pub fn set_alpha(vertices: &mut [Vertex], alpha: f32) {
    for v in vertices {
        v.shade[1] = 1.0 - alpha.clamp(0.0, 1.0);
    }
}

/// [`top_face_unlit`] that blends with what is behind it (a shadow).
pub fn top_face_unlit_alpha(out: &mut Vec<Vertex>, color: [f32; 3], bounds: [f32; 4], z: f32, alpha: f32) {
    let first = out.len();
    top_face_unlit(out, color, bounds, z);
    set_alpha(&mut out[first..], alpha);
}

/// Face towards the lower left of the screen (the +y side).
pub fn left_face(out: &mut Vec<Vertex>, color: [f32; 3], [x0, x1]: [f32; 2], y: f32, [z0, z1]: [f32; 2]) {
    quad(out, FACE_LEFT, color, [[x0, y, z0], [x1, y, z0], [x1, y, z1], [x0, y, z1]], [0.0; 4], [[0.0; 3]; 4]);
}

/// Face towards the lower right of the screen (the +x side).
pub fn right_face(out: &mut Vec<Vertex>, color: [f32; 3], x: f32, [y0, y1]: [f32; 2], [z0, z1]: [f32; 2]) {
    quad(out, FACE_RIGHT, color, [[x, y0, z0], [x, y1, z0], [x, y1, z1], [x, y0, z1]], [0.0; 4], [[0.0; 3]; 4]);
}

/// A free-standing box, e.g. a player or a thing without a picture: the top and the four sides.
/// The two sides that look away from the fixed camera are hidden by the shader until the free
/// camera turns far enough to see them.
pub fn cuboid(out: &mut Vec<Vertex>, color: [f32; 3], [x0, x1, y0, y1]: [f32; 4], [z0, z1]: [f32; 2]) {
    top_face(out, color, [x0, x1, y0, y1], z1);
    left_face(out, color, [x0, x1], y1, [z0, z1]);
    right_face(out, color, x1, [y0, y1], [z0, z1]);
    quad(out, FACE_BACK_Y, color, [[x0, y0, z0], [x1, y0, z0], [x1, y0, z1], [x0, y0, z1]], [0.0; 4], [[0.0; 3]; 4]);
    quad(out, FACE_BACK_X, color, [[x0, y0, z0], [x0, y1, z0], [x0, y1, z1], [x0, y0, z1]], [0.0; 4], [[0.0; 3]; 4]);
}

/// Two triangles. The diagonal is chosen so that a single dark corner does not smear across the
/// whole face: it goes between the corners 1 and 3 when 0 and 2 are the darker pair.
fn quad(out: &mut Vec<Vertex>, face: f32, color: [f32; 3], corners: [[f32; 3]; 4], ao: [f32; 4], point: [[f32; 3]; 4]) {
    quad_occluded(out, face, color, corners, ao, point, None);
}

/// [`quad`] for a face whose corners' occlusion counts (0..=3) are given: the vertices carry all four
/// (`Vertex::occlusion`), so the shader interpolates the occlusion bilinearly over the face instead of
/// along the two triangles, which shows as triangular blotches.
fn quad_occluded(out: &mut Vec<Vertex>, face: f32, color: [f32; 3], corners: [[f32; 3]; 4], ao: [f32; 4], point: [[f32; 3]; 4], counts: Option<[u8; 4]>) {
    let vertex = |i: usize| {
        let mut v = Vertex::flat(corners[i], color, [face, ao[i]], point[i]);
        if let Some(counts) = counts {
            v.occlusion = pack_occlusion(counts, i);
        }
        v
    };
    let order = if ao[0] + ao[2] > ao[1] + ao[3] { [1, 2, 3, 1, 3, 0] } else { [0, 1, 2, 0, 2, 3] };
    for i in order {
        out.push(vertex(i));
    }
}

/// Mesh every block of the chunks `min..=max` of a world, without keeping any cache.
#[cfg(test)]
pub fn build(world: &mut wurfel_sim::World, min: (i32, i32), max: (i32, i32)) -> Vec<Vertex> {
    let mut storage = crate::render_storage::RenderStorage::new();
    storage.update_area(world, min, max);
    storage.vertices()
}

#[cfg(test)]
mod tests {
    use super::*;
    use wurfel_sim::{AirGenerator, Generator, IslandGenerator, World};

    struct Single;
    impl Generator for Single {
        fn generate(&self, x: i32, y: i32, z: i32) -> Block {
            if (x, y, z) == (5, 5, 0) { Block::new(id::STONE, 0) } else { Block::AIR }
        }
    }

    #[test]
    fn the_occlusion_of_the_corners_is_packed_two_bits_each_with_the_vertexs_own_corner() {
        let packed = pack_occlusion([1, 2, 3, 0], 2);
        assert!(packed >= OCCLUSION_FLAG);
        let bits = (packed - OCCLUSION_FLAG) as u32;
        assert_eq!(bits & 255, 1 | (2 << 2) | (3 << 4));
        assert_eq!(bits >> 8, 2);
        // The largest value is exactly representable (a float holds 24 bits).
        assert_eq!(pack_occlusion([3; 4], 3) as u32, 1024 + 3 * 256 + 255);
        // A face without any is 0, and a back quad has none.
        let mut out = Vec::new();
        back_quad(&mut out, FACE_BACK_Y, [1.0; 3], [[0.0; 3], [1.0, 0.0, 0.0], [1.0, 0.0, 1.0], [0.0, 0.0, 1.0]], None);
        assert!(out.iter().all(|v| v.occlusion == 0.0));
    }

    #[test]
    fn every_known_block_has_its_own_colour_and_only_non_blocks_are_magenta() {
        use std::collections::HashSet;
        const MAGENTA: [f32; 3] = [1.0, 0.0, 1.0];
        // The ids the generators and Caveland use explicitly are all different from each other.
        let named = [
            id::GRASS, id::DIRT, id::STONE, id::SAND, id::WATER, 4, 11, 12, 13, 14, 15, 16, 17, 18, 41, 42, 43, 44,
            52, 53, 54, 55, 56, 57, 58, 60, 72,
        ];
        let colours: HashSet<[u32; 3]> = named.iter().map(|&i| base_color(Block::new(i, 0)).map(f32::to_bits)).collect();
        assert_eq!(colours.len(), named.len(), "two named blocks share a colour");
        // The "block test" map shows ids up to 123: none of them may look like the error colour.
        for block_id in 1..wurfel_sim::generator::OBJECT_TYPES_NUM as u8 {
            assert_ne!(base_color(Block::new(block_id, 0)), MAGENTA, "id {block_id}");
        }
        assert_eq!(base_color(Block::new(200, 0)), MAGENTA);
        assert_eq!(base_color(Block::new(255, 0)), MAGENTA);
    }

    #[test]
    fn coal_is_dark_and_sulfur_is_yellow() {
        let coal = base_color(Block::new(caveland_sim::blocks::ids::COAL, 0));
        let sulfur = base_color(Block::new(caveland_sim::blocks::ids::SULFUR, 0));
        assert!(coal.iter().all(|&c| c < 0.2));
        assert!(sulfur[0] > 0.8 && sulfur[1] > 0.8 && sulfur[2] < 0.4);
    }

    #[test]
    fn lone_block_shows_three_faces() {
        let mut world = World::new(Single);
        let vertices = build(&mut world, (0, 0), (0, 0));
        assert_eq!(vertices.len(), 3 * 6);
    }

    #[test]
    fn an_overhang_darkens_the_top_face_below_it() {
        struct Overhang;
        impl Generator for Overhang {
            fn generate(&self, x: i32, y: i32, z: i32) -> Block {
                match (x, y, z) {
                    (5, 5, 0) | (5, 5, 2) => Block::new(id::STONE, 0),
                    _ => Block::AIR,
                }
            }
        }
        let mut world = World::new(Overhang);
        let vertices = build(&mut world, (0, 0), (0, 0));
        let stone = base_color(Block::new(id::STONE, 0));
        let top_at = |z: f32| vertices.iter().find(|v| v.position[2] == z && v.color[0] > stone[0] * 0.7).map(|v| v.color);
        // z=1 is the top of the lower block (shaded by the block at z=2), z=3 the top of the upper one
        let shaded = top_at(1.0).expect("lower top face");
        let open = top_at(3.0).expect("upper top face");
        for c in 0..3 {
            assert!((shaded[c] - stone[c] * 0.8).abs() < 1e-6, "channel {c}: {shaded:?}");
            assert!((open[c] - stone[c]).abs() < 1e-6);
        }
    }

    #[test]
    fn cuboid_is_five_quads_so_it_is_closed_from_every_side() {
        let mut out = Vec::new();
        cuboid(&mut out, [1.0; 3], [0.0, 1.0, 0.0, 1.0], [0.0, 1.0]);
        assert_eq!(out.len(), 30);
    }

    #[test]
    fn empty_world_has_no_geometry() {
        let mut world = World::new(AirGenerator);
        assert!(build(&mut world, (-1, -1), (1, 1)).is_empty());
    }

    #[test]
    fn buried_faces_are_culled() {
        // A whole island is far fewer faces than 3 per solid block.
        let mut world = World::new(IslandGenerator::new(1));
        let vertices = build(&mut world, (0, 0), (0, 0));
        let solid = (0..CHUNK_SIZE_X * CHUNK_SIZE_Y * CHUNK_SIZE_Z)
            .filter(|i| {
                let (x, rest) = (i / (CHUNK_SIZE_Y * CHUNK_SIZE_Z), i % (CHUNK_SIZE_Y * CHUNK_SIZE_Z));
                !world.get(x, rest / CHUNK_SIZE_Z, rest % CHUNK_SIZE_Z).is_air()
            })
            .count();
        assert!(vertices.len() / 6 < solid, "{} quads for {} solid blocks", vertices.len() / 6, solid);
    }

    #[test]
    fn a_vertex_is_56_bytes_with_the_fields_the_shader_reads_in_order() {
        assert_eq!(std::mem::size_of::<Vertex>(), 60);
        assert_eq!(std::mem::offset_of!(Vertex, uv), 44);
        assert_eq!(std::mem::offset_of!(Vertex, layer), 52);
        assert_eq!(std::mem::offset_of!(Vertex, position), 0);
        assert_eq!(std::mem::offset_of!(Vertex, color), 12);
        assert_eq!(std::mem::offset_of!(Vertex, shade), 24);
        assert_eq!(std::mem::offset_of!(Vertex, point), 32);
    }

    #[test]
    fn faces_carry_their_ids_and_the_flat_look_constants_match_the_old_shading() {
        let mut out = Vec::new();
        cuboid(&mut out, [1.0; 3], [0.0, 1.0, 0.0, 1.0], [0.0, 1.0]);
        let ids: Vec<f32> = out.chunks(6).map(|q| q[0].shade[0]).collect();
        assert_eq!(ids, vec![FACE_TOP, FACE_LEFT, FACE_RIGHT, FACE_BACK_Y, FACE_BACK_X]);
        assert!(out.iter().all(|v| v.shade[1] == 0.0 && v.point == [0.0; 3] && v.color == [1.0; 3]));

        let mut marker = Vec::new();
        top_face_unlit(&mut marker, [1.0, 0.9, 0.2], [0.0, 1.0, 0.0, 1.0], 1.0);
        assert!(marker.iter().all(|v| v.shade[0] == FACE_UNLIT));
        assert_eq!(FLAT_SHADES, [0.78, 1.0, 0.58]);
        assert_eq!(
            [FACE_LEFT as usize, FACE_TOP as usize, FACE_RIGHT as usize],
            [Face::Left.index(), Face::Top.index(), Face::Right.index()],
            "the ids are the light engine's face indices"
        );
    }

    #[test]
    fn the_diagonal_avoids_a_single_dark_corner() {
        let corners = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 1.0, 0.0], [0.0, 1.0, 0.0]];
        let mut out = Vec::new();
        quad(&mut out, FACE_TOP, [1.0; 3], corners, [0.0; 4], [[0.0; 3]; 4]);
        assert_eq!(out.iter().map(|v| v.position).collect::<Vec<_>>(), [0, 1, 2, 0, 2, 3].map(|i| corners[i]), "uniform: the default");

        let mut dark_at_0 = Vec::new();
        quad(&mut dark_at_0, FACE_TOP, [1.0; 3], corners, [1.0, 0.0, 0.0, 0.0], [[0.0; 3]; 4]);
        let triangles: Vec<_> = dark_at_0.chunks(3).collect();
        let with_dark = triangles.iter().filter(|t| t.iter().any(|v| v.shade[1] > 0.0)).count();
        assert_eq!(with_dark, 1, "the dark corner belongs to one triangle only, not both");
        assert_eq!(dark_at_0.len(), 6);

        // The other pair dark: the default diagonal 0-2 keeps it in one triangle's far corner.
        let mut dark_at_1 = Vec::new();
        quad(&mut dark_at_1, FACE_TOP, [1.0; 3], corners, [0.0, 1.0, 0.0, 0.0], [[0.0; 3]; 4]);
        let with_dark = dark_at_1.chunks(3).filter(|t| t.iter().any(|v| v.shade[1] > 0.0)).count();
        assert_eq!(with_dark, 1);
    }

    #[test]
    fn the_hidden_sides_of_a_floor_get_no_geometry_but_a_lone_block_has_open_corners() {
        let mut world = World::new(Single);
        let vertices = build(&mut world, (0, 0), (0, 0));
        assert!(vertices.iter().all(|v| v.shade[1] == 0.0), "nothing occludes a lone block");
    }
}
