//! Power: the cable network and what hangs on it (`AbstractPowerBlock`, `CableBlock`,
//! `PowerStationLogic`, `PowerTorch`) and the turret (`Turret`).
//!
//! In Java every power block is a logic object that asks its neighbours, frame by frame, whether
//! they have power. The result is simple: a node has power when it is connected to a power station
//! through nodes that connect to each other. Here that is a flood fill from the stations over the
//! nodes of the loaded chunks, redone when something changed.

use std::collections::{HashMap, HashSet};

use glam::Vec3;
use wurfel_sim::entity::physics::block_at;
use wurfel_sim::entity::EntityId;
use wurfel_sim::{Block, World};

use crate::blocks::ids;
use crate::cells::{find_blocks, neighbour, opposite, SIDES};
use crate::game::Cell;
use crate::team::Team;

/// Which sides of a cable it connects to, by type (`CableBlock.outgoingConnection`). The type is
/// the block value divided by two; the lowest bit of the value says whether the cable has power.
pub fn cable_connects(value: u8, side: u8) -> bool {
    matches!(
        (value / 2, side),
        (0, 1) | (0, 5) | (1, 3) | (1, 7) | (2, 3) | (2, 5) | (3, 5) | (3, 7) | (4, 1) | (4, 7) | (5, 1) | (5, 3)
    )
}

/// The block value of a cable of this type, with or without power.
pub fn cable_value(cable_type: u8, powered: bool) -> u8 {
    cable_type * 2 + u8::from(powered)
}

/// Does this block connect to a neighbour on `side`? Cables only along their direction; the station,
/// the torch and the turret to every side; everything else not at all.
pub fn connects(block: Block, side: u8) -> bool {
    match block.id() {
        ids::POWER_CABLE => cable_connects(block.value(), side),
        ids::POWER_STATION | ids::TORCH | ids::TURRET => true,
        _ => false,
    }
}

/// Does a booster rail (`BoosterLogic`) have power? It looks at its four diagonal neighbours for a
/// powered straight cable pointing at it: on the north-east and south-west sides the NE-SW cable
/// (value 1), on the south-east and north-west sides the SE-NW cable (value 3).
pub fn booster_powered(world: &World, cell: Cell) -> bool {
    [(1u8, 1u8), (3, 3), (5, 1), (7, 3)].into_iter().any(|(side, value)| {
        let n = neighbour(cell, side);
        world.get(n.0, n.1, n.2) == Block::new(ids::POWER_CABLE, value)
    })
}

/// The blocks that take part in the network.
pub const NODE_IDS: [u8; 4] = [ids::POWER_CABLE, ids::POWER_STATION, ids::TORCH, ids::TURRET];

/// A cable that has to change its block value to show whether it has power.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CableUpdate {
    pub cell: Cell,
    pub value: u8,
}

/// Which blocks have power.
#[derive(Debug, Default)]
pub struct PowerGrid {
    powered: HashSet<Cell>,
}

impl PowerGrid {
    /// Does the block at `cell` have power? Only power blocks can; a station always does.
    pub fn is_powered(&self, cell: Cell) -> bool {
        self.powered.contains(&cell)
    }

    pub fn powered_count(&self) -> usize {
        self.powered.len()
    }

    /// Work out again who has power from the blocks in the world. Returns the cables whose value no
    /// longer matches (`CableBlock.update` writes the power bit into the block value).
    pub fn rescan(&mut self, world: &World) -> Vec<CableUpdate> {
        let found = find_blocks(world, &NODE_IDS);
        self.rescan_with(world, &found)
    }

    /// The same, for nodes that have already been looked up: `found` lists cells of
    /// [`NODE_IDS`] blocks.
    pub fn rescan_with(&mut self, world: &World, found: &[(Cell, u8)]) -> Vec<CableUpdate> {
        let nodes: HashMap<Cell, Block> =
            found.iter().map(|&(cell, _)| (cell, world.get(cell.0, cell.1, cell.2))).collect();
        self.powered = flood(&nodes);
        let mut updates: Vec<CableUpdate> = nodes
            .iter()
            .filter(|(_, block)| block.id() == ids::POWER_CABLE)
            .filter_map(|(&cell, block)| {
                let value = cable_value(block.value() / 2, self.powered.contains(&cell));
                (value != block.value()).then_some(CableUpdate { cell, value })
            })
            .collect();
        updates.sort_by_key(|u| u.cell);
        updates
    }
}

