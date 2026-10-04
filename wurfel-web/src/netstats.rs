//! Network debug statistics: latency, jitter, traffic, snapshot rate and prediction error.
//!
//! Pure bookkeeping with the clock passed in (milliseconds), so it is unit tested natively and used
//! by the in-game overlay (F3).

use std::collections::VecDeque;

use wurfel_sim::protocol::ServerStats;

/// Rates are measured over this window.
const WINDOW_MS: f64 = 1000.0;
/// Weight of a new ping sample in the running average (RFC 3550 style jitter uses 1/16; pings are
/// rare here, so react faster).
const EWMA: f64 = 0.2;
/// Min/max are over the most recent this many pings.
const RTT_HISTORY: usize = 20;

/// Events inside the sliding window: `(time_ms, bytes)`.
#[derive(Default)]
struct Rate {
    events: VecDeque<(f64, usize)>,
    total_bytes: u64,
    total_messages: u64,
}

impl Rate {
    fn record(&mut self, now: f64, bytes: usize) {
        self.events.push_back((now, bytes));
        self.total_bytes += bytes as u64;
        self.total_messages += 1;
    }

    fn prune(&mut self, now: f64) {
        while self.events.front().is_some_and(|&(t, _)| now - t > WINDOW_MS) {
            self.events.pop_front();
        }
    }

    /// `(bytes per second, messages per second)` over the window.
    fn per_second(&mut self, now: f64) -> (f64, f64) {
        self.prune(now);
        let bytes: usize = self.events.iter().map(|&(_, b)| b).sum();
        let seconds = WINDOW_MS / 1000.0;
        (bytes as f64 / seconds, self.events.len() as f64 / seconds)
    }
}

#[derive(Default)]
pub struct NetStats {
    incoming: Rate,
    outgoing: Rate,

    rtt_last: Option<f64>,
    rtt_avg: f64,
    jitter: f64,
    rtt_history: VecDeque<f64>,

    snapshots: VecDeque<f64>,
    last_snapshot: Option<f64>,
    snapshot_interval_avg: f64,
    interval_samples: u32,
    server_tick: u64,

    server: Option<ServerStats>,

    prediction_error: f32,
    prediction_error_max: f32,
    frame_ms_avg: f64,
}

/// A frozen view of the numbers for display.
#[derive(Debug, Clone, PartialEq)]
pub struct NetReport {
    pub rtt_ms: Option<f64>,
    pub rtt_avg_ms: Option<f64>,
    pub rtt_min_ms: Option<f64>,
    pub rtt_max_ms: Option<f64>,
    pub jitter_ms: f64,
    pub snapshots_per_s: f64,
    pub snapshot_interval_ms: Option<f64>,
    pub snapshot_age_ms: Option<f64>,
    pub server_tick: u64,
    pub bytes_in_per_s: f64,
    pub bytes_out_per_s: f64,
    pub messages_in_per_s: f64,
    pub messages_out_per_s: f64,
    pub total_in: u64,
    pub total_out: u64,
    pub prediction_error: f32,
    pub prediction_error_max: f32,
    pub fps: f64,
    pub frame_ms: f64,
    pub server: Option<ServerStats>,
}

impl NetStats {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn on_sent(&mut self, bytes: usize, now: f64) {
        self.outgoing.record(now, bytes);
    }

    pub fn on_received(&mut self, bytes: usize, now: f64) {
        self.incoming.record(now, bytes);
    }

    /// The latest round trip in milliseconds, if a pong has arrived yet.
    pub fn last_rtt(&self) -> Option<f64> {
        self.rtt_last
    }

    /// A pong arrived for the ping that was sent at `sent_at`.
    pub fn on_pong(&mut self, sent_at: f64, now: f64) {
        let rtt = (now - sent_at).max(0.0);
        match self.rtt_last {
            None => self.rtt_avg = rtt,
            Some(_) => {
                self.jitter += (((rtt - self.rtt_avg).abs()) - self.jitter) * EWMA;
                self.rtt_avg += (rtt - self.rtt_avg) * EWMA;
            }
        }
        self.rtt_last = Some(rtt);
        self.rtt_history.push_back(rtt);
        if self.rtt_history.len() > RTT_HISTORY {
            self.rtt_history.pop_front();
        }
    }

