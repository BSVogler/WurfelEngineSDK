//! Map debug minimap, ported from the Java `Minimap` and `MiniMapChunkDebug`.
//!
//! * [`Mode::Map`] is the Java `Minimap`: the map seen from above, one diamond per block column
//!   coloured by its topmost block and shaded by height, with chunk outlines, entities (blue
//!   outline plus a coordinate label) and the camera as three rectangles: red = the grid
//!   columns/rows that can be visible, green = the view at ground level, white = the view at the
//!   top of the world.
//! * [`Mode::ChunkGrid`] is the Java `MiniMapChunkDebug`: one 9x9 square per chunk on a 10 pixel
//!   grid, green when loaded, translucent yellow when it is part of the render window.
//!
//! Additions for streaming: a chunk can also be [`ChunkState::Requested`] (orange) or
//! [`ChunkState::Missing`] (red), so the overlay shows what the server is still sending.
//!
//! The module has two parts. The rasteriser ([`render`], [`MinimapRenderer`]) is plain Rust that
//! turns [`MinimapData`] into an RGBA buffer and is tested natively. [`Minimap`] (wasm only) owns a
//! `<canvas>` overlay and shows the buffer.
//!
//! # Coordinates
//!
//! The Java minimap lays blocks out like the engine's staggered grid seen from above: block
//! `(x, y)` is a diamond centred at `((x + 0.5 * (y odd)) * S, y * S / 2)` with diagonals `S`, so
//! the diamonds tile without gaps. Screen space below means the space of `shader.wgsl`: pixels at
//! zoom 1, y down, a ground point `(x, y)` at `(200 * (x + 0.5 * odd), 50 * y)`.

// The module is complete but only used once `web.rs` calls it.
#![allow(dead_code)]

use wurfel_sim::block::id;
use wurfel_sim::grid::from_iso;
use wurfel_sim::{Block, World, CHUNK_SIZE_X, CHUNK_SIZE_Y, CHUNK_SIZE_Z};

/// Screen-space size of a block: width of the footprint, row spacing and block height in the
/// projection (`VIEW_WIDTH`, `VIEW_DEPTH2`, `VIEW_HEIGHT` of the Java engine).
const VIEW_WIDTH: f32 = 200.0;
const VIEW_DEPTH2: f32 = 50.0;
const VIEW_HEIGHT: f32 = 122.0;

const BACKGROUND: [u8; 4] = [10, 12, 16, 150];
const BLACK: [u8; 4] = [0, 0, 0, 255];
const RED: [u8; 4] = [255, 0, 0, 255];
const GREEN: [u8; 4] = [0, 255, 0, 255];
const WHITE: [u8; 4] = [255, 255, 255, 255];
/// `Color.BLUE` with alpha 0.8, as in Java.
const ENTITY_OUTLINE: [u8; 4] = [0, 0, 255, 204];
const REQUESTED: [u8; 4] = [255, 160, 0, 255];
const MISSING: [u8; 4] = [220, 40, 40, 255];
/// The Java debug draws render-window chunks as `(1, 1, 0, 0.1)`.
const RENDER_WINDOW: [u8; 4] = [255, 255, 0, 26];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Java `Minimap`.
    Map,
    /// Java `MiniMapChunkDebug`.
    ChunkGrid,
}

/// What this client knows about a chunk.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChunkState {
    /// Data is here.
    Loaded,
    /// Asked the server, no answer yet.
    Requested,
    /// Not loaded and not requested.
    Missing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChunkInfo {
    pub pos: (i32, i32),
    pub state: ChunkState,
    /// Part of the render storage window (the 3x3 chunks that get meshed).
    pub rendered: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EntityDot {
    pub id: u32,
    /// Isometric ground position and height in blocks (`x = gx`, `y = gy`, `z` up).
    pub pos: [f32; 3],
    pub color: [u8; 3],
    /// Our own entity: drawn with a white outline.
    pub local: bool,
    /// Print the `x | y | z` block coordinates next to it, like the Java minimap.
    pub label: bool,
}

/// The camera in screen space (see the module docs).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CameraView {
    /// The screen-space point in the middle of the canvas.
    pub center: [f32; 2],
    /// Visible size in screen-space pixels: canvas size divided by zoom.
    pub size: [f32; 2],
}

pub struct MinimapData<'a> {
    pub mode: Mode,
    pub world: &'a World,
    /// Chunks shown, inclusive (min, max). In map mode the picture is fitted to this area.
    pub area: ((i32, i32), (i32, i32)),
    pub chunks: Vec<ChunkInfo>,
    pub entities: Vec<EntityDot>,
    pub camera: Option<CameraView>,
    /// Change this whenever blocks change (the cached terrain layer is rebuilt on a new value).
    pub terrain_version: u64,
    /// Device pixels per CSS pixel; scales text and line widths.
    pub pixel_ratio: u32,
}

/// `x | y | z` of an entity's block, the label the Java minimap prints.
pub fn entity_label(entity: &EntityDot) -> String {
    let (x, y) = from_iso(entity.pos[0], entity.pos[1]);
    format!("{x} | {y} | {}", entity.pos[2].floor() as i32)
}

// -------------------------------------------------------------------------------------- layout

/// Where blocks go in the picture.
#[derive(Debug, Clone, Copy)]
struct Layout {
    /// Diagonal of a block diamond in pixels (the Java `scaleX`, 12 there).
    scale: f32,
    /// First block column / row shown.
    x0: i32,
    y0: i32,
    cols: i32,
    rows: i32,
}

impl Layout {
    fn fit(area: ((i32, i32), (i32, i32)), width: u32, height: u32) -> Layout {
        let (min, max) = area;
        let cols = (max.0 - min.0 + 1).max(0) * CHUNK_SIZE_X;
        let rows = (max.1 - min.1 + 1).max(0) * CHUNK_SIZE_Y;
        // Width: `cols` diamonds plus the half-block shift of odd rows. Height: row spacing is
        // half a diamond, plus one half diamond to close the last row.
        let by_width = width as f32 / (cols as f32 + 0.5);
        let by_height = height as f32 / ((rows as f32 + 1.0) / 2.0);
        Layout {
            scale: by_width.min(by_height).max(0.0),
            x0: min.0 * CHUNK_SIZE_X,
            y0: min.1 * CHUNK_SIZE_Y,
            cols,
            rows,
        }
    }

