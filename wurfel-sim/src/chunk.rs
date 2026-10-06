//! The compact chunk, and the `.wec` file format of the Java engine.
//!
//! # Layout
//!
//! Same as the Java `Chunk.data[x][y][z * 3]`: three bytes per block, so a 10x40x32 chunk is 38 KB (12 KB at the Java height of 10).
//!
//! | byte | meaning |
//! |------|---------|
//! | `+0` | block id |
//! | `+1` | block value (variant) |
//! | `+2` | health. `100` in a fresh chunk, `0` once the chunk was generated or loaded (Java calls it damage there) |
//!
//! Blocks are stored `x`-major, then `y`, then `z`: index `((x * SIZE_Y + y) * SIZE_Z + z) * 3`.
//!
//! # File format (`chunk<x>,<y>.wec`)
//!
//! For each layer `z = 0..SIZE_Z`: if the layer has any non-air block, every cell (`y` outer, `x`
//! inner) is a single `0` byte for air or two bytes `id, value`; an empty layer is the two bytes
//! `~e`. The blocks end with `~b`. Whatever follows is a Java object stream with the chunk's
//! entities, which cannot be read here; it is kept as raw bytes ([`Chunk::trailing`]) and written
//! back unchanged so saving a chunk does not lose them.
//!
//! The format has no escaping, so an id or value of `~` (126) cannot be stored. Java writes such
//! blocks anyway and then fails to read them back; [`Chunk::to_wec_bytes`] refuses instead.
//!
//! Deliberate difference from the Java *reader*: after a `~e` it moves on to the next layer. Java
//! forgets to advance `z` there, which only works when empty layers are all at the top.

use std::fmt;

use crate::{Block, Generator, CHUNK_SIZE_X, CHUNK_SIZE_Y, CHUNK_SIZE_Z};

pub const CHUNK_FILE_SUFFIX: &str = "wec";
const SIGN_COMMAND: u8 = b'~';
const SIGN_EMPTY_LAYER: u8 = b'e';
const SIGN_END_BLOCKS: u8 = b'b';

const BYTES_PER_BLOCK: usize = 3;
const BLOCKS: usize = (CHUNK_SIZE_X * CHUNK_SIZE_Y * CHUNK_SIZE_Z) as usize;

/// File name of a chunk, e.g. `chunk-1,0.wec`.
pub fn file_name(cx: i32, cy: i32) -> String {
    format!("chunk{cx},{cy}.{CHUNK_FILE_SUFFIX}")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FormatError {
    /// The data ended before every layer was complete and without a `~b`.
    Truncated { layer: i32 },
    /// More blocks than fit into a chunk, or bytes after the last layer that are not `~b`.
    TooMuchData,
    /// The block cannot be written because its id or value is the command byte `~`.
    Unrepresentable { x: i32, y: i32, z: i32 },
}

impl fmt::Display for FormatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FormatError::Truncated { layer } => write!(f, "chunk file ends inside layer {layer}"),
            FormatError::TooMuchData => write!(f, "chunk file has more blocks than a chunk holds"),
            FormatError::Unrepresentable { x, y, z } => {
                write!(f, "block at {x},{y},{z} uses the reserved byte '~' and cannot be stored")
            }
        }
    }
}

impl std::error::Error for FormatError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chunk {
    pos: (i32, i32),
    data: Vec<u8>,
    modified: bool,
    trailing: Vec<u8>,
}

impl Chunk {
    /// An air chunk. Like Java, the third byte of every block starts at 100.
    pub fn new(pos: (i32, i32)) -> Self {
        let mut data = vec![0; BLOCKS * BYTES_PER_BLOCK];
        for block in data.chunks_exact_mut(BYTES_PER_BLOCK) {
            block[2] = 100;
        }
        Chunk { pos, data, modified: true, trailing: Vec::new() }
    }