    pub fn on_snapshot(&mut self, server_tick: u64, now: f64) {
        if let Some(last) = self.last_snapshot {
            let interval = now - last;
            // The first interval is the starting point, later ones are blended in.
            self.snapshot_interval_avg = if self.interval_samples == 0 {
                interval
            } else {
                self.snapshot_interval_avg + (interval - self.snapshot_interval_avg) * EWMA
            };
            self.interval_samples += 1;
        }
        self.last_snapshot = Some(now);
        self.server_tick = server_tick;
        self.snapshots.push_back(now);
        while self.snapshots.front().is_some_and(|&t| now - t > WINDOW_MS) {
            self.snapshots.pop_front();
        }
    }

    pub fn on_server_stats(&mut self, stats: ServerStats) {
        self.server = Some(stats);
    }

    /// Distance in blocks between where we predicted ourselves and where the server says we are.
    pub fn on_prediction_error(&mut self, blocks: f32) {
        self.prediction_error = blocks;
        self.prediction_error_max = self.prediction_error_max.max(blocks);
    }

    pub fn reset_prediction_peak(&mut self) {
        self.prediction_error_max = 0.0;
    }

    pub fn on_frame(&mut self, dt_ms: f64) {
        self.frame_ms_avg = if self.frame_ms_avg == 0.0 { dt_ms } else { self.frame_ms_avg + (dt_ms - self.frame_ms_avg) * 0.1 };
    }

    pub fn report(&mut self, now: f64) -> NetReport {
        let (bytes_in, messages_in) = self.incoming.per_second(now);
        let (bytes_out, messages_out) = self.outgoing.per_second(now);
        while self.snapshots.front().is_some_and(|&t| now - t > WINDOW_MS) {
            self.snapshots.pop_front();
        }
        let has_rtt = self.rtt_last.is_some();
        NetReport {
            rtt_ms: self.rtt_last,
            rtt_avg_ms: has_rtt.then_some(self.rtt_avg),
            rtt_min_ms: self.rtt_history.iter().copied().reduce(f64::min),
            rtt_max_ms: self.rtt_history.iter().copied().reduce(f64::max),
            jitter_ms: self.jitter,
            snapshots_per_s: self.snapshots.len() as f64 * 1000.0 / WINDOW_MS,
            snapshot_interval_ms: (self.snapshots.len() > 1).then_some(self.snapshot_interval_avg),
            snapshot_age_ms: self.last_snapshot.map(|t| (now - t).max(0.0)),
            server_tick: self.server_tick,
            bytes_in_per_s: bytes_in,
            bytes_out_per_s: bytes_out,
            messages_in_per_s: messages_in,
            messages_out_per_s: messages_out,
            total_in: self.incoming.total_bytes,
            total_out: self.outgoing.total_bytes,
            prediction_error: self.prediction_error,
            prediction_error_max: self.prediction_error_max,
            fps: if self.frame_ms_avg > 0.0 { 1000.0 / self.frame_ms_avg } else { 0.0 },
            frame_ms: self.frame_ms_avg,
            server: self.server,
        }
    }
}

fn kb(bytes: f64) -> String {
    if bytes >= 1024.0 * 1024.0 {
        format!("{:.1} MB", bytes / 1024.0 / 1024.0)
    } else {
        format!("{:.1} KB", bytes / 1024.0)
    }
}

