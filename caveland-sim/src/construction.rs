//! Building: construction sites, the kits that make them, the rails and cable kits, and the flags
//! (`ConstructionSite`, `ConstructionKit`, `InstantConstructionKit`, `DropSpaceFlagConstructionKit`,
//! `DropSpaceFlag`, `Flag`, `Flagpole`).

use glam::Vec2;

use crate::blocks::ids;
use crate::collectible::{CollectibleType, Item};
use crate::container::CollectibleContainer;
use crate::dialog::{option, DialogOption};
use crate::team::Team;

// ---- construction sites -----------------------------------------------------------------------

/// What a site needs to turn into `result` (`ConstructionSite.setResult`): the item types and how
/// many of each.
pub fn needed_items(result: u8) -> Vec<(CollectibleType, usize)> {
    match result {
        ids::OVEN => vec![(CollectibleType::Stone, 2), (CollectibleType::Wood, 1)],
        ids::ROBOT_FACTORY => vec![
            (CollectibleType::Iron, 2),
            (CollectibleType::Powercable, 1),
            (CollectibleType::Stone, 1),
        ],
        _ => vec![(CollectibleType::Iron, 2), (CollectibleType::Wood, 1)],
    }
}

/// The block value a site carries to remember what it will become (`setResult`).
pub fn site_value(result: u8) -> u8 {
    match result {
        ids::OVEN => 0,
        ids::POWER_STATION => 1,
        ids::LIFT => 2,
        ids::ROBOT_FACTORY => 3,
        ids::TURRET => 4,
        _ => 0,
    }
}

/// What a site with this block value becomes (`restoreResultFromValue`).
pub fn site_result(value: u8) -> u8 {
    match value {
        0 => ids::OVEN,
        1 => ids::POWER_STATION,
        2 => ids::LIFT,
        3 => ids::ROBOT_FACTORY,
        _ => ids::TURRET,
    }
}

/// A construction site: collects the needed items and turns into the machine when it has them.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ConstructionSite {
    pub result: u8,
    container: CollectibleContainer,
}

impl ConstructionSite {
    pub fn new(result: u8) -> Self {
        ConstructionSite { result, container: CollectibleContainer::new() }
    }

    /// A site found in the world: what it becomes is in its block value.
    pub fn from_value(value: u8) -> Self {
        Self::new(site_result(value))
    }

    pub fn contents(&self) -> &CollectibleContainer {
        &self.container
    }

    /// Is this type one the site still needs? (`canAddFrontItem`: Java accepts any needed type even
    /// when the site has enough of it already.)
    pub fn accepts(&self, kind: CollectibleType) -> bool {
        needed_items(self.result).iter().any(|(k, _)| *k == kind)
    }

    pub fn add(&mut self, item: Item) -> bool {
        self.accepts(item.kind) && self.container.add(item)
    }

    /// Take the item that was added last.
    pub fn take_last(&mut self) -> Option<Item> {
        let last = self.container.len().checked_sub(1)?;
        self.container.retrieve(last)
    }

    pub fn can_build(&self) -> bool {
        needed_items(self.result).iter().all(|(kind, amount)| self.container.count(*kind) >= *amount)
    }

    /// `getStatusString`: `"1/2 Stone, 0/1 Wood, "`.
    pub fn status(&self) -> String {
        needed_items(self.result)
            .iter()
            .map(|(kind, amount)| format!("{}/{} {}, ", self.container.count(*kind), amount, kind.name()))
            .collect()
    }

    /// Use the items up (`build` does this once the block has been replaced).
    pub fn consume(&mut self) -> Vec<Item> {
        self.container.drain()
    }
}

/// What the toolkit offers to build (`ConstructionKit.interact`), with the names the blocks have.
pub fn toolkit_options() -> Vec<DialogOption> {
    [
        (ids::OVEN, "Oven"),
        (ids::ROBOT_FACTORY, "Robot factory"),
        (ids::POWER_STATION, "Power Station"),
        (ids::TURRET, "Turret"),
    ]
    .into_iter()
    .map(|(id, name)| option(id, name))
    .collect()
}

