//! Mouse picking: which block is under a screen position?
//!
//! Every screen point corresponds to a line through the world along the camera direction. Walking
//! that line from the sky downwards, the first solid block is the one that is visible there.

use wurfel_sim::grid::from_iso;
use wurfel_sim::protocol::ThingState;
use wurfel_sim::{World, CHUNK_SIZE_Z};

use crate::view::View;

/// Screen (px at zoom 1, y down) of a point of the ground frame at height `z`: the projection of
/// `shader.wgsl` for the fixed camera.
#[cfg(test)]
pub fn screen_of(gx: f32, gy: f32, gz: f32) -> (f32, f32) {
    ((gx - gy) * 100.0, (gx + gy) * 50.0 - gz * 122.0)
}

/// The ground point at height `gz` that shows at screen position `(sx, sy)`: the inverse of
/// [`screen_of`] for a fixed height (what dragging a thing along its layer needs).
pub fn ground_at(sx: f32, sy: f32, gz: f32) -> (f32, f32) {
    let diff = sx / 100.0;
    let sum = (sy + gz * 122.0) / 50.0;
    ((sum + diff) / 2.0, (sum - diff) / 2.0)
}

/// [`ground_at`] for a turned camera: the world point that shows at `(sx, sy)`.
pub fn ground_at_view(view: &View, sx: f32, sy: f32, gz: f32) -> (f32, f32) {
    view.unrotate(ground_at(sx, sy, gz))
}

/// How close (screen px at zoom 1) the pointer must be to the middle of a thing to select it.
const THING_RADIUS: f32 = 60.0;

