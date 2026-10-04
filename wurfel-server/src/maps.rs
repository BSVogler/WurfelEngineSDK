//! Maps and savegames on the server's disk. No networking in here.
//!
//! The Java engine keeps maps in a maps folder; this is the same layout, except that it lives on the
//! server and clients receive the chunks over the network instead of reading them from their own disk.
//!
//! ```text
//! <root>/<map id>/meta.wecvar                the map's cvars: mapname, description, generator, generatorSeed
//! <root>/<map id>/chunk<x>,<y>.wec           optional chunks shipped with the map (read-only "root copy")
//! <root>/<map id>/save<N>/chunk<x>,<y>.wec   what save slot N has changed
//! <root>/<map id>/save<N>/meta.wecvar        the save's cvars (sun/moon azimuth, ...)
//! ```
//!
//! Java correspondence: `LoadMenu`/`MapButton` list the folders and show `mapname` and `description`
//! (fallbacks "no map name set" / ""), and clicking a map creates a new save slot
//! (`Map.newSaveSlot`: the first free `save<N>`) and loads it. Java copies the map folder's files
//! into a new slot; here the chunk store falls back to the root copy on its own
//! (`wurfel_sim::storage::ChunkStore`), so a new slot starts empty.
//!
//! Differences from Java: the map's generator is data (`generator` / `generatorSeed` cvars) instead of
//! code; sizes read from disk are capped; and a map id received from the network is never joined onto
//! a path: it is looked up among the folder names that actually exist.
//!
//! # Existing Java maps
//!
//! A maps folder written by the Java engine (`~/Library/Application Support/Wurfel Engine/maps` on
//! macOS, `~/.Wurfel Engine/maps` on Linux, `%APPDATA%\Wurfel Engine\maps` on Windows) can be served as
//! it is: point the server at it. Any folder name is accepted for *existing* maps (Java allowed
//! spaces and capitals); only *new* maps made through [`MapStore::create_map`] are restricted to
//! `[a-z0-9_-]{1,32}`. A map without a `generator` cvar, which is what Java maps look like, uses the
//! `air` generator for chunks that have no file, like Java's default generator. Files in the map
//! folder itself (`chunk<x>,<y>.wec`, `meta.wecvar`) are the read-only base: every write goes to
//! `save<N>/`, and a new save slot does not touch the existing files.
//!
//! Only one (map, save slot) is held in memory at a time by the server; listing a disk map does not
//! load it. Opening one builds a `World` whose chunk store reads `save<N>/`, then the root copy,
//! then asks the generator.
//!
//! Zipped maps (`tools/mapzip.sh` writes `map.zip` with the map folder inside) are not imported by the
//! server: unzip them into the maps folder (`unzip map.zip -d <maps folder>`). A safe in-process
//! unzip (path traversal, symlinks, size bombs) is not worth a new dependency yet.
//!
//! Chunk files of the 2013 text format (`//0` followed by `id:value` cells, see
//! `fixtures/maps/legacy-2013/`) are not supported: the reader reports them as broken files.
#![allow(dead_code)] // until the HTTP layer uses it

use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use wurfel_sim::cvar::CVarSystem;
use wurfel_sim::generator::{create_generator, generators, Generator};
use wurfel_sim::storage::ChunkStore;

/// A server stores at most this many maps.
pub const MAX_MAPS: usize = 256;
/// A map has at most this many save slots.
pub const MAX_SAVES: u32 = 1000;
/// Meta files larger than this are treated as unreadable.
pub const MAX_META_BYTES: u64 = 64 * 1024;
pub const MAX_ID_LEN: usize = 32;
pub const MAX_NAME_CHARS: usize = 60;
pub const MAX_DESCRIPTION_CHARS: usize = 500;

/// Shown when a map has no name, like Java's `MapButton`.
pub const NO_NAME: &str = "no map name set";
const META_FILE: &str = "meta.wecvar";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SaveInfo {
    pub slot: u32,
    /// ISO-8601 UTC time of the newest chunk file, `None` if the slot has no chunks yet.
    pub modified: Option<String>,
    /// Number of chunk files in the slot.
    pub chunks: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MapInfo {
    /// The folder name.
    pub id: String,
    pub name: String,
    pub description: String,
    pub generator: String,
    pub seed: u64,
    pub saves: Vec<SaveInfo>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MapCreate {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub generator: String,
    #[serde(default = "default_seed")]
    pub seed: u64,
}

fn default_seed() -> u64 {
    1
}

#[derive(Debug)]
pub enum MapError {
    Exists,
    NoSuchMap(String),
    NoSuchSave(u32),
    /// The request is not acceptable; the text says why and is fit for showing to a user.
    Invalid(String),
    UnknownGenerator(String),
    Io(io::Error),
}

impl std::fmt::Display for MapError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MapError::Exists => write!(f, "a map with that id already exists"),
            MapError::NoSuchMap(id) => write!(f, "there is no map '{}'", printable(id)),
            MapError::NoSuchSave(slot) => write!(f, "there is no save {slot} for that map"),
            MapError::Invalid(why) => write!(f, "{why}"),
            MapError::UnknownGenerator(id) => {
                let known: Vec<&str> = generators().iter().map(|g| g.id).collect();
                write!(f, "unknown generator '{}' (available: {})", printable(id), known.join(", "))
            }
            MapError::Io(e) => write!(f, "the server could not access its map storage: {e}"),
        }
    }
}

impl std::error::Error for MapError {}

impl From<io::Error> for MapError {
    fn from(e: io::Error) -> Self {
        MapError::Io(e)
    }
}

/// Text from a client, safe to echo back in a message.
fn printable(text: &str) -> String {
    text.chars().filter(|c| !c.is_control()).take(40).collect()
}

/// Everything needed to build the in-memory world: `World::with_store(generator, store)`.
pub struct OpenedWorld {
    pub map: MapInfo,
    pub slot: u32,
    pub generator: Box<dyn Generator>,
    pub store: ChunkStore,
    /// The map's cvars as loaded from `meta.wecvar`.
    pub map_cvars: CVarSystem,
    /// The save slot's cvars as loaded from `save<N>/meta.wecvar` (defaults if none was saved yet).
    pub save_cvars: CVarSystem,
}

