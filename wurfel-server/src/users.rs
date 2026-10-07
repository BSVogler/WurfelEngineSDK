//! Users of this server: who somebody is beyond one connection. A browser keeps a session secret
//! (localStorage) and sends it with every `Join`; the server finds the user it belongs to, or makes
//! a new user and hands out a new secret ([`ServerMsg::Session`]). So a page reload, a reconnect or
//! a server restart keeps your name, colour and admin rights.
//!
//! Only a SHA-256 hash of each secret is kept, so the users file does not hand out sessions. The
//! file is `users.json` in the maps folder (JSON, rewritten whole on every change; there are few
//! users and few changes).
//!
//! [`ServerMsg::Session`]: wurfel_sim::protocol::ServerMsg::Session

use std::collections::hash_map::RandomState;
use std::collections::HashMap;
use std::hash::{BuildHasher, Hasher};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const USERS_FILE: &str = "users.json";

/// Sessions a single user may have (one per browser). The oldest is dropped beyond that.
const MAX_SESSIONS_PER_USER: usize = 8;

pub type UserId = u32;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct User {
    pub id: UserId,
    pub name: String,
    pub color: [u8; 3],
    /// Logged in once with the admin token (`auth <token>`): may change the world for good.
    #[serde(default)]
    pub admin: bool,
    /// Seconds since 1970.
    pub created: u64,
    pub last_seen: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Session {
    user: UserId,
    /// When it was issued, to drop the oldest of a user's sessions first.
    issued: u64,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct File {
    next_id: UserId,
    users: Vec<User>,
    /// By the hex SHA-256 of the secret.
    sessions: HashMap<String, Session>,
}

#[derive(Debug, Default)]
pub struct Users {
    /// Where they are kept; `None` keeps them in memory only (tests, a throwaway world).
    path: Option<PathBuf>,
    data: File,
}

/// What a `Join` with a session secret leads to.
#[derive(Debug, Clone, PartialEq)]
pub struct Login {
    pub user: User,
    /// A new secret for the browser to keep, when the one it sent was unknown (or empty).
    pub new_secret: Option<String>,
}

/// 128 bits from the OS as hex (std seeds every `RandomState` with fresh random keys).
pub fn random_secret() -> String {
    let word = || RandomState::new().build_hasher().finish();
    format!("{:016x}{:016x}", word(), word())
}

fn hash(secret: &str) -> String {
    Sha256::digest(secret.as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

impl Users {
    /// Load the users file, or start empty if there is none. A broken file is kept aside as
    /// `users.json.broken` rather than overwritten.
    pub fn open(path: PathBuf) -> Users {
        let data = match std::fs::read_to_string(&path) {
            Ok(text) => serde_json::from_str(&text).unwrap_or_else(|e| {
                eprintln!("wurfel-server: {} is not readable ({e}); starting with no users", path.display());
                let _ = std::fs::rename(&path, path.with_extension("json.broken"));
                File::default()
            }),
            Err(_) => File::default(),
        };
        Users { path: Some(path), data }
    }

    #[cfg(test)]
    pub fn in_memory() -> Users {
        Users::default()
    }

    pub fn get(&self, id: UserId) -> Option<&User> {
        self.data.users.iter().find(|u| u.id == id)
    }

    fn get_mut(&mut self, id: UserId) -> Option<&mut User> {
        self.data.users.iter_mut().find(|u| u.id == id)
    }

    /// The user of `secret`, or a new user with a new secret if it is unknown. `name` and `color`
    /// are what the client asks for now: a non-empty name replaces the stored one.
    pub fn login(&mut self, secret: &str, name: &str, color: [u8; 3]) -> Login {
        let now = now();
        let known = (!secret.is_empty()).then(|| self.data.sessions.get(&hash(secret)).map(|s| s.user)).flatten();
        let (id, new_secret) = match known.filter(|&id| self.get(id).is_some()) {
            Some(id) => (id, None),
            None => {
                self.data.next_id = self.data.next_id.max(1);
                let id = self.data.next_id;
                self.data.next_id += 1;
                self.data.users.push(User { id, name: String::new(), color, admin: false, created: now, last_seen: now });
                let secret = random_secret();
                self.add_session(id, &secret, now);
                (id, Some(secret))
            }
        };
        let user = self.get_mut(id).expect("just found or made");
        if !name.trim().is_empty() {
            user.name = name.trim().to_string();
        }
        user.color = color;
        user.last_seen = now;
        let user = user.clone();
        self.save();
        Login { user, new_secret }
    }

    fn add_session(&mut self, user: UserId, secret: &str, now: u64) {
        self.data.sessions.insert(hash(secret), Session { user, issued: now });
        let mut mine: Vec<(String, u64)> =
            self.data.sessions.iter().filter(|(_, s)| s.user == user).map(|(h, s)| (h.clone(), s.issued)).collect();
        if mine.len() > MAX_SESSIONS_PER_USER {
            mine.sort_by_key(|(_, issued)| *issued);
            for (h, _) in &mine[..mine.len() - MAX_SESSIONS_PER_USER] {
                self.data.sessions.remove(h);
            }
        }
    }

    /// The user logged in with the admin token: they stay an admin.
    pub fn grant_admin(&mut self, id: UserId) {
        if let Some(user) = self.get_mut(id).filter(|u| !u.admin) {
            user.admin = true;
            self.save();
        }
    }

    /// Write the file (to a temporary file first, so a crash never leaves half a file).
    fn save(&self) {
        let Some(path) = &self.path else { return };
        let result = serde_json::to_string_pretty(&self.data).map_err(std::io::Error::other).and_then(|text| {
            let temp = path.with_extension("json.tmp");
            std::fs::write(&temp, text)?;
            std::fs::rename(&temp, path)
        });
        if let Err(e) = result {
            eprintln!("wurfel-server: cannot save {}: {e}", path.display());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_browser_gets_a_user_and_a_secret_that_brings_it_back() {
        let mut users = Users::in_memory();
        let first = users.login("", "Ann", [1, 2, 3]);
        let secret = first.new_secret.clone().expect("a new secret");
        assert_eq!(first.user.name, "Ann");
        let again = users.login(&secret, "", [4, 5, 6]);
        assert_eq!(again.user.id, first.user.id);
        assert_eq!(again.new_secret, None);
        assert_eq!(again.user.name, "Ann", "an empty name keeps the stored one");
        assert_eq!(again.user.color, [4, 5, 6]);
        // A secret the server does not know makes somebody new.
        let stranger = users.login("made-up", "Bob", [0, 0, 0]);
        assert_ne!(stranger.user.id, first.user.id);
        assert!(stranger.new_secret.is_some());
    }

    #[test]
    fn admin_rights_and_users_survive_a_restart_but_secrets_are_not_stored() {
        let dir = std::env::temp_dir().join(format!("wurfel-users-{}", random_secret()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(USERS_FILE);
        let (id, secret) = {
            let mut users = Users::open(path.clone());
            let login = users.login("", "Ann", [1, 2, 3]);
            users.grant_admin(login.user.id);
            (login.user.id, login.new_secret.unwrap())
        };
        assert!(!std::fs::read_to_string(&path).unwrap().contains(&secret), "only the hash is written");
        let mut users = Users::open(path);
        let login = users.login(&secret, "", [1, 2, 3]);
        assert_eq!(login.user.id, id);
        assert!(login.user.admin);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_user_keeps_only_the_newest_sessions() {
        let mut users = Users::in_memory();
        let id = users.login("", "Ann", [0; 3]).user.id;
        for i in 0..MAX_SESSIONS_PER_USER + 3 {
            users.add_session(id, &format!("s{i}"), i as u64);
        }
        assert_eq!(users.data.sessions.values().filter(|s| s.user == id).count(), MAX_SESSIONS_PER_USER);
        assert!(users.data.sessions.contains_key(&hash(&format!("s{}", MAX_SESSIONS_PER_USER + 2))));
        assert!(!users.data.sessions.contains_key(&hash("s0")));
    }
}
