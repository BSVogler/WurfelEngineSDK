//! `EntityShadow`: the dark spot on the ground under an entity that tells how high it is.
//!
//! The Java shadow was an entity (sprite 6) that followed its parent as a component. Nothing in it
//! needs state, so here it is a query: [`shadow_of`] finds where the spot lies and
//! [`Shadow::layers`] says how to draw it. The renderer (`wurfel-web`) calls it for every entity
//! that casts a shadow, so it works the same for entities simulated locally and for things the
//! client only gets snapshots of. (Shadows cast by point lights are a different thing: see
//! [`crate::light`].)

use glam::Vec3;

use super::physics::block_at;
use crate::World;

/// A shadow on the surface below an entity.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Shadow {
    /// The centre of the spot, on the top of the surface.
    pub position: Vec3,
    /// Opacity of the full-size spot, 0 to 1: dark close to the ground, fading with height.
    pub alpha: f32,
}

/// One quad to draw: the spot is scaled around its centre and drawn in this opacity.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Layer {
    pub scale: f32,
    pub alpha: f32,
}

/// Grey of the spot (the Java colour was `.5, .5, .5`).
pub const COLOR: [f32; 3] = [0.5, 0.5, 0.5];
/// Opacity below which the extra small spot is added.
const SMALL_BELOW: f32 = 0.9;
const SMALL_SCALE: f32 = 0.5;
const SMALL_ALPHA: f32 = 0.2;

/// Where the shadow of something at `position` falls: straight down to the first block that is
/// not transparent. `None` if the position is below that surface (inside the ground), where the
/// Java shadow hid itself.
pub fn shadow_of(world: &World, position: Vec3) -> Option<Shadow> {
    // Look down from the entity, one block at a time.
    let mut probe = position;
    while probe.z > 0.0 && world.blocks().is_transparent(block_at(world, probe)) {
        probe.z -= 1.0;
    }
    // Lower than the first block the surface is the bottom of the world.
    let surface = if position.z < 1.0 { 0.0 } else { probe.z.floor() + 1.0 };
    if position.z < surface {
        return None;
    }
    let alpha = (1.0 - (position.z - surface) / 2.0 + 0.1).clamp(0.0, 1.0);
    Some(Shadow { position: Vec3::new(position.x, position.y, surface), alpha })
}

impl Shadow {
    /// The quads to draw, back to front. High above the ground the spot is faint, so a small
    /// second spot keeps it visible (the Java "always visible smaller shadow").
    pub fn layers(&self) -> Vec<Layer> {
        let mut layers = vec![Layer { scale: 1.0, alpha: self.alpha }];
        if self.alpha < SMALL_BELOW {
            layers.push(Layer { scale: SMALL_SCALE, alpha: SMALL_ALPHA });
        }
        layers
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block::id;
    use crate::{AirGenerator, Block};

    fn world() -> World {
        let mut world = World::new(AirGenerator);
        for x in 0..3 {
            for y in 0..3 {
                world.set(x, y, 0, Block::new(id::SAND, 0));
            }
        }
        world
    }

    fn above(z: f32) -> Vec3 {
        let (gx, gy) = crate::grid::to_iso(1, 1);
        Vec3::new(gx, gy, z)
    }

    #[test]
    fn the_shadow_lies_on_top_of_the_first_solid_block_below() {
        let shadow = shadow_of(&world(), above(3.5)).unwrap();
        assert_eq!(shadow.position, above(1.0), "sand at z = 0 has its top at 1");
    }

    #[test]
    fn a_shadow_falls_on_a_roof_rather_than_the_floor() {
        let mut world = world();
        let (x, y) = (1, 1);
        world.set(x, y, 3, Block::new(id::STONE, 0));
        assert_eq!(shadow_of(&world, above(6.5)).unwrap().position.z, 4.0);
        // Under the roof it is the floor again.
        assert_eq!(shadow_of(&world, above(2.5)).unwrap().position.z, 1.0);
    }

    #[test]
    fn water_and_air_let_the_shadow_through() {
        let mut world = world();
        world.set(1, 1, 1, Block::new(id::WATER, 0));
        assert_eq!(shadow_of(&world, above(2.5)).unwrap().position.z, 1.0);
    }

    #[test]
    fn the_shadow_is_darkest_on_the_ground_and_fades_with_height() {
        let world = world();
        let near = shadow_of(&world, above(1.0)).unwrap().alpha;
        let mid = shadow_of(&world, above(2.0)).unwrap().alpha;
        let far = shadow_of(&world, above(5.0)).unwrap().alpha;
        assert_eq!(near, 1.0, "1.1 clamped");
        assert!((mid - 0.6).abs() < 1e-5);
        assert_eq!(far, 0.0);
    }

    #[test]
    fn a_faint_shadow_gets_a_small_extra_spot_and_a_dark_one_does_not() {
        let world = world();
        assert_eq!(shadow_of(&world, above(1.0)).unwrap().layers().len(), 1);
        let layers = shadow_of(&world, above(3.0)).unwrap().layers();
        assert_eq!(layers, [Layer { scale: 1.0, alpha: 0.1 }, Layer { scale: 0.5, alpha: 0.2 }]);
    }

    #[test]
    fn inside_the_ground_there_is_no_shadow() {
        let mut world = world();
        world.set(1, 1, 1, Block::new(id::STONE, 0));
        // Inside the stone block: the surface found is its top, above the entity.
        assert_eq!(shadow_of(&world, above(1.5)), None);
    }

    #[test]
    fn below_the_first_block_the_shadow_is_at_the_bottom_of_the_world() {
        let shadow = shadow_of(&World::new(AirGenerator), above(0.4)).unwrap();
        assert_eq!(shadow.position.z, 0.0);
    }
}