pub struct MapStore {
    root: PathBuf,
}

/// Rules for the id of a NEW map: 1-32 characters of `[a-z0-9_-]`.
pub fn valid_map_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= MAX_ID_LEN && id.bytes().all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'_' | b'-'))
}

impl MapStore {
    /// Use `root` as the maps folder, creating it if it does not exist.
    pub fn open(root: PathBuf) -> io::Result<MapStore> {
        fs::create_dir_all(&root)?;
        Ok(MapStore { root })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The directory for a new map; the id must follow the rules for new ids.
    fn new_map_dir(&self, id: &str) -> Result<PathBuf, MapError> {
        if !valid_map_id(id) {
            return Err(MapError::Invalid(
                "a map id has 1-32 characters: lowercase letters, digits, '-' and '_'".into(),
            ));
        }
        Ok(self.root.join(id))
    }

    /// The folder names that count as maps, sorted: real directories (not symlinks) whose name is
    /// UTF-8, not hidden and free of control characters.
    fn folder_names(&self) -> io::Result<Vec<(String, PathBuf)>> {
        let mut found = Vec::new();
        for entry in fs::read_dir(&self.root)? {
            let Ok(entry) = entry else { continue };
            if !entry.file_type().is_ok_and(|t| t.is_dir()) {
                continue; // files and symlinks (file_type does not follow links)
            }
            if let Some(name) = entry.file_name().to_str().filter(|n| listable_name(n)) {
                found.push((name.to_string(), entry.path()));
            }
        }
        found.sort();
        found.truncate(MAX_MAPS);
        Ok(found)
    }

    /// The directory of an existing map. The id comes from a request, so it is never joined onto a
    /// path: it must equal the name of a folder that is really there, and the returned path is the
    /// one the file system gave us for it.
    fn existing_map_dir(&self, id: &str) -> Result<PathBuf, MapError> {
        self.folder_names()?
            .into_iter()
            .find(|(name, _)| name == id)
            .map(|(_, path)| path)
            .ok_or_else(|| MapError::NoSuchMap(id.to_string()))
    }

    /// All maps, sorted by id. Folders that are not valid map ids, files and symlinks are ignored, and
    /// unreadable meta files fall back to defaults, so one broken map never hides the others.
    pub fn list(&self) -> io::Result<Vec<MapInfo>> {
        Ok(self.folder_names()?.iter().map(|(id, dir)| describe(id, dir)).collect())
    }

    /// One map, or `NoSuchMap`.
    pub fn get(&self, id: &str) -> Result<MapInfo, MapError> {
        let dir = self.existing_map_dir(id)?;
        Ok(describe(id, &dir))
    }

    pub fn create_map(&self, request: &MapCreate) -> Result<MapInfo, MapError> {
        let dir = self.new_map_dir(&request.id)?;
        let name = request.name.trim();
        let description = request.description.trim();
        if name.is_empty() {
            return Err(MapError::Invalid("the map needs a name".into()));
        }
        if name.chars().count() > MAX_NAME_CHARS || name.chars().any(char::is_control) {
            return Err(MapError::Invalid(format!("the name must be at most {MAX_NAME_CHARS} characters without control characters")));
        }
        if description.chars().count() > MAX_DESCRIPTION_CHARS || description.chars().any(char::is_control) {
            return Err(MapError::Invalid(format!(
                "the description must be at most {MAX_DESCRIPTION_CHARS} characters without control characters"
            )));
        }
        if !generators().iter().any(|g| g.id == request.generator) {
            return Err(MapError::UnknownGenerator(request.generator.clone()));
        }
        // The seed is stored in a signed 32 bit cvar (see `generator_from_cvars`).
        let seed = u32::try_from(request.seed)
            .map_err(|_| MapError::Invalid(format!("the seed must be a whole number from 0 to {}", u32::MAX)))?;
        if self.list()?.len() >= MAX_MAPS {
            return Err(MapError::Invalid(format!("this server stores at most {MAX_MAPS} maps")));
        }

        match fs::create_dir(&dir) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => return Err(MapError::Exists),
            Err(e) => return Err(e.into()),
        }
        let written = (|| -> Result<(), MapError> {
            let mut cvars = CVarSystem::map();
            let bad = |e: wurfel_sim::cvar::CVarError| MapError::Invalid(e.to_string());
            cvars.set_str("mapname", name).map_err(bad)?;
            cvars.set_str("description", description).map_err(bad)?;
            // The cvar file format leaves out values that equal their default. A map whose generator
            // is the default would then look like a Java map without one (which means `air`), so the
            // generator and seed are always written out.
            let mut text = String::new();
            for line in cvars.save_string().lines() {
                let first = line.split_whitespace().next().unwrap_or("");
                if !first.eq_ignore_ascii_case("generator") && !first.eq_ignore_ascii_case("generatorSeed") {
                    text.push_str(line);
                    text.push('\n');
                }
            }
            text.push_str(&format!("generator {}\ngeneratorSeed {}\n", request.generator, seed as i32));
            fs::write(dir.join(META_FILE), text)?;
            Ok(())
        })();
        if let Err(e) = written {
            let _ = fs::remove_dir_all(&dir); // do not leave a half-made map behind
            return Err(e);
        }
        Ok(describe(&request.id, &dir))
    }

