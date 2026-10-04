//! Smooth movement of other players from 30 Hz snapshots.
//!
//! Remote players are drawn slightly in the past: a render clock runs [`DELAY_MS`] behind the
//! newest snapshot, so there are normally two samples to interpolate between. If the network
//! stalls and the clock overtakes the newest sample, the player coasts on its last velocity for a
//! short while ([`MAX_EXTRAPOLATION_MS`]) and then stops. Times are server time (tick count at the
//! server's tick rate), so network jitter does not shake the motion.

use std::collections::VecDeque;

use glam::Vec3;

/// How far behind the newest snapshot remote players are drawn: two snapshot intervals at 30 Hz.
pub const DELAY_MS: f64 = 66.0;
/// How long to keep moving a player on its last velocity when no newer snapshot arrived.
pub const MAX_EXTRAPOLATION_MS: f64 = 100.0;
/// A jump between two snapshots longer than this is a teleport, not movement: do not glide.
pub const TELEPORT_DISTANCE: f32 = 4.0;
/// If the render clock is further than this from where it should be, jump instead of catching up.
const RESYNC_MS: f64 = 250.0;
/// Fraction of the clock error corrected per snapshot.
const CLOCK_GAIN: f64 = 0.1;
const KEPT: usize = 32;

/// The render clock, in server time.
#[derive(Debug, Clone, Default)]
pub struct RenderClock {
    now_ms: f64,
    started: bool,
}

impl RenderClock {
    /// A snapshot stamped `server_ms` arrived: steer the clock to `DELAY_MS` behind it.
    pub fn on_snapshot(&mut self, server_ms: f64) {
        let target = server_ms - DELAY_MS;
        let error = target - self.now_ms;
        if !self.started || error.abs() > RESYNC_MS {
            self.now_ms = target;
            self.started = true;
        } else {
            self.now_ms += error * CLOCK_GAIN;
        }
    }

    /// Advance by a frame.
    pub fn advance(&mut self, dt_ms: f64) {
        if self.started {
            self.now_ms += dt_ms;
        }
    }

    pub fn now_ms(&self) -> f64 {
        self.now_ms
    }
}

#[derive(Debug, Clone, Copy)]
struct Sample {
    t_ms: f64,
    pos: Vec3,
    vel: Vec3,
}

/// The recent snapshots of one player.
#[derive(Debug, Clone, Default)]
pub struct Track {
    samples: VecDeque<Sample>,
}

impl Track {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a snapshot. Out-of-order or repeated ones are dropped; a teleport clears the history.
    pub fn push(&mut self, server_ms: f64, pos: Vec3, vel: Vec3) {
        if let Some(last) = self.samples.back() {
            if server_ms <= last.t_ms {
                return;
            }
            if last.pos.distance(pos) > TELEPORT_DISTANCE {
                self.samples.clear();
            }
        }
        self.samples.push_back(Sample { t_ms: server_ms, pos, vel });
        while self.samples.len() > KEPT {
            self.samples.pop_front();
        }
    }

