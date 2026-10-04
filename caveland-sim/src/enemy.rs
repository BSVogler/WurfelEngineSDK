//! Robots beyond the fighter: the spider that gathers ore and the drone, and the robot factory that
//! builds them (`SpiderRobot`, `Quadrocopter`, `RobotFactory`, `RobotFactoryLinker`).

use glam::Vec3;
use wurfel_sim::entity::ai::MoveToAi;
use wurfel_sim::entity::physics::block_at;
use wurfel_sim::entity::EntityId;
use wurfel_sim::{Block, World};

use crate::blocks::ids;
use crate::cells::cell_at;
use crate::dialog::{option, DialogOption};
use crate::game::{cell_center, Cell};

/// Which kind of `Robot` this is (`Robot.type`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RobotVariant {
    /// The hovering fighter (type 0).
    Fighter,
    /// The walking ore gatherer (type 1, `SpiderRobot`).
    Spider,
    /// The flying drone (type 2, `Quadrocopter`).
    Drone,
}

/// The sprite ids of the variants (`Robot(45)`, `setType(1)` -> 58, `Quadrocopter(59)`).
pub fn sprite_id(variant: RobotVariant) -> u8 {
    match variant {
        RobotVariant::Fighter => 45,
        RobotVariant::Spider => 58,
        RobotVariant::Drone => 59,
    }
}

// ---- the spider -------------------------------------------------------------------------------

/// Blocks a spider mines (`COAL`, `IRONORE`, `CRYSTAL`, `SULFUR`).
pub fn is_resource(block_id: u8) -> bool {
    matches!(block_id, ids::COAL | ids::IRON_ORE | ids::CRYSTAL | ids::SULFUR)
}

/// How far a spider can see an ore block, in blocks (`canSee(point, 12)`).
pub const SPIDER_SIGHT: f32 = 12.0;
/// It looks at this many blocks around itself (`nearbyResources`: x and y from -4 to 3, z -2 to 1).
const SCAN_X: std::ops::Range<i32> = -4..4;
const SCAN_Y: std::ops::Range<i32> = -4..4;
const SCAN_Z: std::ops::Range<i32> = -2..2;
/// The spider hits the ore from this close (`GAME_EDGELENGTH * 1.8`).
pub const SPIDER_REACH: f32 = 1.8;
/// Seconds between two scans for ore while the spider has none to work on (the Java laser sweeps).
pub const SPIDER_SCAN_EVERY: f32 = 0.5;
/// A spider charges this long between two hits (`Robot` energy: 1000 ms), then works for 0.6 s.
pub const SPIDER_CHARGE: f32 = 1.0;
pub const SPIDER_WORK: f32 = 0.6;
/// A flag within this many blocks is where it takes its loot.
pub const SPIDER_FLAG_SIGHT: f32 = 12.0;

/// What a spider is doing.
#[derive(Default)]
pub struct Spider {
    /// The ore it works on.
    pub working_block: Option<Cell>,
    /// Where it brings what it mined: the cell of a drop-space flag.
    pub storage: Option<Cell>,
    /// A piece of loot it carries to the storage.
    pub carry: Option<EntityId>,
    /// Walking to a goal.
    pub walking: Option<MoveToAi>,
    pub scan_in: f32,
    pub charge: f32,
    pub working_for: f32,
}

impl Spider {
    pub fn new() -> Self {
        Spider::default()
    }

    /// Has it something to do, so that it should not wander off?
    pub fn has_job(&self) -> bool {
        self.working_block.is_some() || self.carry.is_some()
    }
}

/// Can something at `from` see the block `cell` within `range` blocks? Walls in between hide it; a
/// transparent block like a torch does not.
pub fn sees_cell(world: &World, from: Vec3, cell: Cell, range: f32) -> bool {
    let target = cell_center(cell);
    if from.distance(target) > range {
        return false;
    }
    let steps = (from.distance(target) / 0.1).ceil().max(1.0) as i32;
    for i in 1..=steps {
        let p = from.lerp(target, i as f32 / steps as f32);
        if cell_at(p) == cell {
            return true;
        }
        let block: Block = block_at(world, p);
        if !block.is_air() && !world.blocks().is_transparent(block) {
            return false;
        }
    }
    true
}

/// The ore blocks around `position` that the robot can see (`SpiderRobot.nearbyResources`).
pub fn nearby_resources(world: &World, position: Vec3) -> Vec<Cell> {
    let (ox, oy, oz) = cell_at(position);
    let eyes = position + Vec3::Z * 0.5;
    let mut found = Vec::new();
    for dx in SCAN_X {
        for dy in SCAN_Y {
            for dz in SCAN_Z {
                let cell = (ox + dx, oy + dy, oz + dz);
                if is_resource(world.get(cell.0, cell.1, cell.2).id()) && sees_cell(world, eyes, cell, SPIDER_SIGHT) {
                    found.push(cell);
                }
            }
        }
    }
    found
}

