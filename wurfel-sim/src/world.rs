//! The world: chunks that are loaded from disk or generated on demand.

use std::collections::{HashMap, HashSet};
use std::io;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use crate::block::{BlockConfig, DefaultBlocks};
use crate::chunk::Chunk;
use crate::grid::chunk_of;
use crate::storage::ChunkStore;
use crate::{Block, Generator, CHUNK_SIZE_X, CHUNK_SIZE_Y, CHUNK_SIZE_Z};

static NEXT_WORLD_ID: AtomicU64 = AtomicU64::new(1);

pub struct World {
    id: u64,
    /// Absent in a remote world, whose chunks arrive over the network (see [`World::remote`]).
    generator: Option<Box<dyn Generator>>,
    store: Option<ChunkStore>,
    chunks: HashMap<(i32, i32), Chunk>,
    /// Chunks whose blocks changed since [`World::take_changed_chunks`], for caches built on top.
    changed: HashSet<(i32, i32)>,
    warnings: Vec<String>,
    blocks: Arc<dyn BlockConfig>,
}

impl World {
    pub fn new(generator: impl Generator + 'static) -> Self {
        World {
            id: NEXT_WORLD_ID.fetch_add(1, Ordering::Relaxed),
            generator: Some(Box::new(generator)),
            store: None,
            chunks: HashMap::new(),
            changed: HashSet::new(),
            warnings: Vec::new(),
            blocks: Arc::new(DefaultBlocks),
        }
    }

    /// A world that loads chunks from `store` and only generates the ones the map does not have.
    pub fn with_store(generator: impl Generator + 'static, store: ChunkStore) -> Self {
        World { store: Some(store), ..World::new(generator) }
    }

    /// A world without a generator: it only knows the chunks it is given with
    /// [`World::insert_chunk`], like a client that streams its world from the server. Whatever is
    /// not loaded counts as unknown: [`World::get`] answers air and physics treats it as a wall.
    pub fn remote() -> Self {
        World {
            id: NEXT_WORLD_ID.fetch_add(1, Ordering::Relaxed),
            generator: None,
            store: None,
            chunks: HashMap::new(),
            changed: HashSet::new(),
            warnings: Vec::new(),
            blocks: Arc::new(DefaultBlocks),
        }
    }

    /// Tell the world what the block ids of the game running in it mean (obstacle, liquid, ...).
    pub fn set_block_config(&mut self, config: Arc<dyn BlockConfig>) {
        self.blocks = config;
    }

    /// The block rules in effect, [`DefaultBlocks`] unless a game installed its own.
    pub fn blocks(&self) -> &dyn BlockConfig {
        self.blocks.as_ref()
    }

    /// The health/damage byte of a block (see [`Chunk::health`]); 0 when the chunk is not loaded.
    pub fn block_health(&self, x: i32, y: i32, z: i32) -> u8 {
        let (cx, cy) = chunk_of(x, y);
        self.chunks
            .get(&(cx, cy))
            .map_or(0, |c| c.health(x.rem_euclid(CHUNK_SIZE_X), y.rem_euclid(CHUNK_SIZE_Y), z))
    }

    /// Set the health byte of a block in a loaded chunk. Returns false if the chunk is not loaded.
    pub fn set_block_health(&mut self, x: i32, y: i32, z: i32, health: u8) -> bool {
        let (cx, cy) = chunk_of(x, y);
        let Some(chunk) = self.chunks.get_mut(&(cx, cy)) else { return false };
        chunk.set_health(x.rem_euclid(CHUNK_SIZE_X), y.rem_euclid(CHUNK_SIZE_Y), z, health);
        self.changed.insert((cx, cy));
        true
    }

    /// Whether this world makes up chunks itself (false for a [`World::remote`] one).
    pub fn has_generator(&self) -> bool {
        self.generator.is_some()
    }

    /// Is the chunk in memory?
    pub fn is_loaded(&self, cx: i32, cy: i32) -> bool {
        self.chunks.contains_key(&(cx, cy))
    }

    /// Is the chunk containing the block column `(x, y)` in memory?
    pub fn is_loaded_at(&self, x: i32, y: i32) -> bool {
        let (cx, cy) = chunk_of(x, y);
        self.is_loaded(cx, cy)
    }

    /// Put a chunk received from elsewhere into the world, replacing what was there.
    pub fn insert_chunk(&mut self, chunk: Chunk) {
        let pos = chunk.pos();
        self.chunks.insert(pos, chunk);
        self.changed.insert(pos);
    }

    /// Forget a chunk. Unsaved changes in it are lost, so the server never does this for modified
    /// chunks; a remote world drops chunks the server stopped sending.
    pub fn unload_chunk(&mut self, cx: i32, cy: i32) -> Option<Chunk> {
        let removed = self.chunks.remove(&(cx, cy));
        if removed.is_some() {
            self.changed.insert((cx, cy));
        }
        removed
    }

    /// Load (generate) every chunk within `radius` chunks of `(cx, cy)`.
    pub fn load_area(&mut self, (cx, cy): (i32, i32), radius: i32) {
        for dx in -radius..=radius {
            for dy in -radius..=radius {
                self.load_chunk(cx + dx, cy + dy);
            }
        }
    }