    /// Where the player is at render time `t_ms`, or `None` before the first snapshot.
    pub fn sample(&self, t_ms: f64) -> Option<Vec3> {
        let first = self.samples.front()?;
        let last = self.samples.back()?;
        if t_ms <= first.t_ms {
            return Some(first.pos);
        }
        if t_ms >= last.t_ms {
            let ahead = (t_ms - last.t_ms).min(MAX_EXTRAPOLATION_MS);
            return Some(last.pos + last.vel * (ahead / 1000.0) as f32);
        }
        let next = self.samples.iter().position(|s| s.t_ms >= t_ms)?;
        let (a, b) = (self.samples[next - 1], self.samples[next]);
        let f = ((t_ms - a.t_ms) / (b.t_ms - a.t_ms)) as f32;
        Some(a.pos.lerp(b.pos, f))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const STEP: f64 = 1000.0 / 30.0;

    fn walking_track(n: usize, speed: f32) -> Track {
        let mut track = Track::new();
        for i in 0..n {
            let t = i as f64 * STEP;
            track.push(t, Vec3::new(speed * (t / 1000.0) as f32, 0.0, 0.0), Vec3::new(speed, 0.0, 0.0));
        }
        track
    }

    #[test]
    fn between_two_snapshots_the_position_is_interpolated() {
        let mut track = Track::new();
        track.push(100.0, Vec3::new(0.0, 0.0, 0.0), Vec3::ZERO);
        track.push(200.0, Vec3::new(3.0, 0.0, 1.2), Vec3::ZERO);
        assert_eq!(track.sample(150.0), Some(Vec3::new(1.5, 0.0, 0.6)));
        assert_eq!(track.sample(125.0), Some(Vec3::new(0.75, 0.0, 0.3)));
    }

    #[test]
    fn before_the_first_snapshot_there_is_nothing_and_then_the_first_position() {
        let mut track = Track::new();
        assert_eq!(track.sample(0.0), None);
        track.push(100.0, Vec3::new(1.0, 2.0, 3.0), Vec3::ZERO);
        assert_eq!(track.sample(50.0), Some(Vec3::new(1.0, 2.0, 3.0)));
    }

    #[test]
    fn steady_walking_is_smooth_at_render_rate() {
        // Samples every 33 ms, drawn every 16.7 ms: each frame advances by the same distance.
        let track = walking_track(10, 4.0);
        let mut previous = track.sample(100.0).unwrap().x;
        for frame in 1..12 {
            let x = track.sample(100.0 + frame as f64 * 1000.0 / 60.0).unwrap().x;
            let step = x - previous;
            assert!((step - 4.0 / 60.0).abs() < 1e-4, "frame {frame} moved {step}");
            previous = x;
        }
    }

    #[test]
    fn starving_coasts_briefly_and_then_stops() {
        let mut track = Track::new();
        track.push(0.0, Vec3::ZERO, Vec3::new(10.0, 0.0, 0.0));
        assert_eq!(track.sample(50.0), Some(Vec3::new(0.5, 0.0, 0.0)), "coasting on the velocity");
        let capped = track.sample(MAX_EXTRAPOLATION_MS).unwrap();
        assert_eq!(track.sample(5000.0), Some(capped), "no runaway when the connection is gone");
        assert_eq!(capped, Vec3::new(1.0, 0.0, 0.0));
    }

    #[test]
    fn out_of_order_snapshots_are_dropped() {
        let mut track = Track::new();
        track.push(100.0, Vec3::new(1.0, 0.0, 0.0), Vec3::ZERO);
        track.push(200.0, Vec3::new(2.0, 0.0, 0.0), Vec3::ZERO);
        track.push(150.0, Vec3::new(9.0, 0.0, 0.0), Vec3::ZERO);
        track.push(200.0, Vec3::new(9.0, 0.0, 0.0), Vec3::ZERO);
        assert_eq!(track.sample(200.0), Some(Vec3::new(2.0, 0.0, 0.0)));
        assert_eq!(track.sample(150.0), Some(Vec3::new(1.5, 0.0, 0.0)));
    }

    #[test]
    fn a_teleport_does_not_glide_across_the_map() {
        let mut track = Track::new();
        track.push(100.0, Vec3::ZERO, Vec3::ZERO);
        track.push(133.0, Vec3::new(50.0, 0.0, 0.0), Vec3::ZERO);
        assert_eq!(track.sample(110.0), Some(Vec3::new(50.0, 0.0, 0.0)), "already there, no sweep");
    }

    #[test]
    fn the_history_is_bounded() {
        let track = walking_track(1000, 1.0);
        assert_eq!(track.samples.len(), KEPT);
    }

    #[test]
    fn the_clock_starts_behind_the_first_snapshot() {
        let mut clock = RenderClock::default();
        clock.advance(16.0);
        assert_eq!(clock.now_ms(), 0.0, "not running before the first snapshot");
        clock.on_snapshot(1000.0);
        assert_eq!(clock.now_ms(), 1000.0 - DELAY_MS);
    }

    #[test]
    fn the_clock_follows_the_server_without_jumping_on_jitter() {
        let mut clock = RenderClock::default();
        clock.on_snapshot(1000.0);
        // Frames at 60 Hz, a snapshot every other frame, but one of them 20 ms late.
        let mut server = 1000.0;
        let mut worst_step = 0.0f64;
        for i in 0..120 {
            let before = clock.now_ms();
            clock.advance(1000.0 / 60.0);
            server += 1000.0 / 60.0;
            if i % 2 == 0 {
                let jitter = if i == 60 { -20.0 } else { 0.0 };
                clock.on_snapshot(server + jitter);
            }
            worst_step = worst_step.max((clock.now_ms() - before - 1000.0 / 60.0).abs());
        }
        assert!(worst_step < 5.0, "the clock moved {worst_step} ms off a frame step");
        assert!((clock.now_ms() - (server - DELAY_MS)).abs() < 3.0, "and stays on target");
    }

    #[test]
    fn a_clock_far_off_resyncs_at_once() {
        let mut clock = RenderClock::default();
        clock.on_snapshot(1000.0);
        clock.on_snapshot(9000.0);
        assert_eq!(clock.now_ms(), 9000.0 - DELAY_MS);
    }
}
