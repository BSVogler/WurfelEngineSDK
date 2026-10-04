//! Which chunks a player should have: everything within `radius` chunks of where they are,
//! nearest first, a few per update so a join does not flood the connection.

use std::collections::HashSet;

pub type ChunkPos = (i32, i32);

pub struct Interest {
    /// Chunks within this distance (in chunks, square) are sent.
    radius: i32,
    /// Chunks beyond `radius + hysteresis` are dropped; the margin stops a player standing on a
    /// chunk border from making the same chunks come and go.
    hysteresis: i32,
    /// At most this many chunks are returned per update.
    budget: usize,
    sent: HashSet<ChunkPos>,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Changes {
    pub send: Vec<ChunkPos>,
    pub unload: Vec<ChunkPos>,
}

impl Interest {
    pub fn new(radius: i32, budget: usize) -> Self {
        Interest { radius, hysteresis: 1, budget, sent: HashSet::new() }
    }

    /// What to send and drop now that the player is in chunk `center`. Chunks returned in `send`
    /// count as sent from here on.
    pub fn update(&mut self, center: ChunkPos) -> Changes {
        let mut wanted: Vec<ChunkPos> = Vec::new();
        for dx in -self.radius..=self.radius {
            for dy in -self.radius..=self.radius {
                let pos = (center.0 + dx, center.1 + dy);
                if !self.sent.contains(&pos) {
                    wanted.push(pos);
                }
            }
        }
        // Nearest first, so the chunk the player stands on arrives before the far corners.
        wanted.sort_by_key(|&(x, y)| ((x - center.0).abs().max((y - center.1).abs()), (x - center.0).pow(2) + (y - center.1).pow(2), x, y));
        wanted.truncate(self.budget);
        self.sent.extend(wanted.iter().copied());

        let keep = self.radius + self.hysteresis;
        let mut unload: Vec<ChunkPos> =
            self.sent.iter().copied().filter(|&(x, y)| (x - center.0).abs().max((y - center.1).abs()) > keep).collect();
        unload.sort_unstable();
        for pos in &unload {
            self.sent.remove(pos);
        }
        Changes { send: wanted, unload }
    }

    #[cfg(test)]
    pub fn sent_count(&self) -> usize {
        self.sent.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_update_sends_the_players_own_chunk_first() {
        let mut interest = Interest::new(2, 3);
        let changes = interest.update((4, -7));
        assert_eq!(changes.send[0], (4, -7), "own chunk first");
        assert_eq!(changes.send.len(), 3, "limited by the budget");
        assert!(changes.send.iter().all(|&(x, y)| (x - 4).abs() <= 1 && (y + 7).abs() <= 1), "then its neighbours");
        assert!(changes.unload.is_empty());
    }

    #[test]
    fn repeated_updates_send_everything_once_and_then_nothing() {
        let mut interest = Interest::new(2, 4);
        let mut all = Vec::new();
        for _ in 0..20 {
            all.extend(interest.update((0, 0)).send);
        }
        assert_eq!(all.len(), 25, "5 x 5 chunks within radius 2");
        let unique: HashSet<_> = all.iter().collect();
        assert_eq!(unique.len(), 25, "no chunk sent twice");
        assert!(interest.update((0, 0)).send.is_empty());
    }

    #[test]
    fn moving_sends_the_new_edge_and_drops_the_far_one() {
        let mut interest = Interest::new(1, 100);
        interest.update((0, 0)); // 3 x 3 around the origin
        assert_eq!(interest.sent_count(), 9);

        let changes = interest.update((1, 0));
        let mut send = changes.send.clone();
        send.sort_unstable();
        assert_eq!(send, vec![(2, -1), (2, 0), (2, 1)], "the new column");
        assert!(changes.unload.is_empty(), "the old column is within the hysteresis margin");

        let changes = interest.update((3, 0));
        assert!(changes.unload.contains(&(-1, 0)) && changes.unload.contains(&(0, -1)), "far chunks are dropped: {:?}", changes.unload);
        assert!(!changes.unload.contains(&(2, 0)), "near chunks stay");
    }

    #[test]
    fn standing_on_a_border_does_not_make_chunks_flicker() {
        let mut interest = Interest::new(1, 100);
        interest.update((0, 0));
        for center in [(1, 0), (0, 0), (1, 0), (0, 0), (1, 0)] {
            let changes = interest.update(center);
            assert!(changes.unload.is_empty(), "{center:?}: {:?}", changes.unload);
        }
        // Everything that was wanted has been sent once; going back and forth costs nothing more.
        assert!(interest.update((0, 0)).send.is_empty());
    }

    #[test]
    fn a_dropped_chunk_is_sent_again_when_the_player_returns() {
        let mut interest = Interest::new(1, 100);
        interest.update((0, 0));
        interest.update((10, 0));
        assert!(!interest.sent.contains(&(0, 0)));
        let back = interest.update((0, 0));
        assert!(back.send.contains(&(0, 0)));
    }

    #[test]
    fn negative_coordinates_work() {
        let mut interest = Interest::new(1, 100);
        let changes = interest.update((-5, -5));
        assert_eq!(changes.send.len(), 9);
        assert!(changes.send.contains(&(-6, -6)) && changes.send.contains(&(-4, -4)));
    }
}