    /// Centre of block column `(x, y)` in pixels.
    fn center(&self, x: i32, y: i32) -> (f32, f32) {
        let s = self.scale;
        (
            ((x - self.x0) as f32 + 0.5 * y.rem_euclid(2) as f32) * s + s / 2.0,
            (y - self.y0) as f32 * s / 2.0 + s / 2.0,
        )
    }

    /// The block column under the pixel centre `(fx, fy)`, if inside the shown area.
    ///
    /// A diamond is `|dx| + |dy| <= S/2`, which is a square in the rotated coordinates
    /// `s = dx + dy`, `d = dx - dy`; the diamonds are exactly the unit cells of that rotated grid.
    fn cell_at(&self, fx: f32, fy: f32) -> Option<(i32, i32)> {
        if self.scale <= 0.0 {
            return None;
        }
        let (rx, ry) = (fx - self.scale / 2.0, fy - self.scale / 2.0);
        let i = ((rx + ry) / self.scale).round() as i32;
        let j = ((rx - ry) / self.scale).round() as i32;
        let y = i - j;
        let x = (i + j - y.rem_euclid(2)) / 2;
        if (0..self.cols).contains(&x) && (0..self.rows).contains(&y) {
            Some((self.x0 + x, self.y0 + y))
        } else {
            None
        }
    }

    /// A screen-space point (ground level) in pixels. See the module docs.
    fn from_screen(&self, sx: f32, sy: f32) -> (f32, f32) {
        let s = self.scale;
        ((sx / VIEW_WIDTH - self.x0 as f32) * s + s / 2.0, (sy / VIEW_DEPTH2 - self.y0 as f32) * s / 2.0 + s / 2.0)
    }

    /// Pixel position of an entity: its ground footprint, like the Java `rectX`/`rectY`.
    fn from_iso(&self, gx: f32, gy: f32) -> (f32, f32) {
        self.from_screen((gx - gy) * 100.0, (gx + gy) * 50.0)
    }

    /// Pixel rectangle covering the whole chunk.
    fn chunk_rect(&self, (cx, cy): (i32, i32)) -> [f32; 4] {
        let s = self.scale;
        let left = (cx * CHUNK_SIZE_X - self.x0) as f32 * s;
        let top = (cy * CHUNK_SIZE_Y - self.y0) as f32 * s / 2.0;
        [left, top, left + CHUNK_SIZE_X as f32 * s, top + CHUNK_SIZE_Y as f32 * s / 2.0 + s / 2.0]
    }
}

// -------------------------------------------------------------------------------------- canvas

struct Canvas<'a> {
    width: i32,
    height: i32,
    pixels: &'a mut [u8],
}

impl Canvas<'_> {
    /// Source-over compositing of a straight-alpha colour.
    fn blend(&mut self, x: i32, y: i32, color: [u8; 4]) {
        if x < 0 || y < 0 || x >= self.width || y >= self.height {
            return;
        }
        let i = (y * self.width + x) as usize * 4;
        let (sa, da) = (color[3] as f32 / 255.0, self.pixels[i + 3] as f32 / 255.0);
        let out_a = sa + da * (1.0 - sa);
        if out_a <= 0.0 {
            return;
        }
        for c in 0..3 {
            let v = (color[c] as f32 * sa + self.pixels[i + c] as f32 * da * (1.0 - sa)) / out_a;
            self.pixels[i + c] = v.round() as u8;
        }
        self.pixels[i + 3] = (out_a * 255.0).round() as u8;
    }

    fn fill_rect(&mut self, [x0, y0, x1, y1]: [i32; 4], color: [u8; 4]) {
        for y in y0.max(0)..y1.min(self.height) {
            for x in x0.max(0)..x1.min(self.width) {
                self.blend(x, y, color);
            }
        }
    }

    /// One-pixel (times `thickness`) outline of the rectangle with the given corners.
    fn rect_outline(&mut self, (x0, y0): (f32, f32), (x1, y1): (f32, f32), color: [u8; 4], thickness: i32) {
        let (x0, x1) = (x0.min(x1).round() as i32, x0.max(x1).round() as i32);
        let (y0, y1) = (y0.min(y1).round() as i32, y0.max(y1).round() as i32);
        for t in 0..thickness {
            self.line((x0, y0 + t), (x1, y0 + t), color);
            self.line((x0, y1 - t), (x1, y1 - t), color);
            self.line((x0 + t, y0), (x0 + t, y1), color);
            self.line((x1 - t, y0), (x1 - t, y1), color);
        }
    }

    fn line(&mut self, (x0, y0): (i32, i32), (x1, y1): (i32, i32), color: [u8; 4]) {
        let steps = (x1 - x0).abs().max((y1 - y0).abs()).max(1);
        for i in 0..=steps {
            let t = i as f32 / steps as f32;
            self.blend(
                (x0 as f32 + (x1 - x0) as f32 * t).round() as i32,
                (y0 as f32 + (y1 - y0) as f32 * t).round() as i32,
                color,
            );
        }
    }

    fn diamond_outline(&mut self, (cx, cy): (f32, f32), half: f32, color: [u8; 4]) {
        let (l, r, t, b) = ((cx - half) as i32, (cx + half) as i32, (cy - half) as i32, (cy + half) as i32);
        let (mx, my) = (cx as i32, cy as i32);
        self.line((l, my), (mx, t), color);
        self.line((mx, t), (r, my), color);
        self.line((r, my), (mx, b), color);
        self.line((mx, b), (l, my), color);
    }

    fn text(&mut self, (x, y): (i32, i32), text: &str, color: [u8; 4], scale: i32) {
        let mut pen = x;
        for ch in text.chars() {
            let glyph = glyph(ch);
            for (row, bits) in glyph.iter().enumerate() {
                for col in 0..3 {
                    if bits >> (2 - col) & 1 == 1 {
                        self.fill_rect(
                            [
                                pen + col * scale,
                                y + row as i32 * scale,
                                pen + (col + 1) * scale,
                                y + (row as i32 + 1) * scale,
                            ],
                            color,
                        );
                    }
                }
            }
            pen += 4 * scale;
        }
    }
}

