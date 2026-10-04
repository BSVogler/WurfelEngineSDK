//! Caveland's map generator (`ChunkGenerator`), its block ids (`CavelandBlocks.CLBlocks`) and the
//! helpers that describe where the caves are.
//!
//! # The map
//!
//! * **Overworld**, `y < 1000`: a flat plain, three layers of dirt with grass on top. It is
//!   unbounded, like the Java map.
//! * **The gap**, `1000 <= y < 1200`: air (nothing to stand on).
//! * **Underworld**, `y >= 1200`: a repeating grid of diamond-shaped cave rooms. A room is 21
//!   blocks wide with 5 blocks of padding and a wall of 1, so rooms repeat every 27 blocks in `x`
//!   and, because `y` is stretched by 0.5, every 54 rows in `y`. Cave number `n` is the `n`-th
//!   column of rooms ([`cave_number`]); cave 0 has the way up to the surface at `(0, 0, 5)`.
//!
//! The float arithmetic is translated literally (`f32`, Java's `%` is Rust's `%`, int to float
//! conversions at the same points) so that every block matches the Java generator. The tests
//! compare against output of the real Java class, see `fixtures/generators/regenerate.sh`.

use crate::generator::{block_from_java_int, EntitySpawn, Generator};
use crate::Block;

/// Block ids. The engine ones (0 to 9) are from `RenderCell.getName`, the others from
/// `CavelandBlocks.CLBlocks`.
pub mod blocks {
    pub const AIR: u8 = 0;
    pub const GRASS: u8 = 1;
    pub const DIRT: u8 = 2;
    pub const STONE: u8 = 3;
    /// Blocks movement but is not drawn.
    pub const INVISIBLE_OBSTACLE: u8 = 4;
    pub const SAND: u8 = 8;
    pub const WATER: u8 = 9;

    pub const CONSTRUCTION_SITE: u8 = 11;
    pub const OVEN: u8 = 12;
    pub const TORCH: u8 = 13;
    pub const POWER_STATION: u8 = 14;
    pub const LIFT: u8 = 15;
    /// Entrance of a cave.
    pub const ENTRY: u8 = 16;
    pub const INDESTRUCTIBLE_OBSTACLE: u8 = 17;
    pub const LIFT_GROUND: u8 = 18;
    pub const CRYSTAL: u8 = 41;
    pub const SULFUR: u8 = 42;
    pub const IRON_ORE: u8 = 43;
    pub const COAL: u8 = 44;
    pub const TURRET: u8 = 52;
    pub const ROBOT_FACTORY: u8 = 53;
    pub const POWER_CABLE: u8 = 54;
    pub const RAILS: u8 = 55;
    pub const BOOSTER_RAILS: u8 = 56;
    pub const FLAG_POLE: u8 = 60;
    pub const TREE: u8 = 72;
    /// Java's `UNDEFINED`, the byte -1.
    pub const UNDEFINED: u8 = 255;
}

/// Every block with a smaller `y` is overworld, every block above is cave (`CAVESBORDER`).
pub const CAVES_BORDER: i32 = 1000;
/// Caves are generated from this `y` on (`GENERATORBORDER`).
pub const GENERATOR_BORDER: i32 = 1200;

/// Width of a room.
const G: f32 = 21.0;
/// Padding between rooms.
const P: f32 = 5.0;
const Y_STRETCH: f32 = 0.5;
/// Thickness of the wall.
const WALL_SIZE: f32 = 1.0;
const ROOM_WITH_PADDING: f32 = G + P + WALL_SIZE;

/// Results of [`inside_outside`].
pub const OUTSIDE: i32 = -1;
pub const WALL: i32 = 0;
pub const INSIDE: i32 = 1;
pub const ENTRY: i32 = 2;
pub const EXIT: i32 = 3;

/// Which cave the block column belongs to; the entry cave is 0. -1 below the underworld.
/// (`getCaveNumber`; the Java version also takes a `z` it does not use.)
pub fn cave_number(x: i32, y: i32) -> i32 {
    if y < GENERATOR_BORDER {
        return -1;
    }
    (x as f32 / ROOM_WITH_PADDING).floor() as i32
}

/// The way up out of cave `cave`: a cell on its upper edge (`getCaveUp`).
pub fn cave_up(cave: i32) -> (i32, i32, i32) {
    ((ROOM_WITH_PADDING * cave as f32 + G) as i32, GENERATOR_BORDER + 66, 7)
}

