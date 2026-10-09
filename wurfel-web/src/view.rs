//! The free camera: the world is turned about the player by a yaw angle before it is projected.
//!
//! The projection itself stays the fixed dimetric one (`shader.wgsl`): the ground frame is rotated
//! about a pivot first, so a quarter turn shows the world from the side. `View` is the CPU twin of
//! that rotation (`view_pos` in the shader): everything that has to agree with what is drawn
//! (name tags, the camera follow, the movement keys) goes through it. With a yaw of 0 it changes
//! nothing, so the fixed camera is the special case.

use wurfel_sim::player::{heading_units, PlayerInput};
use glam::Vec2;

/// Radians per pixel of mouse movement.
pub const MOUSE_SENSITIVITY: f32 = 0.005;

/// Screen (px at zoom 1, y down) of a point of the ground frame, with the projection of the game.
const SCREEN_X: f32 = 100.0;
const SCREEN_Y: f32 = 50.0;
const SCREEN_Z: f32 = 122.0;

/// How strong the perspective of the free camera is: the eye is 1 / this many screen heights from the
/// focus plane (at any zoom, so the field of view stays the same and zooming moves the camera). 0.3 is
/// 3.3 screen heights: what is a screen height nearer than the player is about a third larger.
pub const PERSPECTIVE: f32 = 0.3;

/// Pixels (at zoom 1) of the projection per unit of view depth (`x + y + 0.82 z`): one step along the
/// view ray (1, 1, 0.82) is 1.63 blocks long and changes the depth by 2.67, a block is about 112 px.
/// `DEPTH_PX` in `shader.wgsl` is the same number.
pub const DEPTH_PX: f32 = 68.0;

/// By how much the perspective camera scales what lies at view depth `depth` (relative to the camera's
/// `center_depth`) about the middle of the screen, when the player's depth `focus` keeps its size:
/// 1 at the focus, more towards the viewer, 0 where it is beyond the eye. `zoom` and `height` (px of the
/// canvas) give the distance of the eye. `perspective_scale` in `shader.wgsl` is the same.
pub fn perspective_scale(depth: f32, focus: f32, zoom: f32, height: f32) -> f32 {
    let distance = height / (zoom * PERSPECTIVE);
    let away = distance - (depth - focus) * DEPTH_PX;
    if away < 0.1 * distance {
        0.0
    } else {
        distance / away
    }
}

/// How the world is looked at. This is the one switch between the two cameras: everything that
/// differs (meshing the sides that look away, turning the world with the mouse, the walking keys
/// following the view) asks the mode, and the fixed camera is the unchanged 2.5D one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CameraMode {
    /// The dimetric view of the Java engine: only the three sides that face the camera are meshed.
    Fixed,
    /// The world can be turned about the player (`View::yaw`) and is seen from every side.
    Free,
}

impl CameraMode {
    /// The mode the game starts in. `?camera=free` in the page address overrides it.
    pub const DEFAULT: CameraMode = CameraMode::Fixed;

    /// `fixed` or `free`.
    pub fn parse(name: &str) -> Option<CameraMode> {
        match name.to_ascii_lowercase().as_str() {
            "fixed" => Some(CameraMode::Fixed),
            "free" => Some(CameraMode::Free),
            _ => None,
        }
    }

    pub fn is_free(self) -> bool {
        self == CameraMode::Free
    }

