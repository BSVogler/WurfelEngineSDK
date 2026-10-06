//! Crafting recipes (`Recipe`, `CraftingRecipesList`).

use crate::collectible::{CollectibleType, Item};
use crate::container::{Inventory, INVENTORY_SLOTS};

/// What a recipe produces: an item for the backpack, or something placed in the world.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecipeResult {
    Item(CollectibleType),
    MineCart,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recipe {
    pub name: &'static str,
    pub ingredients: Vec<CollectibleType>,
    pub result: RecipeResult,
}

impl Recipe {
    fn new(name: &'static str, ingredients: &[CollectibleType], result: RecipeResult) -> Self {
        Recipe { name, ingredients: ingredients.to_vec(), result }
    }
}

/// The recipes the game knows, in the order of the Java list.
pub fn recipes() -> Vec<Recipe> {
    use CollectibleType::*;
    use RecipeResult::*;
    vec![
        Recipe::new("TFlint", &[Sulfur, Coal], Item(Explosives)),
        Recipe::new("Construction Kit", &[Wood, Wood], Item(Toolkit)),
        Recipe::new("Torch", &[Wood, Coal], Item(Torch)),
        Recipe::new("Rails Construction Kit", &[Iron, Iron], Item(Rails)),
        Recipe::new("Power Cable", &[Iron, Iron], Item(Powercable)),
        Recipe::new("Minecart", &[Iron, Iron, Wood], MineCart),
        Recipe::new("Drop Space Flag", &[Iron], Item(DropSpaceFlagConstructionKit)),
        Recipe::new("Gunpowder", &[Sulfur, Coal, Coal], Item(Gunpowder)),
    ]
}

/// The inventory slot to take for each ingredient, if every ingredient can be matched to a
/// different slot. (The Java version matched slots by a hand-written chain of `if`s that
/// mishandles some orders; this is the same rule done properly.)
fn assign_slots(recipe: &Recipe, types: &[Option<CollectibleType>; INVENTORY_SLOTS]) -> Option<Vec<usize>> {
    let mut used = [false; INVENTORY_SLOTS];
    let mut slots = Vec::new();
    for ingredient in &recipe.ingredients {
        let slot = (0..INVENTORY_SLOTS).find(|&s| !used[s] && types[s] == Some(*ingredient))?;
        used[slot] = true;
        slots.push(slot);
    }
    Some(slots)
}

/// Does the inventory hold every ingredient (`canCraft`)?
pub fn can_craft(recipe: &Recipe, inventory: &Inventory) -> bool {
    assign_slots(recipe, &inventory.types()).is_some()
}

/// Recipes that can be crafted right now, in list order (`findMatchingRecipes`).
pub fn matching_recipes(inventory: &Inventory) -> Vec<Recipe> {
    recipes().into_iter().filter(|r| can_craft(r, inventory)).collect()
}

/// The list as the crafting dialogue shows it: craftable recipes first, then the rest
/// (`getRecipeOrdered`).
pub fn ordered_recipes(inventory: &Inventory) -> Vec<Recipe> {
    let (mut craftable, rest): (Vec<_>, Vec<_>) = recipes().into_iter().partition(|r| can_craft(r, inventory));
    craftable.extend(rest);
    craftable
}

/// Use up the ingredients and hand over the result. An item result goes into the inventory (there is
/// always room: at least one ingredient was removed). Returns `None`, changing nothing, if the
/// ingredients are missing.
pub fn craft(recipe: &Recipe, inventory: &mut Inventory) -> Option<RecipeResult> {
    let mut slots = assign_slots(recipe, &inventory.types())?;
    // Remove from the back so earlier indices stay valid.
    slots.sort_unstable_by(|a, b| b.cmp(a));
    for slot in slots {
        inventory.retrieve(slot);
    }
    if let RecipeResult::Item(kind) = recipe.result {
        inventory.add(Item::new(kind));
    }
    Some(recipe.result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use CollectibleType::*;

    fn inventory(items: &[CollectibleType]) -> Inventory {
        let mut inv = Inventory::new();
        for &i in items {
            assert!(inv.add(Item::new(i)));
        }
        inv
    }

    fn recipe(name: &str) -> Recipe {
        recipes().into_iter().find(|r| r.name == name).unwrap()
    }

    #[test]
    fn there_are_the_java_recipes_and_gunpowder_with_their_ingredients() {
        let all = recipes();
        assert_eq!(all.len(), 8);
        assert_eq!(recipe("Gunpowder").ingredients, vec![Sulfur, Coal, Coal]);
        assert_eq!(recipe("Minecart").ingredients, vec![Iron, Iron, Wood]);
        assert_eq!(recipe("Torch").ingredients, vec![Wood, Coal]);
    }

    #[test]
    fn ingredients_match_in_any_slot_order() {
        assert!(can_craft(&recipe("Torch"), &inventory(&[Coal, Wood])));
        assert!(can_craft(&recipe("Torch"), &inventory(&[Wood, Stone, Coal])));
        assert!(can_craft(&recipe("Minecart"), &inventory(&[Wood, Iron, Iron])));
        assert!(!can_craft(&recipe("Minecart"), &inventory(&[Iron, Wood, Stone])));
    }

    #[test]
    fn one_item_cannot_fill_two_ingredients() {
        assert!(!can_craft(&recipe("Construction Kit"), &inventory(&[Wood])));
        assert!(can_craft(&recipe("Construction Kit"), &inventory(&[Wood, Wood])));
    }

    #[test]
    fn crafting_consumes_ingredients_and_adds_the_result() {
        let mut inv = inventory(&[Wood, Stone, Coal]);
        let result = craft(&recipe("Torch"), &mut inv);
        assert_eq!(result, Some(RecipeResult::Item(Torch)));
        assert_eq!(inv.types(), [Some(Stone), Some(Torch), None]);
    }

    #[test]
    fn crafting_without_ingredients_changes_nothing() {
        let mut inv = inventory(&[Wood]);
        assert_eq!(craft(&recipe("Torch"), &mut inv), None);
        assert_eq!(inv.types(), [Some(Wood), None, None]);
    }

    #[test]
    fn a_full_inventory_can_still_craft() {
        let mut inv = inventory(&[Iron, Iron, Wood]);
        assert_eq!(craft(&recipe("Minecart"), &mut inv), Some(RecipeResult::MineCart));
        assert!(inv.is_empty(), "the minecart is spawned in the world, not carried");
        let mut full = inventory(&[Iron, Coal, Stone]);
        assert!(craft(&recipe("Drop Space Flag"), &mut full).is_some());
        assert_eq!(full.types(), [Some(Coal), Some(Stone), Some(DropSpaceFlagConstructionKit)]);
    }

    #[test]
    fn craftable_recipes_are_listed_first() {
        let inv = inventory(&[Sulfur, Coal]);
        let ordered = ordered_recipes(&inv);
        assert_eq!(ordered.len(), 8);
        assert_eq!(ordered[0].name, "TFlint");
        assert_eq!(matching_recipes(&inv).len(), 1);
        // Nothing craftable keeps the plain order.
        assert_eq!(ordered_recipes(&Inventory::new()), recipes());
    }
}