/// A 3x5 pixel font for the characters coordinates need. Each row is 3 bits.
fn glyph(ch: char) -> [u8; 5] {
    match ch {
        '0' => [0b111, 0b101, 0b101, 0b101, 0b111],
        '1' => [0b010, 0b110, 0b010, 0b010, 0b111],
        '2' => [0b111, 0b001, 0b111, 0b100, 0b111],
        '3' => [0b111, 0b001, 0b111, 0b001, 0b111],
        '4' => [0b101, 0b101, 0b111, 0b001, 0b001],
        '5' => [0b111, 0b100, 0b111, 0b001, 0b111],
        '6' => [0b111, 0b100, 0b111, 0b101, 0b111],
        '7' => [0b111, 0b001, 0b001, 0b010, 0b010],
        '8' => [0b111, 0b101, 0b111, 0b101, 0b111],
        '9' => [0b111, 0b101, 0b111, 0b001, 0b111],
        '-' => [0b000, 0b000, 0b111, 0b000, 0b000],
        '|' => [0b010, 0b010, 0b010, 0b010, 0b010],
        ':' => [0b000, 0b010, 0b000, 0b010, 0b000],
        '.' => [0b000, 0b000, 0b000, 0b000, 0b010],
        _ => [0; 5],
    }
}

// -------------------------------------------------------------------------------------- colours

/// A colour representing the block. The Java engine samples its sprite; the browser client draws
/// flat colours, so these are the colours of `mesh.rs`.
pub(crate) fn block_color(block: Block) -> [u8; 3] {
    match block.id() {
        id::GRASS => [92, 163, 64],
        id::DIRT => [135, 94, 61],
        id::STONE => [133, 133, 140],
        id::SAND => [224, 204, 135],
        id::WATER => [51, 115, 204],
        _ => [255, 0, 255],
    }
}

/// Shown for columns without any block (the Java `groundblockinstance`, dirt).
const GROUND: Block = Block::new(id::DIRT, 0);

fn chunk_color(state: ChunkState) -> [u8; 4] {
    match state {
        ChunkState::Loaded => BLACK,
        ChunkState::Requested => REQUESTED,
        ChunkState::Missing => MISSING,
    }
}

// ------------------------------------------------------------------------------------ rasteriser