/// How the overlay shows the numbers. Without a connection there is nothing network related to show.
pub fn format_report(r: &NetReport, connected: bool) -> String {
    let mut lines = Vec::new();
    lines.push(format!("{:.0} fps ({:.1} ms/frame)", r.fps, r.frame_ms));
    if !connected {
        lines.push("not connected to a server".to_string());
        return lines.join("\n");
    }
    lines.push(match (r.rtt_ms, r.rtt_avg_ms, r.rtt_min_ms, r.rtt_max_ms) {
        (Some(rtt), Some(avg), Some(min), Some(max)) => {
            format!("ping {rtt:.0} ms (avg {avg:.0}, min {min:.0}, max {max:.0}, jitter {:.1})", r.jitter_ms)
        }
        _ => "ping measuring...".to_string(),
    });
    lines.push(match (r.snapshot_interval_ms, r.snapshot_age_ms) {
        (Some(interval), Some(age)) => {
            format!("snapshots {:.0}/s, every {interval:.0} ms, last {age:.0} ms ago, server tick {}", r.snapshots_per_s, r.server_tick)
        }
        _ => "waiting for snapshots".to_string(),
    });
    lines.push(format!(
        "down {}/s ({:.0} msg/s)  up {}/s ({:.0} msg/s)  total {} / {}",
        kb(r.bytes_in_per_s),
        r.messages_in_per_s,
        kb(r.bytes_out_per_s),
        r.messages_out_per_s,
        kb(r.total_in as f64),
        kb(r.total_out as f64),
    ));
    lines.push(format!(
        "prediction error {:.2} blocks (peak {:.2}), other players drawn {:.0} ms behind",
        r.prediction_error,
        r.prediction_error_max,
        crate::interp::DELAY_MS
    ));
    if let Some(s) = &r.server {
        lines.push(format!(
            "server: {} players, {} entities, {} chunks, tick {:.2} ms (worst {:.1}), up {}s",
            s.players, s.entities, s.loaded_chunks, s.tick_ms_avg, s.tick_ms_max, s.uptime_s
        ));
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn last_rtt_is_none_until_a_pong_and_then_the_newest_sample() {
        let mut s = NetStats::new();
        assert_eq!(s.last_rtt(), None);
        s.on_pong(1000.0, 1040.0);
        s.on_pong(2000.0, 2025.0);
        assert_eq!(s.last_rtt(), Some(25.0));
    }

    #[test]
    fn rtt_is_the_difference_between_send_and_receive() {
        let mut s = NetStats::new();
        s.on_pong(1000.0, 1042.0);
        let r = s.report(1100.0);
        assert_eq!(r.rtt_ms, Some(42.0));
        assert_eq!(r.rtt_avg_ms, Some(42.0), "the first sample is the average");
        assert_eq!(r.jitter_ms, 0.0);
    }

    #[test]
    fn average_and_jitter_follow_the_samples() {
        let mut s = NetStats::new();
        for (i, rtt) in [40.0, 40.0, 40.0, 100.0, 40.0].into_iter().enumerate() {
            s.on_pong(i as f64 * 1000.0, i as f64 * 1000.0 + rtt);
        }
        let r = s.report(5000.0);
        assert_eq!(r.rtt_min_ms, Some(40.0));
        assert_eq!(r.rtt_max_ms, Some(100.0));
        assert!(r.rtt_avg_ms.unwrap() > 40.0 && r.rtt_avg_ms.unwrap() < 100.0);
        assert!(r.jitter_ms > 5.0, "a spike must show up as jitter, got {}", r.jitter_ms);

        let mut steady = NetStats::new();
        for i in 0..10 {
            steady.on_pong(i as f64 * 1000.0, i as f64 * 1000.0 + 50.0);
        }
        assert!(steady.report(10_000.0).jitter_ms < 1e-9, "constant latency has no jitter");
    }

    #[test]
    fn min_and_max_only_look_at_recent_pings() {
        let mut s = NetStats::new();
        s.on_pong(0.0, 500.0); // an old spike
        for i in 1..=RTT_HISTORY {
            s.on_pong(i as f64 * 100.0, i as f64 * 100.0 + 30.0);
        }
        assert_eq!(s.report(10_000.0).rtt_max_ms, Some(30.0));
    }

    #[test]
    fn a_clock_going_backwards_does_not_give_negative_latency() {
        let mut s = NetStats::new();
        s.on_pong(100.0, 90.0);
        assert_eq!(s.report(200.0).rtt_ms, Some(0.0));
    }

    #[test]
    fn traffic_rates_use_a_sliding_window_and_totals_keep_counting() {
        let mut s = NetStats::new();
        for i in 0..10 {
            s.on_received(100, i as f64 * 50.0); // 10 messages in the first 450 ms
        }
        s.on_sent(30, 100.0);
        let r = s.report(500.0);
        assert_eq!(r.bytes_in_per_s, 1000.0);
        assert_eq!(r.messages_in_per_s, 10.0);
        assert_eq!(r.bytes_out_per_s, 30.0);

        let later = s.report(1500.0); // the newest event (t = 450) is now more than a second old
        assert_eq!(later.messages_in_per_s, 0.0, "{later:?}");
        assert_eq!(later.total_in, 1000, "totals are not windowed");
        assert_eq!(later.total_out, 30);
    }

    #[test]
    fn snapshot_rate_interval_and_age() {
        let mut s = NetStats::new();
        for i in 0..30 {
            s.on_snapshot(i * 2, 1000.0 + i as f64 * 33.0);
        }
        let r = s.report(1000.0 + 29.0 * 33.0 + 12.0);
        assert!((r.snapshots_per_s - 30.0).abs() <= 1.0, "{}", r.snapshots_per_s);
        assert!((r.snapshot_interval_ms.unwrap() - 33.0).abs() < 1e-6);
        assert!((r.snapshot_age_ms.unwrap() - 12.0).abs() < 1e-6);
        assert_eq!(r.server_tick, 58);

        // Without new snapshots the age keeps growing: this is how a stalled connection shows up.
        assert!(s.report(5000.0).snapshot_age_ms.unwrap() > 3000.0);
        assert_eq!(s.report(5000.0).snapshots_per_s, 0.0);
    }

    #[test]
    fn prediction_error_tracks_a_peak_until_reset() {
        let mut s = NetStats::new();
        s.on_prediction_error(0.5);
        s.on_prediction_error(0.1);
        let r = s.report(0.0);
        assert_eq!((r.prediction_error, r.prediction_error_max), (0.1, 0.5));
        s.reset_prediction_peak();
        assert_eq!(s.report(0.0).prediction_error_max, 0.0);
    }

    #[test]
    fn frame_time_is_smoothed_into_fps() {
        let mut s = NetStats::new();
        for _ in 0..200 {
            s.on_frame(16.0);
        }
        let r = s.report(0.0);
        assert!((r.fps - 62.5).abs() < 0.5, "{}", r.fps);
    }

    #[test]
    fn an_empty_report_has_no_nan_and_formats() {
        let mut s = NetStats::new();
        let r = s.report(0.0);
        assert!(r.rtt_ms.is_none() && r.snapshot_age_ms.is_none());
        assert_eq!(r.fps, 0.0);
        let text = format_report(&r, true);
        assert!(text.contains("ping measuring") && text.contains("waiting for snapshots"), "{text}");
        assert!(!text.contains("NaN"));
        assert!(format_report(&r, false).contains("not connected"));
    }

    #[test]
    fn the_overlay_text_shows_the_important_numbers() {
        let mut s = NetStats::new();
        s.on_pong(0.0, 48.0);
        s.on_snapshot(120, 100.0);
        s.on_snapshot(122, 133.0);
        s.on_received(2048, 120.0);
        s.on_server_stats(ServerStats {
            players: 3,
            entities: 3,
            loaded_chunks: 9,
            tick_ms_avg: 0.25,
            tick_ms_max: 1.5,
            bytes_out: 0,
            bytes_in: 0,
            uptime_s: 77,
        });
        let text = format_report(&s.report(140.0), true);
        for needle in ["ping 48 ms", "server tick 122", "2.0 KB", "3 players", "9 chunks", "up 77s"] {
            assert!(text.contains(needle), "missing {needle:?} in:\n{text}");
        }
    }
}
