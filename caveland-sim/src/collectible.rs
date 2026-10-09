//! Things the player picks up (`CollectibleType`, `Collectible`).
//!
//! In Java a collectible is an entity that is hidden while it sits in a container. Here an item
//! inside a container is just an [`Item`]; it becomes an entity only while it lies in the world.

use serde::{Deserialize, Serialize};

/// Seconds an explosive burns before it goes off (`TFlint.TIMETILLEXPLOSION`).
pub const FUSE_TIME: f32 = 3.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CollectibleType {
    Rails,
    Wood,
    Explosives,
    Ironore,
    Coal,
    Cristall,
    Sulfur,
    Stone,
    Toolkit,
    Torch,
    Iron,
    Powercable,
    DropSpaceFlagConstructionKit,
    /// Loaded into a cannon, one unit per shot (not in the Java game).
    Gunpowder,
}

impl CollectibleType {
    pub const ALL: [CollectibleType; 14] = [
        CollectibleType::Rails,
        CollectibleType::Wood,
        CollectibleType::Explosives,
        CollectibleType::Ironore,
        CollectibleType::Coal,
        CollectibleType::Cristall,
        CollectibleType::Sulfur,
        CollectibleType::Stone,
        CollectibleType::Toolkit,
        CollectibleType::Torch,
        CollectibleType::Iron,
        CollectibleType::Powercable,
        CollectibleType::DropSpaceFlagConstructionKit,
        CollectibleType::Gunpowder,
    ];

    /// Sprite id (`getId`).
    pub fn sprite_id(self) -> u8 {
        match self {
            CollectibleType::Rails => 16,
            CollectibleType::Wood => 46,
            CollectibleType::Explosives => 47,
            CollectibleType::Ironore => 48,
            CollectibleType::Coal => 49,
            CollectibleType::Cristall => 50,
            CollectibleType::Sulfur => 51,
            CollectibleType::Stone => 52,
            CollectibleType::Toolkit => 53,
            CollectibleType::Torch => 54,
            CollectibleType::Iron => 55,
            CollectibleType::Powercable => 57,
            CollectibleType::DropSpaceFlagConstructionKit => 23,
            // The Java atlas has an unused item sprite 56; no art of its own yet.
            CollectibleType::Gunpowder => 56,
        }
    }

    /// Number of animation sprites (`getAnimationSteps`).
    pub fn animation_steps(self) -> u32 {
        match self {
            CollectibleType::Rails | CollectibleType::Torch => 2,
            CollectibleType::Explosives | CollectibleType::Toolkit | CollectibleType::Gunpowder => 8,
            CollectibleType::Iron => 4,
            CollectibleType::Powercable | CollectibleType::DropSpaceFlagConstructionKit => 1,
            _ => 5,
        }
    }

    /// The Java enum constant name; what saves and recipes refer to.
    pub fn name(self) -> &'static str {
        match self {
            CollectibleType::Rails => "Rails",
            CollectibleType::Wood => "Wood",
            CollectibleType::Explosives => "Explosives",
            CollectibleType::Ironore => "Ironore",
            CollectibleType::Coal => "Coal",
            CollectibleType::Cristall => "Cristall",
            CollectibleType::Sulfur => "Sulfur",
            CollectibleType::Stone => "Stone",
            CollectibleType::Toolkit => "Toolkit",
            CollectibleType::Torch => "Torch",
            CollectibleType::Iron => "Iron",
            CollectibleType::Powercable => "Powercable",
            CollectibleType::DropSpaceFlagConstructionKit => "DropSpaceFlagConstructionKit",
            CollectibleType::Gunpowder => "Gunpowder",
        }
    }

    /// `CollectibleType.fromValue`.
    pub fn from_name(name: &str) -> Option<CollectibleType> {
        CollectibleType::ALL.into_iter().find(|t| t.name() == name)
    }

    /// Can be used from the inventory (`Interactable` collectibles in Java).
    pub fn is_usable(self) -> bool {
        matches!(self, CollectibleType::Explosives | CollectibleType::Torch)
    }
}

/// One collectible. Only explosives carry state: a lit fuse.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Item {
    pub kind: CollectibleType,
    /// Seconds until the explosive goes off; `None` while unlit.
    pub fuse: Option<f32>,
    /// Uses left of a rails or cable kit (`InstantConstructionKit.amountLeft`); 0 for everything else.
    #[serde(default)]
    pub charges: u8,
    /// The side the last piece of a rails or cable kit continued towards (`lastDir`).
    #[serde(default)]
    pub last_dir: u8,
}

/// Pieces one rails or cable kit lays (`InstantConstructionKit.amountLeft`).
pub const KIT_CHARGES: u8 = 3;

impl Item {
    pub fn new(kind: CollectibleType) -> Self {
        let charges = if matches!(kind, CollectibleType::Rails | CollectibleType::Powercable) { KIT_CHARGES } else { 0 };
        Item { kind, fuse: None, charges, last_dir: 0 }
    }

    pub fn is_lit(&self) -> bool {
        self.fuse.is_some()
    }

    /// Light the fuse (`TFlint.interact`). Lighting twice restarts the countdown, like Java.
    pub fn ignite(&mut self) {
        if self.kind == CollectibleType::Explosives {
            self.fuse = Some(FUSE_TIME);
        }
    }

    /// Burn the fuse down. Returns true when the item exploded this step.
    pub fn tick(&mut self, dt: f32) -> bool {
        match self.fuse.as_mut() {
            Some(left) => {
                *left -= dt;
                *left <= 0.0
            }
            None => false,
        }
    }
}

impl From<CollectibleType> for Item {
    fn from(kind: CollectibleType) -> Self {
        Item::new(kind)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_round_trip() {
        for t in CollectibleType::ALL {
            assert_eq!(CollectibleType::from_name(t.name()), Some(t));
        }
        assert_eq!(CollectibleType::from_name("nope"), None);
    }

    #[test]
    fn sprite_ids_are_unique() {
        let mut ids: Vec<u8> = CollectibleType::ALL.iter().map(|t| t.sprite_id()).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), CollectibleType::ALL.len());
    }

    #[test]
    fn a_lit_explosive_goes_off_when_the_fuse_has_burnt_down() {
        let mut flint = Item::new(CollectibleType::Explosives);
        assert!(!flint.tick(10.0), "unlit explosives are harmless");
        flint.ignite();
        assert!(!flint.tick(FUSE_TIME - 0.1));
        assert!(flint.tick(0.2));
    }

    #[test]
    fn only_explosives_can_be_lit() {
        let mut coal = Item::new(CollectibleType::Coal);
        coal.ignite();
        assert!(!coal.is_lit());
    }
}