    /// A chunk filled by asking the generator for every block (Java `Chunk.fill`).
    pub fn generate(pos: (i32, i32), generator: &dyn Generator) -> Self {
        let mut chunk = Chunk::new(pos);
        let (left, top) = chunk.top_left();
        for lx in 0..CHUNK_SIZE_X {
            for ly in 0..CHUNK_SIZE_Y {
                for z in 0..CHUNK_SIZE_Z {
                    let block = generator.generate(left + lx, top + ly, z);
                    let i = Self::index(lx, ly, z);
                    chunk.data[i] = block.id();
                    chunk.data[i + 1] = block.value();
                    chunk.data[i + 2] = 0;
                }
            }
        }
        chunk
    }

    /// Position in chunk coordinates.
    pub fn pos(&self) -> (i32, i32) {
        self.pos
    }

    /// Absolute coordinates of the block column with local index `(0, 0)`.
    pub fn top_left(&self) -> (i32, i32) {
        (self.pos.0 * CHUNK_SIZE_X, self.pos.1 * CHUNK_SIZE_Y)
    }

    /// Does the chunk contain the block column `(x, y)` (absolute coordinates)?
    pub fn has_coord(&self, x: i32, y: i32) -> bool {
        let (left, top) = self.top_left();
        (left..left + CHUNK_SIZE_X).contains(&x) && (top..top + CHUNK_SIZE_Y).contains(&y)
    }

    fn index(lx: i32, ly: i32, z: i32) -> usize {
        (((lx * CHUNK_SIZE_Y + ly) * CHUNK_SIZE_Z + z) as usize) * BYTES_PER_BLOCK
    }

    fn in_range(lx: i32, ly: i32, z: i32) -> bool {
        (0..CHUNK_SIZE_X).contains(&lx) && (0..CHUNK_SIZE_Y).contains(&ly) && (0..CHUNK_SIZE_Z).contains(&z)
    }

    /// Block at a local index. Air outside the chunk.
    pub fn get(&self, lx: i32, ly: i32, z: i32) -> Block {
        if !Self::in_range(lx, ly, z) {
            return Block::AIR;
        }
        let i = Self::index(lx, ly, z);
        Block::new(self.data[i], self.data[i + 1])
    }

    /// Set a block at a local index. Returns false and changes nothing outside the chunk.
    pub fn set(&mut self, lx: i32, ly: i32, z: i32, block: Block) -> bool {
        if !Self::in_range(lx, ly, z) {
            return false;
        }
        let i = Self::index(lx, ly, z);
        if (self.data[i], self.data[i + 1]) != (block.id(), block.value()) {
            self.data[i] = block.id();
            self.data[i + 1] = block.value();
            self.modified = true;
        }
        true
    }

    /// The health/damage byte of a block, 0 outside the chunk.
    pub fn health(&self, lx: i32, ly: i32, z: i32) -> u8 {
        if Self::in_range(lx, ly, z) { self.data[Self::index(lx, ly, z) + 2] } else { 0 }
    }

    pub fn set_health(&mut self, lx: i32, ly: i32, z: i32, health: u8) {
        if Self::in_range(lx, ly, z) {
            let i = Self::index(lx, ly, z) + 2;
            if self.data[i] != health {
                self.data[i] = health;
                self.modified = true;
            }
        }
    }

    /// Changed since it was created, loaded or last saved?
    pub fn is_modified(&self) -> bool {
        self.modified
    }

    pub fn mark_saved(&mut self) {
        self.modified = false;
    }

    /// The raw object stream with entities that followed the blocks in the file, if any.
    pub fn trailing(&self) -> &[u8] {
        &self.trailing
    }

    fn layer_is_empty(&self, z: i32) -> bool {
        (0..CHUNK_SIZE_X).all(|lx| (0..CHUNK_SIZE_Y).all(|ly| self.data[Self::index(lx, ly, z)] == 0))
    }

    /// Encode as a Java-compatible `.wec` file.
    pub fn to_wec_bytes(&self) -> Result<Vec<u8>, FormatError> {
        let mut out = Vec::with_capacity(BLOCKS / 4);
        for z in 0..CHUNK_SIZE_Z {
            if self.layer_is_empty(z) {
                out.extend([SIGN_COMMAND, SIGN_EMPTY_LAYER]);
                continue;
            }
            for ly in 0..CHUNK_SIZE_Y {
                for lx in 0..CHUNK_SIZE_X {
                    let i = Self::index(lx, ly, z);
                    let (id, value) = (self.data[i], self.data[i + 1]);
                    if id == 0 {
                        out.push(0);
                    } else if id == SIGN_COMMAND || value == SIGN_COMMAND {
                        let (left, top) = self.top_left();
                        return Err(FormatError::Unrepresentable { x: left + lx, y: top + ly, z });
                    } else {
                        out.extend([id, value]);
                    }
                }
            }
        }
        out.extend([SIGN_COMMAND, SIGN_END_BLOCKS]);
        out.extend(&self.trailing);
        Ok(out)
    }

