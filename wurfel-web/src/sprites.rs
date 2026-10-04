//! Which sprite shows what, and the geometry that puts a sprite into the isometric world.
//!
//! The art is the Java game's: the atlas names are `b<id>-<value>-<side>` for the three faces of a
//! block (0 left, 1 top, 2 right), `b<id>-<value>` for a block that is a single picture (a tree, a
//! torch), `e<id>-<value>` for entities and `diff/w/<n>` for the player's walking animation. All
//! of that is pure data and arithmetic and tested natively; `web.rs` only uploads the textures.
//!
//! # How a sprite meets the world
//!
//! The Java engine draws every sprite as a screen-aligned picture. The three faces of a block are
//! sheared pictures that exactly fill the screen bounding box of the face, so a face is textured by
//! mapping its projected corners onto the sprite ([`face_uvs`]). Single pictures are placed with the
//! rules of `GameSpaceSprite`: the picture's original box is centred on the anchor, and its bottom edge
//! is the front tip of the footprint diamond ([`billboard`]).

// The entity and player parts are only used by the browser build.
#![cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]

use std::collections::HashMap;

use glam::Vec3;

use crate::atlas::{Atlas, Region};
use crate::mesh::{Vertex, FACE_SPRITE};

/// Screen pixels of one block step at zoom 1 (the Java engine's `VIEW_WIDTH / 2`, `VIEW_DEPTH / 2`
/// and `VIEW_HEIGHT`), as in `shader.wgsl`.
pub const SCREEN_X: f32 = 100.0;
pub const SCREEN_Y: f32 = 50.0;
pub const SCREEN_Z: f32 = 122.0;
/// How far the bottom of an entity's picture lies below the centre of its footprint: the front tip
/// of the footprint diamond (`VIEW_DEPTH / 2`).
pub const FOOTPRINT_TIP: f32 = 50.0;
/// The bottom of the player's oversized picture lies 100 game units (86 pixels) below the feet; the
/// character itself is drawn higher inside that box (`Ejira.render`).
pub const PLAYER_BOX_BOTTOM: f32 = 86.0;

/// The screen position (pixels at zoom 1, y down) of a point in the isometric ground frame.
pub fn project(p: [f32; 3]) -> [f32; 2] {
    [(p[0] - p[1]) * SCREEN_X, (p[0] + p[1]) * SCREEN_Y - p[2] * SCREEN_Z]
}

/// How much one block of height counts in the depth the shader sorts by.
const DEPTH_Z: f32 = 0.82;

/// The depth the shader sorts by: larger is closer to the viewer.
#[cfg(test)]
pub fn depth(p: Vec3) -> f32 {
    p.x + p.y + DEPTH_Z * p.z
}

/// How a block id looks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockLook {
    /// Three faces: region indices of the left, top and right sprite.
    Sided { left: usize, top: usize, right: usize },
    /// One picture standing on the cell.
    Single(usize),
}

pub struct Sprites {
    pub atlas: Atlas,
    blocks: HashMap<(u8, u8), BlockLook>,
}

impl Sprites {
    pub fn new(atlas: Atlas) -> Self {
        let blocks = index_blocks(&atlas);
        Sprites { atlas, blocks }
    }

    /// The sprites of a block. A value without art of its own looks like value 0, as in the Java
    /// game's variants; an id without art has none and is drawn in its flat colour.
    pub fn block(&self, id: u8, value: u8) -> Option<BlockLook> {
        self.blocks.get(&(id, value)).or_else(|| self.blocks.get(&(id, 0))).copied()
    }

    pub fn region(&self, index: usize) -> &Region {
        self.atlas.at(index)
    }

    /// An entity sprite `e<id>-<value>`.
    pub fn entity(&self, id: u8, value: u32) -> Option<&Region> {
        self.atlas.region(&format!("e{id}-{value}"))
    }

    /// The player's walking sprite `diff/w/<n>` (1 to 64).
    pub fn player(&self, frame: u32) -> Option<&Region> {
        self.atlas.region(&format!("diff/w/{frame}"))
    }
}

