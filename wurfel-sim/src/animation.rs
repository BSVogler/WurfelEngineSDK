//! Frame animation of blocks, ported from the Java `Animatable` and `AnimatedBlock` (and its
//! only subclass `Sea`, the waves).
//!
//! The Java block animated itself in `update`. Blocks here are a packed `u16` in a chunk, so the
//! animation is a separate [`BlockAnimation`] that is given the current value and returns the next
//! one, and [`AnimatedBlocks`] keeps one per animated cell and writes the values into a [`World`].
//! The entity counterpart (`EntityAnimation`) is [`crate::entity::animation::Animation`].
//!
//! Differences from the Java engine:
//! * durations are in seconds, not milliseconds;
//! * the Java `runningForth` was never set to `true`, so a looping animation without bounce played
//!   backwards (`3, 2, 1, 0, 3...`). It starts forwards here; with bounce the two behave alike
//!   apart from the first step;
//! * a value past the end of the animation is clamped to the last frame (the Java check was off
//!   by one and read past the array).

use std::collections::HashMap;

use crate::World;

/// Something that can be started and stopped (the Java `Animatable`).
pub trait Animatable {
    fn start(&mut self);
    fn stop(&mut self);
}

/// The frame timing of one animated block.
#[derive(Debug, Clone)]
pub struct BlockAnimation {
    /// How long each frame (block value) is shown, in seconds.
    durations: Vec<f32>,
    counter: f32,
    running: bool,
    looping: bool,
    /// Play back and forth instead of jumping from the last frame to the first.
    bounce: bool,
    running_forth: bool,
}

impl BlockAnimation {
    /// `durations` has the time of every frame; frame `n` is block value `n`. With `looping` false
    /// it stops on the last frame.
    pub fn new(durations: Vec<f32>, autostart: bool, looping: bool) -> Self {
        BlockAnimation { durations, counter: 0.0, running: autostart, looping, bounce: false, running_forth: true }
    }

    /// The waves of `Sea`: four frames of 0.4 s, looping and bouncing.
    pub fn sea() -> Self {
        BlockAnimation::new(vec![0.4; 4], true, true).bounce(true)
    }

    /// Play the animation back and forth (`setBounce`).
    pub fn bounce(mut self, bounce: bool) -> Self {
        self.bounce = bounce;
        self
    }

    pub fn is_running(&self) -> bool {
        self.running
    }

    pub fn frames(&self) -> usize {
        self.durations.len()
    }

    /// Advance by `dt` seconds. `value` is the block's current value; returns the value to set.
    pub fn update(&mut self, dt: f32, value: u8) -> u8 {
        let len = self.durations.len() as i32;
        if !self.running || len == 0 {
            return value;
        }
        self.counter += dt;
        let mut current = (value as i32).min(len - 1);
        let duration = self.durations[current as usize];
        if self.counter >= duration {
            // Stay in the circle: a long step may carry over into the next frame's time.
            self.counter = if duration > 0.0 { self.counter % duration } else { 0.0 };
            current += if self.running_forth { 1 } else { -1 };

            if current >= len {
                if self.looping {
                    if self.bounce && self.running_forth {
                        self.running_forth = false; // go back, repeating no frame
                        current = (len - 2).max(0);
                    } else {
                        current = 0;
                    }
                } else {
                    self.running = false;
                    current = len - 1;
                }
            } else if current < 0 {
                if self.looping {
                    if self.bounce && !self.running_forth {
                        self.running_forth = true;
                        current = 1.min(len - 1);
                    } else {
                        current = len - 1;
                    }
                } else {
                    self.running = false;
                    current = 0;
                }
            }
        }
        current as u8
    }
}

impl Animatable for BlockAnimation {
    fn start(&mut self) {
        self.running = true;
    }

    fn stop(&mut self) {
        self.running = false;
    }
}

/// The animated cells of a world.
#[derive(Debug, Default)]
pub struct AnimatedBlocks {
    /// By cell; the block id is kept to notice that the cell was replaced by something else.
    cells: HashMap<(i32, i32, i32), (u8, BlockAnimation)>,
}

impl AnimatedBlocks {
    pub fn new() -> Self {
        Self::default()
    }

    /// Animate the block at a cell (as it is now). Replaces an earlier animation of the cell.
    pub fn add(&mut self, world: &World, (x, y, z): (i32, i32, i32), animation: BlockAnimation) {
        self.cells.insert((x, y, z), (world.get(x, y, z).id(), animation));
    }

    pub fn len(&self) -> usize {
        self.cells.len()
    }

