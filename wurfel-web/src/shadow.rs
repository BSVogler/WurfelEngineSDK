//! Draws the ground shadows of [`wurfel_sim::entity::shadow`] (the Java `EntityShadow`) under the
//! things that have one: the Java game gave them to collectibles, the mine cart and the lift
//! basket.
//!
//! The spot is a flat square (the diamond of the isometric ground) just above the surface, not
//! touched by the light engine, and blended with the ground by its opacity (`mesh::set_alpha`).

use glam::Vec3;
use wurfel_sim::entity::shadow::{shadow_of, Shadow};
use wurfel_sim::World;

use crate::mesh::{top_face_unlit_alpha, Vertex};

/// Half the side of the full-size spot in blocks (Java: the shadow sprite, about one block wide).
const HALF: f32 = 0.35;
/// Colour of the spot, laid over the ground by the layer's opacity.
const COLOR: [f32; 3] = [0.04, 0.04, 0.06];
/// The most opaque a spot gets (the Java shadow sprite was not black either).
const MAX_ALPHA: f32 = 0.6;
/// Above the surface, so the spot does not fight the ground's own face for the depth test.
const LIFT: f32 = 0.02;

/// Do things of this kind cast a shadow? (The collectibles, the mine cart and the lift basket; the
/// players get theirs from the renderer.)
pub fn casts_shadow(kind: &str) -> bool {
    matches!(
        kind,
        "minecart" | "lift_basket" | "money" | "Wood" | "Coal" | "Torch" | "Explosives" | "Gunpowder" | "Iron" | "Ironore" | "Cristall"
            | "Sulfur" | "Stone" | "Toolkit" | "Rails" | "Powercable"
    )
}

/// Add the spot(s) of one shadow.
pub fn push(out: &mut Vec<Vertex>, shadow: &Shadow) {
    for layer in shadow.layers() {
        let half = HALF * layer.scale;
        if layer.alpha <= 0.0 {
            continue;
        }
        let Vec3 { x, y, z } = shadow.position;
        top_face_unlit_alpha(out, COLOR, [x - half, x + half, y - half, y + half], z + LIFT, layer.alpha * MAX_ALPHA);
    }
}

/// The shadow of something standing or floating at `position`, if it should be drawn.
pub fn push_under(out: &mut Vec<Vertex>, world: &World, position: Vec3) {
    if let Some(shadow) = shadow_of(world, position) {
        push(out, &shadow);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wurfel_sim::block::id;
    use wurfel_sim::{AirGenerator, Block};

    fn world() -> World {
        let mut world = World::new(AirGenerator);
        world.set(1, 1, 0, Block::new(id::SAND, 0));
        world
    }

    fn above(z: f32) -> Vec3 {
        let (gx, gy) = wurfel_sim::grid::to_iso(1, 1);
        Vec3::new(gx, gy, z)
    }

    #[test]
    fn only_collectibles_carts_and_lifts_cast_shadows() {
        assert!(casts_shadow("minecart") && casts_shadow("Coal") && casts_shadow("lift_basket"));
        assert!(!casts_shadow("robot") && !casts_shadow("portal"));
    }

    #[test]
    fn a_low_shadow_is_one_quad_on_the_ground_and_a_high_one_adds_the_small_spot() {
        let world = world();
        let (mut low, mut high) = (Vec::new(), Vec::new());
        push_under(&mut low, &world, above(1.0));
        push_under(&mut high, &world, above(2.5));
        assert_eq!(low.len(), 6);
        assert_eq!(high.len(), 12);
        assert!(low.iter().all(|v| (v.position[2] - 1.02).abs() < 1e-5), "just above the sand's top");
    }

    #[test]
    fn a_very_high_shadow_keeps_only_the_small_spot() {
        let world = world();
        let mut out = Vec::new();
        push_under(&mut out, &world, above(6.0));
        assert_eq!(out.len(), 6, "only the small spot is left, the large one has no opacity");
    }
}
