//! What the map editor draws into the world: the cursor cube on the block under the pointer (the Java
//! `Cursor`, the entity sprite 8 that follows the mouse), the ghost of the block a click would build,
//! and the pictures of the palette's blocks for the toolbar.

use glam::Vec3;
use wurfel_sim::grid::to_iso;
use wurfel_sim::Block;

use crate::atlas::Region;
use crate::editor::PALETTE;
use crate::mesh::{self, Vertex};
use crate::sprites::{self, BlockLook, Sprites};

/// The Java cursor is entity sprite 8 (a cube drawn as lines), shown on the block under the pointer.
const CURSOR_SPRITE: u8 = 8;
/// How much brighter than the world the cube is drawn (Java: light level 10).
const CURSOR_TINT: [f32; 3] = [1.6; 3];
/// How see-through the preview of the block to build is.
const GHOST_ALPHA: f32 = 0.55;
/// How far the preview floats off the faces of a block it replaces, in blocks.
const LIFT: f32 = 0.03;

/// The cursor cube on the block `cell`. Nothing without the entity sprite (the flat marker on the top
/// face stays).
pub fn push_cursor(out: &mut Vec<Vertex>, sprites: Option<&Sprites>, cell: (i32, i32, i32)) {
    let Some((sprites, region)) = sprites.and_then(|s| Some((s, s.entity(CURSOR_SPRITE, 0)?))) else { return };
    let (gx, gy) = to_iso(cell.0, cell.1);
    let anchor = Vec3::new(gx, gy, cell.2 as f32);
    sprites::billboard_biased(out, &sprites.atlas, region, anchor, sprites::FOOTPRINT_TIP, false, CURSOR_TINT, sprites::OVERLAY_BIAS);
}

/// The block `block` as it will look in `cell`, see-through. With the sprites of the fixed camera its
/// three faces; as a box in the block's flat colour for a turned camera (`turned`) or a block that is
/// not three faces.
pub fn push_ghost(out: &mut Vec<Vertex>, sprites: Option<&Sprites>, block: Block, cell: (i32, i32, i32), turned: bool) {
    let (gx, gy) = to_iso(cell.0, cell.1);
    let (x0, x1, y0, y1) = (gx - 0.5 - LIFT, gx + 0.5 + LIFT, gy - 0.5 - LIFT, gy + 0.5 + LIFT);
    let (z0, z1) = (cell.2 as f32 - LIFT, cell.2 as f32 + 1.0 + LIFT);
    let sided = sprites.and_then(|s| match s.block(block.id(), block.value())? {
        BlockLook::Sided { left, top, right } if !turned => Some((s, left, top, right)),
        _ => None,
    });
    let Some((sprites, left, top, right)) = sided else {
        mesh::ghost_cuboid(out, mesh::block_color(block), [x0, x1, y0, y1], [z0, z1], GHOST_ALPHA);
        return;
    };
    // The same corners as the faces of the block itself (left is +y, right is +x).
    let faces = [
        (left, [[x0, y1, z0], [x1, y1, z0], [x1, y1, z1], [x0, y1, z1]]),
        (top, [[x0, y0, z1], [x1, y0, z1], [x1, y1, z1], [x0, y1, z1]]),
        (right, [[x1, y0, z0], [x1, y1, z0], [x1, y1, z1], [x1, y0, z1]]),
    ];
    for (index, corners) in faces {
        mesh::overlay_face(out, [1.0; 3], corners, &sprites.atlas, sprites.region(index), GHOST_ALPHA);
    }
}

/// The size of the box a block picture is laid out in (the pixels of the sprite sheet).
const BOX: (f32, f32) = (200.0, 223.0);

/// One piece of a block picture: a rectangle of a sprite page, and where it goes in the box.
fn part(sprites: &Sprites, region: &Region, base: (f32, f32)) -> serde_json::Value {
    serde_json::json!({
        "page": format!("assets/sprites/{}", sprites.atlas.pages[region.page].file),
        "x": region.x, "y": region.y, "w": region.w, "h": region.h,
        "dx": base.0 + region.offset_x as f32,
        "dy": base.1 + region.top_in_orig() as f32,
    })
}