    pub fn toggled(self) -> CameraMode {
        if self.is_free() { CameraMode::Fixed } else { CameraMode::Free }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct View {
    /// Counter-clockwise turn of the world about the pivot, in radians (0 is the fixed camera).
    pub yaw: f32,
    /// The ground point the world turns about: the player.
    pub pivot: (f32, f32),
    /// A short extra turn of the picture (screen shake), radians. Only what is drawn follows it; the
    /// walking keys do not.
    pub wobble: f32,
}

impl View {
    /// A point of the ground frame as the camera sees it.
    pub fn rotate(&self, (x, y): (f32, f32)) -> (f32, f32) {
        let (sin, cos) = (self.yaw + self.wobble).sin_cos();
        let (dx, dy) = (x - self.pivot.0, y - self.pivot.1);
        (self.pivot.0 + cos * dx - sin * dy, self.pivot.1 + sin * dx + cos * dy)
    }

    /// The ground point that [`Self::rotate`] turns into `point`: what the camera sees at `point` lies
    /// here in the world.
    pub fn unrotate(&self, (x, y): (f32, f32)) -> (f32, f32) {
        let (sin, cos) = (self.yaw + self.wobble).sin_cos();
        let (dx, dy) = (x - self.pivot.0, y - self.pivot.1);
        (self.pivot.0 + cos * dx + sin * dy, self.pivot.1 - sin * dx + cos * dy)
    }

    /// Screen position (px at zoom 1, y down) of a ground point at height `z`.
    pub fn screen_position(&self, point: (f32, f32), z: f32) -> [f32; 2] {
        let (gx, gy) = self.rotate(point);
        [(gx - gy) * SCREEN_X, (gx + gy) * SCREEN_Y - z * SCREEN_Z]
    }

    /// Screen position like [`Self::screen_position`] and the point's view depth (`x + y + 0.82 z` of the
    /// turned point; the camera's `center_depth` is not taken off).
    pub fn project(&self, point: (f32, f32), z: f32) -> ([f32; 2], f32) {
        let (gx, gy) = self.rotate(point);
        (self.screen_position(point, z), gx + gy + crate::sprites::DEPTH_Z * z)
    }

    /// `(cos, sin, pivot x, pivot y)`: the `view` member of the camera uniform.
    pub fn uniform(&self) -> [f32; 4] {
        let (sin, cos) = (self.yaw + self.wobble).sin_cos();
        [cos, sin, self.pivot.0, self.pivot.1]
    }

    /// Turn by the mouse movement of `dx` pixels (right turns the world to the left, so the camera
    /// goes right around the player, like dragging the world).
    pub fn turn(&mut self, dx: f32) {
        self.yaw = (self.yaw - dx * MOUSE_SENSITIVITY).rem_euclid(std::f32::consts::TAU);
    }

    /// Walking keys are directions on the screen: with a turned camera they must mean the same on
    /// the screen, so the world direction they ask for is turned back. The simulation takes the
    /// eight directions of the four keys, so the result is the nearest of them.
    pub fn walk_input(&self, input: PlayerInput) -> PlayerInput {
        if self.yaw == 0.0 {
            return input;
        }
        // The same priorities as `Controllable::walk`: left over right, up over down.
        let x = if input.left { -1.0 } else if input.right { 1.0 } else { 0.0 };
        let y = if input.up { -1.0 } else if input.down { 1.0 } else { 0.0 };
        if x == 0.0 && y == 0.0 {
            return input;
        }
        // Screen frame (x right, y towards the viewer) to the ground frame, undo the turn, and back.
        let s = std::f32::consts::FRAC_1_SQRT_2;
        let ground = (s * (x + y), s * (y - x));
        let (sin, cos) = (-self.yaw).sin_cos();
        let turned = (cos * ground.0 - sin * ground.1, sin * ground.0 + cos * ground.1);
        let screen = (s * (turned.0 - turned.1), s * (turned.0 + turned.1));
        // The nearest of the eight directions (sectors of 45 degrees, 0 is right).
        let sector = ((screen.1.atan2(screen.0) / std::f32::consts::FRAC_PI_4).round() as i32).rem_euclid(8);
        let (dx, dy) = [(1, 0), (1, 1), (0, 1), (-1, 1), (-1, 0), (-1, -1), (0, -1), (1, -1)][sector as usize];
        // The keys keep the snapped direction (the walking animation turns in steps); the movement
        // itself follows the exact direction, so it turns smoothly with the camera.
        let heading = Some(heading_units(Vec2::new(screen.0, screen.1)));
        PlayerInput { up: dy < 0, down: dy > 0, left: dx < 0, right: dx > 0, jump: input.jump, heading }
    }
}

/// The Java engine's `CameraLeapRadius` (view-space units): how far the focus may stray from the
/// middle of the picture before the camera follows.
pub const CAMERA_LEAP_RADIUS: f32 = 90.0;

/// The camera of the Java engine does not stick to the player: it stays where it is while the focus
/// is inside the leap radius and is only dragged along once the focus leaves it, so the player can
/// walk a little before the picture moves.
pub fn follow_within_leap(center: [f32; 2], target: [f32; 2], radius: f32) -> [f32; 2] {
    let (dx, dy) = (center[0] - target[0], center[1] - target[1]);
    let dist = dx.hypot(dy);
    if dist <= radius {
        return center;
    }
    let k = radius / dist;
    [target[0] + dx * k, target[1] + dy * k]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::{FRAC_PI_2, PI};

    fn keys(up: bool, down: bool, left: bool, right: bool) -> PlayerInput {
        PlayerInput { up, down, left, right, ..Default::default() }
    }

    #[test]
    fn unrotating_undoes_rotating() {
        let view = View { yaw: 1.0, pivot: (3.0, 4.0), wobble: 0.1 };
        let back = view.unrotate(view.rotate((7.5, -2.0)));
        assert!((back.0 - 7.5).abs() < 1e-4 && (back.1 + 2.0).abs() < 1e-4, "{back:?}");
    }

    #[test]
    fn the_perspective_keeps_the_focus_and_enlarges_what_is_nearer() {
        let (zoom, height) = (0.5, 1080.0);
        assert_eq!(perspective_scale(7.0, 7.0, zoom, height), 1.0);
        let near = perspective_scale(17.0, 7.0, zoom, height);
        let far = perspective_scale(-3.0, 7.0, zoom, height);
        assert!(near > 1.0 && far < 1.0, "{near} {far}");
        // The eye is `1 / PERSPECTIVE` screen heights away, at any zoom: one screen height nearer
        // than the focus is a third closer to the eye.
        for zoom in [0.25, 0.5, 2.0] {
            let screen_height_in_depth = (height / zoom) / DEPTH_PX;
            let scale = perspective_scale(screen_height_in_depth, 0.0, zoom, height);
            assert!((scale - 1.0 / (1.0 - PERSPECTIVE)).abs() < 1e-4, "{scale}");
        }
        // Nothing beyond the eye is drawn.
        assert_eq!(perspective_scale(1000.0, 0.0, zoom, height), 0.0);
    }

    #[test]
    fn the_shader_uses_the_same_perspective_numbers() {
        let source = include_str!("shader.wgsl");
        assert!(source.contains(&format!("const DEPTH_PX = {:.1};", DEPTH_PX)));
    }

    #[test]
    fn camera_modes_parse_and_toggle() {
        assert_eq!(CameraMode::parse("free"), Some(CameraMode::Free));
        assert_eq!(CameraMode::parse("FIXED"), Some(CameraMode::Fixed));
        assert_eq!(CameraMode::parse("orbit"), None);
        assert_eq!(CameraMode::Fixed.toggled(), CameraMode::Free);
        assert_eq!(CameraMode::Free.toggled(), CameraMode::Fixed);
    }

    #[test]
    fn a_turned_camera_walks_in_the_exact_direction_but_keeps_snapped_keys() {
        let view = View { yaw: 0.2, pivot: (0.0, 0.0), wobble: 0.0 };
        let out = view.walk_input(keys(false, false, false, true));
        assert!(out.right && !out.up && !out.down && !out.left, "keys stay on the nearest of eight");
        let dir = wurfel_sim::player::heading_direction(out.heading.expect("a heading"));
        // "Right" on the screen, with the world turned by 0.2 rad, is turned back by that much.
        assert!((dir.y.atan2(dir.x) + 0.2).abs() < 0.01, "{dir:?}");
    }

    #[test]
    fn no_turn_changes_nothing() {
        let view = View { yaw: 0.0, pivot: (3.0, 4.0), wobble: 0.0 };
        assert_eq!(view.rotate((7.0, -2.0)), (7.0, -2.0));
        assert_eq!(view.screen_position((7.0, -2.0), 1.0), [900.0, 250.0 - 122.0]);
        let input = keys(true, false, false, true);
        assert_eq!(view.walk_input(input), input);
    }

    #[test]
    fn the_pivot_stays_where_it_is_and_distances_are_kept() {
        let view = View { yaw: 1.0, pivot: (3.0, 4.0), wobble: 0.0 };
        let (x, y) = view.rotate((3.0, 4.0));
        assert!((x - 3.0).abs() < 1e-6 && (y - 4.0).abs() < 1e-6);
        let (x, y) = view.rotate((8.0, 1.0));
        assert!((((x - 3.0).powi(2) + (y - 4.0).powi(2)) as f32).sqrt() - 34.0f32.sqrt() < 1e-5);
    }

    #[test]
    fn a_quarter_turn_takes_ground_x_to_ground_y() {
        let view = View { yaw: FRAC_PI_2, pivot: (0.0, 0.0), wobble: 0.0 };
        let (x, y) = view.rotate((1.0, 0.0));
        assert!(x.abs() < 1e-6 && (y - 1.0).abs() < 1e-6);
    }

    #[test]
    fn the_uniform_is_cos_sin_and_the_pivot() {
        let u = View { yaw: PI, pivot: (1.5, -2.0), wobble: 0.0 }.uniform();
        assert!((u[0] + 1.0).abs() < 1e-6 && u[1].abs() < 1e-6 && u[2] == 1.5 && u[3] == -2.0);
    }

    #[test]
    fn turning_by_the_mouse_wraps_around() {
        let mut view = View::default();
        view.turn(-100.0);
        assert!((view.yaw - 0.5).abs() < 1e-6);
        view.turn(200.0);
        assert!(view.yaw > 0.0 && view.yaw < std::f32::consts::TAU, "{}", view.yaw);
    }

    /// Where the player goes on the screen when walking with `input`: the walking direction in the
    /// frame of the keys, found by turning the world direction like the shader does.
    fn on_screen(view: View, input: PlayerInput) -> (f32, f32) {
        let x = if input.left { -1.0 } else if input.right { 1.0 } else { 0.0 };
        let y = if input.up { -1.0 } else if input.down { 1.0 } else { 0.0 };
        let s = std::f32::consts::FRAC_1_SQRT_2;
        let world = (s * (x + y), s * (y - x)); // ground frame
        let (sin, cos) = view.yaw.sin_cos();
        let seen = (cos * world.0 - sin * world.1, sin * world.0 + cos * world.1); // as the camera sees it
        (s * (seen.0 - seen.1), s * (seen.0 + seen.1)) // back to the frame of the keys
    }

    #[test]
    fn keys_keep_their_meaning_on_the_screen_whatever_the_turn() {
        for steps in 0..16 {
            let view = View { yaw: steps as f32 * PI / 8.0, pivot: (0.0, 0.0), wobble: 0.0 };
            for input in [keys(true, false, false, false), keys(false, true, false, false), keys(false, false, true, false), keys(false, false, false, true)] {
                let wanted = on_screen(View::default(), input);
                let got = on_screen(view, view.walk_input(input));
                // The nearest of eight directions is within 22.5 degrees of what was asked for.
                let angle = (wanted.1.atan2(wanted.0) - got.1.atan2(got.0)).abs();
                let angle = angle.min(std::f32::consts::TAU - angle);
                assert!(angle < PI / 8.0 + 1e-3, "yaw step {steps}, {input:?}: off by {angle}");
            }
        }
    }

    #[test]
    fn a_half_turn_inverts_the_keys_and_keeps_jump() {
        let view = View { yaw: PI, pivot: (0.0, 0.0), wobble: 0.0 };
        let mut input = keys(true, false, false, false);
        input.jump = true;
        let turned = view.walk_input(input);
        assert!(turned.down && !turned.up && turned.jump, "{turned:?}");
        assert_eq!(view.walk_input(keys(false, false, false, false)), keys(false, false, false, false));
    }

    #[test]
    fn the_camera_rests_inside_the_leap_radius_and_trails_outside() {
        assert_eq!(follow_within_leap([0.0, 0.0], [50.0, 0.0], 90.0), [0.0, 0.0]);
        let c = follow_within_leap([0.0, 0.0], [200.0, 0.0], 90.0);
        assert!((c[0] - 110.0).abs() < 1e-4 && c[1].abs() < 1e-4);
    }
}