/// `b<id>-<value>-<side>` and `b<id>-<value>` of an atlas.
fn index_blocks(atlas: &Atlas) -> HashMap<(u8, u8), BlockLook> {
    let mut sides: HashMap<(u8, u8), [Option<usize>; 3]> = HashMap::new();
    let mut singles: HashMap<(u8, u8), usize> = HashMap::new();
    for (index, region) in atlas.regions().iter().enumerate() {
        let Some(rest) = region.name.strip_prefix('b') else { continue };
        let parts: Vec<Option<u8>> = rest.split('-').map(|p| p.parse::<u8>().ok()).collect();
        match parts.as_slice() {
            [Some(id), Some(value)] => {
                singles.insert((*id, *value), index);
            }
            [Some(id), Some(value), Some(side)] if *side < 3 => {
                sides.entry((*id, *value)).or_default()[*side as usize] = Some(index);
            }
            _ => {}
        }
    }
    let mut blocks = HashMap::new();
    for (key, index) in singles {
        blocks.insert(key, BlockLook::Single(index));
    }
    for (key, faces) in sides {
        if let [Some(left), Some(top), Some(right)] = faces {
            blocks.insert(key, BlockLook::Sided { left, top, right });
        }
    }
    blocks
}

// ----------------------------------------------------------------------------- block faces

/// Atlas coordinates for the four corners of a face, given in the isometric ground frame.
///
/// The sprite is a picture of the face as it looks on screen, so the corner's position inside the
/// screen bounding box of the face is its position inside the sprite's original box.
pub fn face_uvs(atlas: &Atlas, region: &Region, corners: [[f32; 3]; 4]) -> [[f32; 2]; 4] {
    let screen = corners.map(project);
    let min_x = screen.iter().map(|p| p[0]).fold(f32::INFINITY, f32::min);
    let max_x = screen.iter().map(|p| p[0]).fold(f32::NEG_INFINITY, f32::max);
    let min_y = screen.iter().map(|p| p[1]).fold(f32::INFINITY, f32::min);
    let max_y = screen.iter().map(|p| p[1]).fold(f32::NEG_INFINITY, f32::max);
    // The box is 200 wide and its height is 100 + 122 per block; the sprites are 1 pixel taller or
    // so (173 for 172.5), so scale rather than offset.
    let scale_x = region.orig_w as f32 / (max_x - min_x).max(1e-6);
    let scale_y = region.orig_h as f32 / (max_y - min_y).max(1e-6);
    screen.map(|p| region.orig_point_uv(atlas, (p[0] - min_x) * scale_x, (p[1] - min_y) * scale_y))
}

// ------------------------------------------------------------------------------- billboards

/// Where the screen offset `(dx, dy)` (pixels at zoom 1, y down) from the anchor lies in the world,
/// on the plane through the anchor that faces the camera, so that the whole sprite has the anchor's
/// depth. `toward_camera` moves the plane that much (in depth units) closer to the viewer without
/// moving it on screen, so a sprite is not cut by the ground it stands on.
pub fn billboard_point(anchor: Vec3, dx: f32, dy: f32, toward_camera: f32) -> [f32; 3] {
    // Right on screen is (+x, -y) in the ground frame: one step of 2 * SCREEN_X pixels per block.
    // Down on screen is -z, compensated by a step (t, t) along the ground diagonal that cancels the
    // change of depth: 2t + 0.82 dz = 0, and then dsy = 2 * SCREEN_Y * t - SCREEN_Z * dz = dy.
    let dz = -dy / (SCREEN_Z + 2.0 * SCREEN_Y * DEPTH_Z / 2.0);
    let t = -DEPTH_Z / 2.0 * dz;
    // A step d along (1, 1, e) with e = 2 * SCREEN_Y / SCREEN_Z does not move on screen
    // (2 * SCREEN_Y * d = SCREEN_Z * e * d) but changes the depth by (2 + 0.82 e) d.
    let e = 2.0 * SCREEN_Y / SCREEN_Z;
    let d = toward_camera / (2.0 + DEPTH_Z * e);
    [anchor.x + dx / (2.0 * SCREEN_X) + t + d, anchor.y - dx / (2.0 * SCREEN_X) + t + d, anchor.z + dz + e * d]
}