    /// Unique per `World` value. A cache keyed on world contents can compare it to notice that the
    /// whole world was replaced.
    pub fn id(&self) -> u64 {
        self.id
    }

    /// Load the chunk from the store, or generate it, if it is not loaded yet. A chunk file that is
    /// broken is reported through [`World::take_warnings`] and replaced by generated terrain in
    /// memory only; the file itself is not touched until you save.
    pub fn load_chunk(&mut self, cx: i32, cy: i32) {
        if self.chunks.contains_key(&(cx, cy)) {
            return;
        }
        let loaded = match &self.store {
            Some(store) => store.load(cx, cy).unwrap_or_else(|e| {
                self.warnings.push(format!("chunk {cx},{cy} could not be loaded: {e}"));
                None
            }),
            None => None,
        };
        let chunk = match (loaded, &self.generator) {
            (Some(chunk), _) => chunk,
            (None, Some(generator)) => Chunk::generate((cx, cy), generator.as_ref()),
            // A remote world cannot make a chunk up: it waits for the server to send it.
            (None, None) => return,
        };
        self.chunks.insert((cx, cy), chunk);
        self.changed.insert((cx, cy));
    }

    /// Replace a block. Returns false (and changes nothing) outside the world height, or in a remote
    /// world when the chunk has not arrived.
    pub fn set(&mut self, x: i32, y: i32, z: i32, block: Block) -> bool {
        if !(0..CHUNK_SIZE_Z).contains(&z) {
            return false;
        }
        let (cx, cy) = chunk_of(x, y);
        self.load_chunk(cx, cy);
        let Some(chunk) = self.chunks.get_mut(&(cx, cy)) else { return false }; // remote world, not loaded
        let (lx, ly) = (x.rem_euclid(CHUNK_SIZE_X), y.rem_euclid(CHUNK_SIZE_Y));
        if chunk.get(lx, ly, z) != block {
            chunk.set(lx, ly, z, block);
            self.changed.insert((cx, cy));
        }
        true
    }

    /// The block at absolute coordinates. Air above and below the world. Unloaded chunks are
    /// answered by the generator, so callers at the edge of the loaded area see the real terrain.
    /// (A chunk that has a file in the store but is not loaded yet is answered by the generator too;
    /// load it first if that matters.)
    pub fn get(&self, x: i32, y: i32, z: i32) -> Block {
        if !(0..CHUNK_SIZE_Z).contains(&z) {
            return Block::AIR;
        }
        let (cx, cy) = chunk_of(x, y);
        match self.chunks.get(&(cx, cy)) {
            Some(chunk) => chunk.get(x.rem_euclid(CHUNK_SIZE_X), y.rem_euclid(CHUNK_SIZE_Y), z),
            None => self.generator.as_ref().map_or(Block::AIR, |g| g.generate(x, y, z)),
        }
    }

    /// A loaded chunk.
    pub fn chunk(&self, cx: i32, cy: i32) -> Option<&Chunk> {
        self.chunks.get(&(cx, cy))
    }

    pub fn loaded_chunks(&self) -> impl Iterator<Item = &Chunk> {
        self.chunks.values()
    }

    /// Chunks that were loaded or edited since the last call. Meant for one consumer (a render
    /// cache); it empties the list.
    pub fn take_changed_chunks(&mut self) -> Vec<(i32, i32)> {
        let mut changed: Vec<_> = self.changed.drain().collect();
        changed.sort_unstable();
        changed
    }

    /// Problems met while loading chunks (and writing them, see [`World::save_modified`]).
    pub fn take_warnings(&mut self) -> Vec<String> {
        std::mem::take(&mut self.warnings)
    }

