//! When the game socket closes while playing (the server is being updated, or the network dropped),
//! the client keeps what it shows and tries to join again. This is only the bookkeeping: what to do
//! when a socket closes, and how long to wait before the next try. The sockets and timers stay in
//! `web.rs`, so this runs and is tested natively.

/// Waits between tries: 0.5 s, 1 s, 2 s, 4 s, then 5 s.
const FIRST_DELAY_MS: f64 = 500.0;
const MAX_DELAY_MS: f64 = 5000.0;
/// After this long without getting back in, the connection counts as lost for good.
pub const GIVE_UP_MS: f64 = 300_000.0;

/// How long to wait before try number `attempt` (0 is the first one after the connection broke).
pub fn backoff_ms(attempt: u32) -> f64 {
    (FIRST_DELAY_MS * 2f64.powi(attempt.min(10) as i32)).min(MAX_DELAY_MS)
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Phase {
    /// No session (the preview, or the player left on purpose).
    Off,
    /// The first connection of a session: no welcome yet.
    Connecting,
    /// In the world.
    Playing,
    /// The connection broke at `lost_at`; try number `attempt` is waiting for its timer.
    Waiting { lost_at: f64, attempt: u32 },
    /// Try number `attempt` has a socket open that has not delivered the welcome yet.
    Trying { lost_at: f64, attempt: u32 },
}

/// What a closing socket means.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Closed {
    /// Nothing to do: there is no session (the player left) or the event is stale.
    Ignore,
    /// The server never let us in (wrong address, no such world): report it, as before.
    Refused,
    /// Show "reconnecting" and call [`Reconnect::fire`] with `epoch` after `delay_ms`.
    Retry { delay_ms: f64, epoch: u32 },
    /// Out of time: report the connection as lost.
    GiveUp,
}

#[derive(Debug)]
pub struct Reconnect {
    phase: Phase,
    /// Changes with every session, so a timer of an old session does nothing.
    epoch: u32,
}

impl Reconnect {
    pub fn new() -> Self {
        Reconnect { phase: Phase::Off, epoch: 0 }
    }

    /// The player asked to join (menu: Connect).
    pub fn begin(&mut self) {
        self.epoch += 1;
        self.phase = Phase::Connecting;
    }

    /// The player left on purpose, or we gave up: never reconnect.
    pub fn stop(&mut self) {
        self.epoch += 1;
        self.phase = Phase::Off;
    }

    /// The server's welcome arrived: we are in (again).
    pub fn on_welcome(&mut self) {
        self.phase = Phase::Playing;
    }

    /// True from the moment the connection broke until we are back in or gave up.
    pub fn active(&self) -> bool {
        matches!(self.phase, Phase::Waiting { .. } | Phase::Trying { .. })
    }

    /// The number of the current try, starting at 1 (for the status text).
    pub fn attempt(&self) -> u32 {
        match self.phase {
            Phase::Waiting { attempt, .. } | Phase::Trying { attempt, .. } => attempt + 1,
            _ => 0,
        }
    }

    /// The socket closed (or could not be opened) at `now` (ms).
    pub fn on_closed(&mut self, now: f64) -> Closed {
        let (lost_at, attempt) = match self.phase {
            Phase::Off | Phase::Waiting { .. } => return Closed::Ignore,
            Phase::Connecting => {
                self.phase = Phase::Off;
                return Closed::Refused;
            }
            Phase::Playing => (now, 0),
            Phase::Trying { lost_at, attempt } => {
                if now - lost_at >= GIVE_UP_MS {
                    self.phase = Phase::Off;
                    return Closed::GiveUp;
                }
                (lost_at, attempt + 1)
            }
        };
        self.phase = Phase::Waiting { lost_at, attempt };
        Closed::Retry { delay_ms: backoff_ms(attempt), epoch: self.epoch }
    }