/// After a hit, does the ore give up a piece? (`health % 8 == 0`: every eighth point of damage.)
pub fn drops_loot_at(health: u8) -> bool {
    health > 0 && health % 8 == 0
}

// ---- the robot factory ------------------------------------------------------------------------

/// A robot factory (`RobotFactory`): builds one robot at a time and remembers it
/// (`RobotFactoryLinker`).
#[derive(Debug, Clone, Default)]
pub struct RobotFactory {
    pub linked: Option<EntityId>,
}

/// What can be built there, by option id (`RobotFactory.interact`).
pub fn factory_options() -> Vec<DialogOption> {
    vec![option(0, "Fighter Robot"), option(1, "Robot"), option(2, "Drone")]
}

pub fn variant_for_option(option: u8) -> Option<RobotVariant> {
    match option {
        0 => Some(RobotVariant::Fighter),
        1 => Some(RobotVariant::Spider),
        2 => Some(RobotVariant::Drone),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Vec3;
    use wurfel_sim::grid::to_iso;
    use wurfel_sim::AirGenerator;

    fn world() -> World {
        let mut w = World::new(AirGenerator);
        for x in -10..30 {
            for y in -10..90 {
                w.set(x, y, 0, Block::new(8, 0));
            }
        }
        w
    }

    fn at(x: i32, y: i32, z: f32) -> Vec3 {
        let (gx, gy) = to_iso(x, y);
        Vec3::new(gx, gy, z)
    }

    #[test]
    fn only_four_ores_are_resources() {
        let resources: Vec<u8> = (0..=255u8).filter(|&id| is_resource(id)).collect();
        assert_eq!(resources, vec![ids::CRYSTAL, ids::SULFUR, ids::IRON_ORE, ids::COAL]);
    }

    #[test]
    fn a_spider_finds_ore_it_can_see_and_not_ore_behind_a_wall() {
        let mut w = world();
        // Ore two cells to the south of the spider; stone between the spider and a second ore.
        w.set(10, 42, 1, Block::new(ids::COAL, 0));
        let spider = at(10, 40, 1.0);
        let seen = nearby_resources(&w, spider);
        assert_eq!(seen, vec![(10, 42, 1)]);

        // Ore to the east, with a wall in between: the cell in the same row hides it.
        w.set(12, 40, 1, Block::new(ids::IRON_ORE, 0));
        let open = nearby_resources(&w, spider);
        assert!(open.contains(&(12, 40, 1)), "{open:?}");
        w.set(11, 40, 1, Block::new(3, 0));
        w.set(11, 40, 2, Block::new(3, 0));
        let seen = nearby_resources(&w, spider);
        assert!(!seen.contains(&(12, 40, 1)), "{seen:?}");
        assert!(seen.contains(&(10, 42, 1)));
    }

    #[test]
    fn ore_beyond_the_scan_box_is_ignored() {
        let mut w = world();
        w.set(10, 60, 1, Block::new(ids::COAL, 0));
        assert!(nearby_resources(&w, at(10, 40, 1.0)).is_empty());
    }

    #[test]
    fn sight_ends_at_twelve_blocks() {
        let mut w = world();
        w.set(10, 40, 1, Block::new(ids::COAL, 0));
        let near = at(10, 40, 1.0) + Vec3::new(5.0, 5.0, 0.0);
        let far = at(10, 40, 1.0) + Vec3::new(9.0, 9.0, 0.0);
        assert!(sees_cell(&w, near, (10, 40, 1), SPIDER_SIGHT));
        assert!(!sees_cell(&w, far, (10, 40, 1), SPIDER_SIGHT));
    }

    #[test]
    fn a_piece_of_loot_falls_every_eighth_point_of_damage() {
        let drops: Vec<u8> = (90..=100u8).filter(|&h| drops_loot_at(h)).collect();
        assert_eq!(drops, vec![96]);
        assert!(!drops_loot_at(0));
    }

    #[test]
    fn the_factory_builds_three_things() {
        let options = factory_options();
        assert_eq!(options.iter().map(|o| o.id).collect::<Vec<_>>(), vec![0, 1, 2]);
        assert_eq!(variant_for_option(0), Some(RobotVariant::Fighter));
        assert_eq!(variant_for_option(1), Some(RobotVariant::Spider));
        assert_eq!(variant_for_option(2), Some(RobotVariant::Drone));
        assert_eq!(variant_for_option(3), None);
    }

    #[test]
    fn variants_have_their_sprites() {
        assert_eq!(sprite_id(RobotVariant::Fighter), 45);
        assert_eq!(sprite_id(RobotVariant::Spider), 58);
        assert_eq!(sprite_id(RobotVariant::Drone), 59);
    }

    #[test]
    fn a_new_spider_has_no_job() {
        assert!(!Spider::new().has_job());
        let mut s = Spider::new();
        s.working_block = Some((1, 2, 3));
        assert!(s.has_job());
    }
}