    /// Write every chunk that changed to the store. Returns how many were written. Without a store
    /// nothing is written and 0 is returned. The first failure aborts and is returned; chunks not
    /// written stay marked as modified.
    pub fn save_modified(&mut self) -> io::Result<usize> {
        let Some(store) = &self.store else { return Ok(0) };
        let mut written = 0;
        for chunk in self.chunks.values_mut().filter(|c| c.is_modified()) {
            store.save(chunk)?;
            chunk.mark_saved();
            written += 1;
        }
        Ok(written)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block::id;
    use crate::IslandGenerator;

    #[test]
    fn set_is_visible_through_get_and_respects_height() {
        let mut world = World::new(IslandGenerator::new(3));
        assert!(world.set(-1, -1, 4, Block::new(3, 0)));
        assert_eq!(world.get(-1, -1, 4), Block::new(3, 0));
        assert!(!world.set(0, 0, CHUNK_SIZE_Z, Block::new(3, 0)));
        assert!(!world.set(0, 0, -1, Block::new(3, 0)));
    }

    #[test]
    fn loaded_and_unloaded_chunks_agree() {
        let mut world = World::new(IslandGenerator::new(3));
        let reference = IslandGenerator::new(3);
        world.load_chunk(-1, 0);
        for x in -12..12 {
            for y in -3..45 {
                for z in -1..12 {
                    let expected = if (0..CHUNK_SIZE_Z).contains(&z) {
                        reference.generate(x, y, z)
                    } else {
                        Block::AIR
                    };
                    assert_eq!(world.get(x, y, z), expected, "at {x},{y},{z}");
                }
            }
        }
    }

    #[test]
    fn a_remote_world_only_knows_what_it_was_given() {
        let mut server = World::new(IslandGenerator::new(3));
        server.load_chunk(0, 0);
        let mut client = World::remote();
        assert!(!client.has_generator() && !client.is_loaded(0, 0));
        assert_eq!(client.get(3, 3, 0), Block::AIR, "unknown is air, physics treats it as a wall");
        assert!(!client.set(3, 3, 5, Block::new(3, 0)), "cannot edit what has not arrived");
        client.load_chunk(0, 0);
        assert!(!client.is_loaded(0, 0), "a remote world cannot generate");

        client.insert_chunk(server.chunk(0, 0).unwrap().clone());
        assert!(client.is_loaded_at(3, 3) && client.is_loaded(0, 0));
        for z in 0..CHUNK_SIZE_Z {
            assert_eq!(client.get(3, 3, z), server.get(3, 3, z));
        }
        assert!(client.set(3, 3, 9, Block::new(3, 0)));
        assert!(!client.is_loaded_at(15, 3), "the neighbour is still unknown");

        assert!(client.unload_chunk(0, 0).is_some());
        assert!(!client.is_loaded(0, 0));
        assert!(client.unload_chunk(0, 0).is_none());
    }

    #[test]
    fn load_area_loads_a_square_of_chunks() {
        let mut world = World::new(IslandGenerator::new(1));
        world.load_area((2, -1), 1);
        assert_eq!(world.loaded_chunks().count(), 9);
        assert!(world.is_loaded(1, -2) && world.is_loaded(3, 0) && !world.is_loaded(0, 0));
    }

    #[test]
    fn worlds_have_distinct_ids() {
        assert_ne!(World::new(IslandGenerator::new(1)).id(), World::new(IslandGenerator::new(1)).id());
    }

    #[test]
    fn changes_are_reported_once_per_chunk_and_real_edits_only() {
        let mut world = World::new(IslandGenerator::new(1));
        world.load_chunk(0, 0);
        assert_eq!(world.take_changed_chunks(), vec![(0, 0)], "loading counts as a change");
        assert!(world.take_changed_chunks().is_empty());

        let (x, y) = (3, 3);
        let existing = world.get(x, y, 0);
        world.set(x, y, 0, existing); // same block: nothing changed
        assert!(world.take_changed_chunks().is_empty());

        world.set(x, y, 9, Block::new(id::STONE, 0));
        world.set(x + 1, y, 9, Block::new(id::STONE, 0));
        world.set(-1, 0, 9, Block::new(id::STONE, 0)); // another chunk, not loaded yet
        assert_eq!(world.take_changed_chunks(), vec![(-1, 0), (0, 0)]);
    }

    #[test]
    fn chunk_accessors_expose_the_stored_data() {
        let mut world = World::new(IslandGenerator::new(1));
        assert!(world.chunk(0, 0).is_none());
        world.load_chunk(0, 0);
        assert_eq!(world.chunk(0, 0).unwrap().pos(), (0, 0));
        assert_eq!(world.loaded_chunks().count(), 1);
    }

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("wurfel-world-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn edits_survive_saving_and_loading_in_a_new_world() {
        let dir = temp_dir("persist");
        let mut world = World::with_store(IslandGenerator::new(1), ChunkStore::new(&dir, 0));
        world.set(2, 2, 9, Block::new(id::STONE, 0));
        world.set(-3, 50, 9, Block::new(id::DIRT, 0));
        assert_eq!(world.save_modified().unwrap(), 2);
        assert_eq!(world.save_modified().unwrap(), 0, "nothing left to save");

        let mut again = World::with_store(IslandGenerator::new(1), ChunkStore::new(&dir, 0));
        again.load_chunk(0, 0);
        again.load_chunk(-1, 1);
        assert_eq!(again.get(2, 2, 9), Block::new(id::STONE, 0));
        assert_eq!(again.get(-3, 50, 9), Block::new(id::DIRT, 0));
        // untouched terrain is what the generator makes, loaded back from the file
        assert_eq!(again.get(5, 5, 0), IslandGenerator::new(1).generate(5, 5, 0));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_broken_chunk_file_is_reported_and_not_overwritten() {
        let dir = temp_dir("broken");
        std::fs::create_dir_all(dir.join("save0")).unwrap();
        std::fs::write(dir.join("save0").join("chunk0,0.wec"), [9, 9]).unwrap();

        let mut world = World::with_store(IslandGenerator::new(1), ChunkStore::new(&dir, 0));
        world.load_chunk(0, 0);
        assert_eq!(world.get(5, 5, 0), IslandGenerator::new(1).generate(5, 5, 0), "falls back to terrain");
        let warnings = world.take_warnings();
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(warnings[0].contains("chunk 0,0"));
        assert_eq!(std::fs::read(dir.join("save0").join("chunk0,0.wec")).unwrap(), [9, 9]);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
