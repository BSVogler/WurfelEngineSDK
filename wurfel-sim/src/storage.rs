//! Chunks on disk, laid out like the Java engine's maps:
//!
//! ```text
//! <map dir>/chunk<x>,<y>.wec               shipped with the map, read-only "root" copy
//! <map dir>/save<slot>/chunk<x>,<y>.wec    what a save slot has changed
//! ```
//!
//! Loading prefers the save slot and falls back to the root copy, which is then copied into the
//! slot (Java `Chunk.restoreFromRoot`).

use std::fs;
use std::io;
use std::path::PathBuf;

use crate::chunk::{file_name, Chunk};

#[derive(Debug, Clone)]
pub struct ChunkStore {
    dir: PathBuf,
    slot: u32,
}

impl ChunkStore {
    pub fn new(dir: impl Into<PathBuf>, slot: u32) -> Self {
        ChunkStore { dir: dir.into(), slot }
    }

    /// The folder of the save slot (`<map>/save<N>`); a game mode may keep its own files in it.
    pub fn slot_dir(&self) -> PathBuf {
        self.dir.join(format!("save{}", self.slot))
    }

    /// Load a chunk. `Ok(None)` if the map has no file for it (the caller should generate it).
    /// A file that exists but is broken is an error, so it is never silently replaced.
    pub fn load(&self, cx: i32, cy: i32) -> io::Result<Option<Chunk>> {
        let name = file_name(cx, cy);
        let saved = self.slot_dir().join(&name);
        let bytes = match fs::read(&saved) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                let root = self.dir.join(&name);
                let bytes = match fs::read(&root) {
                    Ok(bytes) => bytes,
                    Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
                    Err(e) => return Err(e),
                };
                fs::create_dir_all(self.slot_dir())?;
                fs::write(&saved, &bytes)?;
                bytes
            }
            Err(e) => return Err(e),
        };
        Chunk::from_wec_bytes((cx, cy), &bytes)
            .map(Some)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("{}: {e}", saved.display())))
    }

    /// Write a chunk into the save slot.
    pub fn save(&self, chunk: &Chunk) -> io::Result<()> {
        let (cx, cy) = chunk.pos();
        let bytes = chunk
            .to_wec_bytes()
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("chunk {cx},{cy}: {e}")))?;
        fs::create_dir_all(self.slot_dir())?;
        // Write next to the target and rename, so a crash cannot leave a half-written chunk.
        let target = self.slot_dir().join(file_name(cx, cy));
        let tmp = target.with_extension("wec.tmp");
        fs::write(&tmp, bytes)?;
        fs::rename(tmp, target)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block::id;
    use crate::{Block, IslandGenerator};

    /// A directory that is removed again when the test ends.
    struct TempDir(PathBuf);
    impl TempDir {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!("wurfel-test-{name}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            TempDir(dir)
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn missing_chunks_are_none() {
        let dir = TempDir::new("missing");
        assert!(ChunkStore::new(&dir.0, 0).load(4, 4).unwrap().is_none());
    }

    #[test]
    fn saved_chunks_load_again_from_the_slot_directory() {
        let dir = TempDir::new("roundtrip");
        let store = ChunkStore::new(&dir.0, 2);
        let mut chunk = Chunk::generate((-1, 3), &IslandGenerator::new(9));
        chunk.set(1, 2, 3, Block::new(id::STONE, 5));
        store.save(&chunk).unwrap();
        assert!(dir.0.join("save2").join("chunk-1,3.wec").is_file());
        let back = store.load(-1, 3).unwrap().unwrap();
        assert_eq!(back.get(1, 2, 3), Block::new(id::STONE, 5));
        // a different slot does not see it
        assert!(ChunkStore::new(&dir.0, 3).load(-1, 3).unwrap().is_none());
    }

    #[test]
    fn a_root_chunk_is_used_and_copied_into_the_slot() {
        let dir = TempDir::new("root");
        let mut chunk = Chunk::new((0, 0));
        chunk.set(0, 0, 0, Block::new(id::GRASS, 0));
        fs::write(dir.0.join("chunk0,0.wec"), chunk.to_wec_bytes().unwrap()).unwrap();

        let store = ChunkStore::new(&dir.0, 1);
        let loaded = store.load(0, 0).unwrap().unwrap();
        assert_eq!(loaded.get(0, 0, 0).id(), id::GRASS);
        assert!(dir.0.join("save1").join("chunk0,0.wec").is_file(), "copied into the slot");

        // from now on the slot copy wins over the root copy
        let mut changed = loaded;
        changed.set(0, 0, 0, Block::new(id::SAND, 0));
        store.save(&changed).unwrap();
        assert_eq!(store.load(0, 0).unwrap().unwrap().get(0, 0, 0).id(), id::SAND);
    }

    #[test]
    fn a_corrupt_file_is_an_error_and_is_left_alone() {
        let dir = TempDir::new("corrupt");
        let store = ChunkStore::new(&dir.0, 0);
        fs::create_dir_all(dir.0.join("save0")).unwrap();
        fs::write(dir.0.join("save0").join("chunk0,0.wec"), [1, 2, 3]).unwrap();
        let err = store.load(0, 0).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
        assert_eq!(fs::read(dir.0.join("save0").join("chunk0,0.wec")).unwrap(), [1, 2, 3]);
    }
}