// ---- rails and cable kits ---------------------------------------------------------------------

/// The ten pieces a rails or cable kit can lay, by number, with the Java labels
/// (`InstantConstructionKit.setOrder`).
const PIECES: [&str; 10] = [
    "Straight SW-NE",
    "Straight NW-SE",
    "Curved SW-SE",
    "Curved SW-NW",
    "Curved NW-NE",
    "Curved SE-NE",
    "up SW-NE",
    "up SE-NW",
    "up NE-SW",
    "up NW-SE",
];

/// The order the pieces are offered in, by the side the last piece led to: the one that continues
/// the line comes first.
fn piece_order(to_side: u8) -> [u8; 10] {
    match to_side {
        1 => [0, 2, 3, 6, 1, 4, 5, 7, 8, 9],
        5 => [0, 4, 5, 8, 6, 1, 3, 2, 7, 9],
        7 => [1, 2, 5, 7, 0, 3, 6, 4, 8, 9],
        3 => [1, 3, 4, 9, 7, 0, 5, 8, 6, 2],
        _ => [0, 1, 2, 3, 4, 5, 6, 7, 8, 9],
    }
}

/// The block value of a piece: the piece number for rails, twice that for cables (the lowest bit of
/// a cable's value is its power).
pub fn piece_value(block_id: u8, piece: u8) -> u8 {
    if block_id == ids::POWER_CABLE {
        piece * 2
    } else {
        piece
    }
}

/// The options of the "Choose direction" box. Each option's id is the block value to place.
pub fn line_options(block_id: u8, to_side: u8) -> Vec<DialogOption> {
    piece_order(to_side).into_iter().map(|p| option(piece_value(block_id, p), PIECES[p as usize])).collect()
}

/// The block a kit of this type lays.
pub fn kit_block(kind: CollectibleType) -> Option<u8> {
    match kind {
        CollectibleType::Rails => Some(ids::RAILS),
        CollectibleType::Powercable => Some(ids::POWER_CABLE),
        _ => None,
    }
}

/// Where the line goes on after a piece: the side of the next cell and whether it is one higher.
/// `orientation` is the way the player faces on the screen (x right, y down); `piece` is the piece
/// number (0 to 9). `None` when the facing does not fit the piece, in which case the previous
/// direction stays (`InstantConstructionKit.interact`).
pub fn continue_line(piece: u8, orientation: Vec2) -> Option<(u8, bool)> {
    let (x, y) = (orientation.x, orientation.y);
    let r = piece;
    Some(if (r == 0 || r == 8) && x >= 0.0 && y <= 0.0 {
        (1, false)
    } else if (r == 0 || r == 6) && x <= 0.0 && y >= 0.0 {
        (5, false)
    } else if (r == 1 || r == 9) && x <= 0.0 && y <= 0.0 {
        (7, false)
    } else if (r == 1 || r == 7) && x >= 0.0 && y >= 0.0 {
        (3, false)
    } else if r == 2 && x >= 0.0 {
        (3, false)
    } else if r == 2 && x <= 0.0 {
        (5, false)
    } else if r == 3 && y <= 0.0 {
        (7, false)
    } else if r == 3 && y >= 0.0 {
        (5, false)
    } else if r == 4 && x <= 0.0 {
        (7, false)
    } else if r == 4 && x >= 0.0 {
        (1, false)
    } else if r == 5 && y <= 0.0 {
        (1, false)
    } else if r == 5 && y >= 0.0 {
        (3, false)
    } else if r == 6 && x >= 0.0 && y <= 0.0 {
        (1, true)
    } else if r == 7 && x <= 0.0 && y <= 0.0 {
        (7, true)
    } else if r == 8 && x <= 0.0 && y >= 0.0 {
        (5, true)
    } else if r == 9 && x >= 0.0 && y >= 0.0 {
        (3, true)
    } else {
        return None;
    })
}