/// Where the way down to cave `cave` ends up (`getCaveDown`).
pub fn cave_down(cave: i32) -> (i32, i32, i32) {
    ((ROOM_WITH_PADDING * cave as f32 + 11.0) as i32, GENERATOR_BORDER + 77, 4)
}

/// The middle of cave `cave` (`getCaveCenter`).
pub fn cave_center(cave: i32) -> (i32, i32, i32) {
    (
        (ROOM_WITH_PADDING * cave as f32 + P + WALL_SIZE + G / 2.0) as i32,
        (GENERATOR_BORDER as f32 + 20.0 + (P + G) / Y_STRETCH) as i32,
        4,
    )
}

/// Position inside the repeating room pattern, with the padding removed. Negative values are the
/// gap between rooms.
fn room_coordinates(x: i32, y: i32) -> (f32, f32) {
    let x_room = ((x as f32 % ROOM_WITH_PADDING) + ROOM_WITH_PADDING) % ROOM_WITH_PADDING - P;
    let y_room = (((y as f32 * Y_STRETCH) % ROOM_WITH_PADDING) + ROOM_WITH_PADDING) % ROOM_WITH_PADDING - P;
    (x_room, y_room)
}

/// What kind of place the cell is: [`OUTSIDE`], [`WALL`], [`INSIDE`], [`ENTRY`] or [`EXIT`]
/// (`insideOutside`). Always [`OUTSIDE`] below the underworld.
pub fn inside_outside(x: i32, y: i32, z: i32) -> i32 {
    if y < GENERATOR_BORDER {
        return OUTSIDE;
    }
    let (mut x_room, y_room) = room_coordinates(x, y);
    if x_room < 0.0 || y_room < 0.0 {
        return OUTSIDE;
    }

    // Fix for even walls on the staggered map.
    if x_room < G / 2.0 && y % 2 == 1 {
        x_room += 1.0;
    }

    let outside_room = x_room + y_room <= G / 2.0 // top left
        || x_room - y_room >= G / 2.0 // top right
        || x_room + y_room >= 3.0 * G / 2.0 // bottom right
        || y_room - x_room >= G / 2.0; // bottom left

    // The same test with the wall thickness taken off.
    let outside_wall = x_room + y_room <= G / 2.0 - WALL_SIZE
        || x_room - y_room >= G / 2.0 + WALL_SIZE
        || x_room + y_room >= 3.0 * G / 2.0 + WALL_SIZE
        || y_room - x_room >= G / 2.0 + WALL_SIZE;

    if outside_wall {
        OUTSIDE
    } else if outside_room {
        WALL
    } else if x_room == 6.0 && y_room == G - 8.0 && z == 2 {
        ENTRY
    } else if x_room == G - 7.0 && y_room == 6.0 && z == 4 {
        EXIT
    } else {
        INSIDE
    }
}

/// `ChunkGenerator`.
pub struct CavelandGenerator;

