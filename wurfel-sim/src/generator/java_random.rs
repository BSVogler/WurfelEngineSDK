//! `java.util.Random`, bit for bit, plus a jump-ahead.
//!
//! The Java `ArenaGenerator` derives a block's randomness by re-seeding a `Random` and calling
//! `nextFloat()` once per unit of `x * y * z`. To reproduce its maps exactly we need the same
//! linear congruential generator; the jump-ahead gives the n-th value in O(log n) instead of
//! looping n times (n can be in the millions for far away blocks).

const MULTIPLIER: u64 = 0x5_DEEC_E66D;
const ADDEND: u64 = 0xB;
const MASK: u64 = (1 << 48) - 1;

pub struct JavaRandom {
    state: u64,
}

impl JavaRandom {
    /// `new Random(seed)`.
    pub fn new(seed: i64) -> Self {
        JavaRandom { state: (seed as u64 ^ MULTIPLIER) & MASK }
    }

    fn next(&mut self, bits: u32) -> i32 {
        self.state = (self.state.wrapping_mul(MULTIPLIER).wrapping_add(ADDEND)) & MASK;
        (self.state >> (48 - bits)) as i32
    }

    pub fn next_int(&mut self) -> i32 {
        self.next(32)
    }

    /// `nextFloat()`: 24 random bits as a fraction of 2^24.
    pub fn next_float(&mut self) -> f32 {
        self.next(24) as f32 / (1u32 << 24) as f32
    }

    /// What `r.setSeed(seed); float f = 0; for (i = 0; i < n; i++) f = r.nextFloat();` leaves in `f`:
    /// 0 for `n <= 0`, otherwise the n-th float. Computed without looping.
    pub fn nth_float(seed: i64, n: i32) -> f32 {
        if n <= 0 {
            return 0.0;
        }
        let start = (seed as u64 ^ MULTIPLIER) & MASK;
        // One step is the affine map s -> a*s + c (mod 2^48); n steps are its n-th power.
        let (mut result, mut power) = ((1u64, 0u64), (MULTIPLIER, ADDEND));
        let mut remaining = n as u32;
        while remaining > 0 {
            if remaining & 1 == 1 {
                result = compose(result, power);
            }
            power = compose(power, power);
            remaining >>= 1;
        }
        let state = (result.0.wrapping_mul(start).wrapping_add(result.1)) & MASK;
        ((state >> 24) as i32) as f32 / (1u32 << 24) as f32
    }
}

/// `second(first(s))` for affine maps `(a, c)` meaning `s -> a*s + c` modulo 2^48.
fn compose(first: (u64, u64), second: (u64, u64)) -> (u64, u64) {
    (
        first.0.wrapping_mul(second.0) & MASK,
        second.0.wrapping_mul(first.1).wrapping_add(second.1) & MASK,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Values written by the real `java.util.Random` (see fixtures/generators/regenerate.sh).
    #[test]
    fn matches_the_real_java_random() {
        let mut seen = 0;
        let mut seed = 0i64;
        for line in include_str!("../../fixtures/generators/random.txt").lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            match parts[0] {
                "seed" => {
                    seed = parts[1].parse().unwrap();
                    let expected: i32 = parts[3].parse().unwrap();
                    assert_eq!(JavaRandom::new(seed).next_int(), expected, "nextInt for seed {seed}");
                }
                "floats" => {
                    let mut random = JavaRandom::new(seed);
                    for (i, bits) in parts[1..].iter().enumerate() {
                        let expected = f32::from_bits(bits.parse::<u32>().unwrap());
                        assert_eq!(random.next_float(), expected, "float {i} for seed {seed}");
                        seen += 1;
                    }
                }
                other => panic!("unexpected fixture line {other}"),
            }
        }
        assert_eq!(seen, 4 * 40);
    }

    #[test]
    fn nth_float_equals_looping_nextfloat() {
        for seed in [0i64, 1, 42, -7, 123456789012345] {
            let mut random = JavaRandom::new(seed);
            let mut last = 0.0;
            assert_eq!(JavaRandom::nth_float(seed, 0), 0.0);
            assert_eq!(JavaRandom::nth_float(seed, -5), 0.0, "negative counts never loop in Java");
            for n in 1..=300 {
                last = random.next_float();
                assert_eq!(JavaRandom::nth_float(seed, n), last, "seed {seed}, n {n}");
            }
            // And far beyond what a loop in a test should do twice.
            let mut random = JavaRandom::new(seed);
            for _ in 0..200_000 {
                last = random.next_float();
            }
            assert_eq!(JavaRandom::nth_float(seed, 200_000), last);
        }
    }
}
