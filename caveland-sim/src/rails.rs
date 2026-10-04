//! Where a cart goes on a rail block: the geometry of `MineCart.update`. (Whether a booster rail
//! has power is [`crate::power::booster_powered`].)
//!
//! The Java code works in "game space": x to the right of the screen, y towards the viewer, one
//! block edge being 141.42 units. The engine here uses the isometric ground frame, which is the
//! same plane turned by 45 degrees (see [`wurfel_sim::entity::screen_to_iso`]); distances and
//! speeds are the same in both. The rail rules are written in game space, because the block values
//! name screen directions ("straight from left bottom to top right"), and converted at the edges.
//!
//! # Block values of a rail (`CLBlocks.RAILS`)
//!
//! | value | shape |
//! |---|---|
//! | 0 | straight, bottom left to top right |
//! | 1 | straight, top left to bottom right |
//! | 2, 4 | curves that run left to right |
//! | 3, 5 | curves that run top to bottom |
//! | 6, 7, 8, 9 | ramps (6 and 7 also follow the straight lines of 0 and 1) |
//!
//! Values 8 and 9 are ramps that do not keep the cart on a line, a Java quirk that is kept.

use glam::Vec2;
use wurfel_sim::entity::{iso_to_screen, screen_to_iso};

/// Half a block edge (`GAME_EDGELENGTH2`), the radius of a curve.
pub const EDGE2: f32 = 0.5;
/// Half the diagonal of a block (`GAME_DIAGLENGTH2`, 100 units).
pub const DIAG2: f32 = 0.707_106_8;

/// Where a cart standing on a rail block at `position` ends up this step, and which way it faces.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Follow {
    /// New horizontal position, isometric ground frame. The height is not changed by the rails.
    pub position: Vec2,
    /// Unit vector the cart now faces and rolls along, isometric ground frame.
    pub orientation: Vec2,
}

/// The direction of `v`, or `None` for a vector too short to have one (a cart in the exact middle
/// of a curve's circle, give or take rounding).
fn unit(v: Vec2) -> Option<Vec2> {
    (v.length() > 1e-4).then(|| v.normalize())
}

/// Put a cart back on the track of a rail block (`switch (value)` of `MineCart.update`).
///
/// `centre` is the centre of the block's footprint, `position` where the cart is and `movement`
/// its horizontal velocity, all in the isometric ground frame. `None` for rail values that have no
/// line to follow (8, 9, and anything above), and for a cart exactly at the middle of a curve's
/// circle.
///
/// Deviation: Java normalises the vector from the curve's centre in three dimensions, so a cart
/// standing on the floor of the block is lifted by up to a quarter block and falls back every step.
/// Here the curve is the circle on the ground and the height is kept.
pub fn follow(value: u8, centre: Vec2, position: Vec2, movement: Vec2) -> Option<Follow> {
    let c = iso_to_screen(centre);
    let d = iso_to_screen(position) - c;
    let m = iso_to_screen(movement);
    let (orientation, offset) = match value {
        0 | 6 => {
            // Moves on y = -x: keeps the screen x of the cart and puts y on the line.
            let o = if m.y >= 0.0 && m.x <= 0.0 { Vec2::new(-1.0, 1.0) } else { Vec2::new(1.0, -1.0) };
            (o, Vec2::new(d.x, -d.x))
        }
        1 | 7 => {
            let o = if m.y >= 0.0 && m.x >= 0.0 { Vec2::new(1.0, 1.0) } else { Vec2::new(-1.0, -1.0) };
            (o, Vec2::new(d.x, d.x))
        }
        3 | 5 => {
            // Down, or standing still on the upper half: down.
            let dir_y = if m.y > 0.0 || (m.y == 0.0 && d.y < 0.0) { 1.0 } else { -1.0 };
            let anchor = Vec2::new(if value == 5 { DIAG2 } else { -DIAG2 }, 0.0);
            (Vec2::new(0.0, dir_y), anchor + unit(d - anchor)? * EDGE2)
        }
        2 | 4 => {
            // Coming from the left: right.
            let dir_x = if m.x > 0.0 || (m.x == 0.0 && d.x < 0.0) { 1.0 } else { -1.0 };
            let anchor = Vec2::new(0.0, if value == 4 { -DIAG2 } else { DIAG2 });
            (Vec2::new(dir_x, 0.0), anchor + unit(d - anchor)? * EDGE2)
        }
        _ => return None,
    };
    Some(Follow { position: centre + screen_to_iso(offset), orientation: screen_to_iso(orientation.normalize()) })
}

/// What a ramp does to a cart moving at `movement` (isometric ground frame, with `z` up): a cart
/// driving up a ramp is launched along it, `None` otherwise.
///
/// Returns the new three-dimensional velocity: the direction of travel tilted up by `0.8` and
/// scaled to 90 % of the old speed.
pub fn ramp_launch(value: u8, movement: glam::Vec3) -> Option<glam::Vec3> {
    let m = iso_to_screen(movement.truncate());
    let up = (value == 6 && m.x > 0.0) || (value == 7 && m.y < 0.0) || (value == 8 && m.x < 0.0) || (value == 9 && m.y > 0.0);
    if !up {
        return None;
    }
    let launched = glam::Vec3::new(m.normalize_or_zero().x, m.normalize_or_zero().y, 0.8).normalize() * movement.length() * 0.9;
    let hor = screen_to_iso(launched.truncate());
    Some(glam::Vec3::new(hor.x, hor.y, launched.z))
}