impl Generator for CavelandGenerator {
    fn generate(&self, x: i32, y: i32, z: i32) -> Block {
        // Java returns an int: id in the low byte, value in the next one.
        let java: i32 = 'result: {
            if y < CAVES_BORDER {
                // Overworld: dirt, then grass on top.
                if z < 3 {
                    break 'result blocks::DIRT as i32;
                }
                if z == 3 {
                    break 'result blocks::GRASS as i32;
                }
                break 'result 0;
            }
            if y < GENERATOR_BORDER {
                break 'result 0;
            }

            // Underworld.
            let place = inside_outside(x, y, z);
            if place == ENTRY {
                break 'result blocks::ENTRY as i32;
            }
            if place == WALL && z <= 4 {
                // The wall is the indestructible obstacle with value 1.
                break 'result blocks::INDESTRUCTIBLE_OBSTACLE as i32 + (1 << 8);
            }
            if place == OUTSIDE {
                break 'result 0;
            }

            // Scattered ore on the floor. int arithmetic wraps like Java's.
            if z == 3 {
                let hash = x
                    .wrapping_mul(y)
                    .wrapping_mul(2)
                    .wrapping_add(x)
                    .wrapping_add(y.wrapping_mul(3))
                    .wrapping_add(500);
                if hash % 8 == y % 7 {
                    if x % 2 == 0 && y % 2 == 0 {
                        break 'result if y % 5 == 0 { blocks::SULFUR } else { blocks::COAL } as i32;
                    } else {
                        break 'result if y % 8 == 0 { blocks::IRON_ORE as i32 } else { blocks::DIRT as i32 };
                    }
                }
            }

            // Floor.
            if z <= 2 {
                break 'result blocks::DIRT as i32;
            }
            0
        };
        block_from_java_int(java)
    }

    /// Portals in the ceiling of every cave: up to the surface from cave 0, otherwise down to the
    /// previous cave.
    fn spawn_entities(&self, x: i32, y: i32, z: i32) -> Vec<EntitySpawn> {
        if y > GENERATOR_BORDER {
            let (x_room, y_room) = room_coordinates(x, y);
            if x_room == G - 5.0 && y_room == P + 2.0 && z == 4 {
                let number = cave_number(x, y);
                let target = if number == 0 { (0, 0, 5) } else { cave_down(number - 1) };
                return vec![EntitySpawn {
                    kind: "ExitPortal",
                    cell: (x, y, z),
                    target: Some(target),
                    extras: vec![("enemy_spawner", "true".to_string())],
                }];
            }
        }
        Vec::new()
    }

    /// The target of the portal out of cave 0 is `(0, 0, 5)`.
    fn spawn_point(&self) -> (i32, i32) {
        (0, 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generator::fixture;

    /// `ChunkGenerator.main` prints this picture of `insideOutside(x, y, 4)`: the real Java output.
    #[test]
    fn reproduces_the_java_ascii_picture_of_the_caves() {
        let expected = include_str!("../fixtures/generators/caveland_ascii.txt");
        let mut picture = String::new();
        for y in GENERATOR_BORDER..GENERATOR_BORDER + 100 {
            for x in 0..100 {
                picture.push_str(match inside_outside(x, y, 4) {
                    1 => ". ",
                    0 => "# ",
                    -1 => "_ ",
                    2 => "i ",
                    3 => "o ",
                    other => panic!("unexpected {other}"),
                });
            }
            picture.push('\n');
        }
        for (row, (got, want)) in picture.lines().zip(expected.lines()).enumerate() {
            assert_eq!(got, want, "row {row} (y = {})", GENERATOR_BORDER + row as i32);
        }
        assert_eq!(picture.lines().count(), expected.lines().count());
        // The picture really contains every kind of place.
        for symbol in ["# ", ". ", "_ ", "o "] {
            assert!(picture.contains(symbol), "no {symbol:?} in the picture");
        }
    }

    #[test]
    fn inside_outside_matches_java_in_four_layers_including_negative_x() {
        let mut checked = 0;
        for line in include_str!("../fixtures/generators/caveland_inside_outside.txt").lines() {
            let (head, values) = line.split_once(':').unwrap();
            let mut coords = head.split_whitespace().map(|p| p.parse::<i32>().unwrap());
            let (x, y) = (coords.next().unwrap(), coords.next().unwrap());
            for (z, java) in [2, 3, 4, 5].into_iter().zip(values.split_whitespace()) {
                assert_eq!(inside_outside(x, y, z), java.parse::<i32>().unwrap(), "at {x},{y},{z}");
                checked += 1;
            }
        }
        assert_eq!(checked, 91 * 46 * 4);
    }

    #[test]
    fn the_overworld_matches_java() {
        let columns = fixture::columns(include_str!("../fixtures/generators/caveland_overworld.txt"));
        assert_eq!(columns.len(), 11 * 9);
        fixture::assert_generator_matches(&CavelandGenerator, &columns);
    }

    #[test]
    fn the_underworld_matches_java_block_for_block() {
        let columns = fixture::columns(include_str!("../fixtures/generators/caveland_underworld.txt"));
        assert_eq!(columns.len(), 64 * 35 + 4 * 16);
        fixture::assert_generator_matches(&CavelandGenerator, &columns);
        // The fixture is not trivially empty: all the underworld block types occur in it.
        let mut seen = std::collections::HashSet::new();
        for (_, _, values) in &columns {
            seen.extend(values.iter().copied());
        }
        for java in [
            blocks::DIRT as i32,
            blocks::ENTRY as i32,
            blocks::COAL as i32,
            blocks::SULFUR as i32,
            blocks::IRON_ORE as i32,
            blocks::INDESTRUCTIBLE_OBSTACLE as i32 + 256,
        ] {
            assert!(seen.contains(&java), "the fixture window has no block {java}");
        }
    }

    #[test]
    fn exact_blocks_at_chosen_places() {
        let g = CavelandGenerator;
        // Overworld column.
        for z in 0..3 {
            assert_eq!(g.generate(7, -30, z), Block::new(blocks::DIRT, 0));
        }
        assert_eq!(g.generate(7, -30, 3), Block::new(blocks::GRASS, 0));
        assert!(g.generate(7, -30, 4).is_air());
        // Nothing between the overworld and the caves.
        assert!(g.generate(0, 1000, 0).is_air() && g.generate(0, 1199, 3).is_air());
        // Cave 0, middle of the first room: floor and open air.
        let (cx, cy, _) = cave_center(0);
        assert!(!g.generate(cx, cy, 1).is_air());
        assert!(g.generate(cx, cy, 5).is_air());
        // Walls are the indestructible obstacle with value 1 up to z = 4.
        let wall = (0..40)
            .flat_map(|x| (1200..1260).map(move |y| (x, y)))
            .find(|&(x, y)| inside_outside(x, y, 4) == WALL)
            .expect("the first room has a wall");
        assert_eq!(g.generate(wall.0, wall.1, 4), Block::new(blocks::INDESTRUCTIBLE_OBSTACLE, 1));
        assert!(g.generate(wall.0, wall.1, 5).is_air(), "the wall is not taller than 4");
    }

    #[test]
    fn spawned_entities_match_java() {
        let mut got = Vec::new();
        for x in -30..=130 {
            for y in 1190..=1300 {
                for z in 0..=8 {
                    for spawn in CavelandGenerator.spawn_entities(x, y, z) {
                        assert_eq!(spawn.kind, "ExitPortal");
                        assert_eq!(spawn.extras, vec![("enemy_spawner", "true".to_string())]);
                        let t = spawn.target.unwrap();
                        let c = spawn.cell;
                        got.push(format!(
                            "ExitPortal at {} {} {} target {} {} {} enemy_spawner true",
                            c.0, c.1, c.2, t.0, t.1, t.2
                        ));
                    }
                }
            }
        }
        let expected: Vec<&str> = include_str!("../fixtures/generators/caveland_spawns.txt").lines().collect();
        assert_eq!(got, expected);
        assert_eq!(got.len(), 12);
        // Cave 0's portal leads to the surface spawn.
        assert!(got.iter().any(|s| s.starts_with("ExitPortal at 21 1212 4 target 0 0 5")));
    }

    #[test]
    fn the_overworld_spawns_nothing() {
        assert!(CavelandGenerator.spawn_entities(21, 1000, 4).is_empty());
        assert!(CavelandGenerator.spawn_entities(21, 1200, 4).is_empty(), "the border row itself spawns nothing (`y > GENERATORBORDER`)");
    }

    #[test]
    fn cave_helpers_match_java() {
        let mut caves = 0;
        let mut numbers = 0;
        for line in include_str!("../fixtures/generators/caveland_helpers.txt").lines() {
            let p: Vec<&str> = line.split_whitespace().collect();
            let int = |i: usize| p[i].parse::<i32>().unwrap();
            if p[0] == "cave" {
                let n = int(1);
                assert_eq!(cave_up(n), (int(3), int(4), int(5)), "cave_up({n})");
                assert_eq!(cave_down(n), (int(7), int(8), int(9)), "cave_down({n})");
                assert_eq!(cave_center(n), (int(11), int(12), int(13)), "cave_center({n})");
                caves += 1;
            } else {
                assert_eq!(p[0], "number");
                assert_eq!(cave_number(int(1), int(2)), int(3), "cave_number({}, {})", p[1], p[2]);
                numbers += 1;
            }
        }
        assert_eq!((caves, numbers), (11, 67 * 3));
    }

    #[test]
    fn cave_numbers_count_room_columns() {
        assert_eq!(cave_number(0, 1200), 0);
        assert_eq!(cave_number(26, 1200), 0);
        assert_eq!(cave_number(27, 1200), 1);
        assert_eq!(cave_number(-1, 1200), -1, "left of cave 0");
        assert_eq!(cave_number(5, 1199), -1, "not in the underworld");
    }

    #[test]
    fn players_spawn_at_the_surface_portal_target() {
        // Java: the exit portal of cave 0 leads to (0, 0, 5) on the surface.
        assert_eq!(CavelandGenerator.spawn_point(), (0, 0));
    }

}
