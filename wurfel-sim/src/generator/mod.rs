//! Map generators. Same contract as the Java engine's `Generator`: a pure function from absolute
//! block coordinates to a block, so any machine can regenerate any part of the world from a seed.
//!
//! The engine's generators (all those of the Java engine) are selectable by name, see
//! [`generators`] and [`create_generator`]. A game adds its own with [`register_generator`] at
//! startup (Caveland's is in the `caveland-sim` crate):
//!
//! | id | Java class | what it makes |
//! |---|---|---|
//! | `island` | `IslandGenerator` | one pyramid-shaped mountain in a shallow sea |
//! | `air` | `AirGenerator` | nothing |
//! | `blocktest` | `BlockTestGenerator` | a floor with one row of every block type |
//! | `fullmap` | `FullMapGenerator` | solid world of a single block type (the seed is the block id) |
//! | `arena` | `ArenaGenerator` (Weapon of Choice demo) | sand floor with scattered pillars |
//! | `terrain` | none, new | terraced highlands with cliffs, lakes and natural arches (noise based) |
//!
//! Not ported: `MinecraftLoader`, which reads a Minecraft save from a hardcoded path on the
//! author's machine through an external library, and the earlier noise-based Rust generators in
//! `legacy/`, which are not from the Java engine.
//!
//! The generators were checked against the real Java code: `fixtures/generators/regenerate.sh`
//! compiles the unmodified Java sources and dumps what they return, and each generator's tests
//! compare against those files.

mod air;
mod arena;
mod blocktest;
mod fullmap;
mod island;
mod terrain;
pub mod java_random;

pub use air::AirGenerator;
pub use arena::ArenaGenerator;
pub use blocktest::{BlockTestGenerator, OBJECT_TYPES_NUM};
pub use fullmap::FullMapGenerator;
pub use island::IslandGenerator;
pub use terrain::TerrainGenerator;

use std::sync::RwLock;

use crate::cvar::CVarSystem;
use crate::Block;

/// The Java engine returns an `int` from `generate`: the low byte is the block id and the next byte
/// its value. `Chunk.fill` stores `(byte)(v & 255)` and `(byte)((v >> 8) & 255)`. This does the
/// same, which matters for generators that return a sign-extended `byte` (ids of 128 and above) or
/// add `1 << 8` for a value.
pub fn block_from_java_int(v: i32) -> Block {
    Block::new((v & 255) as u8, ((v >> 8) & 255) as u8)
}

/// Something the generator wants placed in the world, mirroring what the Java `spawnEntities`
/// spawns. Plain data: what the entity does is up to the game.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntitySpawn {
    /// The Java class, e.g. `"ExitPortal"`.
    pub kind: &'static str,
    /// Block cell it is placed in.
    pub cell: (i32, i32, i32),
    /// Where it leads to, for portals.
    pub target: Option<(i32, i32, i32)>,
    /// Other settings the Java code applied, e.g. `("enemy_spawner", "true")`.
    pub extras: Vec<(&'static str, String)>,
}

/// `Send + Sync` because a generator is a pure function of its inputs, and the server shares one
/// world between threads.
pub trait Generator: Send + Sync {
    /// The block that belongs at the given absolute coordinates.
    fn generate(&self, x: i32, y: i32, z: i32) -> Block;

    /// Entities that belong in this cell (`Generator.spawnEntities`). Called for every cell when a
    /// chunk is generated; almost every cell returns nothing.
    fn spawn_entities(&self, _x: i32, _y: i32, _z: i32) -> Vec<EntitySpawn> {
        Vec::new()
    }

    /// A block column to spawn players near. The world is unbounded; this only says where players
    /// start. The default is the origin.
    fn spawn_point(&self) -> (i32, i32) {
        (0, 0)
    }
}

/// Lets `World::new(create_generator("island", 1).unwrap())` work.
impl Generator for Box<dyn Generator> {
    fn generate(&self, x: i32, y: i32, z: i32) -> Block {
        (**self).generate(x, y, z)
    }