/// Which way a cart on a ramp rolls down by itself (`roll down?`): the facing, in the isometric
/// ground frame, if the ramp's slope pulls the cart at `movement` back.
pub fn ramp_roll(value: u8, movement: Vec2) -> Option<Vec2> {
    let m = iso_to_screen(movement);
    let game = match value {
        6 if m.x <= 0.0 => Vec2::new(-1.0, 1.0),
        7 if m.x >= 0.0 => Vec2::new(1.0, 1.0),
        8 if m.x >= 0.0 => Vec2::new(1.0, -1.0),
        9 if m.x <= 0.0 => Vec2::new(-1.0, -1.0),
        _ => return None,
    };
    Some(screen_to_iso(game.normalize()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use wurfel_sim::grid::to_iso;

    fn centre() -> Vec2 {
        let (gx, gy) = to_iso(5, 10);
        Vec2::new(gx, gy)
    }

    #[test]
    fn a_straight_rail_pulls_the_cart_onto_its_line() {
        // Value 0 runs along iso -y. A cart a bit off to the side (along iso x) is pulled back to the
        // line through the middle of the block, keeping its screen x.
        let c = centre();
        let off = Vec2::new(0.3, 0.0);
        let step = follow(0, c, c + off, Vec2::new(0.0, -5.0)).unwrap();
        let screen_x = iso_to_screen(off).x;
        let on_line = iso_to_screen(step.position - c);
        assert!((on_line.x - screen_x).abs() < 1e-5, "keeps its screen x");
        assert!((on_line.y + on_line.x).abs() < 1e-5, "is on y = -x, got {on_line}");
        // Moving towards the top right (-y in the iso frame): faces (1, -1) in game space.
        let facing = iso_to_screen(step.orientation);
        assert!(facing.x > 0.0 && facing.y < 0.0);
        assert!((step.orientation.length() - 1.0).abs() < 1e-5);
    }

    #[test]
    fn the_cart_faces_the_way_it_rolls() {
        let c = centre();
        let down_left = follow(0, c, c, screen_to_iso(Vec2::new(-1.0, 1.0)) * 3.0).unwrap();
        let up_right = follow(0, c, c, screen_to_iso(Vec2::new(1.0, -1.0)) * 3.0).unwrap();
        assert!((down_left.orientation + up_right.orientation).length() < 1e-5, "opposite directions");
        assert!(iso_to_screen(down_left.orientation).x < 0.0);
        let other = follow(1, c, c, screen_to_iso(Vec2::new(1.0, 1.0)) * 3.0).unwrap();
        assert!(iso_to_screen(other.orientation).x > 0.0 && iso_to_screen(other.orientation).y > 0.0);
    }

    #[test]
    fn a_curve_keeps_the_cart_on_a_circle_of_half_a_block() {
        let c = centre();
        // Value 5: circle around the point one half diagonal to the right of the block's centre.
        let anchor = c + screen_to_iso(Vec2::new(DIAG2, 0.0));
        for off in [Vec2::new(0.0, 0.3), Vec2::new(-0.1, -0.2), Vec2::new(0.2, 0.05)] {
            let pos = c + screen_to_iso(off);
            let step = follow(5, c, pos, screen_to_iso(Vec2::new(0.0, 4.0))).unwrap();
            let r = step.position.distance(anchor);
            assert!((r - EDGE2).abs() < 1e-5, "radius {r} for offset {off}");
        }
    }

    #[test]
    fn curves_decide_their_direction_from_the_movement_or_the_side() {
        let c = centre();
        let from_top = c + screen_to_iso(Vec2::new(0.0, -0.3));
        let standing = follow(3, c, from_top, Vec2::ZERO).unwrap();
        assert!(iso_to_screen(standing.orientation).y > 0.0, "on the upper half it rolls down");
        let up = follow(3, c, from_top, screen_to_iso(Vec2::new(0.0, -2.0))).unwrap();
        assert!(iso_to_screen(up.orientation).y < 0.0, "moving up it keeps going up");
        let left = c + screen_to_iso(Vec2::new(-0.3, 0.0));
        let right = follow(2, c, left, Vec2::ZERO).unwrap();
        assert!(iso_to_screen(right.orientation).x > 0.0, "coming from the left it goes right");
    }

    #[test]
    fn ramps_8_and_9_and_unknown_values_follow_no_line() {
        let c = centre();
        for value in [8, 9, 10, 255] {
            assert_eq!(follow(value, c, c + Vec2::new(0.1, 0.0), Vec2::X), None, "value {value}");
        }
    }

    #[test]
    fn a_cart_on_the_circle_centre_has_nowhere_to_go() {
        let c = centre();
        let anchor = c + screen_to_iso(Vec2::new(DIAG2, 0.0));
        assert_eq!(follow(5, c, anchor, Vec2::X), None);
    }

    #[test]
    fn driving_up_a_ramp_launches_the_cart_along_the_slope() {
        let fast = screen_to_iso(Vec2::new(5.0, 0.0)).extend(0.0);
        let launched = ramp_launch(6, fast).unwrap();
        assert!(launched.z > 0.0);
        assert!((launched.length() - fast.length() * 0.9).abs() < 1e-4, "keeps 90 % of the speed");
        // The horizontal part still points the way the cart was going.
        assert!(launched.truncate().dot(fast.truncate()) > 0.0);
        // Rolling the other way is not a launch.
        assert_eq!(ramp_launch(6, -fast), None);
        assert_eq!(ramp_launch(0, fast), None);
    }

    #[test]
    fn a_cart_rolling_back_down_a_ramp_faces_down_the_slope() {
        let back = screen_to_iso(Vec2::new(-3.0, 0.0));
        let facing = ramp_roll(6, back).unwrap();
        assert!(iso_to_screen(facing).x < 0.0 && iso_to_screen(facing).y > 0.0);
        assert_eq!(ramp_roll(6, -back), None, "going up the ramp does not roll");
        assert_eq!(ramp_roll(0, back), None);
    }
}
