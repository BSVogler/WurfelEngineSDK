//! A force wave: a ring that spreads from a centre, lifts and drops the ground it passes like a ripple,
//! and shoves whatever it overtakes away from the centre.
//!
//! One definition serves both halves. [`Shockwave::lift`] is what the renderer adds to the height of
//! terrain (`shockwave_lift` in `shader.wgsl` is the same formula, and its numbers come from
//! [`Shockwave::shape`]); [`Shockwave::push`] is what the rules give to an entity the ring overtook in a
//! step. The wave is only for looks and pushes: it never changes a block.

use glam::{Vec2, Vec3};

/// How fast the ring spreads, blocks per second.
pub const SPEED: f32 = 14.0;
/// Half the thickness of the ring (the ground rises ahead of the front and sinks behind it), blocks.
pub const WIDTH: f32 = 2.2;
/// How far the ground moves at strength 1 right at the centre, blocks.
pub const AMPLITUDE: f32 = 0.45;
/// The horizontal speed given at strength 1 right at the centre, blocks per second.
pub const PUSH_SPEED: f32 = 14.0;
/// The upward part of the shove, as a share of the horizontal one.
pub const PUSH_LIFT: f32 = 0.45;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Shockwave {
    /// Isometric ground frame, blocks.
    pub center: Vec3,
    /// Seconds since it started.
    pub age: f32,
    /// Where it dies out, blocks.
    pub radius: f32,
    /// 1 is a dynamite blast of radius 3; bigger shoves harder and lifts more.
    pub strength: f32,
}

impl Shockwave {
    pub fn new(center: Vec3, radius: f32, strength: f32) -> Self {
        Shockwave { center, age: 0.0, radius: radius.max(1.0), strength }
    }

    /// Distance from the centre of the ring's middle.
    pub fn front(&self) -> f32 {
        self.age * SPEED
    }

    /// The ring has passed everything.
    pub fn finished(&self) -> bool {
        self.front() - WIDTH > self.radius
    }

    /// How strong the wave still is at distance `d`: full at the centre, none at the radius.
    pub fn falloff(&self, d: f32) -> f32 {
        (1.0 - d / self.radius).clamp(0.0, 1.0)
    }

    /// How far the ground at `(x, y)` is moved up (positive) or down (negative), blocks. A ripple: the
    /// ground rises ahead of the front and sinks behind it, smoothly, and nothing happens outside the ring.
    pub fn lift(&self, x: f32, y: f32) -> f32 {
        let d = Vec2::new(x - self.center.x, y - self.center.y).length();
        let t = (d - self.front()) / WIDTH;
        if t.abs() >= 1.0 {
            return 0.0;
        }
        let window = (t * std::f32::consts::FRAC_PI_2).cos().powi(2);
        AMPLITUDE * self.strength * self.falloff(d) * window * (t * std::f32::consts::PI).sin()
    }

    /// The numbers the shader needs, `[x, y, front, radius]` and `[amplitude, width, 0, 0]`.
    pub fn shape(&self) -> ([f32; 4], [f32; 4]) {
        ([self.center.x, self.center.y, self.front(), self.radius], [AMPLITUDE * self.strength, WIDTH, 0.0, 0.0])
    }

    /// The velocity change for something at `position` when the front moved from `before` to now: only if
    /// the ring crossed it in this step, so every body is shoved once. Away from the centre, a little up.
    pub fn push(&self, position: Vec3, before: f32) -> Option<Vec3> {
        let away = Vec2::new(position.x - self.center.x, position.y - self.center.y);
        let d = away.length();
        if d > self.radius || d <= before || d > self.front() {
            return None;
        }
        let direction = if d > 1e-3 { away / d } else { Vec2::X };
        let speed = PUSH_SPEED * self.strength * self.falloff(d);
        Some(direction.extend(PUSH_LIFT) * speed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wave(age: f32) -> Shockwave {
        Shockwave { age, ..Shockwave::new(Vec3::ZERO, 9.0, 1.0) }
    }

    #[test]
    fn the_ground_rises_ahead_of_the_front_and_sinks_behind_it() {
        let w = wave(0.5); // the front is at 7
        let front = w.front();
        assert!(w.lift(front + 1.0, 0.0) > 0.0, "ahead");
        assert!(w.lift(front - 1.0, 0.0) < 0.0, "behind");
        assert_eq!(w.lift(front + WIDTH + 0.1, 0.0), 0.0, "beyond the ring");
        assert_eq!(w.lift(front - WIDTH - 0.1, 0.0), 0.0, "inside the ring");
    }

    #[test]
    fn the_ripple_is_radial_and_dies_out_towards_the_radius() {
        let w = wave(0.2);
        let f = w.front();
        let a = w.lift(f + 1.0, 0.0);
        assert!((a - w.lift(0.0, f + 1.0)).abs() < 1e-5, "the same in every direction");
        let late = Shockwave { age: 0.6, ..w };
        assert!(late.lift(late.front() + 1.0, 0.0).abs() < a.abs(), "weaker further out");
        assert!(wave(2.0).finished());
        assert!(!wave(0.1).finished());
    }

    #[test]
    fn a_body_is_pushed_once_as_the_front_passes_and_away_from_the_centre() {
        let mut w = Shockwave::new(Vec3::new(1.0, 1.0, 0.0), 9.0, 1.0);
        let at = Vec3::new(4.0, 1.0, 0.0); // 3 blocks along +x
        let mut pushes = Vec::new();
        for _ in 0..40 {
            let before = w.front();
            w.age += 0.05;
            if let Some(v) = w.push(at, before) {
                pushes.push(v);
            }
        }
        assert_eq!(pushes.len(), 1, "once");
        assert!(pushes[0].x > 5.0 && pushes[0].y.abs() < 1e-4 && pushes[0].z > 0.0, "away and up: {:?}", pushes[0]);
        assert!(w.push(Vec3::new(30.0, 0.0, 0.0), 0.0).is_none(), "beyond the radius");
    }

    #[test]
    fn stronger_waves_shove_and_lift_more() {
        let weak = Shockwave { age: 0.3, ..Shockwave::new(Vec3::ZERO, 9.0, 1.0) };
        let strong = Shockwave { strength: 2.0, ..weak };
        let x = weak.front() + 1.0;
        assert!(strong.lift(x, 0.0) > weak.lift(x, 0.0));
        let at = Vec3::new(3.0, 0.0, 0.0);
        assert!(strong.push(at, 0.0).unwrap().x > weak.push(at, 0.0).unwrap().x);
    }
}