/// The two triangles of a sprite standing in the world, anchored at `anchor`.
///
/// `box_bottom` is how many pixels below the anchor's screen position the bottom edge of the
/// sprite's original box lies. `mirror` flips it left to right. `tint` multiplies the sprite.
pub fn billboard(out: &mut Vec<Vertex>, atlas: &Atlas, region: &Region, anchor: Vec3, box_bottom: f32, mirror: bool, tint: [f32; 3]) {
    let page = &atlas.pages[region.page];
    // The sprite's rectangle in pixels relative to the anchor: the original box is centred on it.
    let left = -(region.orig_w as f32) / 2.0 + region.offset_x as f32;
    let top = box_bottom - region.orig_h as f32 + region.top_in_orig() as f32;
    let (right, bottom) = (left + region.w as f32, top + region.h as f32);
    let (u0, u1) = (region.x as f32 / page.width as f32, (region.x + region.w) as f32 / page.width as f32);
    let (v0, v1) = (region.y as f32 / page.height as f32, (region.y + region.h) as f32 / page.height as f32);
    let (u_left, u_right) = if mirror { (u1, u0) } else { (u0, u1) };
    let bias = 0.3;
    let vertex = |dx: f32, dy: f32, u: f32, v: f32| Vertex {
        position: billboard_point(anchor, dx, dy, bias),
        color: tint,
        shade: [FACE_SPRITE, 0.0],
        point: [0.0; 3],
        uv: [u, v],
        layer: region.page as f32,
    };
    let (a, b, c, d) = (
        vertex(left, bottom, u_left, v1),
        vertex(right, bottom, u_right, v1),
        vertex(right, top, u_right, v0),
        vertex(left, top, u_left, v0),
    );
    out.extend_from_slice(&[a, b, c, a, c, d]);
}

// ----------------------------------------------------------------------------------- entities

/// The sprite id of an entity kind of the Caveland game mode, the names the server sends in
/// `ThingState::kind`, and whether it has facing directions and a walking cycle (the robots).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EntityArt {
    pub id: u8,
    pub walks: bool,
    /// Sprites per direction in the walking cycle.
    pub steps: u32,
}

pub fn entity_art(kind: &str) -> Option<EntityArt> {
    let item = |id| Some(EntityArt { id, walks: false, steps: 1 });
    match kind {
        "robot" => Some(EntityArt { id: 45, walks: true, steps: 5 }),
        "friendly_robot" => Some(EntityArt { id: 58, walks: true, steps: 5 }),
        "money" => item(20),
        "minecart" => item(42),
        // The ids of `CollectibleType` in the Java game.
        "Rails" => item(16),
        "Wood" => item(46),
        "Explosives" => item(47),
        "Ironore" => item(48),
        "Coal" => item(49),
        "Cristall" => item(50),
        "Sulfur" => item(51),
        "Stone" => item(52),
        "Toolkit" => item(53),
        "Torch" => item(54),
        "Iron" => item(55),
        "Powercable" => item(57),
        "DropSpaceFlagConstructionKit" => item(23),
        _ => None,
    }
}

/// The direction number 0 to 7 of a facing, as `Robot.playAttackAnimation` and `MovableEntity`
/// choose it. `facing` is a unit vector in the Java screen-aligned game space (x right, y down).
pub fn direction8(facing: [f32; 2]) -> u32 {
    let (x, y) = (facing[0], facing[1]);
    let sin60 = (std::f32::consts::PI / 3.0).sin();
    if x < -sin60 {
        1
    } else if x < -0.5 {
        if y < 0.0 { 2 } else { 0 }
    } else if x < 0.5 {
        if y < 0.0 { 3 } else { 7 }
    } else if x < sin60 {
        if y < 0.0 { 4 } else { 6 }
    } else {
        5
    }
}

/// A movement in the ground frame as the Java game's screen-aligned game-space direction: screen
/// right is `x - y` and screen down is `(x + y)` in game units (the depth axis is squashed to half
/// on screen). `None` when standing still.
pub fn facing_of(vx: f32, vy: f32) -> Option<[f32; 2]> {
    let (x, y) = (vx - vy, vx + vy);
    let length = (x * x + y * y).sqrt();
    (length > 1e-4).then(|| [x / length, y / length])
}

/// The `diff/w/<n>` frame of the player: 8 steps per direction, directions in the order of
/// `Ejira.updateSprite` (south, south-east, east, north-east, north, north-west, west, south-west).
pub fn player_frame(facing: [f32; 2], cycle: f32) -> u32 {
    let direction = match direction8(facing) {
        // `direction8` numbers like the robots (west 1, north-west 2...); Ejira starts at south = 0.
        0 => 7,
        1 => 6,
        2 => 5,
        3 => 4,
        4 => 3,
        5 => 2,
        6 => 1,
        _ => 0,
    };
    let step = ((cycle.rem_euclid(1.0)) * 8.0) as u32 % 8;
    direction * 8 + 1 + step
}

