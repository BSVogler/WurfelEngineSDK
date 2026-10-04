use super::{block_from_java_int, Generator};
use crate::Block;

/// Every block is the same block (`FullMapGenerator`). Handy for testing one block type.
///
/// The Java id is a signed `byte`, so ids of 128 and above come back as negative ints and the
/// engine then reads 255 as their value byte. That is reproduced.
pub struct FullMapGenerator {
    id: u8,
}

impl FullMapGenerator {
    pub fn new(id: u8) -> Self {
        FullMapGenerator { id }
    }
}

impl Generator for FullMapGenerator {
    fn generate(&self, _x: i32, _y: i32, _z: i32) -> Block {
        block_from_java_int(self.id as i8 as i32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generator::fixture;

    #[test]
    fn matches_the_java_generator_for_every_tested_id() {
        let mut checked = 0;
        let mut id = 0u8;
        for line in include_str!("../../fixtures/generators/fullmap.txt").lines() {
            if let Some(rest) = line.strip_prefix("id ") {
                id = rest.parse().unwrap();
            } else {
                let columns = fixture::columns(line);
                fixture::assert_generator_matches(&FullMapGenerator::new(id), &columns);
                checked += 1;
            }
        }
        assert_eq!(checked, 9);
    }

    #[test]
    fn high_ids_get_the_sign_extension_value_byte() {
        let block = FullMapGenerator::new(200).generate(0, 0, 0);
        assert_eq!((block.id(), block.value()), (200, 255));
        let block = FullMapGenerator::new(7).generate(0, 0, 9);
        assert_eq!((block.id(), block.value()), (7, 0));
    }
}