/// The pictures of the palette's blocks for the toolbar (`wurfelEditor.update`): per block the pieces
/// of its sprites and where they sit in a 200 x 223 box, drawn by the page like the Java block table.
/// The brush shows the value it is set to, the others their first picture. Without sprites an empty
/// list, and the toolbar falls back to a colour swatch.
pub fn palette_previews(sprites: Option<&Sprites>, brush: Block) -> serde_json::Value {
    let Some(sprites) = sprites else { return serde_json::json!([]) };
    let pictures = PALETTE.iter().map(|&(id, _)| {
        let value = if brush.id() == id { brush.value() } else { 0 };
        let parts = match sprites.block(id, value) {
            Some(BlockLook::Sided { left, top, right }) => vec![
                part(sprites, sprites.region(left), (0.0, 50.0)),
                part(sprites, sprites.region(right), (100.0, 50.0)),
                part(sprites, sprites.region(top), (0.0, 0.0)),
            ],
            Some(BlockLook::Single(index)) => {
                let region = sprites.region(index);
                vec![part(sprites, region, (0.0, BOX.1 - region.orig_h as f32))]
            }
            None => Vec::new(),
        };
        serde_json::json!({ "w": BOX.0, "h": BOX.1, "parts": parts })
    });
    serde_json::Value::Array(pictures.collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::atlas::Atlas;

    fn sprites() -> Sprites {
        let atlas = "p.png\nsize: 512, 512\nformat: RGBA8888\nfilter: Linear,Linear\nrepeat: none\n".to_string()
            + &["b3-0-0 0 0 100 173", "b3-0-2 100 0 100 173", "b3-0-1 0 200 200 100", "e8-0 0 300 200 223"]
                .map(|r| {
                    let n: Vec<&str> = r.split(' ').collect();
                    format!("{}\n  rotate: false\n  xy: {}, {}\n  size: {}, {}\n  orig: {}, {}\n  offset: 0, 0\n  index: -1\n", n[0], n[1], n[2], n[3], n[4], n[3], n[4])
                })
                .concat();
        Sprites::new(Atlas::parse(&atlas).expect("a valid atlas"))
    }

    #[test]
    fn a_sided_block_ghost_is_three_faces_and_a_turned_one_a_box() {
        let sprites = sprites();
        let stone = Block::new(wurfel_sim::block::id::STONE, 0);
        let (mut sided, mut turned) = (Vec::new(), Vec::new());
        push_ghost(&mut sided, Some(&sprites), stone, (1, 2, 3), false);
        push_ghost(&mut turned, Some(&sprites), stone, (1, 2, 3), true);
        assert_eq!(sided.len(), 3 * 6);
        assert_eq!(turned.len(), 6 * 6);
        // Without sprites it is the box as well.
        let mut flat = Vec::new();
        push_ghost(&mut flat, None, stone, (1, 2, 3), false);
        assert_eq!(flat.len(), 6 * 6);
    }

    #[test]
    fn the_cursor_is_the_entity_sprite_and_the_palette_gets_block_pictures() {
        let sprites = sprites();
        let mut out = Vec::new();
        push_cursor(&mut out, Some(&sprites), (0, 0, 0));
        assert_eq!(out.len(), 6);
        push_cursor(&mut out, None, (0, 0, 0));
        assert_eq!(out.len(), 6, "no sprite, no cube");

        let previews = palette_previews(Some(&sprites), Block::new(wurfel_sim::block::id::STONE, 0));
        let previews = previews.as_array().unwrap();
        assert_eq!(previews.len(), PALETTE.len());
        let stone = PALETTE.iter().position(|(id, _)| *id == wurfel_sim::block::id::STONE).unwrap();
        assert_eq!(previews[stone]["parts"].as_array().unwrap().len(), 3);
        assert_eq!(previews[stone]["parts"][0]["page"], "assets/sprites/p.png");
        assert_eq!(palette_previews(None, Block::AIR), serde_json::json!([]));
    }
}
