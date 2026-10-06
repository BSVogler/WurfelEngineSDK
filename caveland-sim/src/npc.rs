//! Characters that talk and wander: Vanya, the Shopkeeper and the Bird.
//!
//! Each is plain data and rules; [`crate::Caveland`] keeps them next to the engine's entities and
//! feeds them what they need (positions, the world, dt).

use std::collections::VecDeque;

use glam::{Vec2, Vec3};
use wurfel_sim::entity::physics::is_on_ground;
use wurfel_sim::entity::Entity;
use wurfel_sim::generator::java_random::JavaRandom;
use wurfel_sim::World;

use crate::collectible::CollectibleType;
use crate::game::{cell_floor, Cell};

// ---- Vanya ------------------------------------------------------------------------------------

/// Speed of Vanya walking and flying to a waypoint, blocks per second (`d.scl(1.3f)`, `d.scl(2f)`).
pub const VANYA_WALK_SPEED: f32 = 1.3;
pub const VANYA_FLY_SPEED: f32 = 2.0;
/// Vanya hops all the time (`jump(6, true)`), blocks per second.
pub const VANYA_JUMP_SPEED: f32 = 6.0;
/// Closer than this to a waypoint counts as arrived: a quarter of a block.
const ARRIVED: f32 = 0.25;
/// Other Vanyas this close are removed: there is only one guide (`10 * GAME_EDGELENGTH`).
pub const VANYA_DUPLICATE_RADIUS: f32 = 10.0;
/// Vanya turns to a player this close when idle (`4 * GAME_EDGELENGTH`).
pub const VANYA_LOOK_RADIUS: f32 = 4.0;

#[derive(Debug, Clone, Copy, PartialEq)]
struct Waypoint {
    fly: bool,
    goal: Vec3,
    /// Horizontal distance when the waypoint became the active one.
    initial_distance: f32,
}

/// One line Vanya says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Chat {
    pub text: &'static str,
    /// A yes/no question (`BoxModes.BOOLEAN`) instead of "next".
    pub choice: bool,
}

/// The guide of the tutorial (`Vanya`): talks the player through the first minutes and walks or
/// flies ahead to the next place.
#[derive(Debug, Clone, Default)]
pub struct Vanya {
    chat_counter: u32,
    tutorial_step: u8,
    completed_step: u8,
    waypoints: VecDeque<Waypoint>,
    next: Option<Waypoint>,
}

impl Vanya {
    pub fn new() -> Self {
        Self::default()
    }

    /// How far the tutorial has got (`getCompletedTutorialStep`, which returns the step asked for).
    pub fn tutorial_step(&self) -> u8 {
        self.tutorial_step
    }

    /// Move the tutorial on; going back is ignored (`setTutorialStep`).
    pub fn set_tutorial_step(&mut self, step: u8) {
        if step > self.tutorial_step {
            self.tutorial_step = step;
        }
    }

    pub fn is_moving(&self) -> bool {
        self.next.is_some()
    }

    /// Can be talked to: not while she is on her way somewhere.
    pub fn interactable(&self) -> bool {
        !self.is_moving()
    }

    /// Walk to a cell (`goTo`). A cell she is already heading for is not queued again.
    pub fn go_to(&mut self, cell: Cell) {
        self.queue(false, cell);
    }

    /// Fly to a cell (`flyTo`).
    pub fn fly_to(&mut self, cell: Cell) {
        self.queue(true, cell);
    }

    fn queue(&mut self, fly: bool, cell: Cell) {
        let goal = cell_floor(cell);
        if self.next.is_none_or(|w| w.goal.distance(goal) > 0.0) {
            // The distance is taken when the waypoint becomes the active one.
            self.waypoints.push_back(Waypoint { fly, goal, initial_distance: 0.0 });
        }
    }