/// The thing under the pointer: the nearest to it on the screen, measured to the middle of its
/// sprite (half a block above its feet). Several things in the same place: the later one wins.
pub fn pick_thing(things: &[ThingState], view: &View, sx: f32, sy: f32) -> Option<u32> {
    things
        .iter()
        .map(|t| {
            let [x, y] = view.screen_position((t.pos[0], t.pos[1]), t.pos[2] + 0.5);
            (t.id, (x - sx).hypot(y - sy))
        })
        .filter(|&(_, distance)| distance <= THING_RADIUS)
        .min_by(|a, b| a.1.total_cmp(&b.1).then(std::cmp::Ordering::Greater))
        .map(|(id, _)| id)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pick {
    /// The visible block under the pointer.
    pub hit: (i32, i32, i32),
    /// The empty cell the ray passed just before it, i.e. where a new block would go.
    pub place: (i32, i32, i32),
}

/// `view` is the camera's turn (quarter turns of the fixed camera); the default is the unturned one.
/// `(sx, sy)` is a position in screen space: pixels at zoom 1 with y pointing down, the same space
/// `shader.wgsl` projects into. `top` is the editor's layer limit: layers above it are not drawn,
/// so the ray passes through them (and the cell where a block would go may be one of them).
pub fn pick(world: &World, view: &View, sx: f32, sy: f32, top: Option<i32>) -> Option<Pick> {
    const STEP: f32 = 0.05;
    let mut previous = None;
    let mut gz = CHUNK_SIZE_Z as f32;
    while gz >= 0.0 {
        let (gx, gy) = ground_at_view(view, sx, sy, gz);
        let (x, y) = from_iso(gx, gy);
        let z = gz.floor() as i32;
        // Terrain that has not arrived yet cannot be picked.
        if world.has_generator() || world.is_loaded_at(x, y) {
            let hidden = top.is_some_and(|top| z > top);
            if !hidden && !world.get(x, y, z).is_air() {
                return previous.map(|place| Pick { hit: (x, y, z), place });
            }
            previous = Some((x, y, z));
        } else {
            previous = None;
        }
        gz -= STEP;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use wurfel_sim::grid::to_iso;
    use wurfel_sim::{Block, Generator};

    /// Air with a single stone block at (5, 5, 0), plus optionally one right behind and above it.
    struct Pillar(Vec<(i32, i32, i32)>);
    impl Generator for Pillar {
        fn generate(&self, x: i32, y: i32, z: i32) -> Block {
            if self.0.contains(&(x, y, z)) { Block::new(3, 0) } else { Block::AIR }
        }
    }

    fn screen(gx: f32, gy: f32, gz: f32) -> (f32, f32) {
        screen_of(gx, gy, gz)
    }

    #[test]
    fn the_layer_limit_hides_what_is_above_it_from_the_pointer() {
        // A stone at layer 0 under a stone at layer 3, in the same column.
        let world = World::new(Pillar(vec![(5, 5, 0), (5, 5, 3)]));
        let (gx, gy) = to_iso(5, 5);
        let (sx, sy) = screen(gx, gy, 4.0); // the top face of the upper stone
        assert_eq!(pick(&world, &View::default(), sx, sy, None).unwrap().hit, (5, 5, 3));
        // Limited to layer 1 the upper stone is gone, and the lower one is hit through its place.
        let (sx, sy) = screen(gx, gy, 1.0);
        let limited = pick(&world, &View::default(), sx, sy, Some(1)).unwrap();
        assert_eq!((limited.hit, limited.place), ((5, 5, 0), (5, 5, 1)));
        // The top layer itself can be hit; the cell above it is where a block would go.
        let top = pick(&World::new(Pillar(vec![(5, 5, 1)])), &View::default(), sx, screen(gx, gy, 2.0).1, Some(1)).unwrap();
        assert_eq!((top.hit, top.place), ((5, 5, 1), (5, 5, 2)));
    }

    #[test]
    fn the_ground_point_of_a_screen_position_is_the_inverse_of_the_projection() {
        let (sx, sy) = screen_of(3.5, -2.0, 4.0);
        let (gx, gy) = ground_at(sx, sy, 4.0);
        assert!((gx - 3.5).abs() < 1e-4 && (gy + 2.0).abs() < 1e-4);
    }

    #[test]
    fn a_turned_camera_picks_what_it_shows() {
        let world = World::new(Pillar(vec![(5, 5, 0)]));
        let view = View { yaw: std::f32::consts::FRAC_PI_2, pivot: (2.0, 3.0), wobble: 0.0 };
        let (gx, gy) = to_iso(5, 5);
        let [sx, sy] = view.screen_position((gx, gy), 1.0);
        let pick = pick(&world, &view, sx, sy, None).expect("block under pointer");
        assert_eq!((pick.hit, pick.place), ((5, 5, 0), (5, 5, 1)));
        let thing = ThingState { id: 1, kind: "Wood".into(), pos: [gx, gy, 0.0], lit: false };
        let [tx, ty] = view.screen_position((gx, gy), 0.5);
        assert_eq!(pick_thing(&[thing], &view, tx, ty), Some(1));
    }

    #[test]
    fn the_nearest_thing_within_reach_of_the_pointer_is_picked() {
        let thing = |id, x, y, z| ThingState { id, kind: "Wood".into(), pos: [x, y, z], lit: false };
        let things = [thing(1, 5.0, 5.0, 0.0), thing(2, 6.0, 5.0, 0.0)];
        let (sx, sy) = screen_of(5.0, 5.0, 0.5);
        assert_eq!(pick_thing(&things, &View::default(), sx + 5.0, sy), Some(1));
        let (sx, sy) = screen_of(6.0, 5.0, 0.5);
        assert_eq!(pick_thing(&things, &View::default(), sx, sy - 5.0), Some(2));
        assert_eq!(pick_thing(&things, &View::default(), sx + 400.0, sy), None, "too far");
        assert_eq!(pick_thing(&[], &View::default(), 0.0, 0.0), None);
        // Two on the same spot: the one placed later is on top.
        let stacked = [thing(1, 5.0, 5.0, 0.0), thing(2, 5.0, 5.0, 0.0)];
        let (sx, sy) = screen_of(5.0, 5.0, 0.5);
        assert_eq!(pick_thing(&stacked, &View::default(), sx, sy), Some(2));
    }

    #[test]
    fn clicking_the_top_face_places_on_top() {
        let world = World::new(Pillar(vec![(5, 5, 0)]));
        let (gx, gy) = to_iso(5, 5);
        let (sx, sy) = screen(gx, gy, 1.0);
        let pick = pick(&world, &View::default(), sx, sy, None).expect("block under pointer");
        assert_eq!(pick.hit, (5, 5, 0));
        assert_eq!(pick.place, (5, 5, 1));
    }

    #[test]
    fn clicking_the_side_faces_hits_the_same_block_from_the_front() {
        let world = World::new(Pillar(vec![(5, 5, 0)]));
        let (gx, gy) = to_iso(5, 5);
        for (fx, fy) in [(gx + 0.5, gy), (gx, gy + 0.5)] {
            // Middle of the +x face and of the +y face, half way up the block.
            let (sx, sy) = screen(fx, fy, 0.5);
            let pick = pick(&world, &View::default(), sx, sy, None).expect("block under pointer");
            assert_eq!(pick.hit, (5, 5, 0));
            assert!(world.get(pick.place.0, pick.place.1, pick.place.2).is_air());
            assert_ne!(pick.place, pick.hit);
        }
    }

    #[test]
    fn sky_and_out_of_world_are_misses() {
        let world = World::new(Pillar(vec![(5, 5, 0)]));
        assert_eq!(pick(&world, &View::default(), 3000.0, -500.0, None), None);
        assert_eq!(pick(&world, &View::default(), 0.0, 0.0, None), None);
    }

    #[test]
    fn the_nearer_block_wins_over_one_behind_it() {
        // A three-high column one block-diagonal in front of (5, 5, 0) hides it.
        let near = (6, 7, 0);
        let world = World::new(Pillar(vec![(5, 5, 0), near, (6, 7, 1), (6, 7, 2)]));
        let (gx, gy) = to_iso(near.0, near.1);
        let (sx, sy) = screen(gx, gy, 3.0); // top of the near column
        assert_eq!(pick(&world, &View::default(), sx, sy, None).unwrap().hit, (6, 7, 2));
    }
}
