//! Soft contact shadows under the sprites (players, robots, collectibles, carts): a blob on the
//! ground that is stretched and pushed away from the sun, so a sprite stands on the terrain instead
//! of floating over it. It replaces the hard flat square of `shadow.rs` while it is on.
//!
//! A blob is two fans of triangles laid just above the surface, unlit (`FACE_UNLIT`) and see-through
//! per vertex, so the opacity falls off smoothly from the middle to the rim without a texture:
//!
//! * the **shadow**: an ellipse along the sun's ground direction, away from the sun, as long as the
//!   sprite's height times `cot(sun elevation)` asks for (at most [`MAX_REACH`] heights), faint, and
//!   faint at night, where it only keeps its roundness (the real sun shadow is the terrain's job);
//! * the **contact** spot: a small round core right under the feet that is always there.
//!
//! The opacity fades with the height above the ground (a jump or a flying thing, as
//! `wurfel_sim::entity::shadow`). Where the ground under the pushed-away centre is at another height
//! than under the feet the blob stays round under the feet (a flat decal across a ledge would hover or
//! sink into the block).
//!
//! What sprites do to the sun's shadow field of the terrain (casting) is not here; see the README.

use glam::Vec3;
use wurfel_sim::entity::shadow::shadow_of;
use wurfel_sim::World;

use crate::mesh::{Vertex, FACE_UNLIT};
use crate::sprites::Sprites;

/// Colour of the blob.
const COLOR: [f32; 3] = [0.03, 0.03, 0.06];
/// Above the surface, so a blob does not fight the ground's own face for the depth test; the contact
/// core lies a little higher than the shadow it sits in.
const LIFT: f32 = 0.02;
const LIFT_CORE: f32 = 0.03;
/// The longest a shadow is, in heights of the sprite.
pub const MAX_REACH: f32 = 2.5;
/// Opacity of the stretched shadow in the dark and with the sun high.
const SHADOW_ALPHA: (f32, f32) = (0.2, 0.52);
/// Opacity of the contact core.
const CORE_ALPHA: (f32, f32) = (0.28, 0.4);
/// Corners of the rim of the shadow and of the core.
const SEGMENTS: usize = 12;
const CORE_SEGMENTS: usize = 8;
/// Most blobs of one frame (the dynamic buffer is shared with everything else that moves).
pub const MAX_BLOBS: usize = 48;

/// Settings, from the menu (`spriteShadows`) or the page address (`?spriteshadows=0`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Settings {
    pub enabled: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { enabled: true }
    }
}

/// What casts a blob: where its feet are and how big it is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Caster {
    /// The middle of the feet, isometric blocks.
    pub position: Vec3,
    /// Half the width of the footprint in blocks.
    pub radius: f32,
    /// Height of the sprite in blocks.
    pub height: f32,
}

impl Caster {
    pub fn player(position: Vec3) -> Caster {
        Caster { position, radius: 0.3, height: wurfel_sim::player::PLAYER_HEIGHT }
    }

    /// The caster of a thing of the game mode, sized by its picture; `None` for a kind without art.
    pub fn thing(sprites: &Sprites, kind: &str, position: Vec3) -> Option<Caster> {
        let art = crate::sprites::entity_art(kind)?;
        let region = sprites.entity(art.id, art.value).or_else(|| sprites.entity(art.id, 0))?;
        // 200 pixels across a block's diagonal, 122 pixels up a block.
        Some(Caster {
            position,
            radius: (region.w as f32 / 200.0 * 0.55).clamp(0.18, 0.8),
            height: (region.h as f32 / crate::sprites::SCREEN_Z).clamp(0.15, 2.5),
        })
    }
}

/// The sun for a frame: the unit vector towards it (isometric frame) and how strong its shadows are
/// (`sunshadow::strength`: 0 at night, 1 with the sun high).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sun {
    pub direction: Vec3,
    pub strength: f32,
}

/// The blobs of one frame: the sun they all share and a count that keeps them within [`MAX_BLOBS`].
#[derive(Debug, Clone, Copy)]
pub struct Blobs {
    pub sun: Sun,
    drawn: usize,
}

impl Blobs {
    pub fn new(sun: Sun) -> Blobs {
        Blobs { sun, drawn: 0 }
    }

