//! Collectible containers and the player's inventory (`CollectibleContainer`, `Inventory`).

use serde::{Deserialize, Serialize};

use crate::collectible::{CollectibleType, Item};

/// An unlimited stack of items, e.g. what sits inside an oven.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CollectibleContainer {
    content: Vec<Item>,
}

impl CollectibleContainer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add at the back.
    pub fn add(&mut self, item: Item) -> bool {
        self.content.push(item);
        true
    }

    /// Add at the front (slot 0).
    pub fn add_front(&mut self, item: Item) -> bool {
        self.content.insert(0, item);
        true
    }

    pub fn get(&self, index: usize) -> Option<&Item> {
        self.content.get(index)
    }

    pub fn len(&self) -> usize {
        self.content.len()
    }

    pub fn is_empty(&self) -> bool {
        self.content.is_empty()
    }

    pub fn items(&self) -> &[Item] {
        &self.content
    }

    /// Remove and return the item in slot `index`.
    pub fn retrieve(&mut self, index: usize) -> Option<Item> {
        (index < self.content.len()).then(|| self.content.remove(index))
    }

    /// Remove and return the first item of a type. (Java returned the last item looked at even
    /// when nothing matched; that bug is not kept.)
    pub fn retrieve_type(&mut self, kind: CollectibleType) -> Option<Item> {
        let index = self.content.iter().position(|i| i.kind == kind)?;
        Some(self.content.remove(index))
    }

    /// How many items of a type are inside.
    pub fn count(&self, kind: CollectibleType) -> usize {
        self.content.iter().filter(|i| i.kind == kind).count()
    }

    /// Rotate the content so that the neighbour of the front item comes first (`switchItems`).
    pub fn switch_items(&mut self, left: bool) {
        if self.content.is_empty() {
            return;
        }
        if left {
            self.content.rotate_left(1);
        } else {
            self.content.rotate_right(1);
        }
    }

    /// Empty the container, e.g. when its block is destroyed.
    pub fn drain(&mut self) -> Vec<Item> {
        std::mem::take(&mut self.content)
    }

    /// Burn the fuses of everything inside; returns how many items went off (and were removed).
    pub fn tick(&mut self, dt: f32) -> usize {
        let before = self.content.len();
        self.content.retain_mut(|item| !item.tick(dt));
        before - self.content.len()
    }
}

/// Number of slots in a player's inventory.
pub const INVENTORY_SLOTS: usize = 3;

/// The backpack: three slots, slot 0 is the one in hand.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Inventory {
    slots: CollectibleContainer,
}