    /// Create the first free save slot and return its number (Java `Map.newSaveSlot`).
    pub fn new_save_slot(&self, map_id: &str) -> Result<u32, MapError> {
        let dir = self.existing_map_dir(map_id)?;
        for slot in 0..MAX_SAVES {
            // create_dir is atomic: if two requests race, one of them gets the next number.
            match fs::create_dir(dir.join(format!("save{slot}"))) {
                Ok(()) => return Ok(slot),
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e.into()),
            }
        }
        Err(MapError::Invalid(format!("a map has at most {MAX_SAVES} saves")))
    }

    pub fn save_slot_exists(&self, map_id: &str, slot: u32) -> bool {
        self.existing_map_dir(map_id).is_ok_and(|dir| slot < MAX_SAVES && is_real_dir(&dir.join(format!("save{slot}"))))
    }

    /// Everything to load `slot` of a map into memory.
    pub fn open_world(&self, map_id: &str, slot: u32) -> Result<OpenedWorld, MapError> {
        let dir = self.existing_map_dir(map_id)?;
        if !self.save_slot_exists(map_id, slot) {
            return Err(MapError::NoSuchSave(slot));
        }
        let map = describe(map_id, &dir);
        let generator = create_generator(&map.generator, map.seed).ok_or_else(|| MapError::UnknownGenerator(map.generator.clone()))?;
        let map_cvars = load_cvars(CVarSystem::map(), &dir.join(META_FILE));
        let save_cvars = load_cvars(CVarSystem::save_slot(), &dir.join(format!("save{slot}")).join(META_FILE));
        Ok(OpenedWorld { map, slot, generator, store: ChunkStore::new(dir, slot), map_cvars, save_cvars })
    }

    /// Persist the save slot's cvars (`save<N>/meta.wecvar`, Java `CVarSystemSave`).
    pub fn save_save_cvars(&self, map_id: &str, slot: u32, cvars: &CVarSystem) -> Result<(), MapError> {
        let dir = self.existing_map_dir(map_id)?;
        if !self.save_slot_exists(map_id, slot) {
            return Err(MapError::NoSuchSave(slot));
        }
        cvars.save(dir.join(format!("save{slot}")).join(META_FILE))?;
        Ok(())
    }

    /// A fresh server has no maps: make the default island so there is something to load. Returns
    /// the map if it was created.
    pub fn ensure_default_map(&self) -> Result<Option<MapInfo>, MapError> {
        if !self.list()?.is_empty() {
            return Ok(None);
        }
        let request = MapCreate {
            id: "island".into(),
            name: "Island".into(),
            description: "One mountain rising from a shallow sea.".into(),
            generator: "island".into(),
            seed: 1,
        };
        match self.create_map(&request) {
            Ok(map) => Ok(Some(map)),
            Err(MapError::Exists) => Ok(None), // someone else just made it
            Err(e) => Err(e),
        }
    }
}

/// A folder name that can be listed as a map: not hidden, no control characters.
fn listable_name(name: &str) -> bool {
    !name.is_empty() && !name.starts_with('.') && !name.chars().any(char::is_control)
}

/// Does the meta file give `name` a value? (The cvar system cannot tell "absent" from "default".)
fn declares(meta: &str, name: &str) -> bool {
    meta.lines().any(|line| {
        let mut words = line.split_whitespace();
        words.next().is_some_and(|w| w.eq_ignore_ascii_case(name)) && words.next().is_some()
    })
}

fn describe(id: &str, dir: &Path) -> MapInfo {
    let meta = read_capped(&dir.join(META_FILE));
    let mut cvars = CVarSystem::map();
    if let Some(text) = &meta {
        cvars.load_str(text);
    }
    let text = |name: &str, max: usize| -> String {
        cvars.get_str(name).map(|s| s.chars().filter(|c| !c.is_control()).take(max).collect()).unwrap_or_default()
    };
    let name = text("mapname", MAX_NAME_CHARS);
    // Java maps do not name a generator; Java then generates chunks without a file with its default
    // generator, which makes air.
    let generator = if meta.as_deref().is_some_and(|m| declares(m, "generator")) { text("generator", MAX_ID_LEN) } else { "air".to_string() };
    MapInfo {
        id: id.to_string(),
        name: if name.trim().is_empty() { NO_NAME.to_string() } else { name },
        description: text("description", MAX_DESCRIPTION_CHARS),
        generator,
        seed: cvars.get_i32("generatorSeed").unwrap_or(1) as u32 as u64,
        saves: scan_saves(dir),
    }
}

/// The file's text, unless it is missing, too large or not UTF-8.
fn read_capped(path: &Path) -> Option<String> {
    let file = fs::File::open(path).ok()?;
    let mut text = String::new();
    file.take(MAX_META_BYTES + 1).read_to_string(&mut text).ok()?;
    (text.len() as u64 <= MAX_META_BYTES).then_some(text)
}

fn is_real_dir(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_dir())
}

/// Read a meta file with a size cap and apply it to `cvars`. Anything wrong with the file leaves the
/// defaults in place.
fn load_cvars(mut cvars: CVarSystem, path: &Path) -> CVarSystem {
    if let Some(text) = read_capped(path) {
        cvars.load_str(&text);
    }
    cvars
}

fn scan_saves(map_dir: &Path) -> Vec<SaveInfo> {
    let Ok(entries) = fs::read_dir(map_dir) else { return Vec::new() };
    let mut saves = Vec::new();
    for entry in entries.flatten() {
        if !entry.file_type().is_ok_and(|t| t.is_dir()) {
            continue;
        }
        let name = entry.file_name();
        let Some(digits) = name.to_str().and_then(|n| n.strip_prefix("save")) else { continue };
        // Only the canonical spelling (no '+', no leading zeros) is a save slot.
        let Ok(slot) = digits.parse::<u32>() else { continue };
        if slot >= MAX_SAVES || slot.to_string() != digits {
            continue;
        }
        let (mut chunks, mut newest): (usize, Option<SystemTime>) = (0, None);
        if let Ok(files) = fs::read_dir(entry.path()) {
            for file in files.flatten() {
                let name = file.file_name();
                let name = name.to_string_lossy();
                if name.starts_with("chunk") && name.ends_with(".wec") && file.file_type().is_ok_and(|t| t.is_file()) {
                    chunks += 1;
                    if let Ok(time) = file.metadata().and_then(|m| m.modified()) {
                        newest = Some(newest.map_or(time, |n| n.max(time)));
                    }
                }
            }
        }
        saves.push(SaveInfo { slot, modified: newest.and_then(iso8601), chunks });
    }
    saves.sort_by_key(|s| s.slot);
    saves
}

