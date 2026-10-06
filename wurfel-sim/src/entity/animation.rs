//! `EntityAnimation`: steps the parent's sprite value through a list of frame times.
//!
//! The block counterpart is [`crate::animation::BlockAnimation`]. Durations are in seconds (the
//! Java engine used milliseconds). Like the Java component it is added once and left alone; the
//! [`Animatable`] methods are for code that still holds the animation before adding it.

use super::{Component, Entity};
use crate::animation::Animatable;
use crate::World;

/// Plays frames `0..n` on the parent's `sprite_value`, `durations[i]` seconds for frame `i`.
pub struct Animation {
    durations: Vec<f32>,
    counter: f32,
    running: bool,
    looping: bool,
}

impl Animation {
    /// With `looping` false the parent is disposed after the last frame (a one-shot effect such as
    /// a puff of smoke or blood).
    pub fn new(durations: Vec<f32>, autostart: bool, looping: bool) -> Self {
        Animation { durations, counter: 0.0, running: autostart, looping }
    }

    /// Start with a head start in the first frame (`setOffset`), in seconds.
    pub fn with_offset(mut self, seconds: f32) -> Self {
        self.counter = seconds;
        self
    }

    pub fn is_running(&self) -> bool {
        self.running
    }
}

impl Animatable for Animation {
    fn start(&mut self) {
        self.running = true;
    }

    fn stop(&mut self) {
        self.running = false;
    }
}

impl Component for Animation {
    fn update(&mut self, parent: &mut Entity, _world: &World, dt: f32) -> bool {
        if !self.running {
            return true;
        }
        self.counter += dt;
        let frame = parent.sprite_value as usize;
        if frame >= self.durations.len() {
            // The value was set past the end from outside: stop instead of reading out of range.
            self.running = false;
        } else if self.counter >= self.durations[frame] {
            parent.sprite_value = parent.sprite_value.saturating_add(1);
            self.counter = 0.0;
            if parent.sprite_value as usize >= self.durations.len() {
                if self.looping {
                    parent.sprite_value = 0;
                } else {
                    parent.dispose();
                    self.running = false;
                    return false;
                }
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use glam::Vec3;

    use super::*;
    use crate::entity::{Entities, EntityId};
    use crate::AirGenerator;

    fn animated(animation: Animation) -> (Entities, EntityId, World) {
        let mut entities = Entities::new();
        let mut entity = Entity::new("flag", 40).at(Vec3::new(1.0, 1.0, 5.0));
        entity.add_component(Box::new(animation));
        let id = entities.spawn(entity);
        (entities, id, World::new(AirGenerator))
    }

    #[test]
    fn a_looping_animation_advances_the_sprite_value_and_wraps() {
        let (mut entities, id, world) = animated(Animation::new(vec![0.3, 0.3], true, true));
        let mut seen = Vec::new();
        for _ in 0..8 {
            entities.update(&world, 0.3);
            seen.push(entities.get(id).unwrap().sprite_value);
        }
        assert_eq!(seen, [1, 0, 1, 0, 1, 0, 1, 0]);
    }

    #[test]
    fn a_one_shot_animation_removes_its_entity_after_the_last_frame() {
        let (mut entities, id, world) = animated(Animation::new(vec![0.3], true, false));
        entities.update(&world, 0.2);
        assert!(entities.get(id).is_some());
        entities.update(&world, 0.2);
        assert!(entities.get(id).is_none());
    }

    #[test]
    fn it_waits_for_start_and_an_offset_shortens_the_first_frame() {
        let mut idle = Animation::new(vec![0.3, 0.3], false, true);
        assert!(!idle.is_running());
        idle.start();
        assert!(idle.is_running());
        idle.stop();
        let (mut entities, id, world) = animated(idle);
        entities.update(&world, 1.0);
        assert_eq!(entities.get(id).unwrap().sprite_value, 0, "not started");

        let (mut entities, id, world) = animated(Animation::new(vec![0.3, 0.3], true, true).with_offset(0.25));
        entities.update(&world, 0.1);
        assert_eq!(entities.get(id).unwrap().sprite_value, 1);
    }

    #[test]
    fn a_sprite_value_set_past_the_end_stops_the_animation_without_panicking() {
        let (mut entities, id, world) = animated(Animation::new(vec![0.3, 0.3], true, true));
        entities.get_mut(id).unwrap().sprite_value = 9;
        entities.update(&world, 1.0);
        entities.update(&world, 1.0);
        assert_eq!(entities.get(id).unwrap().sprite_value, 9);
    }
}
