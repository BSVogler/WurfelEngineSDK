use super::Generator;
use crate::Block;

/// Nothing but air (`AirGenerator`).
pub struct AirGenerator;

impl Generator for AirGenerator {
    fn generate(&self, _x: i32, _y: i32, _z: i32) -> Block {
        Block::AIR
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generator::fixture;

    #[test]
    fn matches_the_java_generator() {
        let columns = fixture::columns(include_str!("../../fixtures/generators/air.txt"));
        assert_eq!(columns.len(), 25);
        fixture::assert_generator_matches(&AirGenerator, &columns);
        assert!(AirGenerator.generate(1_000_000, -3, 5).is_air());
    }
}