    /// [`push`] one more blob, unless the frame's budget is used up (then `false`).
    pub fn add(&mut self, out: &mut Vec<Vertex>, world: &World, caster: &Caster) -> bool {
        if self.drawn >= MAX_BLOBS {
            return false;
        }
        let drawn = push(out, world, caster, &self.sun);
        self.drawn += drawn as usize;
        drawn
    }
}

/// Where a blob is, as the two axes of its ellipse and its centre on the ground.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Shape {
    pub centre: (f32, f32),
    /// Unit vector of the long axis (away from the sun) and the half lengths along it and across.
    pub axis: (f32, f32),
    pub half_long: f32,
    pub half_wide: f32,
    /// How far the sprite's shadow reaches beyond its feet, in blocks (0 when the sun is up or down).
    pub reach: f32,
}

/// The ellipse of the shadow of `caster` in sunlight `sun`.
pub fn shape(caster: &Caster, sun: &Sun) -> Shape {
    let (sx, sy) = (sun.direction.x, sun.direction.y);
    let horizontal = (sx * sx + sy * sy).sqrt();
    // Away from the sun; a sun straight overhead has no direction and the blob stays round.
    let axis = if horizontal > 1e-4 { (-sx / horizontal, -sy / horizontal) } else { (1.0, 0.0) };
    // With the sun under the horizon the shadow of the real light is gone and the blob is round, so
    // it fades in with the height of the sun instead of snapping.
    let up = (sun.direction.z / 0.2).clamp(0.0, 1.0);
    let cot = if sun.direction.z > 0.02 { (horizontal / sun.direction.z).min(MAX_REACH) } else { MAX_REACH };
    let reach = (caster.height * cot * up).min(caster.height * MAX_REACH);
    let r = caster.radius * 1.15;
    Shape {
        centre: (caster.position.x + axis.0 * reach * 0.4, caster.position.y + axis.1 * reach * 0.4),
        axis,
        half_long: r + reach * 0.45,
        half_wide: r / (1.0 + 0.1 * reach),
        reach,
    }
}

/// Add the blob of `caster`. Returns whether anything was drawn (nothing when it is inside the ground).
pub fn push(out: &mut Vec<Vertex>, world: &World, caster: &Caster, sun: &Sun) -> bool {
    let Some(ground) = shadow_of(world, caster.position) else { return false };
    if ground.alpha <= 0.0 {
        return false;
    }
    let mut shape = shape(caster, sun);
    // A flat decal across a ledge would float or sink: keep the centre under the feet then.
    let moved = Vec3::new(shape.centre.0, shape.centre.1, caster.position.z);
    let level = shadow_of(world, moved).map(|g| g.position.z);
    if level != Some(ground.position.z) {
        shape.centre = (caster.position.x, caster.position.y);
        shape.half_long = shape.half_long.min(caster.radius * 1.15 + shape.reach * 0.2);
    }
    let z = ground.position.z;
    let lerp = |range: (f32, f32)| range.0 + (range.1 - range.0) * sun.strength.clamp(0.0, 1.0);
    // The long shadow: soft, the middle a little darker than the rim.
    fan(out, [shape.centre.0, shape.centre.1, z + LIFT], &shape, SEGMENTS, &[(0.5, 0.62), (1.0, 0.0)], lerp(SHADOW_ALPHA) * ground.alpha);
    // The contact core under the feet: round, small.
    let core = Shape {
        centre: (caster.position.x, caster.position.y),
        axis: shape.axis,
        half_long: caster.radius * 0.7,
        half_wide: caster.radius * 0.7,
        reach: 0.0,
    };
    fan(out, [core.centre.0, core.centre.1, z + LIFT_CORE], &core, CORE_SEGMENTS, &[(1.0, 0.0)], lerp(CORE_ALPHA) * ground.alpha);
    true
}