    /// The next line of the conversation (`nextChat`). `confirm` is the answer to the last question.
    /// Some steps say nothing (the counter only moves on), which is `None`.
    pub fn next_chat(&mut self, confirm: bool) -> Option<Chat> {
        let line = |text, choice| Some(Chat { text, choice });
        match self.chat_counter {
            0 => {
                self.chat_counter += 1;
                line("Oh hello! Are you alright? I saw your spaceship crashing. But you look good. \nWelcome to Caveland! I'm Vanya. I will be your guide.\n", false)
            }
            1 => {
                self.chat_counter += 1;
                line("I guess you wonder why I can speak. On this planet some things are bit different than you are used to", false)
            }
            2 => {
                self.chat_counter += 1;
                self.tutorial_step = 1;
                line("I will explain you later. First let's go. It's dangerous here. Follow me.", false)
            }
            3 => {
                if self.tutorial_step > 0 {
                    self.chat_counter += 1;
                } else {
                    self.chat_counter = 0;
                }
                None
            }
            4 => {
                self.tutorial_step = 2;
                self.chat_counter += 1;
                line("You can use your jetpack if you press the jump button a second time in air. Press it at the peak of your jump to jump higher.", false)
            }
            5 => {
                self.chat_counter += 1;
                None
            }
            6 => {
                self.tutorial_step = 3;
                self.chat_counter = 7;
                line("You must go through the caves. Go though that hole there. I will see you at the other side.", false)
            }
            7 => {
                self.chat_counter += 1;
                None
            }
            8 => {
                self.chat_counter += 1;
                line("At the end of the tracks there is a cable missing. You must repair it in order to continue. Collect one sulfur and one coal to craft dynamite.", false)
            }
            9 => {
                self.chat_counter += 1;
                line("Once you have the dynamite you can use it to obtain iron ore. Put the iron ore and one coal block in the oven to get iron. With the iron you can craft a minecart.", false)
            }
            10 => {
                self.chat_counter += 1;
                line("Don't destroy the track or else you must find another way to exit.", false)
            }
            11 => {
                self.chat_counter += 1;
                None
            }
            12 => {
                self.chat_counter += 1;
                line("Do you want me to wait at the other side?", true)
            }
            13 => {
                if confirm {
                    self.tutorial_step = 4;
                }
                None
            }
            _ => None,
        }
    }

    /// Advance one step: the tutorial sends her to the next place, she walks there and hops.
    /// `look_at` is the direction to the nearest player within [`VANYA_LOOK_RADIUS`], if any.
    /// Returns true if she hopped (for the sound).
    pub fn update(&mut self, entity: &mut Entity, world: &World, look_at: Option<Vec2>) -> bool {
        // Tutorial steps send her ahead, once each.
        let plan: [(u8, bool, Cell); 5] = [
            (1, false, (-2, 1, 6)),
            (2, false, (-2, 10, 6)),
            (3, true, (2, 13, 7)),
            (4, true, (17, 24, 6)),
            (5, true, (25, 20, 7)),
        ];
        for (step, fly, cell) in plan {
            if self.tutorial_step >= step && self.completed_step < step {
                if fly {
                    self.fly_to(cell)
                } else {
                    self.go_to(cell)
                }
                self.completed_step = step;
            }
        }
        if self.next.is_none() {
            self.activate_next(entity);
        }

        let mut hopped = false;
        let floating = self.next.is_some_and(|w| w.fly);
        if let Some(body) = entity.body.as_mut() {
            body.floating = floating;
        }
        if !floating && is_on_ground(world, entity.position, entity.dimension_z) {
            if let Some(body) = entity.body.as_mut() {
                body.jump_with(VANYA_JUMP_SPEED);
                hopped = true;
            }
        }

        if let Some(wp) = self.next {
            self.steer(entity, wp);
        } else if let Some(dir) = look_at {
            if let Some(body) = entity.body.as_mut() {
                body.set_speed_horizontal(0.0);
                body.set_orientation(dir.normalize_or_zero());
            }
        }
        hopped
    }

    fn activate_next(&mut self, entity: &Entity) {
        if let Some(mut wp) = self.waypoints.pop_front() {
            wp.initial_distance = horizontal(entity.position, wp.goal);
            self.next = Some(wp);
        }
    }

    fn steer(&mut self, entity: &mut Entity, wp: Waypoint) {
        let position = entity.position;
        let far = horizontal(position, wp.goal) > ARRIVED || (wp.fly && position.distance(wp.goal) > ARRIVED);
        if !far {
            self.next = None;
            self.activate_next(entity);
            return;
        }
        let Some(body) = entity.body.as_mut() else { return };
        let to = wp.goal - position;
        let d = if wp.fly {
            let mut d = Vec3::new(to.x, to.y, 0.0);
            if horizontal(position, wp.goal) > wp.initial_distance / 2.0 {
                // The first half climbs (limited to a little above the top of the world)...
                d = d.normalize_or_zero();
                if (position.z.floor() as i32) < wurfel_sim::caveland::HEIGHT + 2 {
                    d.z = 1.0;
                }
            } else {
                // ...the second half descends to the goal.
                d.z = to.z;
            }
            d.normalize_or_zero() * VANYA_FLY_SPEED
        } else {
            let mut d = Vec3::new(to.x, to.y, 0.0).normalize_or_zero() * VANYA_WALK_SPEED;
            d.z = body.movement.z; // keep the vertical momentum
            d
        };
        body.set_movement(d);
    }
}

fn horizontal(a: Vec3, b: Vec3) -> f32 {
    Vec2::new(a.x - b.x, a.y - b.y).length()
}

