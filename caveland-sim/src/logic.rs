//! Block logic: blocks with behaviour (`gameobjects/logicblocks`). Each logic block is plain data
//! keyed by its cell and updated by [`crate::Caveland`].
//!
//! Ported: the oven. Not yet: construction site, power station/cable/torch, lift, turret, robot
//! factory, flag pole, cave entry, booster rails.

use crate::collectible::{CollectibleType, Item};
use crate::container::{CollectibleContainer, Inventory};

/// Seconds an iron ore takes to smelt (`PRODUCTIONTIME`).
pub const OVEN_PRODUCTION_TIME: f32 = 3.0;
/// Burn time one piece of coal adds, in seconds.
pub const COAL_BURN_TIME: f32 = 20.0;
/// Burn time one piece of wood adds, in seconds.
pub const WOOD_BURN_TIME: f32 = 5.0;

/// Something an oven did that the game has to carry out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OvenEvent {
    /// A bar of iron came out; place it on top of the oven.
    Produced(CollectibleType),
    /// The fire went out.
    Extinguished,
    /// The fire started.
    Lit,
}

/// `OvenLogic`: feed it coal or wood to burn and iron ore to smelt.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct OvenLogic {
    container: CollectibleContainer,
    burn_time: f32,
    production_countdown: f32,
}

impl OvenLogic {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_burning(&self) -> bool {
        self.burn_time > 0.0
    }

    pub fn burn_time(&self) -> f32 {
        self.burn_time
    }

    /// Seconds until the current ore is done, 0 when idle.
    pub fn production_countdown(&self) -> f32 {
        self.production_countdown
    }

    pub fn contents(&self) -> &CollectibleContainer {
        &self.container
    }

    /// Does the oven take this? Returns false (and the caller keeps the item) for anything but
    /// coal, wood and iron ore.
    pub fn add_collectible(&mut self, item: Item) -> bool {
        match item.kind {
            CollectibleType::Coal => self.burn_time += COAL_BURN_TIME,
            CollectibleType::Wood => self.burn_time += WOOD_BURN_TIME,
            CollectibleType::Ironore => {}
            _ => return false,
        }
        self.container.add(item)
    }

    /// The player uses the oven: the item in hand goes in, if the oven wants it (`interact`). Java
    /// takes the item out of the inventory even when the oven refuses it and then loses track of it;
    /// here a refused item stays in hand.
    pub fn interact(&mut self, inventory: &mut Inventory) -> bool {
        let Some(item) = inventory.retrieve(0) else { return false };
        if self.add_collectible(item) {
            true
        } else {
            inventory.add_front(item);
            false
        }
    }

    /// Advance by `dt` seconds.
    pub fn update(&mut self, dt: f32) -> Vec<OvenEvent> {
        let mut events = Vec::new();
        let was_burning = self.is_burning();
        if was_burning {
            if self.production_countdown == 0.0 && self.container.retrieve_type(CollectibleType::Ironore).is_some() {
                self.production_countdown = OVEN_PRODUCTION_TIME;
            }
            let before = self.production_countdown;
            self.production_countdown = (self.production_countdown - dt).max(0.0);
            if before > 0.0 && self.production_countdown == 0.0 {
                events.push(OvenEvent::Produced(CollectibleType::Iron));
            }
            self.burn_time -= dt;
            if self.burn_time <= 0.0 {
                self.burn_time = 0.0;
                events.push(OvenEvent::Extinguished);
            }
        }
        events
    }

    /// Everything inside, for when the oven is destroyed.
    pub fn release(&mut self) -> Vec<Item> {
        self.container.drain()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use CollectibleType::*;

    fn run(oven: &mut OvenLogic, seconds: f32) -> Vec<OvenEvent> {
        let mut events = Vec::new();
        let steps = (seconds * 60.0).round() as usize;
        for _ in 0..steps {
            events.extend(oven.update(1.0 / 60.0));
        }
        events
    }

    #[test]
    fn coal_and_wood_add_burn_time() {
        let mut oven = OvenLogic::new();
        assert!(!oven.is_burning());
        assert!(oven.add_collectible(Item::new(Coal)));
        assert!(oven.add_collectible(Item::new(Wood)));
        assert_eq!(oven.burn_time(), 25.0);
        assert!(oven.is_burning());
    }

    #[test]
    fn it_refuses_other_things() {
        let mut oven = OvenLogic::new();
        assert!(!oven.add_collectible(Item::new(Stone)));
        assert!(oven.contents().is_empty());
    }

    #[test]
    fn a_refused_item_stays_in_hand() {
        let mut oven = OvenLogic::new();
        let mut inv = Inventory::new();
        inv.add(Item::new(Stone));
        inv.add(Item::new(Coal));
        assert!(!oven.interact(&mut inv));
        assert_eq!(inv.types(), [Some(Stone), Some(Coal), None]);
        inv.retrieve(0);
        assert!(oven.interact(&mut inv));
        assert!(inv.is_empty());
        assert!(oven.is_burning());
    }

    #[test]
    fn smelting_takes_three_seconds_of_fire_and_yields_iron() {
        let mut oven = OvenLogic::new();
        oven.add_collectible(Item::new(Coal));
        oven.add_collectible(Item::new(Ironore));
        let events = run(&mut oven, 2.5);
        assert!(events.is_empty(), "not done yet: {events:?}");
        assert!(oven.production_countdown() > 0.0);
        let events = run(&mut oven, 1.0);
        assert_eq!(events, vec![OvenEvent::Produced(Iron)]);
        assert_eq!(oven.contents().count(Ironore), 0, "the ore was used up");
    }

    #[test]
    fn nothing_happens_without_fire() {
        let mut oven = OvenLogic::new();
        oven.add_collectible(Item::new(Ironore));
        let events = run(&mut oven, 10.0);
        assert!(events.is_empty());
        assert_eq!(oven.contents().count(Ironore), 1);
    }

    #[test]
    fn the_fire_goes_out_after_the_fuel_is_used_up() {
        let mut oven = OvenLogic::new();
        oven.add_collectible(Item::new(Wood));
        let events = run(&mut oven, 6.0);
        assert_eq!(events, vec![OvenEvent::Extinguished]);
        assert!(!oven.is_burning());
    }

    #[test]
    fn two_ores_are_smelted_one_after_the_other() {
        let mut oven = OvenLogic::new();
        oven.add_collectible(Item::new(Coal));
        oven.add_collectible(Item::new(Ironore));
        oven.add_collectible(Item::new(Ironore));
        let events = run(&mut oven, 8.0);
        let produced = events.iter().filter(|e| matches!(e, OvenEvent::Produced(Iron))).count();
        assert_eq!(produced, 2);
    }

    #[test]
    fn destroying_the_oven_releases_its_content() {
        let mut oven = OvenLogic::new();
        oven.add_collectible(Item::new(Coal));
        oven.add_collectible(Item::new(Ironore));
        assert_eq!(oven.release().len(), 2);
        assert!(oven.contents().is_empty());
    }
}
