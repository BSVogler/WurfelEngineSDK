//! Screen shake (Java `Camera.shake`): a hit or a blast jolts the picture for a moment.
//!
//! The Java camera only moved in screen space. Here the jolt also turns the map a little about the
//! player (`yaw`), so the isometric projection tilts away and back instead of just sliding; the
//! shader's view turn is made for exactly that (`View::wobble`).

/// Radians of turn per pixel of shake strength: a 100 px blast turns the map about 3 degrees.
const YAW_PER_PX: f32 = 0.0005;
/// How fast the turn oscillates (radians per second of the sine), so it wobbles instead of jittering.
const WOBBLE_SPEED: f32 = 45.0;
/// The most a single shake can be, in pixels, however many stack.
const MAX_AMPLITUDE: f32 = 160.0;

/// What a shake does to the picture this frame.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Jolt {
    /// Screen pixels to move the camera by.
    pub screen: [f32; 2],
    /// Radians to turn the map by, on top of the player's own view turn.
    pub yaw: f32,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Shake {
    /// Strength in pixels at the start of the running shake, time left and total time in ms.
    running: Option<(f32, f32, f32)>,
    clock: f32,
}

impl Shake {
    /// Start a shake of `amplitude` pixels that dies down over `millis`. A new shake does not cut a
    /// stronger running one short.
    pub fn add(&mut self, amplitude: f32, millis: f32) {
        if amplitude <= 0.0 || millis <= 0.0 {
            return;
        }
        let amplitude = amplitude.min(MAX_AMPLITUDE);
        let stronger = match self.running {
            Some((a, left, total)) => amplitude > a * left / total,
            None => true,
        };
        if stronger {
            self.running = Some((amplitude, millis, millis));
        }
    }

    /// Advance by `dt_ms`; `random` gives numbers in -1..1.
    pub fn step(&mut self, dt_ms: f32, mut random: impl FnMut() -> f32) -> Jolt {
        let Some((amplitude, left, total)) = self.running.as_mut() else { return Jolt::default() };
        *left -= dt_ms;
        if *left <= 0.0 {
            self.running = None;
            return Jolt::default();
        }
        self.clock += dt_ms / 1000.0;
        let strength = *amplitude * *left / *total;
        Jolt { screen: [random() * strength, random() * strength], yaw: (self.clock * WOBBLE_SPEED).sin() * strength * YAW_PER_PX }
    }
}

/// The shake of an explosion of `radius` blocks that is `distance` blocks away: the Java strength
/// (`radius * 100 / 3` px) close by, fading to nothing at 20 blocks.
pub fn blast_amplitude(radius: i32, distance: f32) -> f32 {
    const FULL_WITHIN: f32 = 4.0;
    const NONE_BEYOND: f32 = 20.0;
    let falloff = ((NONE_BEYOND - distance) / (NONE_BEYOND - FULL_WITHIN)).clamp(0.0, 1.0);
    radius.max(0) as f32 * 100.0 / 3.0 * falloff
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_shake_dies_down_and_ends() {
        let mut shake = Shake::default();
        shake.add(30.0, 100.0);
        let first = shake.step(10.0, || 1.0);
        let later = shake.step(70.0, || 1.0);
        assert!(first.screen[0] > later.screen[0] && later.screen[0] > 0.0);
        assert_eq!(shake.step(50.0, || 1.0), Jolt::default());
        assert!(shake.running.is_none());
    }

    #[test]
    fn the_map_turns_a_little_with_the_shake_and_not_without() {
        let mut shake = Shake::default();
        assert_eq!(shake.step(16.0, || 1.0).yaw, 0.0);
        shake.add(100.0, 400.0);
        let max = (0..30).map(|_| shake.step(10.0, || 0.0).yaw.abs()).fold(0.0, f32::max);
        assert!(max > 0.005 && max < 0.06, "a few degrees at most, was {max}");
    }

    #[test]
    fn a_weaker_shake_does_not_cut_a_stronger_one_short() {
        let mut shake = Shake::default();
        shake.add(100.0, 400.0);
        shake.add(10.0, 100.0);
        assert!(shake.step(1.0, || 1.0).screen[0] > 90.0);
    }

    #[test]
    fn a_blast_is_felt_close_by_and_not_far_away() {
        assert!((blast_amplitude(3, 2.0) - 100.0).abs() < 1e-3);
        assert!(blast_amplitude(3, 12.0) < 100.0 && blast_amplitude(3, 12.0) > 0.0);
        assert_eq!(blast_amplitude(3, 25.0), 0.0);
    }
}