/// The robot sprite value: its direction plus 8 per walking step (`MovableEntity`).
pub fn robot_value(facing: [f32; 2], cycle: f32, steps: u32) -> u32 {
    direction8(facing) + 8 * ((cycle.rem_euclid(1.0) * steps as f32) as u32 % steps.max(1))
}

/// Which way something faces and how far through its walking cycle it is, worked out from where it
/// has been seen (the server sends positions only).
#[derive(Debug, Clone, PartialEq)]
pub struct Anim {
    last: Option<Vec3>,
    /// Unit vector in the Java game space (x right, y down); south until it first moves.
    pub facing: [f32; 2],
    /// Position in the walking cycle, 0 to 1.
    pub cycle: f32,
}

/// Slower than this (blocks per second) is standing still.
const MOVING_SPEED: f32 = 0.3;
/// How fast the facing turns towards the direction of travel (per second, exponential).
const TURN_RATE: f32 = 14.0;

impl Default for Anim {
    fn default() -> Self {
        Anim { last: None, facing: [0.0, 1.0], cycle: 0.0 }
    }
}

impl Anim {
    /// Advance by `dt` seconds with the thing now at `pos`. One walking cycle covers `stride` blocks.
    pub fn update(&mut self, pos: Vec3, dt: f32, stride: f32) {
        let Some(last) = self.last.replace(pos) else { return };
        if dt <= 0.0 {
            return;
        }
        let (dx, dy) = (pos.x - last.x, pos.y - last.y);
        let distance = (dx * dx + dy * dy).sqrt();
        match facing_of(dx, dy).filter(|_| distance / dt > MOVING_SPEED && distance < 3.0) {
            Some(target) => {
                let k = 1.0 - (-TURN_RATE * dt).exp();
                let (x, y) = (self.facing[0] + (target[0] - self.facing[0]) * k, self.facing[1] + (target[1] - self.facing[1]) * k);
                let length = (x * x + y * y).sqrt();
                // Turning exactly backwards cancels out; then just flip.
                self.facing = if length > 1e-3 { [x / length, y / length] } else { target };
                self.cycle = (self.cycle + distance / stride).fract();
            }
            None => self.cycle = 0.0, // standing: the first frame of the cycle
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn real() -> Sprites {
        let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/sprites/sprites.atlas")).unwrap();
        Sprites::new(Atlas::parse(&text).unwrap())
    }

    #[test]
    fn blocks_with_three_faces_and_single_pictures_are_told_apart() {
        let sprites = real();
        let grass = sprites.block(1, 0).expect("grass");
        let BlockLook::Sided { left, top, right } = grass else { panic!("{grass:?}") };
        assert_eq!(sprites.region(left).name, "b1-0-0");
        assert_eq!(sprites.region(top).name, "b1-0-1");
        assert_eq!(sprites.region(right).name, "b1-0-2");
        let BlockLook::Single(tree) = sprites.block(72, 0).expect("tree") else { panic!("tree is one picture") };
        assert_eq!(sprites.region(tree).name, "b72-0");
    }

    #[test]
    fn a_value_without_art_looks_like_value_zero_and_an_unknown_id_has_no_look() {
        let sprites = real();
        assert_eq!(sprites.block(1, 200), sprites.block(1, 0));
        // stone has an extra variant of its own
        assert_ne!(sprites.block(3, 1), sprites.block(3, 0));
        assert_eq!(sprites.block(99, 0), None);
        assert_eq!(sprites.block(0, 0).map(|_| ()), Some(()), "b0 is the Java placeholder, air is never meshed");
    }

    #[test]
    fn every_entity_kind_the_caveland_mode_sends_has_art_in_the_atlas() {
        let sprites = real();
        for kind in [
            "robot", "friendly_robot", "money", "minecart", "Rails", "Wood", "Explosives", "Ironore", "Coal", "Cristall",
            "Sulfur", "Stone", "Toolkit", "Torch", "Iron", "Powercable",
        ] {
            let art = entity_art(kind).unwrap_or_else(|| panic!("{kind} has no art"));
            assert!(sprites.entity(art.id, 0).is_some(), "{kind}: e{}-0", art.id);
        }
        assert_eq!(entity_art("something new"), None);
    }

    #[test]
    fn the_ids_match_the_collectibles_of_the_game_rules() {
        use caveland_sim::CollectibleType;
        for kind in CollectibleType::ALL {
            let art = entity_art(kind.name()).unwrap_or_else(|| panic!("{} has no art", kind.name()));
            assert_eq!(art.id, kind.sprite_id(), "{}", kind.name());
        }
    }

    #[test]
    fn every_walking_frame_of_every_direction_exists() {
        let sprites = real();
        for frame in 1..=64 {
            assert!(sprites.player(frame).is_some(), "diff/w/{frame}");
        }
        for value in 0..40 {
            assert!(sprites.entity(45, value).is_some(), "robot sprite e45-{value}");
        }
    }

    #[test]
    fn facing_picks_the_same_eight_directions_as_the_java_game() {
        assert_eq!(direction8([0.0, 1.0]), 7, "south");
        assert_eq!(direction8([0.0, -1.0]), 3, "north");
        assert_eq!(direction8([1.0, 0.0]), 5, "east");
        assert_eq!(direction8([-1.0, 0.0]), 1, "west");
        assert_eq!(direction8([0.7, -0.7]), 4);
        assert_eq!(direction8([0.7, 0.7]), 6);
        assert_eq!(direction8([-0.7, -0.7]), 2);
        assert_eq!(direction8([-0.7, 0.7]), 0);
    }

    #[test]
    fn walking_down_the_screen_faces_south_and_the_player_frames_cycle_through_eight_steps() {
        // +x +y in the ground frame is straight down the screen
        let south = facing_of(1.0, 1.0).unwrap();
        assert!((south[0]).abs() < 1e-6 && (south[1] - 1.0).abs() < 1e-6);
        assert_eq!(player_frame(south, 0.0), 1, "south is the first direction, first step");
        assert_eq!(player_frame(south, 0.99), 8);
        let west = facing_of(-1.0, 1.0).unwrap();
        assert_eq!(player_frame(west, 0.0), 6 * 8 + 1);
        let frames: Vec<u32> = (0..8).map(|i| player_frame(south, i as f32 / 8.0 + 0.01)).collect();
        assert_eq!(frames, (1..=8).collect::<Vec<_>>());
        assert_eq!(facing_of(0.0, 0.0), None);
        assert_eq!(player_frame(south, -0.25), player_frame(south, 0.75), "the cycle wraps");
    }

    #[test]
    fn walking_turns_the_facing_and_advances_the_cycle_and_standing_resets_it() {
        let mut anim = Anim::default();
        anim.update(Vec3::ZERO, 0.016, 1.6);
        assert_eq!((anim.facing, anim.cycle), ([0.0, 1.0], 0.0), "the first sighting only records the place");
        // walk along -x+y: screen left. 4 blocks per second for a second
        let mut pos = Vec3::ZERO;
        for _ in 0..60 {
            pos += Vec3::new(-4.0 / 60.0, 4.0 / 60.0, 0.0);
            anim.update(pos, 1.0 / 60.0, 1.6);
        }
        assert!(anim.facing[0] < -0.99, "turned to face west: {:?}", anim.facing);
        let travelled = 4.0 * 2.0_f32.sqrt(); // a second at 4 blocks per axis step
        assert!((anim.cycle - (travelled / 1.6).fract()).abs() < 0.05, "{travelled} blocks are {} cycles: {}", travelled / 1.6, anim.cycle);
        for _ in 0..3 {
            anim.update(pos, 1.0 / 60.0, 1.6);
        }
        assert_eq!(anim.cycle, 0.0, "standing still");
        assert!(anim.facing[0] < -0.99, "and it keeps facing the way it went");
    }

    #[test]
    fn a_teleport_does_not_spin_or_run_the_cycle() {
        let mut anim = Anim::default();
        anim.update(Vec3::ZERO, 0.016, 1.6);
        anim.update(Vec3::new(40.0, 0.0, 0.0), 0.016, 1.6);
        assert_eq!((anim.facing, anim.cycle), ([0.0, 1.0], 0.0));
    }

    #[test]
    fn a_robots_value_is_its_direction_plus_eight_per_step() {
        assert_eq!(robot_value([0.0, 1.0], 0.0, 5), 7);
        assert_eq!(robot_value([0.0, 1.0], 0.5, 5), 7 + 8 * 2);
        assert_eq!(robot_value([1.0, 0.0], 0.99, 5), 5 + 8 * 4);
    }

    #[test]
    fn a_face_is_textured_with_its_own_sprite_corner_to_corner() {
        let sprites = real();
        let BlockLook::Sided { left, top, right } = sprites.block(1, 0).unwrap() else { panic!() };
        // One block at the origin, faces as `mesh.rs` builds them.
        let (x0, x1, y0, y1, z0, z1) = (-0.5, 0.5, -0.5, 0.5, 0.0, 1.0);
        let faces = [
            (left, [[x0, y1, z0], [x1, y1, z0], [x1, y1, z1], [x0, y1, z1]]),
            (top, [[x0, y0, z1], [x1, y0, z1], [x1, y1, z1], [x0, y1, z1]]),
            (right, [[x1, y0, z0], [x1, y1, z0], [x1, y1, z1], [x1, y0, z1]]),
        ];
        for (index, corners) in faces {
            let region = sprites.region(index);
            let page = &sprites.atlas.pages[region.page];
            let uvs = face_uvs(&sprites.atlas, region, corners);
            let (u0, u1) = (region.x as f32 / page.width as f32, (region.x + region.w) as f32 / page.width as f32);
            let (v0, v1) = (region.y as f32 / page.height as f32, (region.y + region.h) as f32 / page.height as f32);
            for uv in uvs {
                assert!(uv[0] >= u0 - 1e-6 && uv[0] <= u1 + 1e-6, "{}: u {} outside {u0}..{u1}", region.name, uv[0]);
                assert!(uv[1] >= v0 - 1e-6 && uv[1] <= v1 + 1e-6, "{}: v {} outside {v0}..{v1}", region.name, uv[1]);
            }
            let (min_u, max_u) = (uvs.iter().map(|p| p[0]).fold(f32::MAX, f32::min), uvs.iter().map(|p| p[0]).fold(f32::MIN, f32::max));
            let (min_v, max_v) = (uvs.iter().map(|p| p[1]).fold(f32::MAX, f32::min), uvs.iter().map(|p| p[1]).fold(f32::MIN, f32::max));
            assert!((min_u - u0).abs() < 1e-5 && (max_u - u1).abs() < 1e-5, "{}: the corners span the sprite's width", region.name);
            assert!((min_v - v0).abs() < 1e-5 && (max_v - v1).abs() < 1e-5, "{}: and its height", region.name);
        }
    }

    #[test]
    fn the_left_face_sprite_is_sheared_like_the_projected_face() {
        // The art of the left face: left edge higher (rows 0 to 123), right edge lower (49 to 172).
        // So the corner of the face that projects to the upper left must get a sprite row near the top.
        let sprites = real();
        let BlockLook::Sided { left, .. } = sprites.block(1, 0).unwrap() else { panic!() };
        let region = sprites.region(left);
        let corners = [[-0.5, 0.5, 0.0], [0.5, 0.5, 0.0], [0.5, 0.5, 1.0], [-0.5, 0.5, 1.0]];
        let screen = corners.map(project);
        assert!(screen[3][1] < screen[2][1], "the left top corner is higher on screen than the right top corner");
        let uvs = face_uvs(&sprites.atlas, region, corners);
        let page = &sprites.atlas.pages[region.page];
        let row = |uv: [f32; 2]| uv[1] * page.height as f32 - region.y as f32;
        assert!(row(uvs[3]).abs() < 0.5, "upper left corner at the top row of the sprite, got {}", row(uvs[3]));
        assert!((row(uvs[1]) - 172.5).abs() < 1.0, "lower right corner at the bottom row, got {}", row(uvs[1]));
    }

    #[test]
    fn a_billboard_keeps_one_depth_and_spans_the_screen_rectangle_asked_for() {
        let anchor = Vec3::new(3.0, 2.0, 5.0);
        let base = depth(anchor);
        let at_screen = |dx: f32, dy: f32| {
            let p = billboard_point(anchor, dx, dy, 0.0);
            (project(p), depth(Vec3::from(p)))
        };
        let origin = project(anchor.to_array());
        for (dx, dy) in [(-100.0, -200.0), (100.0, -200.0), (100.0, 40.0), (-100.0, 40.0), (0.0, 0.0)] {
            let (screen, d) = at_screen(dx, dy);
            assert!((screen[0] - origin[0] - dx).abs() < 0.05, "x of ({dx}, {dy}): {}", screen[0] - origin[0]);
            assert!((screen[1] - origin[1] - dy).abs() < 0.05, "y of ({dx}, {dy}): {}", screen[1] - origin[1]);
            assert!((d - base).abs() < 1e-3, "depth of ({dx}, {dy}) differs by {}", d - base);
        }
    }

    #[test]
    fn the_bias_moves_a_billboard_towards_the_viewer_without_moving_it_on_screen() {
        let anchor = Vec3::new(3.0, 2.0, 5.0);
        let plain = billboard_point(anchor, 20.0, -30.0, 0.0);
        let biased = billboard_point(anchor, 20.0, -30.0, 0.3);
        let (a, b) = (project(plain), project(biased));
        assert!((a[0] - b[0]).abs() < 0.05 && (a[1] - b[1]).abs() < 0.05, "{a:?} vs {b:?}");
        assert!((depth(Vec3::from(biased)) - depth(Vec3::from(plain)) - 0.3).abs() < 1e-3);
    }

    #[test]
    fn a_billboard_stands_on_the_footprint_tip_with_its_box_centred() {
        let sprites = real();
        let region = sprites.entity(46, 0).expect("wood");
        assert_eq!((region.orig_w, region.orig_h), (200, 223));
        let anchor = Vec3::new(0.0, 0.0, 0.0);
        let mut out = Vec::new();
        billboard(&mut out, &sprites.atlas, region, anchor, FOOTPRINT_TIP, false, [1.0; 3]);
        assert_eq!(out.len(), 6);
        let screen: Vec<[f32; 2]> = out.iter().map(|v| project(v.position)).collect();
        let min_y = screen.iter().map(|p| p[1]).fold(f32::MAX, f32::min);
        let max_y = screen.iter().map(|p| p[1]).fold(f32::MIN, f32::max);
        let min_x = screen.iter().map(|p| p[0]).fold(f32::MAX, f32::min);
        let max_x = screen.iter().map(|p| p[0]).fold(f32::MIN, f32::max);
        // bottom of the art: the original box's bottom (50 below the centre) minus the 70 px the art sits above it
        assert!((max_y - (50.0 - region.offset_y as f32)).abs() < 0.1, "bottom {max_y}");
        assert!((max_y - min_y - region.h as f32).abs() < 0.1, "height");
        assert!((min_x - (-100.0 + region.offset_x as f32)).abs() < 0.1, "left {min_x}");
        assert!((max_x - min_x - region.w as f32).abs() < 0.1, "width");
        assert!(out.iter().all(|v| v.layer == region.page as f32 && v.shade[0] == FACE_SPRITE));
    }

    #[test]
    fn mirroring_swaps_the_texture_sides() {
        let sprites = real();
        let region = sprites.entity(46, 0).unwrap();
        let (mut plain, mut mirrored) = (Vec::new(), Vec::new());
        billboard(&mut plain, &sprites.atlas, region, Vec3::ZERO, FOOTPRINT_TIP, false, [1.0; 3]);
        billboard(&mut mirrored, &sprites.atlas, region, Vec3::ZERO, FOOTPRINT_TIP, true, [1.0; 3]);
        for (a, b) in plain.iter().zip(&mirrored) {
            assert_eq!(a.position, b.position);
            assert_eq!(a.uv[1], b.uv[1]);
        }
        assert_eq!(plain[0].uv[0], mirrored[1].uv[0]);
        assert_eq!(plain[1].uv[0], mirrored[0].uv[0]);
    }

    #[test]
    fn the_players_feet_land_at_the_footprint_centre() {
        let sprites = real();
        for frame in [1u32, 9, 20, 37, 64] {
            let region = sprites.player(frame).unwrap();
            let mut out = Vec::new();
            billboard(&mut out, &sprites.atlas, region, Vec3::ZERO, PLAYER_BOX_BOTTOM, false, [1.0; 3]);
            let bottom = out.iter().map(|v| project(v.position)[1]).fold(f32::MIN, f32::max);
            // a raised foot or a bob moves the lowest pixel of some frames up to a quarter of the body above the feet
            assert!((-20.0..=60.0).contains(&bottom), "frame {frame}: feet {bottom}px below the centre");
        }
    }
}
