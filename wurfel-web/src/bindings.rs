//! Key and mouse bindings, as configured in the menu (`window.wurfelSettings.keys`).
//!
//! Each action has two slots holding a lowercased `KeyboardEvent.key` (" " is space) or
//! `"mouse0"` / `"mouse1"` / `"mouse2"`. An empty slot is unbound.

use std::collections::{HashMap, HashSet};

pub const ACTIONS: [&str; 9] = ["up", "down", "left", "right", "jump", "place", "break", "zoomIn", "zoomOut"];

#[derive(Debug, Clone, PartialEq)]
pub struct Bindings {
    slots: HashMap<String, Vec<String>>,
}

impl Default for Bindings {
    /// The same defaults as the menu, used until it reports its settings.
    fn default() -> Self {
        let defaults: [(&str, [&str; 2]); 9] = [
            ("up", ["w", "arrowup"]),
            ("down", ["s", "arrowdown"]),
            ("left", ["a", "arrowleft"]),
            ("right", ["d", "arrowright"]),
            ("jump", [" ", ""]),
            ("place", ["mouse0", ""]),
            ("break", ["mouse2", ""]),
            ("zoomIn", ["e", ""]),
            ("zoomOut", ["q", ""]),
        ];
        Bindings {
            slots: defaults.iter().map(|(a, keys)| (a.to_string(), keys.iter().map(|k| k.to_string()).collect())).collect(),
        }
    }
}

impl Bindings {
    /// Take the bindings from the menu's settings. Unknown actions are ignored and an action that
    /// is missing keeps its default, so a half-filled object cannot leave the player without controls.
    pub fn from_settings(settings: &HashMap<String, Vec<String>>) -> Self {
        let mut bindings = Bindings::default();
        for action in ACTIONS {
            if let Some(slots) = settings.get(action) {
                bindings.slots.insert(action.to_string(), slots.iter().map(|k| k.to_lowercase()).collect());
            }
        }
        bindings
    }

    fn bound(&self, action: &str) -> impl Iterator<Item = &str> {
        self.slots.get(action).into_iter().flatten().map(String::as_str).filter(|k| !k.is_empty())
    }

    /// Is any key of this action in the set of currently held keys?
    pub fn held(&self, action: &str, held: &HashSet<String>) -> bool {
        self.bound(action).any(|k| held.contains(k))
    }

    pub fn matches_key(&self, action: &str, key: &str) -> bool {
        let key = key.to_lowercase();
        self.bound(action).any(|k| k == key)
    }

    pub fn matches_button(&self, action: &str, button: i16) -> bool {
        self.bound(action).any(|k| k.strip_prefix("mouse").and_then(|n| n.parse::<i16>().ok()) == Some(button))
    }
}

/// `#rrggbb` (or `rrggbb`) as red, green, blue. Anything else is `None`.
pub fn parse_hex_color(text: &str) -> Option<[u8; 3]> {
    let hex = text.trim().trim_start_matches('#');
    if hex.len() != 6 || !hex.is_ascii() {
        return None;
    }
    let channel = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
    Some([channel(0)?, channel(2)?, channel(4)?])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn held(keys: &[&str]) -> HashSet<String> {
        keys.iter().map(|k| k.to_string()).collect()
    }

    #[test]
    fn defaults_match_the_menu() {
        let b = Bindings::default();
        assert!(b.held("up", &held(&["w"])) && b.held("up", &held(&["arrowup"])));
        assert!(b.held("jump", &held(&[" "])));
        assert!(!b.held("up", &held(&["s"])));
        assert!(b.matches_button("place", 0) && b.matches_button("break", 2));
        assert!(!b.matches_button("place", 2));
        assert!(b.matches_key("zoomIn", "E"), "event keys arrive in either case");
    }

    #[test]
    fn rebinding_replaces_the_defaults_of_that_action_only() {
        let mut settings = HashMap::new();
        settings.insert("jump".to_string(), vec!["x".to_string(), "".to_string()]);
        let b = Bindings::from_settings(&settings);
        assert!(b.held("jump", &held(&["x"])));
        assert!(!b.held("jump", &held(&[" "])), "the old binding is gone");
        assert!(b.held("up", &held(&["w"])), "other actions keep their defaults");
    }

    #[test]
    fn empty_slots_never_match_and_unknown_actions_are_ignored() {
        let mut settings = HashMap::new();
        settings.insert("up".to_string(), vec!["".to_string(), "".to_string()]);
        settings.insert("fly".to_string(), vec!["f".to_string()]);
        let b = Bindings::from_settings(&settings);
        assert!(!b.held("up", &held(&[""])), "an unbound action cannot be triggered by an empty key");
        assert!(!b.matches_key("up", ""));
        assert!(!b.matches_key("fly", "f"));
    }

    #[test]
    fn the_mouse_can_be_bound_to_anything() {
        let mut settings = HashMap::new();
        settings.insert("jump".to_string(), vec!["mouse1".to_string()]);
        settings.insert("place".to_string(), vec!["q".to_string()]);
        let b = Bindings::from_settings(&settings);
        assert!(b.matches_button("jump", 1) && !b.matches_button("jump", 0));
        assert!(b.matches_key("place", "q") && !b.matches_button("place", 0));
        assert!(!b.matches_button("jump", -1));
    }

    #[test]
    fn hex_colours_parse_strictly() {
        assert_eq!(parse_hex_color("#ff8000"), Some([255, 128, 0]));
        assert_eq!(parse_hex_color(" 00ff7F "), Some([0, 255, 127]));
        for bad in ["", "#fff", "#gg0000", "#12345678", "red", "#ff80", "ééé"] {
            assert_eq!(parse_hex_color(bad), None, "{bad:?}");
        }
    }
}