    pub fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }

    /// Advance all animations by `dt` seconds and write the new values into the world. Cells whose
    /// block was replaced or that are not loaded (any more) are forgotten. Returns how many
    /// values changed.
    pub fn update(&mut self, world: &mut World, dt: f32) -> usize {
        let mut changed = 0;
        self.cells.retain(|&(x, y, z), (id, animation)| {
            let block = world.get(x, y, z);
            if block.id() != *id || !world.is_loaded_at(x, y) {
                return false;
            }
            let value = animation.update(dt, block.value());
            if value != block.value() {
                world.set(x, y, z, crate::Block::new(block.id(), value));
                changed += 1;
            }
            true
        });
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block::id;
    use crate::{AirGenerator, Block};

    /// Step once per frame duration and collect the values shown.
    fn values(mut animation: BlockAnimation, steps: usize, start: u8) -> Vec<u8> {
        let mut value = start;
        let mut out = vec![value];
        for _ in 0..steps {
            value = animation.update(0.4, value);
            out.push(value);
        }
        out
    }

    #[test]
    fn a_looping_animation_runs_through_the_frames_and_starts_over() {
        let animation = BlockAnimation::new(vec![0.4; 3], true, true);
        assert_eq!(values(animation, 6, 0), [0, 1, 2, 0, 1, 2, 0]);
    }

    #[test]
    fn bounce_goes_back_without_repeating_the_end_frames() {
        assert_eq!(values(BlockAnimation::sea(), 9, 0), [0, 1, 2, 3, 2, 1, 0, 1, 2, 3]);
    }

    #[test]
    fn without_looping_it_stops_on_the_last_frame() {
        let mut animation = BlockAnimation::new(vec![0.4; 3], true, false);
        let mut value = 0;
        for _ in 0..5 {
            value = animation.update(0.4, value);
        }
        assert_eq!(value, 2);
        assert!(!animation.is_running());
    }

    #[test]
    fn a_stopped_animation_keeps_the_value_until_started() {
        let mut animation = BlockAnimation::new(vec![0.4; 3], false, true);
        assert_eq!(animation.update(10.0, 1), 1);
        animation.start();
        assert_eq!(animation.update(0.5, 1), 2);
        animation.stop();
        assert_eq!(animation.update(10.0, 2), 2);
    }

    #[test]
    fn frames_last_as_long_as_their_duration_says() {
        let mut animation = BlockAnimation::new(vec![1.0, 0.2], true, true);
        assert_eq!(animation.update(0.9, 0), 0);
        assert_eq!(animation.update(0.2, 0), 1, "1.1 s in: the first frame is over");
        // 0.1 s of the overshoot carried into the second frame (the Java `counter %= duration`).
        assert_eq!(animation.update(0.05, 1), 1);
        assert_eq!(animation.update(0.1, 1), 0, "the second frame is short");
    }

    #[test]
    fn a_value_past_the_end_and_a_single_frame_do_not_panic() {
        let mut animation = BlockAnimation::new(vec![0.4; 2], true, true);
        assert_eq!(animation.update(0.4, 200), 0, "clamped to the last frame, then wraps");
        let mut one = BlockAnimation::sea();
        one.durations.truncate(1);
        for _ in 0..4 {
            assert_eq!(one.update(0.4, 0), 0);
        }
        assert_eq!(BlockAnimation::new(Vec::new(), true, true).update(1.0, 5), 5);
    }

    #[test]
    fn animated_blocks_change_the_value_in_the_world_and_keep_the_id() {
        let mut world = World::new(AirGenerator);
        world.set(1, 1, 1, Block::new(id::WATER, 0));
        let mut animated = AnimatedBlocks::new();
        animated.add(&world, (1, 1, 1), BlockAnimation::sea());
        assert_eq!(animated.update(&mut world, 0.1), 0, "frame not over yet");
        assert_eq!(animated.update(&mut world, 0.3), 1);
        assert_eq!(world.get(1, 1, 1), Block::new(id::WATER, 1));
    }

    #[test]
    fn a_replaced_block_is_no_longer_animated() {
        let mut world = World::new(AirGenerator);
        world.set(1, 1, 1, Block::new(id::WATER, 0));
        let mut animated = AnimatedBlocks::new();
        animated.add(&world, (1, 1, 1), BlockAnimation::sea());
        world.set(1, 1, 1, Block::new(id::STONE, 0));
        assert_eq!(animated.update(&mut world, 1.0), 0);
        assert!(animated.is_empty());
        assert_eq!(world.get(1, 1, 1), Block::new(id::STONE, 0));
    }
}