/// Everything connected to a station through mutually connecting nodes.
fn flood(nodes: &HashMap<Cell, Block>) -> HashSet<Cell> {
    let mut powered = HashSet::new();
    let mut open: Vec<Cell> = nodes.iter().filter(|(_, b)| b.id() == ids::POWER_STATION).map(|(&c, _)| c).collect();
    powered.extend(open.iter().copied());
    while let Some(cell) = open.pop() {
        let block = nodes[&cell];
        for side in SIDES {
            let next = neighbour(cell, side);
            let Some(&other) = nodes.get(&next) else { continue };
            if connects(block, side) && connects(other, opposite(side)) && powered.insert(next) {
                open.push(next);
            }
        }
    }
    powered
}

// ---- Turret -----------------------------------------------------------------------------------

/// The turret is on the robots' team (`teamId = 1`): it shoots entities of the other teams.
pub const TURRET_TEAM: Team = Team::Robots;
/// How far a turret shoots, in blocks (`MAXDISTANCE`).
pub const TURRET_RANGE: f32 = 20.0;
/// It only looks for targets this close horizontally (`GAME_DIAGLENGTH * 4`), in blocks.
pub const TURRET_SIGHT: f32 = 4.0 * std::f32::consts::SQRT_2;
/// Seconds a turret needs to come online or go offline: the Java rate is 0.005 per millisecond.
const ONLINE_RATE: f32 = 5.0;
/// The turret's gun is the machine gun (`Weapon` 4): seconds between shots, shots per magazine,
/// seconds to reload, and the damage of one bullet.
pub const GUN_DELAY: f32 = 0.075;
pub const GUN_MAGAZINE: u32 = 14;
pub const GUN_RELOAD: f32 = 1.3;
pub const GUN_DAMAGE: f32 = 50.0;
/// The gun sits this far above the block's floor when offline and rises by the second figure when
/// online (`0.8 + online * 0.6` edge lengths).
const GUN_HEIGHT: f32 = 0.8;
const GUN_RISE: f32 = 0.6;

/// A turret: needs power, rises when it has some and shoots the first robot of another team in
/// sight.
#[derive(Debug, Clone, Default)]
pub struct Turret {
    /// 0 offline, 1 online (the gun is up).
    online: f32,
    delay: f32,
    loaded: u32,
    reloading: f32,
}

impl Turret {
    pub fn new() -> Self {
        Turret { online: 0.0, delay: 0.0, loaded: GUN_MAGAZINE, reloading: 0.0 }
    }

    pub fn online(&self) -> f32 {
        self.online
    }

    pub fn is_online(&self) -> bool {
        self.online >= 1.0
    }

    /// Where the gun is, for a block cell's floor position.
    pub fn gun_position(&self, floor: Vec3) -> Vec3 {
        floor + Vec3::Z * (GUN_HEIGHT + self.online * GUN_RISE)
    }

    /// Advance by `dt` seconds with or without power.
    pub fn update(&mut self, powered: bool, dt: f32) {
        let rate = if powered { ONLINE_RATE } else { -ONLINE_RATE };
        self.online = (self.online + rate * dt).clamp(0.0, 1.0);
        self.delay = (self.delay - dt).max(0.0);
        if self.reloading > 0.0 {
            self.reloading -= dt;
            if self.reloading <= 0.0 {
                self.reloading = 0.0;
                self.loaded = GUN_MAGAZINE;
            }
        } else if self.loaded == 0 && self.delay == 0.0 {
            self.reloading = GUN_RELOAD; // automatic reload
        }
    }

    /// Can it shoot right now?
    pub fn can_fire(&self) -> bool {
        self.is_online() && self.delay == 0.0 && self.loaded > 0 && self.reloading == 0.0
    }

