//! Friendships between players on this server. They last as long as both are connected and are
//! forgotten when the server restarts. A heart in the Tab list is an invite; the other player
//! hearts back to accept, and from then on both count as friends.

use std::collections::HashSet;

#[derive(Debug, Default)]
pub struct Friends {
    /// Accepted friendships, each stored once as `(smaller id, larger id)`.
    pairs: HashSet<(u32, u32)>,
    /// Open invites as `(from, to)`.
    invites: HashSet<(u32, u32)>,
}

/// What one player sees: everything is a list of player ids in ascending order.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct View {
    pub friends: Vec<u32>,
    pub sent: Vec<u32>,
    pub received: Vec<u32>,
}

fn pair(a: u32, b: u32) -> (u32, u32) {
    (a.min(b), a.max(b))
}

impl Friends {
    /// `from` hearts or un-hearts `to`. Returns whether anything changed.
    ///
    /// Hearting somebody who already invited you accepts their invite; un-hearting withdraws your
    /// own invite, declines theirs, or ends the friendship, whichever there is.
    pub fn heart(&mut self, from: u32, to: u32, on: bool) -> bool {
        if from == to {
            return false;
        }
        if on {
            if self.pairs.contains(&pair(from, to)) || self.invites.contains(&(from, to)) {
                return false;
            }
            if self.invites.remove(&(to, from)) {
                self.pairs.insert(pair(from, to));
            } else {
                self.invites.insert((from, to));
            }
            true
        } else {
            let ended = self.pairs.remove(&pair(from, to));
            let withdrawn = self.invites.remove(&(from, to));
            let declined = self.invites.remove(&(to, from));
            ended || withdrawn || declined
        }
    }

    #[cfg(test)]
    pub fn are_friends(&self, a: u32, b: u32) -> bool {
        a != b && self.pairs.contains(&pair(a, b))
    }

    /// A player left: forget all their friendships and invites. Returns the players who had one
    /// with them, so they can be told.
    pub fn remove_player(&mut self, id: u32) -> Vec<u32> {
        let mut affected: Vec<u32> = self
            .pairs
            .iter()
            .filter(|&&(a, b)| a == id || b == id)
            .map(|&(a, b)| if a == id { b } else { a })
            .chain(self.invites.iter().filter(|&&(a, b)| a == id || b == id).map(|&(a, b)| if a == id { b } else { a }))
            .collect();
        affected.sort_unstable();
        affected.dedup();
        self.pairs.retain(|&(a, b)| a != id && b != id);
        self.invites.retain(|&(a, b)| a != id && b != id);
        affected
    }

    /// All friendships, each once as `(smaller id, larger id)`, in order.
    pub fn pairs(&self) -> Vec<(u32, u32)> {
        let mut pairs: Vec<(u32, u32)> = self.pairs.iter().copied().collect();
        pairs.sort_unstable();
        pairs
    }

    pub fn view(&self, player: u32) -> View {
        let mut view = View::default();
        for &(a, b) in &self.pairs {
            if a == player {
                view.friends.push(b);
            } else if b == player {
                view.friends.push(a);
            }
        }
        for &(from, to) in &self.invites {
            if from == player {
                view.sent.push(to);
            } else if to == player {
                view.received.push(from);
            }
        }
        view.friends.sort_unstable();
        view.sent.sort_unstable();
        view.received.sort_unstable();
        view
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_heart_is_an_invite_until_it_is_returned() {
        let mut f = Friends::default();
        assert!(f.heart(1, 2, true));
        assert_eq!(f.view(1), View { friends: vec![], sent: vec![2], received: vec![] });
        assert_eq!(f.view(2), View { friends: vec![], sent: vec![], received: vec![1] });
        assert!(!f.are_friends(1, 2), "an open invite is not a friendship");

        assert!(f.heart(2, 1, true), "hearting back accepts");
        assert!(f.are_friends(1, 2) && f.are_friends(2, 1));
        assert_eq!(f.view(1), View { friends: vec![2], sent: vec![], received: vec![] });
        assert_eq!(f.view(2), View { friends: vec![1], sent: vec![], received: vec![] });
    }

    #[test]
    fn repeating_a_heart_changes_nothing() {
        let mut f = Friends::default();
        assert!(f.heart(1, 2, true));
        assert!(!f.heart(1, 2, true), "the invite is already there");
        f.heart(2, 1, true);
        assert!(!f.heart(1, 2, true), "already friends");
        assert!(!f.heart(2, 1, true));
        assert!(!f.heart(3, 3, true), "nobody befriends themselves");
    }

    #[test]
    fn taking_a_heart_back_withdraws_declines_or_ends() {
        let mut f = Friends::default();
        f.heart(1, 2, true);
        assert!(f.heart(1, 2, false), "withdraw my invite");
        assert_eq!(f.view(2), View::default());

        f.heart(1, 2, true);
        assert!(f.heart(2, 1, false), "decline their invite");
        assert_eq!(f.view(1), View::default());
        assert!(f.heart(2, 1, true), "after a decline the other may invite in turn");
        assert_eq!(f.view(1).received, vec![2]);

        let mut f = Friends::default();
        f.heart(1, 2, true);
        f.heart(2, 1, true);
        assert!(f.heart(2, 1, false), "either side can end it");
        assert!(!f.are_friends(1, 2));
        assert!(!f.heart(2, 1, false), "nothing left to take back");
    }

    #[test]
    fn leaving_clears_everything_and_names_who_to_tell() {
        let mut f = Friends::default();
        f.heart(1, 2, true);
        f.heart(2, 1, true); // 1 and 2 are friends
        f.heart(3, 1, true); // 3 invited 1
        f.heart(1, 4, true); // 1 invited 4
        f.heart(5, 6, true); // unrelated
        assert_eq!(f.remove_player(1), vec![2, 3, 4]);
        assert_eq!(f.view(1), View::default());
        assert_eq!(f.view(2), View::default());
        assert_eq!(f.view(5).sent, vec![6], "others are untouched");
        assert!(f.remove_player(1).is_empty());
    }
}