/// A fan of triangles over `shape`: a middle vertex with `peak` opacity and rings at the given
/// `(fraction of the radius, fraction of the opacity)`, the first ring joined to the middle, each
/// ring to the next by a strip.
fn fan(out: &mut Vec<Vertex>, middle: [f32; 3], shape: &Shape, segments: usize, rings: &[(f32, f32)], peak: f32) {
    let vertex = |position: [f32; 3], alpha: f32| Vertex::flat(position, COLOR, [FACE_UNLIT, 1.0 - alpha.clamp(0.0, 1.0)], [0.0; 3]);
    let across = (-shape.axis.1, shape.axis.0);
    let point = |angle: f32, t: f32| {
        let (sin, cos) = angle.sin_cos();
        let (a, b) = (shape.half_long * t * cos, shape.half_wide * t * sin);
        [middle[0] + shape.axis.0 * a + across.0 * b, middle[1] + shape.axis.1 * a + across.1 * b, middle[2]]
    };
    let angle = |k: usize| k as f32 / segments as f32 * std::f32::consts::TAU;
    let centre = vertex(middle, peak);
    let mut previous: Option<(f32, f32)> = None;
    for &(t, alpha) in rings {
        for k in 0..segments {
            let (a0, a1) = (angle(k), angle(k + 1));
            let (p0, p1) = (vertex(point(a0, t), peak * alpha), vertex(point(a1, t), peak * alpha));
            match previous {
                None => out.extend_from_slice(&[centre, p0, p1]),
                Some((inner, alpha_inner)) => {
                    let (q0, q1) = (vertex(point(a0, inner), peak * alpha_inner), vertex(point(a1, inner), peak * alpha_inner));
                    out.extend_from_slice(&[q0, p0, p1, q0, p1, q1]);
                }
            }
        }
        previous = Some((t, alpha));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wurfel_sim::block::id;
    use wurfel_sim::{AirGenerator, Block};

    /// A grass floor (its top at height 3) over a few blocks, and east of the isometric `gx = 1.5` a
    /// ledge one block higher.
    fn world() -> World {
        let mut world = World::new(AirGenerator);
        for x in -12..12 {
            for y in -12..12 {
                world.set(x, y, 2, Block::new(id::GRASS, 0));
                if wurfel_sim::grid::to_iso(x, y).0 > 1.5 {
                    world.set(x, y, 3, Block::new(id::STONE, 0));
                }
            }
        }
        world
    }

    fn sun(x: f32, y: f32, z: f32) -> Sun {
        Sun { direction: Vec3::new(x, y, z).normalize(), strength: 1.0 }
    }

    fn at(x: f32, y: f32, z: f32) -> Caster {
        Caster::player(Vec3::new(x, y, z))
    }

    #[test]
    fn the_shadow_falls_away_from_the_sun_and_is_longer_when_the_sun_is_low() {
        let high = shape(&at(0.0, 0.0, 3.0), &sun(0.2, 0.0, 1.0));
        let low = shape(&at(0.0, 0.0, 3.0), &sun(1.0, 0.0, 0.5));
        assert!(low.reach > high.reach * 3.0, "{} vs {}", low.reach, high.reach);
        assert!(low.centre.0 < -0.1 && low.centre.1.abs() < 1e-4, "the sun is at +x, the shadow goes to -x: {:?}", low.centre);
        assert_eq!(low.axis.0.signum(), -1.0);
        assert!(low.half_long > low.half_wide * 2.0, "elongated: {low:?}");
        let overhead = shape(&at(0.0, 0.0, 3.0), &sun(0.0, 0.0, 1.0));
        assert!(overhead.reach < 1e-3 && (overhead.half_long - overhead.half_wide).abs() < 1e-3, "round: {overhead:?}");
        assert!(shape(&at(0.0, 0.0, 3.0), &sun(1.0, 0.0, 0.001)).reach <= 1.4 * MAX_REACH + 1e-3, "a grazing sun is capped");
    }

    #[test]
    fn at_night_the_blob_is_round_and_faint_but_still_there() {
        let night = Sun { direction: Vec3::new(0.3, 0.2, -0.9).normalize(), strength: 0.0 };
        let s = shape(&at(0.0, 0.0, 3.0), &night);
        assert!(s.reach < 1e-4 && (s.half_long - s.half_wide).abs() < 1e-4, "{s:?}");
        let world = world();
        let (mut dark, mut lit) = (Vec::new(), Vec::new());
        assert!(push(&mut dark, &world, &at(0.0, 0.0, 3.0), &night));
        assert!(push(&mut lit, &world, &at(0.0, 0.0, 3.0), &sun(1.0, 0.0, 1.0)));
        let peak = |v: &[Vertex]| v.iter().map(|v| 1.0 - v.shade[1]).fold(0.0f32, f32::max);
        assert!(peak(&dark) > 0.15 && peak(&dark) < peak(&lit), "{} {}", peak(&dark), peak(&lit));
    }

    #[test]
    fn a_blob_lies_flat_just_above_the_ground_and_fades_to_nothing_at_its_rim() {
        let world = world();
        let mut out = Vec::new();
        assert!(push(&mut out, &world, &at(0.0, 0.0, 3.0), &sun(1.0, 0.5, 0.8)));
        assert!(out.len() > 60 && out.len() % 3 == 0, "{}", out.len());
        assert!(out.iter().all(|v| v.shade[0] == FACE_UNLIT && v.layer < 0.0));
        assert!(out.iter().all(|v| v.position[2] > 3.0 && v.position[2] < 3.05), "on the grass top at 3");
        let alphas: Vec<f32> = out.iter().map(|v| 1.0 - v.shade[1]).collect();
        assert!(alphas.iter().any(|&a| a < 1e-6), "the rim is invisible");
        assert!(alphas.iter().cloned().fold(0.0, f32::max) > 0.3, "the middle is dark");
        assert!(alphas.iter().all(|&a| (0.0..=1.0).contains(&a)));
    }

    #[test]
    fn a_blob_fades_with_height_and_is_gone_inside_the_ground_or_far_above() {
        let world = world();
        let peak = |z: f32| {
            let mut out = Vec::new();
            push(&mut out, &world, &at(0.0, 0.0, z), &sun(0.3, 0.0, 1.0));
            out.iter().map(|v| 1.0 - v.shade[1]).fold(0.0f32, f32::max)
        };
        assert!(peak(3.0) > peak(4.0) && peak(4.0) > peak(4.8), "{} {} {}", peak(3.0), peak(4.0), peak(4.8));
        assert_eq!(peak(7.0), 0.0, "high above the ground: no blob");
        let mut out = Vec::new();
        assert!(!push(&mut out, &world, &at(0.0, 0.0, 2.5), &sun(0.3, 0.0, 1.0)), "inside the block");
        assert!(out.is_empty());
    }

    #[test]
    fn a_ledge_keeps_the_blob_under_the_feet() {
        let world = world();
        // Feet at gx = 1 on the lower floor; the sun is at -x so the shadow is thrown towards +x, onto the higher floor.
        let towards_ledge = Sun { direction: Vec3::new(-1.0, 0.0, 0.5).normalize(), strength: 1.0 };
        let mut out = Vec::new();
        assert!(push(&mut out, &world, &at(1.0, 0.0, 3.0), &towards_ledge));
        let xs = out.iter().map(|v| v.position[0]);
        let (lo, hi) = (xs.clone().fold(f32::MAX, f32::min), xs.fold(f32::MIN, f32::max));
        assert!(((lo + hi) / 2.0 - 1.0).abs() < 0.2, "centred under the feet: {lo} {hi}");
        // On open floor the same sun throws it over.
        let mut open = Vec::new();
        assert!(push(&mut open, &world, &at(-2.0, 0.0, 3.0), &towards_ledge));
        let mid = open.iter().map(|v| v.position[0]).sum::<f32>() / open.len() as f32;
        assert!(mid > -1.9, "pushed to +x: {mid}");
    }

    #[test]
    fn a_frame_draws_at_most_max_blobs() {
        let world = world();
        let mut blobs = Blobs::new(sun(1.0, 0.0, 1.0));
        let mut out = Vec::new();
        let drawn = (0..MAX_BLOBS + 10).filter(|_| blobs.add(&mut out, &world, &at(0.0, 0.0, 3.0))).count();
        assert_eq!(drawn, MAX_BLOBS);
    }

    #[test]
    fn things_are_sized_by_their_pictures() {
        let sprites = Sprites::new(crate::atlas::Atlas::parse(include_str!("../assets/sprites/sprites.atlas")).expect("atlas"));
        let cart = Caster::thing(&sprites, "minecart", Vec3::ZERO).expect("the cart has art");
        let coal = Caster::thing(&sprites, "Coal", Vec3::ZERO).expect("coal has art");
        assert!(cart.radius > coal.radius && cart.height > coal.height, "{cart:?} {coal:?}");
        assert!(Caster::thing(&sprites, "something new", Vec3::ZERO).is_none());
    }
}