    fn spawn_entities(&self, x: i32, y: i32, z: i32) -> Vec<EntitySpawn> {
        (**self).spawn_entities(x, y, z)
    }

    fn spawn_point(&self) -> (i32, i32) {
        (**self).spawn_point()
    }
}

/// Description of a selectable generator, for menus and settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GeneratorInfo {
    /// Stable name used in the `generator` map cvar and in saves.
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    /// Does the map's `generatorSeed` change the result?
    pub uses_seed: bool,
}

static ENGINE_GENERATORS: [GeneratorInfo; 6] = [
    GeneratorInfo {
        id: "island",
        name: "Island",
        description: "One mountain in a shallow sea (Wurfel Engine default).",
        uses_seed: true,
    },
    GeneratorInfo {
        id: "arena",
        name: "Arena",
        description: "Sand floor with scattered pillars (Weapon of Choice demo).",
        uses_seed: true,
    },
    GeneratorInfo {
        id: "blocktest",
        name: "Block test",
        description: "A floor with one row for each block type.",
        uses_seed: false,
    },
    GeneratorInfo {
        id: "fullmap",
        name: "Solid block",
        description: "The whole world is one block type; the seed is the block id (1 is grass).",
        uses_seed: true,
    },
    GeneratorInfo {
        id: "terrain",
        name: "Highlands",
        description: "Terraced highlands with sheer cliffs, lakes and natural stone arches. Nothing floats.",
        uses_seed: true,
    },
    GeneratorInfo { id: "air", name: "Empty", description: "Nothing at all.", uses_seed: false },
];

/// Makes a generator from a seed.
pub type GeneratorConstructor = fn(u64) -> Box<dyn Generator>;

/// Generators games added with [`register_generator`], after the engine's own.
static REGISTERED: RwLock<Vec<(GeneratorInfo, GeneratorConstructor)>> = RwLock::new(Vec::new());

/// Make a game's generator selectable by name, like the engine's own. Registering an id again
/// replaces the earlier one, so a game may call this as often as it likes. Engine ids cannot be
/// replaced.
pub fn register_generator(info: GeneratorInfo, create: GeneratorConstructor) {
    assert!(!ENGINE_GENERATORS.iter().any(|g| g.id == info.id), "'{}' is an engine generator", info.id);
    let mut registered = REGISTERED.write().unwrap_or_else(|e| e.into_inner());
    registered.retain(|(known, _)| known.id != info.id);
    registered.push((info, create));
}

fn registered() -> std::sync::RwLockReadGuard<'static, Vec<(GeneratorInfo, GeneratorConstructor)>> {
    REGISTERED.read().unwrap_or_else(|e| e.into_inner())
}

/// All generators: the engine's in a stable order, then the registered ones in registration order.
pub fn generators() -> Vec<GeneratorInfo> {
    ENGINE_GENERATORS.iter().copied().chain(registered().iter().map(|(info, _)| *info)).collect()
}

pub fn generator_info(id: &str) -> Option<GeneratorInfo> {
    let id = id.trim().to_ascii_lowercase();
    generators().into_iter().find(|info| info.id == id)
}

/// Build a generator by id (case-insensitive). `seed` is ignored by generators that do not use one
/// (see [`GeneratorInfo::uses_seed`]). `None` for an unknown id.
pub fn create_generator(id: &str, seed: u64) -> Option<Box<dyn Generator>> {
    let id = id.trim().to_ascii_lowercase();
    Some(match id.as_str() {
        "island" => Box::new(IslandGenerator::new(seed)),
        "arena" => Box::new(ArenaGenerator::new(seed)),
        "blocktest" => Box::new(BlockTestGenerator),
        "fullmap" => Box::new(FullMapGenerator::new(seed as u8)),
        "terrain" => Box::new(TerrainGenerator::new(seed)),
        "air" => Box::new(AirGenerator),
        _ => {
            let create = registered().iter().find(|(info, _)| info.id == id)?.1;
            create(seed)
        }
    })
}