// ---- flags ------------------------------------------------------------------------------------

/// A flag (`Flag`): belongs to a team and is where that team respawns.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Flag {
    pub team: Team,
}

/// How far above its pole the flag flies (`2.5 * GAME_EDGELENGTH`).
pub const FLAG_HEIGHT: f32 = 2.5;

#[cfg(test)]
mod tests {
    use super::*;

    fn item(kind: CollectibleType) -> Item {
        Item::new(kind)
    }

    #[test]
    fn the_needs_match_the_java_recipes() {
        assert_eq!(needed_items(ids::OVEN), vec![(CollectibleType::Stone, 2), (CollectibleType::Wood, 1)]);
        assert_eq!(
            needed_items(ids::ROBOT_FACTORY),
            vec![(CollectibleType::Iron, 2), (CollectibleType::Powercable, 1), (CollectibleType::Stone, 1)]
        );
        for other in [ids::POWER_STATION, ids::LIFT, ids::TURRET] {
            assert_eq!(needed_items(other), vec![(CollectibleType::Iron, 2), (CollectibleType::Wood, 1)]);
        }
    }

    #[test]
    fn a_site_remembers_what_it_becomes_in_its_block_value() {
        for result in [ids::OVEN, ids::POWER_STATION, ids::LIFT, ids::ROBOT_FACTORY, ids::TURRET] {
            assert_eq!(site_result(site_value(result)), result);
            assert_eq!(ConstructionSite::from_value(site_value(result)).result, result);
        }
        assert_eq!(site_result(99), ids::TURRET, "unknown values mean a turret, like Java");
    }

    #[test]
    fn a_site_builds_when_everything_is_in() {
        let mut site = ConstructionSite::new(ids::OVEN);
        assert!(!site.can_build());
        assert!(site.add(item(CollectibleType::Stone)));
        assert!(site.add(item(CollectibleType::Wood)));
        assert!(!site.can_build(), "only one stone");
        assert_eq!(site.status(), "1/2 Stone, 1/1 Wood, ");
        assert!(site.add(item(CollectibleType::Stone)));
        assert!(site.can_build());
        assert_eq!(site.consume().len(), 3);
        assert!(!site.can_build(), "the items are used up");
    }

    #[test]
    fn a_site_refuses_what_it_does_not_need() {
        let mut site = ConstructionSite::new(ids::OVEN);
        assert!(!site.accepts(CollectibleType::Iron));
        assert!(!site.add(item(CollectibleType::Iron)));
        assert!(site.contents().is_empty());
    }

    #[test]
    fn extra_items_are_accepted_but_do_not_count_twice() {
        let mut site = ConstructionSite::new(ids::OVEN);
        for _ in 0..3 {
            site.add(item(CollectibleType::Wood));
        }
        assert_eq!(site.contents().count(CollectibleType::Wood), 3);
        assert!(!site.can_build(), "wood is not stone");
    }

    #[test]
    fn taking_gives_back_the_last_item() {
        let mut site = ConstructionSite::new(ids::OVEN);
        site.add(item(CollectibleType::Stone));
        site.add(item(CollectibleType::Wood));
        assert_eq!(site.take_last().map(|i| i.kind), Some(CollectibleType::Wood));
        assert_eq!(site.take_last().map(|i| i.kind), Some(CollectibleType::Stone));
        assert_eq!(site.take_last(), None);
    }

    #[test]
    fn the_toolkit_offers_four_machines() {
        let ids_offered: Vec<u8> = toolkit_options().iter().map(|o| o.id).collect();
        assert_eq!(ids_offered, vec![ids::OVEN, ids::ROBOT_FACTORY, ids::POWER_STATION, ids::TURRET]);
    }