    /// The timer for a retry went off. True if a new socket should be opened now.
    pub fn fire(&mut self, epoch: u32) -> bool {
        match self.phase {
            Phase::Waiting { lost_at, attempt } if epoch == self.epoch => {
                self.phase = Phase::Trying { lost_at, attempt };
                true
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_delays_double_up_to_the_cap() {
        let delays: Vec<f64> = (0..7).map(backoff_ms).collect();
        assert_eq!(delays, [500.0, 1000.0, 2000.0, 4000.0, 5000.0, 5000.0, 5000.0]);
        assert_eq!(backoff_ms(u32::MAX), 5000.0, "no overflow for absurd counts");
    }

    fn playing() -> Reconnect {
        let mut r = Reconnect::new();
        r.begin();
        r.on_welcome();
        r
    }

    #[test]
    fn a_connection_lost_while_playing_is_retried_with_backoff() {
        let mut r = playing();
        let Closed::Retry { delay_ms, epoch } = r.on_closed(10_000.0) else { panic!("expected a retry") };
        assert_eq!(delay_ms, 500.0);
        assert!(r.active());
        assert_eq!(r.attempt(), 1);
        assert!(r.fire(epoch));
        // The try failed.
        assert_eq!(r.on_closed(10_600.0), Closed::Retry { delay_ms: 1000.0, epoch });
        assert!(r.fire(epoch));
        assert_eq!(r.on_closed(11_700.0), Closed::Retry { delay_ms: 2000.0, epoch });
        assert_eq!(r.attempt(), 3);
    }

    #[test]
    fn a_welcome_ends_the_reconnecting_and_the_next_loss_starts_over() {
        let mut r = playing();
        let Closed::Retry { epoch, .. } = r.on_closed(0.0) else { panic!() };
        assert!(r.fire(epoch));
        r.on_welcome();
        assert!(!r.active());
        assert_eq!(r.on_closed(50_000.0), Closed::Retry { delay_ms: 500.0, epoch }, "fresh backoff and a fresh 60 s");
    }

    #[test]
    fn it_gives_up_after_a_minute() {
        let mut r = playing();
        let Closed::Retry { epoch, .. } = r.on_closed(1000.0) else { panic!() };
        let mut now = 1000.0;
        loop {
            assert!(r.fire(epoch));
            now += 5000.0;
            match r.on_closed(now) {
                Closed::Retry { .. } => assert!(now - 1000.0 < GIVE_UP_MS),
                Closed::GiveUp => break,
                other => panic!("{other:?}"),
            }
        }
        assert!(now - 1000.0 >= GIVE_UP_MS);
        assert!(!r.active());
        assert_eq!(r.on_closed(now), Closed::Ignore, "after giving up nothing is retried");
    }

    #[test]
    fn leaving_on_purpose_never_reconnects() {
        let mut r = playing();
        r.stop();
        assert_eq!(r.on_closed(0.0), Closed::Ignore);
        // Also while a retry is pending: the old timer does nothing.
        let mut r = playing();
        let Closed::Retry { epoch, .. } = r.on_closed(0.0) else { panic!() };
        r.stop();
        assert!(!r.fire(epoch));
        assert!(!r.active());
    }

    #[test]
    fn a_timer_of_an_earlier_session_is_ignored() {
        let mut r = playing();
        let Closed::Retry { epoch: old, .. } = r.on_closed(0.0) else { panic!() };
        r.stop();
        r.begin();
        r.on_welcome();
        let Closed::Retry { epoch: new, .. } = r.on_closed(100.0) else { panic!() };
        assert_ne!(old, new);
        assert!(!r.fire(old));
        assert!(r.fire(new));
    }

    #[test]
    fn a_refused_first_connection_is_reported_not_retried() {
        let mut r = Reconnect::new();
        r.begin();
        assert_eq!(r.on_closed(0.0), Closed::Refused);
        assert!(!r.active());
        assert_eq!(r.on_closed(1.0), Closed::Ignore);
    }
}