    /// Decode a Java-compatible `.wec` file for the chunk at `pos`.
    pub fn from_wec_bytes(pos: (i32, i32), bytes: &[u8]) -> Result<Chunk, FormatError> {
        let mut chunk = Chunk::new(pos);
        // Java overwrites every block it reads, including the health byte (with 0). Blocks it never
        // reaches (an early `~b`) keep the constructor's values.
        let (mut x, mut y, mut z) = (0, 0, 0);
        let mut id: Option<u8> = None;
        let mut command = false;
        let mut ended = false;
        let mut consumed = bytes.len();

        for (n, &byte) in bytes.iter().enumerate() {
            if byte == SIGN_COMMAND {
                command = true; // Java: a second `~` while waiting for the command changes nothing
                continue;
            }
            if command {
                command = false;
                match byte {
                    SIGN_EMPTY_LAYER => {
                        if z >= CHUNK_SIZE_Z {
                            return Err(FormatError::TooMuchData);
                        }
                        for lx in 0..CHUNK_SIZE_X {
                            for ly in 0..CHUNK_SIZE_Y {
                                chunk.write_cell(lx, ly, z, 0, 0);
                            }
                        }
                        (x, y, z) = (0, 0, z + 1);
                        continue;
                    }
                    SIGN_END_BLOCKS => {
                        ended = true;
                        consumed = n + 1;
                        break;
                    }
                    // Any other byte after `~` is plain data, the `~` is dropped (as Java does).
                    _ => {}
                }
            }
            if z >= CHUNK_SIZE_Z {
                return Err(FormatError::TooMuchData);
            }
            match id.take() {
                None if byte == 0 => {
                    chunk.write_cell(x, y, z, 0, 0);
                    advance(&mut x, &mut y, &mut z);
                }
                None => id = Some(byte),
                Some(block_id) => {
                    chunk.write_cell(x, y, z, block_id, byte);
                    advance(&mut x, &mut y, &mut z);
                }
            }
        }

        if !ended && (z < CHUNK_SIZE_Z || id.is_some() || command) {
            return Err(FormatError::Truncated { layer: z });
        }
        if ended {
            chunk.trailing = bytes[consumed..].to_vec();
        }
        chunk.modified = true; // Java marks a loaded chunk modified
        Ok(chunk)
    }

    fn write_cell(&mut self, lx: i32, ly: i32, z: i32, id: u8, value: u8) {
        let i = Self::index(lx, ly, z);
        self.data[i] = id;
        self.data[i + 1] = value;
        self.data[i + 2] = 0;
    }
}