/// The generator a map asks for through its `generator` and `generatorSeed` cvars (see
/// [`CVarSystem::map`]). `None` if the name is not a known generator.
pub fn generator_from_cvars(cvars: &CVarSystem) -> Option<Box<dyn Generator>> {
    let id = cvars.get_str("generator").ok()?;
    // The seed cvar is a signed int; reinterpret it as unsigned so negative seeds are distinct too.
    let seed = cvars.get_i32("generatorSeed").ok()? as u32 as u64;
    create_generator(id, seed)
}

/// SplitMix64: tiny, dependency-free and good enough for picking a spot on the map.
pub(crate) fn splitmix64(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// Reading the fixtures written by the real Java code (`fixtures/generators`). Public for the tests
/// of games whose generators are checked against the same Java harness.
#[doc(hidden)]
pub mod fixture {
    use super::{block_from_java_int, Generator};

    /// Lines of the form `x y: v0 v1 v2 ...` (the raw `int` Java returned for z = 0, 1, 2...).
    pub fn columns(text: &str) -> Vec<(i32, i32, Vec<i32>)> {
        text.lines()
            .filter(|l| !l.trim().is_empty())
            .map(|line| {
                let (head, values) = line.split_once(':').unwrap_or_else(|| panic!("bad fixture line {line:?}"));
                let mut coords = head.split_whitespace().map(|p| p.parse::<i32>().unwrap());
                let (x, y) = (coords.next().unwrap(), coords.next().unwrap());
                (x, y, values.split_whitespace().map(|v| v.parse().unwrap()).collect())
            })
            .collect()
    }

    pub fn assert_generator_matches(generator: &dyn Generator, columns: &[(i32, i32, Vec<i32>)]) {
        for (x, y, values) in columns {
            for (z, &java) in values.iter().enumerate() {
                let expected = block_from_java_int(java);
                let got = generator.generate(*x, *y, z as i32);
                assert_eq!(got, expected, "at {x},{y},{z}: Java returned {java}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::World;

    #[test]
    fn the_registry_lists_every_generator_once_in_a_stable_order() {
        let ids: Vec<_> = generators().iter().map(|g| g.id).collect();
        // Other tests may register generators of their own (the registry is global): they come after.
        assert_eq!(ids[..6], ["island", "arena", "blocktest", "fullmap", "terrain", "air"]);
        for info in generators() {
            assert!(create_generator(info.id, 5).is_some(), "{} cannot be created", info.id);
            assert!(!info.name.is_empty() && !info.description.is_empty());
        }
        assert!(create_generator("nope", 1).is_none());
        assert!(create_generator("", 1).is_none());
    }

    #[test]
    fn lookup_ignores_case_and_surrounding_spaces() {
        assert_eq!(generator_info(" Arena ").unwrap().id, "arena");
        assert!(create_generator("ISLAND", 1).is_some());
    }

    #[test]
    fn created_generators_behave_like_the_concrete_ones() {
        let boxed = create_generator("island", 9).unwrap();
        let concrete = IslandGenerator::new(9);
        assert_eq!(boxed.spawn_point(), concrete.peak());
        for (x, y, z) in [(0, 0, 0), (3, 4, 5), (concrete.peak().0, concrete.peak().1, 8)] {
            assert_eq!(boxed.generate(x, y, z), concrete.generate(x, y, z));
        }
        assert_eq!(create_generator("fullmap", 7).unwrap().generate(1, 2, 3).id(), 7);
        assert_eq!(create_generator("fullmap", 7 + 256).unwrap().generate(1, 2, 3).id(), 7, "only the low byte is a block id");
    }

    #[test]
    fn a_world_can_be_built_from_a_boxed_generator() {
        let mut world = World::new(create_generator("fullmap", crate::block::id::DIRT as u64).unwrap());
        world.load_chunk(0, 0);
        assert_eq!(world.get(0, 0, 3).id(), crate::block::id::DIRT);
    }

    struct Checkerboard(u64);
    impl Generator for Checkerboard {
        fn generate(&self, x: i32, y: i32, z: i32) -> Block {
            if z == 0 && (x + y) % 2 == 0 { Block::new(self.0 as u8, 0) } else { Block::AIR }
        }
    }

    #[test]
    fn a_game_registers_its_own_generators_after_the_engines() {
        let info = GeneratorInfo { id: "test-checkerboard", name: "Checkers", description: "test", uses_seed: true };
        register_generator(info, |seed| Box::new(Checkerboard(seed)));
        register_generator(info, |seed| Box::new(Checkerboard(seed + 1))); // replaces, not duplicates
        let ids: Vec<_> = generators().iter().map(|g| g.id).collect();
        assert_eq!(ids.iter().filter(|&&id| id == "test-checkerboard").count(), 1);
        assert!(ids.iter().position(|&id| id == "test-checkerboard") > ids.iter().position(|&id| id == "air"));
        assert_eq!(generator_info(" Test-Checkerboard ").unwrap().name, "Checkers");
        assert_eq!(create_generator("test-checkerboard", 2).unwrap().generate(0, 0, 0).id(), 3);
    }

    #[test]
    #[should_panic(expected = "engine generator")]
    fn engine_generators_cannot_be_replaced() {
        register_generator(GeneratorInfo { id: "island", name: "", description: "", uses_seed: false }, |_| Box::new(AirGenerator));
    }

    #[test]
    fn seeds_change_only_the_generators_that_use_them() {
        for info in generators() {
            let (a, b) = (create_generator(info.id, 1).unwrap(), create_generator(info.id, 123_456).unwrap());
            let differs = (-30..30).any(|x| {
                (-30..30).any(|y| (0..4).any(|z| a.generate(x, y, z) != b.generate(x, y, z)))
            });
            assert_eq!(differs, info.uses_seed, "{}", info.id);
        }
    }

    #[test]
    fn the_map_cvars_choose_the_generator() {
        let mut cvars = CVarSystem::map();
        assert_eq!(cvars.get_str("generator"), Ok("island"));
        assert_eq!(cvars.get_i32("generatorSeed"), Ok(1));
        let island = generator_from_cvars(&cvars).unwrap();
        assert_eq!(island.spawn_point(), IslandGenerator::new(1).peak());

        cvars.set("generator", "arena").unwrap();
        cvars.set("generatorSeed", "-5").unwrap();
        let arena = generator_from_cvars(&cvars).unwrap();
        let expected = ArenaGenerator::new(-5i32 as u32 as u64);
        assert_eq!(arena.generate(0, 0, 1), expected.generate(0, 0, 1));

        cvars.set("generator", "nonsense").unwrap();
        assert!(generator_from_cvars(&cvars).is_none());
    }

    #[test]
    fn the_choice_survives_saving_the_map_settings() {
        let mut map = CVarSystem::map();
        // Defaults are not written: a fresh map file still only has its version.
        assert_eq!(map.save_string(), "mapversion 4\n");
        map.set("generator", "arena").unwrap();
        map.set("generatorSeed", "42").unwrap();
        let text = map.save_string();
        assert_eq!(text, "generator arena\ngeneratorseed 42\nmapversion 4\n");

        let mut loaded = CVarSystem::map();
        assert_eq!(loaded.load_str(&text).applied, 3);
        assert_eq!(loaded.get_str("generator"), Ok("arena"));
        assert_eq!(loaded.get_i32("generatorSeed"), Ok(42));
    }

    #[test]
    fn the_default_spawn_is_the_origin_and_spawns_nothing() {
        assert_eq!(AirGenerator.spawn_point(), (0, 0));
        assert!(AirGenerator.spawn_entities(0, 0, 0).is_empty());
    }

    #[test]
    fn java_ints_become_blocks_like_chunk_fill() {
        assert_eq!(block_from_java_int(17 + (1 << 8)), Block::new(17, 1));
        assert_eq!(block_from_java_int(-64), Block::new(192, 255));
        assert_eq!(block_from_java_int(0), Block::AIR);
    }
}