    #[test]
    fn every_ordering_offers_each_piece_once() {
        for side in [0u8, 1, 3, 5, 7] {
            let mut order = piece_order(side).to_vec();
            order.sort_unstable();
            assert_eq!(order, (0..10).collect::<Vec<u8>>(), "side {side}");
        }
    }

    #[test]
    fn the_first_offer_continues_the_line() {
        assert_eq!(line_options(ids::RAILS, 1)[0].label, "Straight SW-NE");
        assert_eq!(line_options(ids::RAILS, 3)[0].label, "Straight NW-SE");
        assert_eq!(line_options(ids::RAILS, 5)[0].label, "Straight SW-NE");
        assert_eq!(line_options(ids::RAILS, 7)[0].label, "Straight NW-SE");
    }

    #[test]
    fn cable_values_are_twice_the_piece_so_the_low_bit_stays_free() {
        let cables = line_options(ids::POWER_CABLE, 0);
        assert!(cables.iter().all(|o| o.id % 2 == 0));
        assert_eq!(cables.iter().map(|o| o.id).max(), Some(18));
        let rails = line_options(ids::RAILS, 0);
        assert_eq!(rails.iter().map(|o| o.id).collect::<Vec<_>>(), (0..10).collect::<Vec<u8>>());
    }

    #[test]
    fn kits_lay_their_own_block() {
        assert_eq!(kit_block(CollectibleType::Rails), Some(ids::RAILS));
        assert_eq!(kit_block(CollectibleType::Powercable), Some(ids::POWER_CABLE));
        assert_eq!(kit_block(CollectibleType::Iron), None);
    }

    #[test]
    fn a_straight_piece_goes_on_the_way_the_player_faces() {
        assert_eq!(continue_line(0, Vec2::new(1.0, -1.0)), Some((1, false)));
        assert_eq!(continue_line(0, Vec2::new(-1.0, 1.0)), Some((5, false)));
        assert_eq!(continue_line(1, Vec2::new(-1.0, -1.0)), Some((7, false)));
        assert_eq!(continue_line(1, Vec2::new(1.0, 1.0)), Some((3, false)));
    }

    #[test]
    fn a_piece_going_up_continues_one_cell_higher() {
        assert_eq!(continue_line(6, Vec2::new(1.0, -1.0)), Some((1, true)));
        assert_eq!(continue_line(7, Vec2::new(-1.0, -1.0)), Some((7, true)));
        assert_eq!(continue_line(8, Vec2::new(-1.0, 1.0)), Some((5, true)));
        assert_eq!(continue_line(9, Vec2::new(1.0, 1.0)), Some((3, true)));
    }

    #[test]
    fn a_facing_that_does_not_fit_leaves_the_direction_alone() {
        // A straight SW-NE piece (0) needs the player to face one of two quadrants.
        assert_eq!(continue_line(0, Vec2::new(1.0, 1.0)), None);
        assert_eq!(continue_line(1, Vec2::new(1.0, -1.0)), None);
    }

    #[test]
    fn curves_pick_the_side_from_the_facing() {
        assert_eq!(continue_line(2, Vec2::new(1.0, 0.5)), Some((3, false)));
        assert_eq!(continue_line(2, Vec2::new(-1.0, 0.5)), Some((5, false)));
        assert_eq!(continue_line(3, Vec2::new(0.5, -1.0)), Some((7, false)));
        assert_eq!(continue_line(4, Vec2::new(1.0, 0.0)), Some((1, false)));
        assert_eq!(continue_line(5, Vec2::new(0.0, 1.0)), Some((3, false)));
    }

    #[test]
    fn rails_and_cable_kits_start_with_three_pieces() {
        assert_eq!(Item::new(CollectibleType::Rails).charges, 3);
        assert_eq!(Item::new(CollectibleType::Powercable).charges, 3);
        assert_eq!(Item::new(CollectibleType::Coal).charges, 0);
    }
}