/// Step to the next cell of a layer: `x` fastest, then `y`, then the next layer.
fn advance(x: &mut i32, y: &mut i32, z: &mut i32) {
    *x += 1;
    if *x == CHUNK_SIZE_X {
        *x = 0;
        *y += 1;
        if *y == CHUNK_SIZE_Y {
            *y = 0;
            *z += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block::id;
    use crate::IslandGenerator;

    const LAYER_CELLS: usize = (CHUNK_SIZE_X * CHUNK_SIZE_Y) as usize;

    #[test]
    fn a_fresh_chunk_is_air_with_health_100_and_three_bytes_per_block() {
        let chunk = Chunk::new((0, 0));
        assert_eq!(chunk.data.len(), (CHUNK_SIZE_X * CHUNK_SIZE_Y * CHUNK_SIZE_Z) as usize * 3);
        assert!(chunk.get(3, 4, 5).is_air());
        assert_eq!(chunk.health(3, 4, 5), 100);
        assert_eq!(chunk.health(-1, 0, 0), 0);
    }

    #[test]
    fn get_and_set_respect_local_bounds() {
        let mut chunk = Chunk::new((2, -1));
        assert!(chunk.set(9, 39, CHUNK_SIZE_Z - 1, Block::new(id::STONE, 4)));
        assert_eq!(chunk.get(9, 39, CHUNK_SIZE_Z - 1), Block::new(id::STONE, 4));
        assert!(!chunk.set(10, 0, 0, Block::new(id::STONE, 0)));
        assert!(!chunk.set(0, 0, CHUNK_SIZE_Z, Block::new(id::STONE, 0)));
        assert!(chunk.get(10, 0, 0).is_air());
        assert!(chunk.has_coord(20, -40) && chunk.has_coord(29, -1));
        assert!(!chunk.has_coord(30, -40) && !chunk.has_coord(20, 0));
    }

    #[test]
    fn modified_flag_follows_real_changes_only() {
        let mut chunk = Chunk::new((0, 0));
        chunk.mark_saved();
        chunk.set(0, 0, 0, Block::AIR); // already air
        assert!(!chunk.is_modified());
        chunk.set(0, 0, 0, Block::new(id::DIRT, 0));
        assert!(chunk.is_modified());
    }

    #[test]
    fn exact_bytes_of_a_chunk_with_one_stone_block() {
        // The reference bytes are what Java's `Chunk.save` produces for this chunk: layer 0 is
        // `3,0` then 399 zero bytes (all other cells are air), layers 1-9 are empty (`~e`).
        let mut chunk = Chunk::new((0, 0));
        chunk.set(0, 0, 0, Block::new(id::STONE, 0));
        let mut expected = vec![3, 0];
        expected.extend(std::iter::repeat_n(0u8, LAYER_CELLS - 1));
        for _ in 1..CHUNK_SIZE_Z {
            expected.extend(*b"~e");
        }
        expected.extend(*b"~b");
        assert_eq!(chunk.to_wec_bytes().unwrap(), expected);
    }

    #[test]
    fn layers_are_written_with_y_outer_and_x_inner() {
        let mut chunk = Chunk::new((0, 0));
        chunk.set(2, 1, 0, Block::new(id::SAND, 7));
        let bytes = chunk.to_wec_bytes().unwrap();
        // cell (x=2, y=1) is number 1 * 10 + 2 = 12 in a layer; 12 zero bytes come before it.
        assert!(bytes[..12].iter().all(|&b| b == 0));
        assert_eq!(&bytes[12..14], &[id::SAND, 7]);
    }

    #[test]
    fn an_island_chunk_survives_a_round_trip() {
        let generator = IslandGenerator::new(1);
        for pos in [(0, 0), (-1, 2), (3, -3)] {
            let chunk = Chunk::generate(pos, &generator);
            let bytes = chunk.to_wec_bytes().unwrap();
            let back = Chunk::from_wec_bytes(pos, &bytes).unwrap();
            for lx in 0..CHUNK_SIZE_X {
                for ly in 0..CHUNK_SIZE_Y {
                    for z in 0..CHUNK_SIZE_Z {
                        assert_eq!(back.get(lx, ly, z), chunk.get(lx, ly, z), "{pos:?} at {lx},{ly},{z}");
                    }
                }
            }
            assert_eq!(back.to_wec_bytes().unwrap(), bytes, "re-encoding is stable");
        }
    }

    #[test]
    fn the_file_is_much_smaller_than_the_chunk() {
        let chunk = Chunk::generate((5, 5), &IslandGenerator::new(1));
        let bytes = chunk.to_wec_bytes().unwrap();
        assert!(bytes.len() < chunk.data.len() / 2, "{} bytes", bytes.len());
    }

    #[test]
    fn an_empty_layer_in_the_middle_does_not_shift_the_layers_above() {
        // Layer 0 and layer 2 have blocks, layer 1 is empty. Java's reader would put layer 2 onto
        // layer 1; this reader must not.
        let mut chunk = Chunk::new((0, 0));
        chunk.set(1, 1, 0, Block::new(id::STONE, 0));
        chunk.set(1, 1, 2, Block::new(id::GRASS, 0));
        let back = Chunk::from_wec_bytes((0, 0), &chunk.to_wec_bytes().unwrap()).unwrap();
        assert_eq!(back.get(1, 1, 0).id(), id::STONE);
        assert!(back.get(1, 1, 1).is_air());
        assert_eq!(back.get(1, 1, 2).id(), id::GRASS);
    }

    #[test]
    fn entity_bytes_after_the_blocks_are_preserved() {
        let mut bytes = Chunk::new((0, 0)).to_wec_bytes().unwrap();
        let entities = [0xAC, 0xED, 0x00, 0x05, b'~', b'|', 2, 1, 2, 3];
        bytes.extend(entities);
        let chunk = Chunk::from_wec_bytes((0, 0), &bytes).unwrap();
        assert_eq!(chunk.trailing(), entities);
        assert_eq!(chunk.to_wec_bytes().unwrap(), bytes, "saving writes them back unchanged");
    }

    #[test]
    fn an_early_end_marker_leaves_the_rest_air() {
        let mut bytes = vec![3, 0];
        bytes.extend(std::iter::repeat_n(0u8, LAYER_CELLS - 1));
        bytes.extend(*b"~b");
        let chunk = Chunk::from_wec_bytes((0, 0), &bytes).unwrap();
        assert_eq!(chunk.get(0, 0, 0).id(), id::STONE);
        assert!(chunk.get(0, 0, 1).is_air());
    }

    #[test]
    fn loading_zeroes_the_health_byte_of_every_cell_it_reads_like_java() {
        // `~e` layers and explicit cells are both overwritten with health 0...
        let empty = Chunk::from_wec_bytes((0, 0), &Chunk::new((0, 0)).to_wec_bytes().unwrap()).unwrap();
        assert_eq!(empty.health(0, 0, 0), 0);
        let mut with_block = Chunk::new((0, 0));
        with_block.set(0, 0, 0, Block::new(id::DIRT, 0));
        let loaded = Chunk::from_wec_bytes((0, 0), &with_block.to_wec_bytes().unwrap()).unwrap();
        assert_eq!(loaded.health(0, 0, 0), 0);
        // ...but cells after an early `~b` are never read and keep the constructor's 100.
        let early = Chunk::from_wec_bytes((0, 0), b"\0~b").unwrap();
        assert_eq!(early.health(0, 0, 0), 0);
        assert_eq!(early.health(1, 0, 0), 100);
    }

    #[test]
    fn broken_files_are_errors_not_garbage() {
        let good = Chunk::generate((0, 0), &IslandGenerator::new(1)).to_wec_bytes().unwrap();
        // cut off in the middle of the blocks
        assert!(matches!(
            Chunk::from_wec_bytes((0, 0), &good[..good.len() / 2]),
            Err(FormatError::Truncated { .. })
        ));
        // nothing at all
        assert!(matches!(Chunk::from_wec_bytes((0, 0), &[]), Err(FormatError::Truncated { layer: 0 })));
        // a block id with its value missing
        let mut odd = vec![0u8; LAYER_CELLS * CHUNK_SIZE_Z as usize - 1];
        odd.push(5);
        assert!(matches!(Chunk::from_wec_bytes((0, 0), &odd), Err(FormatError::Truncated { .. })));
        // more blocks than a chunk holds
        let mut too_much = vec![0u8; LAYER_CELLS * CHUNK_SIZE_Z as usize];
        too_much.push(0);
        assert_eq!(Chunk::from_wec_bytes((0, 0), &too_much), Err(FormatError::TooMuchData));
    }

    #[test]
    fn the_reserved_byte_is_refused_when_writing() {
        let mut chunk = Chunk::new((1, 1));
        chunk.set(2, 3, 4, Block::new(b'~', 0));
        assert_eq!(chunk.to_wec_bytes(), Err(FormatError::Unrepresentable { x: 12, y: 43, z: 4 }));
        let mut chunk = Chunk::new((0, 0));
        chunk.set(0, 0, 0, Block::new(id::STONE, b'~'));
        assert!(chunk.to_wec_bytes().is_err());
    }

    #[test]
    fn file_names_match_the_java_engine() {
        assert_eq!(file_name(-1, 2), "chunk-1,2.wec");
    }
}