// ---- Shopkeeper -------------------------------------------------------------------------------

/// What the shopkeeper sells and for how much money. The Java `Shopkeeper` is a stub that shows one
/// selection called "test"; these goods and prices are new.
pub const SHOP_STOCK: [(CollectibleType, u32); 7] = [
    (CollectibleType::Torch, 3),
    (CollectibleType::Wood, 2),
    (CollectibleType::Stone, 2),
    (CollectibleType::Coal, 4),
    (CollectibleType::Explosives, 12),
    (CollectibleType::Rails, 8),
    (CollectibleType::Toolkit, 25),
];

/// The price of a good, if the shopkeeper sells it.
pub fn price_of(kind: CollectibleType) -> Option<u32> {
    SHOP_STOCK.iter().find(|(k, _)| *k == kind).map(|(_, p)| *p)
}

// ---- Bird -------------------------------------------------------------------------------------

/// Seconds between the vertical nudges (`timeSinceLastDirChange > 2000`).
const BIRD_NUDGE_EVERY: f32 = 2.0;

/// A floating thing that drifts around at random (`Bird`).
pub struct Bird {
    since_nudge: f32,
    random: JavaRandom,
}

impl Bird {
    pub fn new(seed: i64) -> Self {
        Bird { since_nudge: 0.0, random: JavaRandom::new(seed) }
    }

