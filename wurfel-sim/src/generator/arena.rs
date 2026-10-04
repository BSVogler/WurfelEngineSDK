use super::java_random::JavaRandom;
use super::{splitmix64, Generator};
use crate::block::id;
use crate::Block;

/// The "Weapon of Choice" demo arena (`ArenaGenerator`): a sand floor with scattered pillars of
/// dirt that have grass on top. About one column in twenty is a pillar.
///
/// Java chose its seed with `Math.random()` on the first call; here it is a parameter. The pillar
/// test is the original one, quirks included: the "random" value for a column is the
/// `x * y * z`-th float of the seeded generator (so columns with the same product look alike), and
/// for a product of zero or less (the whole axes and the negative quadrants) it is 0, which always
/// passes `< 0.05`, so those columns are all pillars.
pub struct ArenaGenerator {
    seed: i64,
}

impl ArenaGenerator {
    /// Derive the Java seed from `seed`. Never 0, which the Java code treated as "not seeded yet".
    pub fn new(seed: u64) -> Self {
        let mut state = seed;
        let derived = splitmix64(&mut state) as i64;
        ArenaGenerator { seed: if derived == 0 { 1 } else { derived } }
    }

    /// Use exactly this `long` seed, as the Java field would hold it.
    pub fn with_java_seed(seed: i64) -> Self {
        ArenaGenerator { seed }
    }

    fn random(&self, x: i32, y: i32, z: i32) -> f32 {
        // `int field = x * y * z;` wraps around in Java.
        let field = x.wrapping_mul(y).wrapping_mul(z);
        JavaRandom::nth_float(self.seed, field)
    }
}

impl Generator for ArenaGenerator {
    fn generate(&self, x: i32, y: i32, z: i32) -> Block {
        if z == 0 {
            Block::new(id::SAND, 0)
        } else if z == 1 && self.random(x, y, z) < 0.05 {
            Block::new(id::DIRT, 0)
        } else if z == 2 && self.generate(x, y, 1).id() == id::DIRT {
            Block::new(id::GRASS, 0)
        } else {
            Block::AIR
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generator::fixture;

    #[test]
    fn matches_the_java_generator_for_four_seeds() {
        let mut seed = 0i64;
        let mut section = String::new();
        let mut sections = 0;
        let flush = |seed: i64, section: &str, sections: &mut i32| {
            if section.is_empty() {
                return;
            }
            let columns = fixture::columns(section);
            assert_eq!(columns.len(), 37 * 37);
            fixture::assert_generator_matches(&ArenaGenerator::with_java_seed(seed), &columns);
            *sections += 1;
        };
        for line in include_str!("../../fixtures/generators/arena.txt").lines() {
            if let Some(rest) = line.strip_prefix("seed ") {
                flush(seed, &section, &mut sections);
                section.clear();
                seed = rest.parse().unwrap();
            } else {
                section.push_str(line);
                section.push('\n');
            }
        }
        flush(seed, &section, &mut sections);
        assert_eq!(sections, 4);
    }

    #[test]
    fn pillars_have_grass_on_top_and_are_about_one_in_twenty() {
        let g = ArenaGenerator::new(7);
        let (mut pillars, mut columns) = (0, 0);
        for x in 1..80 {
            for y in 1..80 {
                columns += 1;
                let (z1, z2) = (g.generate(x, y, 1), g.generate(x, y, 2));
                if z1.id() == id::DIRT {
                    pillars += 1;
                    assert_eq!(z2.id(), id::GRASS);
                } else {
                    assert!(z1.is_air() && z2.is_air());
                }
                assert_eq!(g.generate(x, y, 0).id(), id::SAND);
                assert!(g.generate(x, y, 3).is_air());
            }
        }
        let share = pillars as f32 / columns as f32;
        assert!((0.02..0.09).contains(&share), "{pillars} pillars in {columns} columns");
    }

    #[test]
    fn the_axes_and_negative_quadrants_are_solid_with_pillars_like_in_java() {
        let g = ArenaGenerator::new(7);
        for (x, y) in [(0, 0), (0, 9), (13, 0), (-4, 6), (5, -8)] {
            assert_eq!(g.generate(x, y, 1).id(), id::DIRT, "column {x},{y}");
        }
    }

    #[test]
    fn the_same_seed_gives_the_same_arena() {
        let (a, b) = (ArenaGenerator::new(3), ArenaGenerator::new(3));
        for (x, y) in [(5, 7), (31, 2), (-2, 40)] {
            assert_eq!(a.generate(x, y, 1), b.generate(x, y, 1));
        }
    }
}