/// Renders without keeping state. Prefer [`MinimapRenderer`] when drawing every frame.
pub fn render(data: &MinimapData, width: u32, height: u32) -> Vec<u8> {
    MinimapRenderer::default().render(data, width, height).to_vec()
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TerrainKey {
    mode: Mode,
    size: (u32, u32),
    area: ((i32, i32), (i32, i32)),
    ratio: u32,
    version: u64,
    chunks: u64,
}

/// Draws the minimap and caches the expensive part. The terrain layer (colours, chunk outlines,
/// the whole picture in chunk grid mode) depends only on the blocks and chunk states, so it is
/// rebuilt only when `terrain_version` or the chunk list changes. Entities and the camera are
/// drawn on top every call, which is cheap.
#[derive(Default)]
pub struct MinimapRenderer {
    terrain: Vec<u8>,
    key: Option<TerrainKey>,
    out: Vec<u8>,
    /// How often the terrain layer was rebuilt (for tests and diagnostics).
    pub terrain_builds: u32,
}

impl MinimapRenderer {
    /// RGBA, straight alpha, `width * height * 4` bytes, row by row from the top.
    pub fn render(&mut self, data: &MinimapData, width: u32, height: u32) -> &[u8] {
        let len = width as usize * height as usize * 4;
        let key = TerrainKey {
            mode: data.mode,
            size: (width, height),
            area: data.area,
            ratio: data.pixel_ratio.max(1),
            version: data.terrain_version,
            chunks: hash_chunks(&data.chunks),
        };
        if self.key.as_ref() != Some(&key) || self.terrain.len() != len {
            self.terrain.clear();
            self.terrain.resize(len, 0);
            build_terrain(data, width, height, &mut self.terrain);
            self.key = Some(key);
            self.terrain_builds += 1;
        }
        self.out.clear();
        self.out.extend_from_slice(&self.terrain);
        if data.mode == Mode::Map {
            let mut canvas = Canvas { width: width as i32, height: height as i32, pixels: &mut self.out };
            draw_overlay(data, Layout::fit(data.area, width, height), &mut canvas);
        }
        &self.out
    }
}

fn hash_chunks(chunks: &[ChunkInfo]) -> u64 {
    // FNV-1a over everything that changes the picture.
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    let mut mix = |v: i64| {
        for byte in v.to_le_bytes() {
            hash = (hash ^ byte as u64).wrapping_mul(0x0100_0000_01b3);
        }
    };
    for c in chunks {
        mix(c.pos.0 as i64);
        mix(c.pos.1 as i64);
        mix(c.state as i64);
        mix(c.rendered as i64);
    }
    hash
}

fn build_terrain(data: &MinimapData, width: u32, height: u32, pixels: &mut [u8]) {
    let mut canvas = Canvas { width: width as i32, height: height as i32, pixels };
    for px in canvas.pixels.chunks_exact_mut(4) {
        px.copy_from_slice(&BACKGROUND);
    }
    if width == 0 || height == 0 {
        return;
    }
    match data.mode {
        Mode::Map => build_map_terrain(data, &mut canvas),
        Mode::ChunkGrid => draw_chunk_grid(data, &mut canvas),
    }
}

fn build_map_terrain(data: &MinimapData, canvas: &mut Canvas) {
    let layout = Layout::fit(data.area, canvas.width as u32, canvas.height as u32);
    if layout.cols == 0 || layout.rows == 0 {
        return;
    }

    // Topmost block of every column of every loaded chunk.
    let mut tops = vec![i8::MIN; (layout.cols * layout.rows) as usize]; // i8::MIN = not loaded
    let mut colors = vec![[0u8; 3]; tops.len()];
    let mut max_z = 0i32;
    for info in data.chunks.iter().filter(|c| c.state == ChunkState::Loaded) {
        let Some(chunk) = data.world.chunk(info.pos.0, info.pos.1) else { continue };
        let (bx, by) = (info.pos.0 * CHUNK_SIZE_X - layout.x0, info.pos.1 * CHUNK_SIZE_Y - layout.y0);
        for lx in 0..CHUNK_SIZE_X {
            for ly in 0..CHUNK_SIZE_Y {
                let (col, row) = (bx + lx, by + ly);
                if !(0..layout.cols).contains(&col) || !(0..layout.rows).contains(&row) {
                    continue;
                }
                let top = (0..CHUNK_SIZE_Z).rev().find(|&z| !chunk.get(lx, ly, z).is_air());
                let block = top.map_or(GROUND, |z| chunk.get(lx, ly, z));
                let i = (row * layout.cols + col) as usize;
                tops[i] = top.map_or(-1, |z| z as i8);
                colors[i] = block_color(block);
                max_z = max_z.max(top.unwrap_or(0));
            }
        }
    }

    // The Java shading: colour * 1.5 * (topZ + 2) / (maxZ + 1).
    for y in 0..canvas.height {
        for x in 0..canvas.width {
            let Some((bx, by)) = layout.cell_at(x as f32 + 0.5, y as f32 + 0.5) else { continue };
            let i = ((by - layout.y0) * layout.cols + (bx - layout.x0)) as usize;
            if tops[i] == i8::MIN {
                continue; // not loaded: leave the background
            }
            let shade = 1.5 * (tops[i] as i32 + 2) as f32 / (max_z + 1) as f32;
            let [r, g, b] = colors[i].map(|c| (c as f32 * shade).round().clamp(0.0, 255.0) as u8);
            canvas.blend(x, y, [r, g, b, 255]);
        }
    }

    // Chunk outlines: black for loaded like the Java version, orange/red while streaming.
    let t = data.pixel_ratio.max(1) as i32;
    for info in &data.chunks {
        let [x0, y0, x1, y1] = layout.chunk_rect(info.pos);
        canvas.rect_outline((x0, y0), (x1, y1), chunk_color(info.state), t);
        if info.rendered {
            canvas.fill_rect([x0 as i32, y0 as i32, x1 as i32, y1 as i32], RENDER_WINDOW);
        }
    }
}

/// `MiniMapChunkDebug`: 9x9 squares on a 10 pixel grid around the centre of the picture.
fn draw_chunk_grid(data: &MinimapData, canvas: &mut Canvas) {
    let r = data.pixel_ratio.max(1) as i32;
    let (ox, oy) = (canvas.width / 2, canvas.height / 2);
    let square = |canvas: &mut Canvas, (cx, cy): (i32, i32), color: [u8; 4]| {
        let (x, y) = (ox + cx * 10 * r, oy + cy * 10 * r);
        canvas.fill_rect([x, y, x + 9 * r, y + 9 * r], color);
    };
    for info in &data.chunks {
        let color = match info.state {
            ChunkState::Loaded => GREEN,
            ChunkState::Requested => REQUESTED,
            ChunkState::Missing => MISSING,
        };
        // Missing chunks are only worth a square when they are about to be rendered.
        if info.state != ChunkState::Missing || info.rendered {
            square(canvas, info.pos, color);
        }
    }
    for info in data.chunks.iter().filter(|c| c.rendered) {
        square(canvas, info.pos, RENDER_WINDOW);
    }
}

fn draw_overlay(data: &MinimapData, layout: Layout, canvas: &mut Canvas) {
    if layout.scale <= 0.0 {
        return;
    }
    let ratio = data.pixel_ratio.max(1) as i32;

    // Camera rectangles.
    if let Some(cam) = data.camera {
        let (left, right) = (cam.center[0] - cam.size[0] / 2.0, cam.center[0] + cam.size[0] / 2.0);
        let (top, bottom) = (cam.center[1] - cam.size[1] / 2.0, cam.center[1] + cam.size[1] / 2.0);

        // Red: the block columns/rows that can be visible (Java `getVisible*Border`): columns one
        // block wider than the view on each side, rows from the top to the bottom of the view.
        let first_col = (left / VIEW_WIDTH - 1.0).floor();
        let last_col = (right / VIEW_WIDTH + 1.0).floor();
        let s = layout.scale;
        let col_px = |c: f32| (c - layout.x0 as f32) * s;
        let (_, row_top) = layout.from_screen(0.0, top);
        let (_, row_bottom) = layout.from_screen(0.0, bottom);
        canvas.rect_outline((col_px(first_col), row_top), (col_px(last_col + 1.0), row_bottom), RED, ratio);

        // Green: the view on the ground. White: the same view seen at the top of the world, which
        // lies further to the front by the height of the world in screen pixels.
        let top_shift = CHUNK_SIZE_Z as f32 * VIEW_HEIGHT;
        for (shift, color) in [(0.0, GREEN), (top_shift, WHITE)] {
            let a = layout.from_screen(left, top + shift);
            let b = layout.from_screen(right, bottom + shift);
            canvas.rect_outline(a, b, color, ratio);
        }
    }

    // Entities: blue outline like Java, a dot in the entity colour, and the coordinates in red.
    for entity in &data.entities {
        let (cx, cy) = layout.from_iso(entity.pos[0], entity.pos[1]);
        let half = (layout.scale / 2.0).max(2.0 * ratio as f32);
        canvas.diamond_outline((cx, cy), half, if entity.local { WHITE } else { ENTITY_OUTLINE });
        let dot = ratio.max((layout.scale / 4.0) as i32);
        let [r, g, b] = entity.color;
        canvas.fill_rect([cx as i32 - dot / 2, cy as i32 - dot / 2, cx as i32 + (dot + 1) / 2, cy as i32 + (dot + 1) / 2], [r, g, b, 255]);
        if entity.label {
            canvas.text((cx as i32 + half as i32 + ratio, cy as i32 - 2 * ratio), &entity_label(entity), RED, ratio);
        }
    }
}

// ------------------------------------------------------------------------------------ wasm glue

#[cfg(target_arch = "wasm32")]
mod glue {
    use wasm_bindgen::prelude::*;
    use wasm_bindgen::{Clamped, JsCast};
    use web_sys::{CanvasRenderingContext2d, Document, HtmlCanvasElement, ImageData};

    use super::{MinimapData, MinimapRenderer};

    /// A canvas pinned to the bottom right corner of the page that shows the minimap.
    pub struct Minimap {
        canvas: HtmlCanvasElement,
        context: CanvasRenderingContext2d,
        renderer: MinimapRenderer,
        visible: bool,
        pixel_ratio: u32,
    }

    impl Minimap {
        /// Create the overlay, hidden. `css_width` / `css_height` are in CSS pixels; the canvas is
        /// rendered at device resolution (`pixel_ratio` = rounded device pixel ratio, at least 1).
        pub fn new(document: &Document, css_width: u32, css_height: u32, pixel_ratio: u32) -> Result<Minimap, JsValue> {
            let pixel_ratio = pixel_ratio.max(1);
            let canvas: HtmlCanvasElement = document.create_element("canvas")?.dyn_into()?;
            canvas.set_width(css_width * pixel_ratio);
            canvas.set_height(css_height * pixel_ratio);
            canvas.set_attribute(
                "style",
                &format!(
                    "position:fixed;right:12px;bottom:12px;width:{css_width}px;height:{css_height}px;\
                     pointer-events:none;image-rendering:pixelated;z-index:5;display:none;\
                     border:1px solid rgba(255,255,255,.25)"
                ),
            )?;
            canvas.set_attribute("aria-hidden", "true")?;
            document.body().ok_or("no <body>")?.append_child(&canvas)?;
            let context: CanvasRenderingContext2d =
                canvas.get_context("2d")?.ok_or("no 2d context")?.dyn_into()?;
            Ok(Minimap { canvas, context, renderer: MinimapRenderer::default(), visible: false, pixel_ratio })
        }

        pub fn set_visible(&mut self, visible: bool) {
            self.visible = visible;
            let _ = self.canvas.style().set_property("display", if visible { "block" } else { "none" });
        }

        /// Returns the new visibility.
        pub fn toggle(&mut self) -> bool {
            self.set_visible(!self.visible);
            self.visible
        }

        pub fn is_visible(&self) -> bool {
            self.visible
        }

        pub fn pixel_ratio(&self) -> u32 {
            self.pixel_ratio
        }

        /// Redraw. Does nothing while hidden. `data.pixel_ratio` should be [`Self::pixel_ratio`].
        pub fn update(&mut self, data: &MinimapData) {
            if !self.visible {
                return;
            }
            let (w, h) = (self.canvas.width(), self.canvas.height());
            let pixels = self.renderer.render(data, w, h);
            if let Ok(image) = ImageData::new_with_u8_clamped_array_and_sh(Clamped(pixels), w, h) {
                let _ = self.context.put_image_data(&image, 0.0, 0.0);
            }
        }

        /// Remove the canvas from the page.
        pub fn remove(&self) {
            self.canvas.remove();
        }
    }
}

#[cfg(target_arch = "wasm32")]
#[allow(unused_imports)] // until web.rs creates one
pub use glue::Minimap;

// ------------------------------------------------------------------------------------------ tests

#[cfg(test)]
mod tests {
    use super::*;
    use wurfel_sim::grid::to_iso;
    use wurfel_sim::{AirGenerator, IslandGenerator};

    const W: u32 = 150;
    const H: u32 = 300;

    fn island_world(chunks: &[(i32, i32)]) -> World {
        let mut world = World::new(IslandGenerator::new(1));
        for &(cx, cy) in chunks {
            world.load_chunk(cx, cy);
        }
        world
    }

    fn data(world: &World, area: ((i32, i32), (i32, i32)), chunks: Vec<ChunkInfo>) -> MinimapData<'_> {
        MinimapData {
            mode: Mode::Map,
            world,
            area,
            chunks,
            entities: Vec::new(),
            camera: None,
            terrain_version: 1,
            pixel_ratio: 1,
        }
    }

    fn loaded(pos: (i32, i32)) -> ChunkInfo {
        ChunkInfo { pos, state: ChunkState::Loaded, rendered: false }
    }

    fn pixel(pixels: &[u8], width: u32, x: i32, y: i32) -> [u8; 4] {
        let i = (y as usize * width as usize + x as usize) * 4;
        [pixels[i], pixels[i + 1], pixels[i + 2], pixels[i + 3]]
    }

    fn at(pixels: &[u8], (x, y): (f32, f32)) -> [u8; 4] {
        pixel(pixels, W, x.round() as i32, y.round() as i32)
    }

    const ONE_CHUNK: ((i32, i32), (i32, i32)) = ((0, 0), (0, 0));

    #[test]
    fn empty_inputs_do_not_panic_and_have_the_right_size() {
        let world = World::new(AirGenerator);
        for (w, h) in [(0, 0), (1, 1), (7, 3), (W, H)] {
            let pixels = render(&data(&world, ((-1, -1), (1, 1)), vec![]), w, h);
            assert_eq!(pixels.len(), (w * h * 4) as usize);
        }
        // Loaded but empty chunks: only air, so only the ground colour shows.
        let mut empty = World::new(AirGenerator);
        empty.load_chunk(0, 0);
        let pixels = render(&data(&empty, ONE_CHUNK, vec![loaded((0, 0))]), W, H);
        assert_eq!(pixels.len(), (W * H * 4) as usize);
        for mode in [Mode::Map, Mode::ChunkGrid] {
            let mut d = data(&empty, ONE_CHUNK, vec![]);
            d.mode = mode;
            render(&d, W, H);
        }
    }

    #[test]
    fn layout_maps_pixels_to_the_right_blocks_and_back() {
        let layout = Layout::fit(((-1, -1), (1, 1)), W, H);
        for y in layout.y0..layout.y0 + layout.rows {
            for x in layout.x0..layout.x0 + layout.cols {
                let (cx, cy) = layout.center(x, y);
                assert_eq!(layout.cell_at(cx, cy), Some((x, y)), "centre of {x},{y}");
                let near = layout.scale * 0.4; // inside the diamond, clear of its tips
                for (dx, dy) in [(near, 0.0), (-near, 0.0), (0.0, near), (0.0, -near)] {
                    assert_eq!(layout.cell_at(cx + dx, cy + dy), Some((x, y)));
                }
                assert!(cx >= 0.0 && cx <= W as f32 && cy >= 0.0 && cy <= H as f32, "{x},{y} fits in the image");
            }
        }
        assert_eq!(layout.cell_at(-5.0, -5.0), None);
        assert_eq!(layout.cell_at(W as f32 + 5.0, 10.0), None);
    }

    #[test]
    fn the_island_peak_is_drawn_in_shaded_grass() {
        let world = island_world(&[(0, 0)]);
        let (px, py) = IslandGenerator::new(1).peak();
        let layout = Layout::fit(ONE_CHUNK, W, H);
        let pixels = render(&data(&world, ONE_CHUNK, vec![loaded((0, 0))]), W, H);

        // Peak column: top block grass at z = 8, highest column, so shade = 1.5 * 10 / 9 = 1.667.
        // grass (92, 163, 64) * 1.667 = (153.3, 271 -> clamped 255, 106.7).
        let c = at(&pixels, layout.center(px, py));
        assert_eq!(c, [153, 255, 107, 255]);

        // A column far from the peak but inside the chunk is water at z = 2:
        // shade = 1.5 * 4 / 9 = 0.667, water (51, 115, 204) -> (34, 77, 136).
        let (wx, wy) = if px < 5 { (9, 39) } else { (0, 0) };
        let c = at(&pixels, layout.center(wx, wy));
        assert_eq!(c, [34, 77, 136, 255]);
    }

    #[test]
    fn unloaded_chunks_show_background_and_a_status_coloured_outline() {
        let world = island_world(&[(0, 0)]);
        let area = ((0, 0), (1, 0));
        let layout = Layout::fit(area, W, H);
        let chunks = vec![
            loaded((0, 0)),
            ChunkInfo { pos: (1, 0), state: ChunkState::Requested, rendered: false },
        ];
        let pixels = render(&data(&world, area, chunks), W, H);

        // A column inside the requested (not loaded) chunk is only background.
        let inside = at(&pixels, layout.center(15, 20));
        assert_eq!(inside, BACKGROUND);
        // Its outline is orange, the loaded neighbour's is black.
        let [x0, y0, ..] = layout.chunk_rect((1, 0));
        assert_eq!(pixel(&pixels, W, x0.round() as i32 + 3, y0.round() as i32), REQUESTED);
        let [x0, y0, ..] = layout.chunk_rect((0, 0));
        assert_eq!(pixel(&pixels, W, x0.round() as i32 + 3, y0.round() as i32), BLACK);
    }

    #[test]
    fn entities_are_drawn_where_they_stand_with_an_outline() {
        let world = island_world(&[(0, 0)]);
        let layout = Layout::fit(ONE_CHUNK, W, H);
        let (gx, gy) = to_iso(5, 20);
        let mut d = data(&world, ONE_CHUNK, vec![loaded((0, 0))]);
        d.entities = vec![
            EntityDot { id: 1, pos: [gx, gy, 1.0], color: [250, 20, 30], local: false, label: false },
            EntityDot { id: 2, pos: [gx + 2.0, gy, 1.0], color: [20, 250, 30], local: true, label: false },
        ];
        let pixels = render(&d, W, H);

        let (cx, cy) = layout.center(5, 20);
        assert_eq!(at(&pixels, (cx, cy)), [250, 20, 30, 255], "dot at the block centre");
        // Outline tip, half a diamond to the left: blue with alpha 0.8 over whatever is below.
        let (tx, ty) = (cx - layout.scale / 2.0, cy);
        let blue_near_tip = (-1..=1).any(|dx| {
            (-1..=1).any(|dy| {
                let p = at(&pixels, (tx + dx as f32, ty + dy as f32));
                p[2] > 150 && p[0] < 130 // blue (alpha 0.8) over whatever colour is below
            })
        });
        assert!(blue_near_tip, "blue outline at the left tip");

        // The second entity is two blocks along +gx (two diamonds to the right on the diagonal).
        let (ex, ey) = layout.from_iso(gx + 2.0, gy);
        assert_eq!(at(&pixels, (ex, ey)), [20, 250, 30, 255]);
        let (tx, ty) = (ex - layout.scale / 2.0, ey);
        let white_near_tip = (-1..=1).any(|dx| (-1..=1).any(|dy| at(&pixels, (tx + dx as f32, ty + dy as f32)) == WHITE));
        assert!(white_near_tip, "local entity has a white outline at its left tip");
    }

    #[test]
    fn entity_labels_use_block_coordinates_and_get_drawn_in_red() {
        let world = island_world(&[(0, 0)]);
        let (gx, gy) = to_iso(5, 20);
        let entity = EntityDot { id: 1, pos: [gx, gy, 3.7], color: [1, 2, 3], local: true, label: true };
        assert_eq!(entity_label(&entity), "5 | 20 | 3");

        let mut d = data(&world, ONE_CHUNK, vec![loaded((0, 0))]);
        let without = {
            let mut e = entity;
            e.label = false;
            d.entities = vec![e];
            render(&d, W, H)
        };
        d.entities = vec![entity];
        let with = render(&d, W, H);
        let red_pixels = |p: &[u8]| p.chunks_exact(4).filter(|c| c[..] == RED).count();
        // "5 | 20 | 3" is 8 glyphs of the font; the pixel count of those glyphs is fixed.
        assert_eq!(red_pixels(&without), 0);
        let expected: u32 = "5 | 20 | 3".chars().map(|c| glyph(c).iter().map(|r| r.count_ones()).sum::<u32>()).sum();
        let found = red_pixels(&with) as u32;
        // Glyph pixels can be hidden under the entity's own outline, never added.
        assert!(found > 0 && found <= expected, "found {found}, expected at most {expected}");
    }

    #[test]
    fn the_font_draws_the_expected_pixels() {
        let mut pixels = vec![0u8; 16 * 8 * 4];
        let mut canvas = Canvas { width: 16, height: 8, pixels: &mut pixels };
        canvas.text((1, 1), "1|", WHITE, 1);
        // '1' = 010 / 110 / 010 / 010 / 111 at x = 1..4, y = 1..6.
        let lit = |x: i32, y: i32| pixel(&pixels, 16, x, y)[3] == 255;
        assert!(lit(2, 1) && !lit(1, 1) && !lit(3, 1)); // top row: middle only
        assert!(lit(1, 2) && lit(2, 2)); // second row: left and middle
        assert!(lit(1, 5) && lit(2, 5) && lit(3, 5)); // base
        // '|' starts 4 pixels later and is a vertical bar in its middle column.
        assert!((1..=5).all(|y| lit(6, y)) && !lit(5, 3) && !lit(7, 3));
    }

    #[test]
    fn camera_rectangles_sit_at_the_projected_positions() {
        let world = island_world(&[(0, 0)]);
        let layout = Layout::fit(ONE_CHUNK, W, H);
        let mut d = data(&world, ONE_CHUNK, vec![loaded((0, 0))]);
        // A view 1000 x 600 screen pixels wide centred on the ground point of block (5, 20).
        let center = [5.0 * VIEW_WIDTH, 20.0 * VIEW_DEPTH2];
        d.camera = Some(CameraView { center, size: [1000.0, 600.0] });
        let pixels = render(&d, W, H);

        // Green rectangle: left edge x and the vertical middle, in pixels.
        let (left, mid_y) = layout.from_screen(center[0] - 500.0, center[1]);
        assert_eq!(at(&pixels, (left, mid_y)), GREEN);
        // Its top edge, on the vertical middle of the rectangle.
        let (mid_x, top) = layout.from_screen(center[0], center[1] - 300.0);
        assert_eq!(at(&pixels, (mid_x, top)), GREEN);

        // White rectangle: the same view, shifted forward by the world height. It starts where
        // the green one ends in y here (600 < 32 * 122), so check its own top edge.
        let (mid_x, white_top) = layout.from_screen(center[0], center[1] - 300.0 + CHUNK_SIZE_Z as f32 * VIEW_HEIGHT);
        assert!(white_top > top, "the top-of-world view lies further to the front");
        if (white_top.round() as u32) < H {
            assert_eq!(at(&pixels, (mid_x, white_top)), WHITE);
        }

        // Red: columns widened by one block each side. Left edge of column floor(left/200 - 1).
        let first_col = ((center[0] - 500.0) / VIEW_WIDTH - 1.0).floor();
        let red_x = (first_col - layout.x0 as f32) * layout.scale;
        assert_eq!(at(&pixels, (red_x, mid_y)), RED);
    }

    #[test]
    fn chunk_grid_mode_matches_the_java_debug_view() {
        let world = World::new(AirGenerator);
        let mut d = data(&world, ((-1, -1), (1, 1)), vec![]);
        d.mode = Mode::ChunkGrid;
        d.chunks = vec![
            loaded((0, 0)),
            ChunkInfo { pos: (1, 0), state: ChunkState::Loaded, rendered: true },
            ChunkInfo { pos: (0, 1), state: ChunkState::Requested, rendered: false },
            ChunkInfo { pos: (-1, 0), state: ChunkState::Missing, rendered: true },
        ];
        let pixels = render(&d, W, H);
        let (ox, oy) = (W as i32 / 2, H as i32 / 2);

        // Loaded: a 9x9 green square at the centre; the gap column (the 10th pixel) is empty.
        assert_eq!(pixel(&pixels, W, ox, oy), GREEN);
        assert_eq!(pixel(&pixels, W, ox + 8, oy + 8), GREEN);
        assert_eq!(pixel(&pixels, W, ox + 9, oy), BACKGROUND);
        // Loaded and in the render window: the yellow tint (alpha 0.1) over green.
        let tinted = pixel(&pixels, W, ox + 10, oy);
        assert_eq!(tinted, [26, 255, 0, 255], "0.1 yellow over green: {tinted:?}");
        // Requested chunks are orange, a missing chunk that is about to be rendered is red.
        assert_eq!(pixel(&pixels, W, ox, oy + 10), REQUESTED);
        let missing = pixel(&pixels, W, ox - 10, oy);
        assert!(missing[0] > 200 && missing[1] < 100, "red-ish, got {missing:?}");
    }

    #[test]
    fn the_terrain_layer_is_cached_until_blocks_or_chunks_change() {
        let mut world = island_world(&[(0, 0)]);
        let mut renderer = MinimapRenderer::default();
        let chunks = vec![loaded((0, 0))];

        let mut d = data(&world, ONE_CHUNK, chunks.clone());
        renderer.render(&d, W, H);
        assert_eq!(renderer.terrain_builds, 1);

        // Moving entities and the camera only redraw the overlay.
        d.entities = vec![EntityDot { id: 1, pos: [3.0, 3.0, 1.0], color: [9, 9, 9], local: false, label: true }];
        d.camera = Some(CameraView { center: [0.0, 0.0], size: [500.0, 500.0] });
        let with_overlay = renderer.render(&d, W, H).to_vec();
        assert_eq!(renderer.terrain_builds, 1);

        // A block edit with a new terrain_version rebuilds and changes the picture.
        let (px, py) = IslandGenerator::new(1).peak();
        world.set(px, py, 9, Block::new(id::STONE, 0));
        let mut d = data(&world, ONE_CHUNK, chunks.clone());
        d.terrain_version = 2;
        let after_edit = renderer.render(&d, W, H).to_vec();
        assert_eq!(renderer.terrain_builds, 2);
        assert_ne!(after_edit, with_overlay);

        // A chunk changing state rebuilds too.
        let mut d = data(&world, ONE_CHUNK, vec![ChunkInfo { pos: (0, 0), state: ChunkState::Requested, rendered: false }]);
        d.terrain_version = 2;
        renderer.render(&d, W, H);
        assert_eq!(renderer.terrain_builds, 3);
    }

    #[test]
    fn a_stone_block_placed_on_top_shows_up_in_stone_colour() {
        let mut world = island_world(&[(0, 0)]);
        let (px, py) = IslandGenerator::new(1).peak();
        let layout = Layout::fit(ONE_CHUNK, W, H);
        let before = at(&render(&data(&world, ONE_CHUNK, vec![loaded((0, 0))]), W, H), layout.center(px, py));
        world.set(px, py, 9, Block::new(id::STONE, 0));
        let after = at(&render(&data(&world, ONE_CHUNK, vec![loaded((0, 0))]), W, H), layout.center(px, py));
        assert_ne!(before, after);
        // Stone (133, 133, 140) at the new maximum height z = 9: shade 1.5 * 11 / 10 = 1.65.
        assert_eq!(after, [219, 219, 231, 255]);
    }

    #[test]
    fn every_world_chunk_fits_and_draws_with_negative_coordinates() {
        let chunks: Vec<(i32, i32)> = (-1..=1).flat_map(|x| (-1..=1).map(move |y| (x, y))).collect();
        let world = island_world(&chunks);
        let d = data(&world, ((-1, -1), (1, 1)), chunks.iter().map(|&c| loaded(c)).collect());
        let pixels = render(&d, W, H);
        let opaque = pixels.chunks_exact(4).filter(|p| p[3] == 255).count();
        // The map covers most of the picture (the diamond lattice leaves a ragged edge).
        assert!(opaque > (W * H) as usize / 2, "only {opaque} opaque pixels");
    }

    /// Writes PNGs of a few interesting states when `MINIMAP_PNG_DIR` is set, to look at them.
    #[test]
    fn dump_pngs_for_a_visual_check() {
        let Ok(dir) = std::env::var("MINIMAP_PNG_DIR") else { return };
        let chunks: Vec<(i32, i32)> = (-1..=1).flat_map(|x| (-1..=1).map(move |y| (x, y))).collect();
        let mut world = island_world(&chunks);
        let (px, py) = IslandGenerator::new(1).peak();
        world.set(px + 2, py, 9, Block::new(id::STONE, 0));
        let area = ((-1, -1), (1, 1));
        let mut infos: Vec<ChunkInfo> = chunks.iter().map(|&c| ChunkInfo { pos: c, state: ChunkState::Loaded, rendered: c == (0, 0) }).collect();
        infos[0].state = ChunkState::Requested;
        infos[8].state = ChunkState::Missing;
        let (gx, gy) = to_iso(px, py);
        let mut d = data(&world, area, infos);
        d.pixel_ratio = 2;
        d.entities = vec![
            EntityDot { id: 1, pos: [gx, gy + 2.0, 8.0], color: [240, 190, 50], local: true, label: true },
            EntityDot { id: 2, pos: [gx - 6.0, gy - 3.0, 1.0], color: [150, 90, 220], local: false, label: true },
        ];
        d.camera = Some(CameraView { center: [(gx - gy) * 100.0, (gx + gy) * 50.0], size: [1400.0, 800.0] });
        let (w, h) = (300, 600);
        write_png(&format!("{dir}/minimap-map.png"), w, h, &render(&d, w, h));

        d.mode = Mode::ChunkGrid;
        write_png(&format!("{dir}/minimap-chunks.png"), 120, 120, &render(&d, 120, 120));
    }

    /// Minimal PNG writer (stored deflate blocks), enough to look at a buffer.
    fn write_png(path: &str, w: u32, h: u32, rgba: &[u8]) {
        fn crc(data: &[u8]) -> u32 {
            let mut c = 0xffff_ffffu32;
            for &b in data {
                c ^= b as u32;
                for _ in 0..8 {
                    c = if c & 1 == 1 { 0xedb8_8320 ^ (c >> 1) } else { c >> 1 };
                }
            }
            !c
        }
        fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], body: &[u8]) {
            out.extend((body.len() as u32).to_be_bytes());
            let mut tagged = kind.to_vec();
            tagged.extend(body);
            out.extend(&tagged);
            out.extend(crc(&tagged).to_be_bytes());
        }
        let mut raw = Vec::new();
        for row in rgba.chunks(w as usize * 4) {
            raw.push(0);
            raw.extend(row);
        }
        let mut z = vec![0x78, 0x01];
        let blocks: Vec<&[u8]> = raw.chunks(65_535).collect();
        for (i, block) in blocks.iter().enumerate() {
            z.push((i == blocks.len() - 1) as u8);
            z.extend((block.len() as u16).to_le_bytes());
            z.extend((!(block.len() as u16)).to_le_bytes());
            z.extend(*block);
        }
        let (mut a, mut b) = (1u32, 0u32);
        for &byte in &raw {
            a = (a + byte as u32) % 65_521;
            b = (b + a) % 65_521;
        }
        z.extend(((b << 16) | a).to_be_bytes());
        let mut png = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
        let mut ihdr = Vec::new();
        ihdr.extend(w.to_be_bytes());
        ihdr.extend(h.to_be_bytes());
        ihdr.extend([8, 6, 0, 0, 0]);
        chunk(&mut png, b"IHDR", &ihdr);
        chunk(&mut png, b"IDAT", &z);
        chunk(&mut png, b"IEND", &[]);
        std::fs::write(path, png).unwrap();
    }
}