    /// Nudge the bird: a little random sideways push every step, a vertical one every two seconds.
    /// The Java code adds `random * dt_ms * k` to the velocity; `dt` is seconds here.
    pub fn update(&mut self, entity: &mut Entity, dt: f32) {
        let Some(body) = entity.body.as_mut() else { return };
        self.since_nudge += dt;
        if self.since_nudge > BIRD_NUDGE_EVERY {
            self.since_nudge %= BIRD_NUDGE_EVERY;
            let dz = (self.random.next_float() - 0.5) * dt * 100.0;
            body.add_movement(Vec3::new(0.0, 0.0, dz));
        }
        let (dx, dy) = ((self.random.next_float() - 0.5) * dt * 20.0, (self.random.next_float() - 0.5) * dt * 20.0);
        body.add_movement(Vec3::new(dx, dy, 0.0));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wurfel_sim::block::Block;
    use wurfel_sim::entity::Entities;
    use wurfel_sim::grid::to_iso;
    use wurfel_sim::AirGenerator;

    const DT: f32 = 1.0 / 60.0;

    fn floor_world() -> World {
        let mut world = World::new(AirGenerator);
        for x in -10..30 {
            for y in -10..90 {
                world.set(x, y, 0, Block::new(8, 0));
            }
        }
        world
    }

    fn vanya_entity() -> Entity {
        let (gx, gy) = to_iso(10, 40);
        Entity::new("Vanya", 40).movable().at(Vec3::new(gx, gy, 1.0))
    }

    #[test]
    fn the_conversation_follows_the_java_script() {
        let mut v = Vanya::new();
        assert!(v.next_chat(true).unwrap().text.starts_with("Oh hello!"));
        assert!(v.next_chat(true).unwrap().text.starts_with("I guess you wonder"));
        assert_eq!(v.tutorial_step(), 0);
        assert!(v.next_chat(true).unwrap().text.starts_with("I will explain you later"));
        assert_eq!(v.tutorial_step(), 1, "the third line starts the tutorial");
        assert_eq!(v.next_chat(true), None, "step 3 is silent");
        let jetpack = v.next_chat(true).unwrap();
        assert!(jetpack.text.contains("jetpack") && !jetpack.choice);
        assert_eq!(v.tutorial_step(), 2);
        assert_eq!(v.next_chat(true), None);
        assert!(v.next_chat(true).unwrap().text.contains("through the caves"));
        assert_eq!(v.tutorial_step(), 3);
        assert_eq!(v.next_chat(true), None);
        assert!(v.next_chat(true).unwrap().text.contains("cable missing"));
        assert!(v.next_chat(true).unwrap().text.contains("dynamite"));
        assert!(v.next_chat(true).unwrap().text.contains("Don't destroy the track"));
        assert_eq!(v.next_chat(true), None);
        let question = v.next_chat(true).unwrap();
        assert!(question.choice, "the last line is a yes/no question");
    }

    #[test]
    fn the_answer_to_the_question_decides_whether_she_waits() {
        let ask = |answer| {
            let mut v = Vanya::new();
            v.set_tutorial_step(3);
            v.chat_counter = 12;
            assert!(v.next_chat(true).unwrap().choice);
            assert_eq!(v.next_chat(answer), None);
            v.tutorial_step()
        };
        assert_eq!(ask(true), 4);
        assert_eq!(ask(false), 3);
    }

    #[test]
    fn the_tutorial_step_only_moves_forward() {
        let mut v = Vanya::new();
        v.set_tutorial_step(3);
        v.set_tutorial_step(1);
        assert_eq!(v.tutorial_step(), 3);
    }

    #[test]
    fn she_walks_to_the_first_waypoint_and_can_be_talked_to_again() {
        let world = floor_world();
        let mut entities = Entities::new();
        let id = entities.spawn(vanya_entity());
        let mut v = Vanya::new();
        v.go_to((12, 40, 1));
        assert!(v.interactable(), "queued, but not on her way yet");
        let goal = cell_floor((12, 40, 1));
        let mut hopped = false;
        for step in 0..60 * 20 {
            hopped |= v.update(entities.get_mut(id).unwrap(), &world, None);
            entities.update(&world, DT);
            if !v.is_moving() && step > 1 {
                break;
            }
        }
        let e = entities.get(id).unwrap();
        assert!(!v.is_moving(), "never arrived");
        assert!(horizontal(e.position, goal) < 0.5, "stopped {} blocks from the goal", horizontal(e.position, goal));
        assert!(hopped, "she hops all the time");
        assert!(v.interactable());
    }

    #[test]
    fn while_she_is_on_her_way_she_cannot_be_talked_to() {
        let world = floor_world();
        let mut e = vanya_entity();
        let mut v = Vanya::new();
        v.go_to((20, 40, 1));
        v.update(&mut e, &world, None);
        assert!(v.is_moving());
        assert!(!v.interactable());
    }

    #[test]
    fn a_waypoint_she_is_already_heading_for_is_not_queued_twice() {
        let world = floor_world();
        let mut e = vanya_entity();
        let mut v = Vanya::new();
        v.go_to((20, 40, 1));
        v.update(&mut e, &world, None);
        v.go_to((20, 40, 1));
        assert!(v.waypoints.is_empty());
    }

    #[test]
    fn tutorial_steps_send_her_to_their_places_once() {
        let world = floor_world();
        let mut e = vanya_entity();
        let mut v = Vanya::new();
        v.set_tutorial_step(1);
        v.update(&mut e, &world, None);
        assert_eq!(v.next.map(|w| (w.goal, w.fly)), Some((cell_floor((-2, 1, 6)), false)));
        v.set_tutorial_step(3);
        v.update(&mut e, &world, None);
        // Steps 2 and 3 are queued behind the first waypoint.
        let queued: Vec<_> = v.waypoints.iter().map(|w| (w.goal, w.fly)).collect();
        assert_eq!(queued, vec![(cell_floor((-2, 10, 6)), false), (cell_floor((2, 13, 7)), true)]);
        v.update(&mut e, &world, None);
        assert_eq!(v.waypoints.len(), 2, "no step is queued twice");
    }

    #[test]
    fn a_flying_waypoint_lifts_off_the_ground() {
        let world = floor_world();
        let mut entities = Entities::new();
        let id = entities.spawn(vanya_entity());
        let mut v = Vanya::new();
        v.fly_to((10, 50, 5));
        let start_z = entities.get(id).unwrap().position.z;
        for _ in 0..60 {
            v.update(entities.get_mut(id).unwrap(), &world, None);
            entities.update(&world, DT);
        }
        let body = entities.get(id).unwrap().body.as_ref().unwrap();
        assert!(body.floating);
        assert!(entities.get(id).unwrap().position.z > start_z + 0.5, "did not climb");
    }

    #[test]
    fn idle_she_looks_at_the_player() {
        let world = floor_world();
        let mut e = vanya_entity();
        let mut v = Vanya::new();
        v.update(&mut e, &world, Some(Vec2::new(0.0, 1.0)));
        let o = e.body.as_ref().unwrap().orientation();
        assert!((o - Vec2::new(0.0, 1.0)).length() < 1e-4, "{o}");
    }

    #[test]
    fn the_shop_sells_what_it_prices() {
        for (kind, price) in SHOP_STOCK {
            assert_eq!(price_of(kind), Some(price));
            assert!(price > 0);
        }
        assert_eq!(price_of(CollectibleType::Iron), None);
    }

    #[test]
    fn the_bird_drifts_and_is_seeded() {
        let run = |seed| {
            let mut e = Entity::new("Bird", 40).movable();
            let mut b = Bird::new(seed);
            for _ in 0..300 {
                b.update(&mut e, DT);
            }
            e.body.as_ref().unwrap().movement
        };
        assert_ne!(run(1), Vec3::ZERO);
        assert_eq!(run(1), run(1));
        assert_ne!(run(1), run(2));
    }
}