/// `2026-10-04T18:00:00Z`. `None` for times before 1970.
pub fn iso8601(time: SystemTime) -> Option<String> {
    let seconds = time.duration_since(UNIX_EPOCH).ok()?.as_secs() as i64;
    let (days, rest) = (seconds.div_euclid(86_400), seconds.rem_euclid(86_400));
    let (year, month, day) = civil_from_days(days);
    Some(format!("{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z", rest / 3600, rest % 3600 / 60, rest % 60))
}

/// Days since 1970-01-01 to (year, month, day) in the proleptic Gregorian calendar
/// (Howard Hinnant's `civil_from_days`).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z.rem_euclid(146_097);
    let year_of_era = (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * month_index + 2) / 5 + 1) as u32;
    let month = if month_index < 10 { month_index + 3 } else { month_index - 9 } as u32;
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::time::Duration;
    use wurfel_sim::block::id;
    use wurfel_sim::chunk::Chunk;
    use wurfel_sim::{Block, World};

    /// A unique directory under the system temp dir, removed on drop. The maps folder is a
    /// subdirectory so tests can check that nothing is ever created next to it.
    struct Sandbox {
        parent: PathBuf,
    }

    impl Sandbox {
        fn new() -> Self {
            static COUNTER: AtomicU32 = AtomicU32::new(0);
            let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().subsec_nanos();
            let parent = std::env::temp_dir().join(format!("wurfel-maps-test-{}-{}-{nanos}", std::process::id(), COUNTER.fetch_add(1, Ordering::SeqCst)));
            fs::create_dir_all(&parent).unwrap();
            Sandbox { parent }
        }

        fn root(&self) -> PathBuf {
            self.parent.join("maps")
        }

        fn store(&self) -> MapStore {
            MapStore::open(self.root()).unwrap()
        }

        /// Names of everything in the sandbox next to the maps folder.
        fn siblings(&self) -> Vec<String> {
            let mut names: Vec<String> =
                fs::read_dir(&self.parent).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
            names.sort();
            names
        }
    }

    impl Drop for Sandbox {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.parent);
        }
    }

    fn request(id: &str) -> MapCreate {
        MapCreate { id: id.into(), name: "My island".into(), description: "A test map".into(), generator: "island".into(), seed: 7 }
    }

    fn at(seconds: u64) -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(seconds)
    }

    #[test]
    fn opening_creates_the_root_and_an_empty_store_lists_nothing() {
        let sandbox = Sandbox::new();
        assert!(!sandbox.root().exists());
        let store = sandbox.store();
        assert!(sandbox.root().is_dir());
        assert!(store.list().unwrap().is_empty());
        // opening twice is fine
        MapStore::open(sandbox.root()).unwrap();
    }

    #[test]
    fn a_created_map_is_listed_with_everything_that_was_given() {
        let sandbox = Sandbox::new();
        let store = sandbox.store();
        let created = store
            .create_map(&MapCreate { name: "  Çave   Land – 洞窟  ".into(), description: "  with  spaces  ".into(), seed: 4_000_000_000, ..request("cave-1") })
            .unwrap();
        assert_eq!(created.id, "cave-1");
        assert_eq!(created.name, "Çave   Land – 洞窟", "outer spaces are trimmed, inner ones and unicode survive");
        assert_eq!(created.description, "with  spaces");
        assert_eq!(created.generator, "island");
        assert_eq!(created.seed, 4_000_000_000, "seeds above i32::MAX round trip");
        assert!(created.saves.is_empty());
        assert_eq!(store.list().unwrap(), vec![created.clone()]);
        assert_eq!(store.get("cave-1").unwrap(), created);
    }

    #[test]
    fn default_valued_meta_still_reads_back() {
        // generator island and seed 1 are the cvar defaults, which the cvar file format leaves out.
        let sandbox = Sandbox::new();
        let store = sandbox.store();
        let created = store.create_map(&MapCreate { seed: 1, ..request("plain") }).unwrap();
        assert_eq!((created.generator.as_str(), created.seed), ("island", 1));
    }

    #[test]
    fn maps_are_sorted_and_duplicates_are_refused() {
        let sandbox = Sandbox::new();
        let store = sandbox.store();
        for id in ["zeta", "alpha", "mid"] {
            store.create_map(&request(id)).unwrap();
        }
        let ids: Vec<_> = store.list().unwrap().into_iter().map(|m| m.id).collect();
        assert_eq!(ids, ["alpha", "mid", "zeta"]);
        assert!(matches!(store.create_map(&request("mid")), Err(MapError::Exists)));
        assert_eq!(store.list().unwrap().len(), 3);
    }

    #[test]
    fn bad_input_is_refused_with_a_reason_and_leaves_nothing_behind() {
        let sandbox = Sandbox::new();
        let store = sandbox.store();
        let long_name = "x".repeat(MAX_NAME_CHARS + 1);
        let long_description = "d".repeat(MAX_DESCRIPTION_CHARS + 1);
        let cases: Vec<(&str, MapCreate)> = vec![
            ("empty name", MapCreate { name: "   ".into(), ..request("a") }),
            ("long name", MapCreate { name: long_name, ..request("a") }),
            ("newline in name", MapCreate { name: "a\nb".into(), ..request("a") }),
            ("nul in name", MapCreate { name: "a\0b".into(), ..request("a") }),
            ("long description", MapCreate { description: long_description, ..request("a") }),
            ("newline in description", MapCreate { description: "a\nmapname hacked".into(), ..request("a") }),
            ("seed too large", MapCreate { seed: u64::from(u32::MAX) + 1, ..request("a") }),
        ];
        for (what, case) in cases {
            assert!(matches!(store.create_map(&case), Err(MapError::Invalid(_))), "{what}");
        }
        match store.create_map(&MapCreate { generator: "nope".into(), ..request("a") }) {
            Err(e @ MapError::UnknownGenerator(_)) => assert!(e.to_string().contains("island"), "names what exists: {e}"),
            other => panic!("{:?}", other.err()),
        }
        assert!(store.list().unwrap().is_empty(), "refused requests create no folder");
        // the longest allowed values are fine
        store
            .create_map(&MapCreate { name: "n".repeat(MAX_NAME_CHARS), description: "d".repeat(MAX_DESCRIPTION_CHARS), ..request("ok") })
            .unwrap();
    }

    #[test]
    fn map_ids_cannot_escape_the_maps_folder() {
        let sandbox = Sandbox::new();
        let store = sandbox.store();
        let before = sandbox.siblings();
        let hostile = [
            "", ".", "..", "../escaped", "a/b", "a\\b", "/etc/passwd", "C:\\x", "x\0y", "UPPER", "with space", "tab\t", "dot.dot",
            "ключ", "ａｂｃ", "a\u{202e}b", "~", "-/../x", &"a".repeat(MAX_ID_LEN + 1), &"a".repeat(5000),
        ];
        for id in hostile {
            assert!(!valid_map_id(id), "{id:?}");
            assert!(matches!(store.create_map(&request(id)), Err(MapError::Invalid(_))), "create {id:?}");
            assert!(matches!(store.new_save_slot(id), Err(MapError::NoSuchMap(_))), "slot {id:?}");
            assert!(matches!(store.open_world(id, 0), Err(MapError::NoSuchMap(_))), "open {id:?}");
            assert!(matches!(store.get(id), Err(MapError::NoSuchMap(_))), "get {id:?}");
            assert!(!store.save_slot_exists(id, 0), "exists {id:?}");
        }
        assert_eq!(sandbox.siblings(), before, "nothing was created outside the maps folder");
        assert!(store.list().unwrap().is_empty());
        for ok in ["a", "a-b_c", "0", &"z".repeat(MAX_ID_LEN)] {
            assert!(valid_map_id(ok), "{ok}");
        }
    }

    #[test]
    fn error_messages_do_not_echo_control_characters() {
        let shown = MapError::NoSuchMap("evil\n[server] fake line".into()).to_string();
        assert!(!shown.contains('\n'), "{shown:?}");
    }

    #[test]
    fn save_slots_take_the_lowest_free_number() {
        let sandbox = Sandbox::new();
        let store = sandbox.store();
        store.create_map(&request("m")).unwrap();
        assert!(!store.save_slot_exists("m", 0));
        assert_eq!(store.new_save_slot("m").unwrap(), 0);
        assert_eq!(store.new_save_slot("m").unwrap(), 1);
        assert_eq!(store.new_save_slot("m").unwrap(), 2);
        assert!(store.save_slot_exists("m", 1));

        fs::remove_dir(sandbox.root().join("m").join("save1")).unwrap();
        assert!(!store.save_slot_exists("m", 1));
        assert_eq!(store.new_save_slot("m").unwrap(), 1, "the gap is reused, like Java's first free index");
        assert_eq!(store.new_save_slot("m").unwrap(), 3);

        assert!(matches!(store.new_save_slot("missing"), Err(MapError::NoSuchMap(_))));
        assert!(!store.save_slot_exists("m", u32::MAX));
    }

    #[test]
    fn listing_shows_saves_with_chunk_counts_and_the_newest_change() {
        let sandbox = Sandbox::new();
        let store = sandbox.store();
        store.create_map(&request("m")).unwrap();
        let (s0, s1) = (store.new_save_slot("m").unwrap(), store.new_save_slot("m").unwrap());
        let dir0 = sandbox.root().join("m").join(format!("save{s0}"));
        for (name, time) in [("chunk0,0.wec", 1_000_000_000), ("chunk-1,0.wec", 1_709_164_800)] {
            let file = fs::File::create(dir0.join(name)).unwrap();
            file.set_modified(at(time)).unwrap();
        }
        fs::write(dir0.join("notes.txt"), "not a chunk").unwrap();
        fs::write(dir0.join(META_FILE), "").unwrap();

        let saves = store.list().unwrap().remove(0).saves;
        assert_eq!(
            saves,
            vec![
                SaveInfo { slot: 0, modified: Some("2024-02-29T00:00:00Z".into()), chunks: 2 },
                SaveInfo { slot: s1, modified: None, chunks: 0 },
            ]
        );
    }

    #[test]
    fn only_canonical_save_folders_count_as_saves() {
        let sandbox = Sandbox::new();
        let store = sandbox.store();
        store.create_map(&request("m")).unwrap();
        let dir = sandbox.root().join("m");
        for name in ["save0", "save007", "save+1", "save", "saves", "save-1", "backup", "save1000", "save99999999999"] {
            fs::create_dir(dir.join(name)).unwrap();
        }
        fs::write(dir.join("save5"), "a file, not a folder").unwrap();
        let slots: Vec<u32> = store.get("m").unwrap().saves.iter().map(|s| s.slot).collect();
        assert_eq!(slots, [0]);
    }

    #[test]
    fn iso8601_is_correct_across_leap_years_and_centuries() {
        let cases = [
            (0, "1970-01-01T00:00:00Z"),
            (86_399, "1970-01-01T23:59:59Z"),
            (951_782_400, "2000-02-29T00:00:00Z"), // 2000 is a leap year
            (1_000_000_000, "2001-09-09T01:46:40Z"),
            (1_709_164_800, "2024-02-29T00:00:00Z"),
            (1_709_251_200, "2024-03-01T00:00:00Z"),
            (4_078_000_000, "2099-03-24T01:46:40Z"), // expected values computed independently with Python
            (4_102_444_800, "2100-01-01T00:00:00Z"),
            (4_107_542_400, "2100-03-01T00:00:00Z"), // 2100 is not a leap year: no Feb 29
            (1_791_126_000, "2026-10-04T15:00:00Z"),
        ];
        for (seconds, expected) in cases {
            assert_eq!(iso8601(at(seconds)).as_deref(), Some(expected), "{seconds}");
        }
        assert_eq!(iso8601(UNIX_EPOCH - Duration::from_secs(1)), None, "before 1970");
    }

    #[test]
    fn broken_meta_files_never_hide_a_map() {
        let sandbox = Sandbox::new();
        let store = sandbox.store();
        let root = sandbox.root();
        let broken: [(&str, Vec<u8>); 5] = [
            ("garbage", b"\xff\xfe\x00 not utf8 \x80".to_vec()),
            ("noise", b"mapname\ngenerator\n=== \n\n   \nfoo bar baz\n".to_vec()),
            ("huge", vec![b'x'; (MAX_META_BYTES as usize) + 10]),
            ("empty", Vec::new()),
            ("wrongtype", b"generatorSeed notanumber\nmapname ok\n".to_vec()),
        ];
        for (id, bytes) in &broken {
            fs::create_dir(root.join(id)).unwrap();
            fs::write(root.join(id).join(META_FILE), bytes).unwrap();
        }
        fs::create_dir(root.join("nometa")).unwrap();
        let maps = store.list().unwrap();
        assert_eq!(maps.len(), broken.len() + 1);
        let by = |id: &str| maps.iter().find(|m| m.id == id).unwrap();
        for id in ["garbage", "noise", "huge", "empty", "nometa"] {
            let m = by(id);
            assert_eq!((m.name.as_str(), m.generator.as_str(), m.seed), (NO_NAME, "air", 1), "{id}");
        }
        assert_eq!(by("wrongtype").name, "ok", "one bad line does not spoil the others");
        assert_eq!(by("wrongtype").seed, 1);
    }

    #[test]
    fn a_huge_name_in_a_meta_file_is_cut_down() {
        let sandbox = Sandbox::new();
        let store = sandbox.store();
        fs::create_dir(sandbox.root().join("big")).unwrap();
        fs::write(sandbox.root().join("big").join(META_FILE), format!("mapname {}\ndescription {}\n", "n".repeat(5000), "d".repeat(5000))).unwrap();
        let map = store.get("big").unwrap();
        assert_eq!((map.name.chars().count(), map.description.chars().count()), (MAX_NAME_CHARS, MAX_DESCRIPTION_CHARS));
    }

    #[test]
    fn files_hidden_folders_and_symlinks_are_not_maps() {
        let sandbox = Sandbox::new();
        let store = sandbox.store();
        let root = sandbox.root();
        store.create_map(&request("real")).unwrap();
        fs::write(root.join("afile"), "x").unwrap();
        for name in [".hidden", ".git"] {
            fs::create_dir(root.join(name)).unwrap();
        }
        #[cfg(unix)]
        {
            fs::create_dir(root.join("with\nnewline")).unwrap();
            let outside = sandbox.parent.join("outside");
            fs::create_dir(&outside).unwrap();
            std::os::unix::fs::symlink(&outside, root.join("link")).unwrap();
            assert!(matches!(store.get("link"), Err(MapError::NoSuchMap(_))), "a symlink is not a map");
            assert!(matches!(store.new_save_slot("link"), Err(MapError::NoSuchMap(_))));
            assert!(fs::read_dir(&outside).unwrap().next().is_none(), "nothing was created behind the link");
        }
        let ids: Vec<_> = store.list().unwrap().into_iter().map(|m| m.id).collect();
        assert_eq!(ids, ["real"]);
        assert!(matches!(store.get(".hidden"), Err(MapError::NoSuchMap(_))));
    }

    #[test]
    fn the_number_of_maps_is_limited() {
        let sandbox = Sandbox::new();
        let store = sandbox.store();
        for i in 0..MAX_MAPS {
            fs::create_dir(sandbox.root().join(format!("m{i}"))).unwrap();
        }
        assert_eq!(store.list().unwrap().len(), MAX_MAPS);
        assert!(matches!(store.create_map(&request("one-more")), Err(MapError::Invalid(_))));
        assert!(!sandbox.root().join("one-more").exists());
    }

    #[test]
    fn a_world_opened_from_a_save_keeps_its_changes() {
        let sandbox = Sandbox::new();
        let store = sandbox.store();
        store.create_map(&MapCreate { generator: "island".into(), seed: 5, ..request("m") }).unwrap();
        let slot = store.new_save_slot("m").unwrap();

        let opened = store.open_world("m", slot).unwrap();
        assert_eq!((opened.slot, opened.map.id.as_str(), opened.map.seed), (slot, "m", 5));
        let mut world = World::with_store(opened.generator, opened.store);
        let untouched = world.get(3, 5, 9);
        world.load_chunk(0, 0);
        assert!(world.set(3, 5, 9, Block::new(id::STONE, 0)));
        assert_eq!(world.save_modified().unwrap(), 1);
        assert!(world.take_warnings().is_empty());

        // another save slot of the same map is unaffected, and the changed one gets it back
        let other = store.new_save_slot("m").unwrap();
        let mut fresh = {
            let o = store.open_world("m", other).unwrap();
            World::with_store(o.generator, o.store)
        };
        fresh.load_chunk(0, 0);
        assert_eq!(fresh.get(3, 5, 9), untouched);

        let mut again = {
            let o = store.open_world("m", slot).unwrap();
            World::with_store(o.generator, o.store)
        };
        again.load_chunk(0, 0);
        assert_eq!(again.get(3, 5, 9), Block::new(id::STONE, 0));
        assert_eq!(store.get("m").unwrap().saves[0].chunks, 1);
    }

    #[test]
    fn opening_reports_what_is_missing_or_wrong() {
        let sandbox = Sandbox::new();
        let store = sandbox.store();
        store.create_map(&request("m")).unwrap();
        assert!(matches!(store.open_world("nope", 0), Err(MapError::NoSuchMap(_))));
        assert!(matches!(store.open_world("m", 0), Err(MapError::NoSuchSave(0))), "no save yet");

        fs::create_dir(sandbox.root().join("bad")).unwrap();
        fs::write(sandbox.root().join("bad").join(META_FILE), "generator nope\n").unwrap();
        store.new_save_slot("bad").unwrap();
        match store.open_world("bad", 0) {
            Err(MapError::UnknownGenerator(g)) => assert_eq!(g, "nope"),
            other => panic!("{:?}", other.err()),
        }
        assert!(matches!(store.open_world("m", MAX_SAVES + 5), Err(MapError::NoSuchSave(_))));
    }

    #[test]
    fn every_generator_can_be_used_for_a_map() {
        let sandbox = Sandbox::new();
        let store = sandbox.store();
        for info in generators() {
            let id = format!("g-{}", info.id);
            store.create_map(&MapCreate { generator: info.id.into(), ..request(&id) }).unwrap();
            let slot = store.new_save_slot(&id).unwrap();
            let opened = store.open_world(&id, slot).unwrap_or_else(|e| panic!("{}: {e}", info.id));
            assert_eq!(opened.map.generator, info.id);
        }
    }

    #[test]
    fn save_cvars_round_trip_and_default_when_absent() {
        let sandbox = Sandbox::new();
        let store = sandbox.store();
        store.create_map(&request("m")).unwrap();
        let slot = store.new_save_slot("m").unwrap();

        let opened = store.open_world("m", slot).unwrap();
        assert_eq!(opened.save_cvars.get_f32("LEsunAzimuth"), Ok(0.0));
        assert_eq!(opened.map_cvars.get_str("mapname"), Ok("My island"));

        let mut cvars = opened.save_cvars.clone();
        cvars.set_f32("LEsunAzimuth", 123.5).unwrap();
        store.save_save_cvars("m", slot, &cvars).unwrap();
        assert_eq!(store.open_world("m", slot).unwrap().save_cvars.get_f32("LEsunAzimuth"), Ok(123.5));

        assert!(matches!(store.save_save_cvars("m", slot + 1, &cvars), Err(MapError::NoSuchSave(_))));
        assert!(matches!(store.save_save_cvars("../x", slot, &cvars), Err(MapError::NoSuchMap(_))));
    }

    #[test]
    fn a_fresh_server_gets_the_default_map_exactly_once() {
        let sandbox = Sandbox::new();
        let store = sandbox.store();
        let made = store.ensure_default_map().unwrap().expect("created on an empty store");
        assert_eq!((made.id.as_str(), made.name.as_str(), made.generator.as_str()), ("island", "Island", "island"));
        assert!(store.ensure_default_map().unwrap().is_none(), "second call does nothing");
        assert_eq!(store.list().unwrap().len(), 1);

        // a server that already has some other map is left alone
        let other = Sandbox::new();
        let store = other.store();
        store.create_map(&request("mine")).unwrap();
        assert!(store.ensure_default_map().unwrap().is_none());
        assert_eq!(store.list().unwrap().len(), 1);
    }

    #[test]
    fn map_info_serialises_for_the_api() {
        let sandbox = Sandbox::new();
        let store = sandbox.store();
        store.create_map(&request("m")).unwrap();
        store.new_save_slot("m").unwrap();
        let json = serde_json::to_value(store.get("m").unwrap()).unwrap();
        assert_eq!(json["id"], "m");
        assert_eq!(json["saves"][0]["slot"], 0);
        assert!(json["saves"][0]["modified"].is_null());
        let create: MapCreate = serde_json::from_str(r#"{"id":"x","name":"X","generator":"island"}"#).unwrap();
        assert_eq!((create.seed, create.description.as_str()), (1, ""), "seed and description are optional");
    }

    /// A map as the Java engine leaves it: no generator cvar, a root chunk, one save slot.
    fn java_style_map(sandbox: &Sandbox, folder: &str) -> PathBuf {
        let dir = sandbox.root().join(folder);
        fs::create_dir_all(dir.join("save0")).unwrap();
        fs::write(dir.join(META_FILE), "mapname Old Cave\ndescription made with the Java engine\nMapVersion 4\n").unwrap();
        let mut chunk = Chunk::new((0, 0));
        chunk.set(1, 2, 3, Block::new(id::STONE, 0));
        fs::write(dir.join(wurfel_sim::chunk::file_name(0, 0)), chunk.to_wec_bytes().unwrap()).unwrap();
        dir
    }

    #[test]
    fn a_java_style_map_is_listed_and_uses_air_where_it_has_no_chunk() {
        let sandbox = Sandbox::new();
        let store = sandbox.store();
        java_style_map(&sandbox, "Old Cave 2");
        let map = store.get("Old Cave 2").unwrap();
        assert_eq!(map.id, "Old Cave 2", "capitals and spaces are fine for existing maps");
        assert_eq!((map.name.as_str(), map.description.as_str()), ("Old Cave", "made with the Java engine"));
        assert_eq!(map.generator, "air", "no generator cvar means Java's default generator");
        assert_eq!(map.saves, vec![SaveInfo { slot: 0, modified: None, chunks: 0 }]);

        let opened = store.open_world("Old Cave 2", 0).unwrap();
        let mut world = World::with_store(opened.generator, opened.store);
        world.load_chunk(0, 0);
        assert_eq!(world.get(1, 2, 3), Block::new(id::STONE, 0), "the root chunk is used");
        world.load_chunk(5, 5);
        assert!(world.get(5 * 10 + 1, 5 * 40 + 1, 1).is_air(), "a chunk without a file is generated by air");
        assert!(world.take_warnings().is_empty());
    }

    #[test]
    fn writes_go_to_the_save_slot_and_never_to_the_maps_root_files() {
        let sandbox = Sandbox::new();
        let store = sandbox.store();
        let dir = java_style_map(&sandbox, "javamap");
        let root_chunk = dir.join(wurfel_sim::chunk::file_name(0, 0));
        let (bytes, modified) = (fs::read(&root_chunk).unwrap(), fs::metadata(&root_chunk).unwrap().modified().unwrap());
        let meta_before = fs::read(dir.join(META_FILE)).unwrap();

        // a new save slot disturbs nothing
        let slot = store.new_save_slot("javamap").unwrap();
        assert_eq!(slot, 1, "save0 already exists");
        assert_eq!(fs::read(&root_chunk).unwrap(), bytes);
        assert_eq!(fs::read(dir.join(META_FILE)).unwrap(), meta_before);
        assert!(fs::read_dir(dir.join("save1")).unwrap().next().is_none(), "the new slot starts empty");

        let opened = store.open_world("javamap", slot).unwrap();
        let mut world = World::with_store(opened.generator, opened.store);
        world.load_chunk(0, 0);
        assert!(world.set(1, 2, 3, Block::new(id::DIRT, 0)));
        assert_eq!(world.save_modified().unwrap(), 1);

        assert_eq!(fs::read(&root_chunk).unwrap(), bytes, "the root chunk is the read-only base");
        assert_eq!(fs::metadata(&root_chunk).unwrap().modified().unwrap(), modified);
        assert!(dir.join("save1").join(wurfel_sim::chunk::file_name(0, 0)).is_file());
        assert!(!dir.join("save0").join(wurfel_sim::chunk::file_name(0, 0)).exists(), "other slots are untouched");
        let mut names: Vec<String> = fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        names.sort();
        assert_eq!(names, ["chunk0,0.wec", "meta.wecvar", "save0", "save1"], "no new files in the map folder itself");
    }

    #[test]
    fn entity_bytes_after_the_blocks_survive_loading_and_saving_byte_for_byte() {
        let sandbox = Sandbox::new();
        sandbox.store();
        let dir = java_style_map(&sandbox, "withents");
        let root_file = dir.join(wurfel_sim::chunk::file_name(0, 0));
        let mut original = fs::read(&root_file).unwrap();
        original.extend_from_slice(b"\xac\xed\x00\x05 pretend this is Java serialised entity data \x00\xff");
        fs::write(&root_file, &original).unwrap();

        let chunk = ChunkStore::new(&dir, 0).load(0, 0).unwrap().expect("the root chunk is found");
        assert_eq!(chunk.get(1, 2, 3), Block::new(id::STONE, 0));
        assert_eq!(chunk.to_wec_bytes().unwrap(), original, "re-saving reproduces the file exactly, entities included");
    }

    #[test]
    fn disk_folder_names_are_matched_not_joined() {
        let sandbox = Sandbox::new();
        let store = sandbox.store();
        java_style_map(&sandbox, "Spaced Name");
        java_style_map(&sandbox, "plain");
        let before = sandbox.siblings();
        for hostile in ["../plain", "plain/..", "plain/", "./plain", "Spaced Name/../plain", "PLAIN\0", "spaced name", "plain ", " plain", "/", ""] {
            assert!(matches!(store.get(hostile), Err(MapError::NoSuchMap(_))), "{hostile:?}");
            assert!(matches!(store.new_save_slot(hostile), Err(MapError::NoSuchMap(_))), "{hostile:?}");
            assert!(matches!(store.open_world(hostile, 0), Err(MapError::NoSuchMap(_))), "{hostile:?}");
        }
        assert_eq!(sandbox.siblings(), before);
        assert_eq!(store.new_save_slot("plain").unwrap(), 1, "exact names do work");
        assert_eq!(store.new_save_slot("Spaced Name").unwrap(), 1);
        let ids: Vec<_> = store.list().unwrap().into_iter().map(|m| m.id).collect();
        assert_eq!(ids, ["Spaced Name", "plain"], "listed by folder name, byte order");
    }

    #[test]
    fn a_new_map_may_not_take_the_name_of_an_existing_java_folder_in_another_case() {
        let sandbox = Sandbox::new();
        let store = sandbox.store();
        java_style_map(&sandbox, "cave");
        assert!(matches!(store.create_map(&request("cave")), Err(MapError::Exists)));
        assert_eq!(store.get("cave").unwrap().name, "Old Cave", "the existing map is untouched");
    }

    #[test]
    fn a_map_with_an_explicit_island_generator_is_not_mistaken_for_a_java_map() {
        let sandbox = Sandbox::new();
        let store = sandbox.store();
        // island and seed 1 are the cvar defaults: they must still be written out.
        let made = store.create_map(&MapCreate { seed: 1, ..request("explicit") }).unwrap();
        assert_eq!(made.generator, "island");
        let meta = fs::read_to_string(sandbox.root().join("explicit").join(META_FILE)).unwrap();
        assert!(meta.contains("generator island") && meta.contains("generatorSeed 1"), "{meta}");
        assert_eq!(store.open_world("explicit", store.new_save_slot("explicit").unwrap()).unwrap().map.generator, "island");
    }

    /// Real data: a chunk file from the 2013 jars `WE11.jar` / `MCWE11.jar` (identical in both).
    /// It is the engine's old text format, which is not the format of the current engine.
    #[test]
    fn a_chunk_file_of_the_2013_text_format_is_reported_not_loaded_as_garbage() {
        let sandbox = Sandbox::new();
        let store = sandbox.store();
        let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/maps/legacy-2013");
        let dir = sandbox.root().join("legacy");
        fs::create_dir_all(&dir).unwrap();
        fs::copy(fixtures.join("chunk0,0.wec"), dir.join("chunk0,0.wec")).unwrap();
        fs::copy(fixtures.join("map.wem"), dir.join("map.wem")).unwrap();

        let map = store.get("legacy").unwrap();
        assert_eq!((map.name.as_str(), map.generator.as_str()), (NO_NAME, "air"), "no meta.wecvar: it is listed with fallbacks");

        let slot = store.new_save_slot("legacy").unwrap();
        let opened = store.open_world("legacy", slot).unwrap();
        let result = opened.store.load(0, 0);
        assert!(result.is_err(), "a text chunk must be an error, not decoded into random blocks: {:?}", result.map(|c| c.map(|_| "a chunk")));

        let mut world = World::with_store(create_generator("air", 1).unwrap(), opened.store);
        world.load_chunk(0, 0);
        assert_eq!(world.take_warnings().len(), 1, "the world keeps going on generated terrain and reports the broken file");
        assert!(world.get(1, 1, 1).is_air());
    }
}