impl Inventory {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.slots.len()
    }

    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    pub fn is_full(&self) -> bool {
        self.len() >= INVENTORY_SLOTS
    }

    /// Add at the back. False if full.
    pub fn add(&mut self, item: Item) -> bool {
        !self.is_full() && self.slots.add(item)
    }

    /// Add at the front. False if full.
    pub fn add_front(&mut self, item: Item) -> bool {
        !self.is_full() && self.slots.add_front(item)
    }

    pub fn get(&self, slot: usize) -> Option<&Item> {
        self.slots.get(slot)
    }

    /// The item in hand (`getFrontCollectible`).
    pub fn front(&self) -> Option<&Item> {
        self.slots.get(0)
    }

    /// Remove and return a slot's item.
    pub fn retrieve(&mut self, slot: usize) -> Option<Item> {
        self.slots.retrieve(slot)
    }

    /// Remove an item of this type, preferring the back slots like Java's `getCollectible` (slot 2
    /// first, then 1, then 0), so the item in hand stays.
    pub fn retrieve_type(&mut self, kind: CollectibleType) -> Option<Item> {
        let slot = (0..self.len()).rev().find(|&s| self.slots.get(s).is_some_and(|i| i.kind == kind))?;
        self.slots.retrieve(slot)
    }

    /// The types per slot (`getContentDef`).
    pub fn types(&self) -> [Option<CollectibleType>; INVENTORY_SLOTS] {
        [0, 1, 2].map(|s| self.slots.get(s).map(|i| i.kind))
    }

    /// How many items of a type the inventory holds (`contains`).
    pub fn count(&self, kind: CollectibleType) -> usize {
        self.slots.count(kind)
    }

    pub fn switch_items(&mut self, left: bool) {
        self.slots.switch_items(left);
    }

    pub fn items(&self) -> &[Item] {
        self.slots.items()
    }

    pub fn tick(&mut self, dt: f32) -> usize {
        self.slots.tick(dt)
    }

    pub fn drain(&mut self) -> Vec<Item> {
        self.slots.drain()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use CollectibleType::*;

    fn item(kind: CollectibleType) -> Item {
        Item::new(kind)
    }

    #[test]
    fn inventory_holds_three_items() {
        let mut inv = Inventory::new();
        assert!(inv.add(item(Wood)) && inv.add(item(Coal)) && inv.add(item(Iron)));
        assert!(inv.is_full());
        assert!(!inv.add(item(Stone)), "full");
        assert!(!inv.add_front(item(Stone)), "full");
        assert_eq!(inv.types(), [Some(Wood), Some(Coal), Some(Iron)]);
    }

    #[test]
    fn add_front_puts_the_item_in_hand() {
        let mut inv = Inventory::new();
        inv.add(item(Wood));
        inv.add_front(item(Coal));
        assert_eq!(inv.front().unwrap().kind, Coal);
        assert_eq!(inv.types(), [Some(Coal), Some(Wood), None]);
    }

    #[test]
    fn retrieving_by_type_prefers_the_back_so_the_item_in_hand_stays() {
        let mut inv = Inventory::new();
        inv.add(item(Wood));
        inv.add(item(Coal));
        inv.add(item(Wood));
        inv.retrieve_type(Wood).unwrap();
        assert_eq!(inv.types(), [Some(Wood), Some(Coal), None]);
        assert!(inv.retrieve_type(Iron).is_none());
        assert_eq!(inv.len(), 2, "a failed lookup takes nothing");
    }

    #[test]
    fn counting_and_retrieving_slots() {
        let mut inv = Inventory::new();
        inv.add(item(Wood));
        inv.add(item(Wood));
        assert_eq!(inv.count(Wood), 2);
        assert_eq!(inv.count(Coal), 0);
        assert!(inv.retrieve(5).is_none());
        assert_eq!(inv.retrieve(0).unwrap().kind, Wood);
        assert_eq!(inv.count(Wood), 1);
    }

    #[test]
    fn switching_rotates_the_slots() {
        let mut inv = Inventory::new();
        inv.add(item(Wood));
        inv.add(item(Coal));
        inv.add(item(Iron));
        inv.switch_items(true);
        assert_eq!(inv.types(), [Some(Coal), Some(Iron), Some(Wood)]);
        inv.switch_items(false);
        assert_eq!(inv.types(), [Some(Wood), Some(Coal), Some(Iron)]);
        Inventory::new().switch_items(true); // empty: nothing happens
    }

    #[test]
    fn unlimited_container_retrieves_the_first_match() {
        let mut c = CollectibleContainer::new();
        for _ in 0..10 {
            c.add(item(Coal));
        }
        c.add(item(Ironore));
        assert_eq!(c.len(), 11);
        assert_eq!(c.retrieve_type(Ironore).unwrap().kind, Ironore);
        assert!(c.retrieve_type(Ironore).is_none());
        assert_eq!(c.count(Coal), 10);
    }

    #[test]
    fn lit_explosives_in_a_container_go_off_and_disappear() {
        let mut c = CollectibleContainer::new();
        let mut flint = item(Explosives);
        flint.ignite();
        c.add(flint);
        c.add(item(Explosives));
        assert_eq!(c.tick(1.0), 0);
        assert_eq!(c.tick(1.1), 1);
        assert_eq!(c.len(), 1, "the unlit one is still there");
    }
}