    /// Take a shot, if it can. Returns whether it did.
    pub fn fire(&mut self) -> bool {
        if !self.can_fire() {
            return false;
        }
        self.loaded -= 1;
        self.delay = GUN_DELAY;
        true
    }
}

/// A robot a turret might aim at.
#[derive(Debug, Clone, Copy)]
pub struct Candidate {
    pub id: EntityId,
    pub position: Vec3,
    pub team: Team,
}

/// Is something opaque between two points? The turret's own block does not count.
pub fn line_blocked(world: &World, from: Vec3, to: Vec3) -> bool {
    let length = from.distance(to);
    let steps = (length / 0.1).ceil() as i32;
    (1..steps).any(|i| {
        let p = from.lerp(to, i as f32 / steps as f32);
        let block = block_at(world, p);
        !block.is_air() && block.id() != ids::TURRET && !world.blocks().is_transparent(block)
    })
}

/// Pick what to shoot at (`Turret.update`): `candidates` are the robots near the turret in the order
/// the world lists them. Like the Java loop it looks at them one by one; one on the turret's own
/// team ends the search, one behind a wall is skipped, and the first one in the open is the target
/// if it is within range.
pub fn pick_target(world: &World, gun: Vec3, candidates: &[Candidate]) -> Option<Candidate> {
    for &c in candidates {
        if c.team == TURRET_TEAM {
            return None;
        }
        let target_point = c.position;
        if line_blocked(world, gun, target_point) {
            continue;
        }
        return (gun.distance(target_point) <= TURRET_RANGE).then_some(c);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use wurfel_sim::AirGenerator;

    fn world() -> World {
        World::new(AirGenerator)
    }

    fn put(world: &mut World, cell: Cell, id: u8, value: u8) {
        assert!(world.set(cell.0, cell.1, cell.2, Block::new(id, value)));
    }

    /// A row going south-east from `start`: `n` cells, each the SE neighbour of the previous.
    fn row_se(start: Cell, n: usize) -> Vec<Cell> {
        let mut cells = vec![start];
        for _ in 1..n {
            cells.push(neighbour(*cells.last().unwrap(), 3));
        }
        cells
    }

    #[test]
    fn cable_types_connect_the_sides_java_lists() {
        let expect = [
            (0, [1, 5]),
            (1, [3, 7]),
            (2, [3, 5]),
            (3, [5, 7]),
            (4, [1, 7]),
            (5, [1, 3]),
        ];
        for (cable_type, sides) in expect {
            for side in 0..8u8 {
                for powered in [0u8, 1] {
                    let value = cable_type * 2 + powered;
                    assert_eq!(cable_connects(value, side), sides.contains(&side), "type {cable_type} side {side}");
                }
            }
        }
    }

    #[test]
    fn power_flows_from_a_station_along_cables_to_a_torch() {
        let mut w = world();
        let cells = row_se((10, 40, 1), 4);
        put(&mut w, cells[0], ids::POWER_STATION, 0);
        put(&mut w, cells[1], ids::POWER_CABLE, 2); // type 1: SE-NW
        put(&mut w, cells[2], ids::POWER_CABLE, 2);
        put(&mut w, cells[3], ids::TORCH, 0);
        let mut grid = PowerGrid::default();
        grid.rescan(&w);
        for cell in &cells {
            assert!(grid.is_powered(*cell), "{cell:?} has no power");
        }
    }

    #[test]
    fn a_torch_without_a_station_has_no_power() {
        let mut w = world();
        put(&mut w, (10, 40, 1), ids::TORCH, 0);
        let mut grid = PowerGrid::default();
        grid.rescan(&w);
        assert!(!grid.is_powered((10, 40, 1)));
    }

    #[test]
    fn a_gap_in_the_line_cuts_the_power() {
        let mut w = world();
        let cells = row_se((10, 40, 1), 5);
        put(&mut w, cells[0], ids::POWER_STATION, 0);
        put(&mut w, cells[1], ids::POWER_CABLE, 2);
        // cells[2] stays empty
        put(&mut w, cells[3], ids::POWER_CABLE, 2);
        put(&mut w, cells[4], ids::TORCH, 0);
        let mut grid = PowerGrid::default();
        grid.rescan(&w);
        assert!(grid.is_powered(cells[1]));
        assert!(!grid.is_powered(cells[3]));
        assert!(!grid.is_powered(cells[4]));
    }

    #[test]
    fn a_cable_that_points_the_wrong_way_does_not_connect() {
        let mut w = world();
        let cells = row_se((10, 40, 1), 3);
        put(&mut w, cells[0], ids::POWER_STATION, 0);
        put(&mut w, cells[1], ids::POWER_CABLE, 0); // type 0: NE-SW, not SE-NW
        put(&mut w, cells[2], ids::TORCH, 0);
        let mut grid = PowerGrid::default();
        grid.rescan(&w);
        assert!(!grid.is_powered(cells[1]));
        assert!(!grid.is_powered(cells[2]));
    }

    #[test]
    fn cables_show_their_power_in_the_block_value() {
        let mut w = world();
        let cells = row_se((10, 40, 1), 3);
        put(&mut w, cells[0], ids::POWER_STATION, 0);
        put(&mut w, cells[1], ids::POWER_CABLE, 2);
        put(&mut w, cells[2], ids::POWER_CABLE, 2);
        let mut grid = PowerGrid::default();
        let updates = grid.rescan(&w);
        assert_eq!(
            updates,
            vec![CableUpdate { cell: cells[1], value: 3 }, CableUpdate { cell: cells[2], value: 3 }]
        );
        for u in updates {
            put(&mut w, u.cell, ids::POWER_CABLE, u.value);
        }
        assert!(grid.rescan(&w).is_empty(), "the values now agree with the network");
        // Take the station away: the cables lose power again.
        put(&mut w, cells[0], 0, 0);
        let off = grid.rescan(&w);
        assert_eq!(off, vec![CableUpdate { cell: cells[1], value: 2 }, CableUpdate { cell: cells[2], value: 2 }]);
    }

    #[test]
    fn power_goes_around_a_corner() {
        let mut w = world();
        let a = (10, 40, 1);
        let b = neighbour(a, 3);
        let c = neighbour(b, 5); // south-west of b
        put(&mut w, a, ids::POWER_STATION, 0);
        // b is reached from a through b's side 7, so the bend is type 3 (sides 5 and 7): it connects
        // back to a and onwards to side 5.
        put(&mut w, b, ids::POWER_CABLE, cable_value(3, false));
        put(&mut w, c, ids::TORCH, 0);
        let mut grid = PowerGrid::default();
        grid.rescan(&w);
        assert!(grid.is_powered(c));
    }

    #[test]
    fn a_booster_takes_power_from_a_powered_straight_cable_beside_it() {
        let mut w = world();
        let booster = (10, 40, 1);
        assert!(!booster_powered(&w, booster));
        // A powered cable of the right direction on the south-east side.
        put(&mut w, neighbour(booster, 3), ids::POWER_CABLE, 3);
        assert!(booster_powered(&w, booster));
        // The same cable without power does not count, nor does one pointing the other way.
        put(&mut w, neighbour(booster, 3), ids::POWER_CABLE, 2);
        assert!(!booster_powered(&w, booster));
        put(&mut w, neighbour(booster, 3), ids::POWER_CABLE, 1);
        assert!(!booster_powered(&w, booster));
        // North-east takes the NE-SW cable.
        put(&mut w, neighbour(booster, 1), ids::POWER_CABLE, 1);
        assert!(booster_powered(&w, booster));
    }

    fn gun() -> Vec3 {
        Vec3::new(0.0, 0.0, 1.5)
    }

    #[test]
    fn a_turret_comes_online_in_a_fifth_of_a_second_and_goes_off_again() {
        let mut t = Turret::new();
        for _ in 0..11 {
            t.update(true, 1.0 / 60.0);
        }
        assert!(!t.is_online());
        for _ in 0..2 {
            t.update(true, 1.0 / 60.0);
        }
        assert!(t.is_online());
        assert!((t.gun_position(Vec3::ZERO).z - 1.4).abs() < 1e-5);
        for _ in 0..13 {
            t.update(false, 1.0 / 60.0);
        }
        assert_eq!(t.online(), 0.0);
        assert!(!t.can_fire());
    }

    #[test]
    fn the_machine_gun_fires_fourteen_shots_then_reloads() {
        let mut t = Turret::new();
        t.update(true, 1.0);
        let mut shots = 0;
        let mut time = 0.0;
        while time < 1.0 {
            if t.fire() {
                shots += 1;
            }
            t.update(true, 1.0 / 60.0);
            time += 1.0 / 60.0;
        }
        // 14 shots at 75 ms take 1.05 s, so not all of them fit in one second, but nearly.
        assert!((12..=14).contains(&shots), "{shots} shots");
        // Empty the magazine, then it needs 1.3 s.
        let mut t = Turret::new();
        t.update(true, 1.0);
        let mut fired = 0;
        for _ in 0..2000 {
            if t.fire() {
                fired += 1;
            }
            t.update(true, 1.0 / 60.0);
            if fired == GUN_MAGAZINE {
                break;
            }
        }
        assert_eq!(fired, GUN_MAGAZINE);
        t.update(true, GUN_DELAY);
        assert!(!t.can_fire(), "the magazine is empty");
        for _ in 0..60 {
            t.update(true, 1.0 / 60.0);
        }
        assert!(!t.can_fire(), "still reloading after a second");
        for _ in 0..30 {
            t.update(true, 1.0 / 60.0);
        }
        assert!(t.can_fire(), "reloaded after 1.3 s");
    }

    #[test]
    fn an_offline_turret_does_not_fire() {
        let mut t = Turret::new();
        assert!(!t.fire());
    }

    fn candidate(id: u32, x: f32, team: Team) -> Candidate {
        Candidate { id, position: Vec3::new(x, 0.0, 1.5), team }
    }

    #[test]
    fn it_shoots_a_robot_of_another_team_in_the_open() {
        let w = world();
        let target = pick_target(&w, gun(), &[candidate(7, 5.0, Team::Player)]);
        assert_eq!(target.map(|c| c.id), Some(7));
    }

    #[test]
    fn a_robot_on_its_own_team_ends_the_search() {
        let w = world();
        let list = [candidate(1, 3.0, Team::Robots), candidate(2, 5.0, Team::Player)];
        assert!(pick_target(&w, gun(), &list).is_none(), "the Java loop stops at the first robot it finds");
    }

    #[test]
    fn a_robot_behind_a_wall_is_skipped_for_the_next_one() {
        let mut w = world();
        // A wall at x = 2.. in iso ground coordinates is awkward to place by cell; fill a column of
        // cells around the line instead.
        for x in -5..10 {
            for y in -30..30 {
                for z in 1..3 {
                    // Only block the first robot's side of the map (negative gx).
                    let (gx, _) = wurfel_sim::grid::to_iso(x, y);
                    if (-3.0..-1.0).contains(&gx) {
                        put(&mut w, (x, y, z), 3, 0);
                    }
                }
            }
        }
        let behind_wall = Candidate { id: 1, position: Vec3::new(-6.0, 0.0, 1.5), team: Team::Player };
        let in_the_open = candidate(2, 5.0, Team::Player);
        let target = pick_target(&w, gun(), &[behind_wall, in_the_open]);
        assert_eq!(target.map(|c| c.id), Some(2));
        assert!(line_blocked(&w, gun(), behind_wall.position));
        assert!(!line_blocked(&w, gun(), in_the_open.position));
    }

    #[test]
    fn a_robot_out_of_range_is_not_shot_and_ends_the_search() {
        let w = world();
        let list = [candidate(1, 25.0, Team::Player), candidate(2, 5.0, Team::Player)];
        assert!(pick_target(&w, gun(), &list).is_none());
    }
}
