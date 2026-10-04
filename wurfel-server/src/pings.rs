//! Each player's latest round-trip time, as reported by the player's own client in its `Ping`
//! messages. The server cannot measure it better than the client can, and it is only shown in the
//! Tab player list, so it is taken on trust but kept within a sane range.

use std::collections::HashMap;

/// Longer than this is shown as this: the number only has to say "bad".
const MAX_PING_MS: f32 = 9999.0;

#[derive(Debug, Default)]
pub struct Pings {
    by_player: HashMap<u32, u32>,
}

impl Pings {
    /// Remember a report. Not-a-number, negative and absurd values are clamped, never rejected.
    pub fn set(&mut self, player: u32, rtt_ms: f32) {
        let ms = if rtt_ms.is_finite() { rtt_ms.clamp(0.0, MAX_PING_MS) } else { MAX_PING_MS };
        self.by_player.insert(player, ms.round() as u32);
    }

    pub fn remove(&mut self, player: u32) {
        self.by_player.remove(&player);
    }

    /// `(player, ms)` for everyone who has reported, ordered by player id.
    pub fn list(&self) -> Vec<(u32, u32)> {
        let mut list: Vec<(u32, u32)> = self.by_player.iter().map(|(&id, &ms)| (id, ms)).collect();
        list.sort_unstable();
        list
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_reports_in_player_order_and_keeps_the_latest() {
        let mut pings = Pings::default();
        pings.set(7, 40.4);
        pings.set(2, 15.0);
        pings.set(7, 55.6);
        assert_eq!(pings.list(), vec![(2, 15), (7, 56)]);
    }

    #[test]
    fn removed_players_disappear() {
        let mut pings = Pings::default();
        pings.set(1, 10.0);
        pings.set(2, 20.0);
        pings.remove(1);
        assert_eq!(pings.list(), vec![(2, 20)]);
    }

    #[test]
    fn silly_values_are_clamped() {
        let mut pings = Pings::default();
        pings.set(1, -5.0);
        pings.set(2, f32::NAN);
        pings.set(3, 1.0e9);
        assert_eq!(pings.list(), vec![(1, 0), (2, 9999), (3, 9999)]);
    }
}
