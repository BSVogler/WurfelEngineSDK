//! Characters, machines and the story of Caveland: what `extras.rs` adds to the core rules.

use caveland_sim::blocks::ids;
use caveland_sim::cells::neighbour;
use caveland_sim::collectible::{CollectibleType as C, Item};
use caveland_sim::game::{cell_center, cell_floor, Cell};
use caveland_sim::player::{Action, Controls};
use caveland_sim::power::TargetMode;
use caveland_sim::{Caveland, DialogMode, EntityKind, ExtraEvent, GameEvent, Team, Tuning};
use glam::Vec3;
use wurfel_sim::block::Block;
use wurfel_sim::entity::{Entities, EntityId};
use wurfel_sim::grid::{from_iso, to_iso};
use wurfel_sim::{AirGenerator, World};

const DT: f32 = 1.0 / 60.0;

struct Game {
    world: World,
    entities: Entities,
    caveland: Caveland,
    events: Vec<GameEvent>,
    extra: Vec<ExtraEvent>,
}

impl Game {
    /// A sand floor at z = 0 around block (10, 40).
    fn new() -> Game {
        let mut world = World::new(AirGenerator);
        Caveland::install(&mut world);
        for x in -10..60 {
            for y in -20..100 {
                world.set(x, y, 0, Block::new(ids::SAND, 0));
            }
        }
        Game { world, entities: Entities::new(), caveland: Caveland::new(Tuning::default(), 1), events: Vec::new(), extra: Vec::new() }
    }

    fn player_at(&mut self, x: i32, y: i32) -> EntityId {
        let (gx, gy) = to_iso(x, y);
        self.caveland.spawn_player(&mut self.entities, 0, Vec3::new(gx, gy, 1.0))
    }

    fn step(&mut self, steps: usize) {
        for _ in 0..steps {
            let events = self.caveland.tick(&mut self.entities, &mut self.world, DT);
            self.events.extend(events);
            self.extra.extend(self.caveland.drain_extra_events());
        }
    }

    fn seconds(&mut self, s: f32) {
        self.step((s * 60.0).round() as usize);
    }

    fn act(&mut self, player: EntityId, action: Action) {
        self.caveland.act(&mut self.entities, &mut self.world, player, action);
        self.events.extend(self.caveland.drain_events());
        self.extra.extend(self.caveland.drain_extra_events());
    }

    fn give(&mut self, player: EntityId, kind: C) {
        assert!(self.caveland.player_mut(player).unwrap().inventory.add(Item::new(kind)));
    }

    fn pack(&self, player: EntityId) -> Vec<C> {
        self.caveland.player(player).unwrap().inventory.items().iter().map(|i| i.kind).collect()
    }

    fn position(&self, id: EntityId) -> Vec3 {
        self.entities.get(id).unwrap().position
    }

    fn cell(&self, id: EntityId) -> Cell {
        let p = self.position(id);
        let (x, y) = from_iso(p.x, p.y);
        (x, y, p.z.floor() as i32)
    }

    fn block(&self, cell: Cell) -> Block {
        self.world.get(cell.0, cell.1, cell.2)
    }

    fn put(&mut self, cell: Cell, id: u8, value: u8) {
        assert!(self.world.set(cell.0, cell.1, cell.2, Block::new(id, value)));
    }

    fn teleport(&mut self, id: EntityId, cell: Cell) {
        let e = self.entities.get_mut(id).unwrap();
        e.position = cell_floor(cell);
        e.body.as_mut().unwrap().movement = Vec3::ZERO;
    }

    fn saw_extra(&self, f: impl Fn(&ExtraEvent) -> bool) -> bool {
        self.extra.iter().any(f)
    }

    fn saw(&self, f: impl Fn(&GameEvent) -> bool) -> bool {
        self.events.iter().any(f)
    }

    fn dialog_text(&self, player: EntityId) -> Option<String> {
        self.caveland.open_dialog(player).map(|d| format!("{}|{}", d.title, d.text))
    }

    fn things(&self, kind: EntityKind) -> Vec<EntityId> {
        self.caveland.things().into_iter().filter(|&(_, k)| k == kind).map(|(id, _)| id).collect()
    }
}

// ---- Vanya ----------------------------------------------------------------------------------

#[test]
fn talking_to_vanya_opens_her_first_line_and_answering_goes_on() {
    let mut g = Game::new();
    let player = g.player_at(10, 40);
    let (gx, gy) = to_iso(10, 40);
    g.caveland.spawn_vanya(&mut g.entities, Vec3::new(gx + 1.0, gy, 1.0));
    g.step(5);
    g.act(player, Action::Interact);
    let first = g.dialog_text(player).expect("a dialog opens");
    assert!(first.starts_with("Vanya|Oh hello!"), "{first}");
    assert_eq!(g.caveland.open_dialog(player).unwrap().mode, DialogMode::Simple);
    assert!(g.saw(|e| matches!(e, GameEvent::Sound { name: "huhu", .. })));

    g.act(player, Action::Choose(1));
    assert!(g.dialog_text(player).unwrap().contains("I guess you wonder"));
    g.act(player, Action::Choose(1));
    assert!(g.dialog_text(player).unwrap().contains("Follow me"));
    g.act(player, Action::Choose(1));
    assert_eq!(g.dialog_text(player), None, "the next step is silent");
    assert!(g.saw_extra(|e| matches!(e, ExtraEvent::TutorialStep { step: 1 })), "the third line starts the tutorial");
}

#[test]
fn after_the_tutorial_starts_vanya_leaves_and_cannot_be_talked_to_on_the_way() {
    let mut g = Game::new();
    let player = g.player_at(10, 40);
    let (gx, gy) = to_iso(10, 40);
    let vanya = g.caveland.spawn_vanya(&mut g.entities, Vec3::new(gx + 1.0, gy, 1.0));
    g.step(5);
    for _ in 0..3 {
        g.act(player, Action::Interact);
        if g.caveland.open_dialog(player).is_some() {
            g.act(player, Action::Choose(1));
        }
    }
    // The first two lines opened; the third line was shown by the loop's last choose.
    let before = g.position(vanya);
    g.seconds(1.5);
    let after = g.position(vanya);
    assert!((after - before).truncate().length() > 0.5, "she should be on her way to the first waypoint");
    g.act(player, Action::Interact);
    assert!(g.caveland.open_dialog(player).is_none(), "she cannot be talked to while moving");
}

#[test]
fn cancelling_vanyas_line_goes_on_like_a_no() {
    let mut g = Game::new();
    let player = g.player_at(10, 40);
    let (gx, gy) = to_iso(10, 40);
    g.caveland.spawn_vanya(&mut g.entities, Vec3::new(gx + 1.0, gy, 1.0));
    g.step(5);
    g.act(player, Action::Interact);
    g.act(player, Action::Cancel);
    assert!(g.dialog_text(player).unwrap().contains("I guess you wonder"), "cancel advances the script");
}

#[test]
fn an_answer_the_dialog_does_not_offer_is_ignored() {
    let mut g = Game::new();
    let player = g.player_at(10, 40);
    let (gx, gy) = to_iso(10, 40);
    g.caveland.spawn_vanya(&mut g.entities, Vec3::new(gx + 1.0, gy, 1.0));
    g.step(5);
    g.act(player, Action::Interact);
    let first = g.dialog_text(player).unwrap();
    g.act(player, Action::Choose(7));
    assert_eq!(g.dialog_text(player), Some(first), "option 7 does not exist in a plain line");
}

#[test]
fn there_is_only_one_vanya() {
    let mut g = Game::new();
    g.player_at(10, 40);
    let (gx, gy) = to_iso(10, 40);
    g.caveland.spawn_vanya(&mut g.entities, Vec3::new(gx, gy, 1.0));
    g.caveland.spawn_vanya(&mut g.entities, Vec3::new(gx + 2.0, gy, 1.0));
    g.step(5);
    assert_eq!(g.things(EntityKind::Vanya).len(), 1);
}

#[test]
fn nobody_nearby_means_no_dialog() {
    let mut g = Game::new();
    let player = g.player_at(10, 40);
    let (gx, gy) = to_iso(10, 40);
    g.caveland.spawn_vanya(&mut g.entities, Vec3::new(gx + 6.0, gy, 1.0));
    g.step(5);
    g.act(player, Action::Interact);
    assert_eq!(g.dialog_text(player), None);
}

// ---- shop -----------------------------------------------------------------------------------

#[test]
fn the_shop_sells_goods_for_money() {
    let mut g = Game::new();
    let player = g.player_at(10, 40);
    let (gx, gy) = to_iso(10, 40);
    g.caveland.spawn_shopkeeper(&mut g.entities, Vec3::new(gx + 1.0, gy, 1.0));
    g.step(5);
    g.caveland.set_money(10);
    g.act(player, Action::Interact);
    let dialog = g.caveland.open_dialog(player).expect("the shop opens").clone();
    assert_eq!(dialog.title, "Shopkeeper");
    assert_eq!(dialog.mode, DialogMode::Selection);
    assert_eq!(dialog.options[0].label, "Buy Torch (3)");
    assert!(g.saw(|e| matches!(e, GameEvent::Sound { name: "merchantWelcome", .. })));

    g.act(player, Action::Choose(0));
    assert_eq!(g.pack(player), vec![C::Torch]);
    assert_eq!(g.caveland.money(), 7);
    assert!(g.saw_extra(|e| matches!(e, ExtraEvent::Bought { kind: C::Torch, price: 3, .. })));
    assert!(g.saw(|e| matches!(e, GameEvent::Sound { name: "merchantAha", .. })));
}

#[test]
fn the_shop_refuses_when_there_is_not_enough_money() {
    let mut g = Game::new();
    let player = g.player_at(10, 40);
    let (gx, gy) = to_iso(10, 40);
    g.caveland.spawn_shopkeeper(&mut g.entities, Vec3::new(gx + 1.0, gy, 1.0));
    g.step(5);
    g.caveland.set_money(2);
    g.act(player, Action::Interact);
    g.act(player, Action::Choose(0));
    assert!(g.pack(player).is_empty());
    assert_eq!(g.caveland.money(), 2);
    assert!(g.saw(|e| matches!(e, GameEvent::Sound { name: "interactionFail", .. })));
}

#[test]
fn the_shop_refuses_when_the_pack_is_full() {
    let mut g = Game::new();
    let player = g.player_at(10, 40);
    let (gx, gy) = to_iso(10, 40);
    g.caveland.spawn_shopkeeper(&mut g.entities, Vec3::new(gx + 1.0, gy, 1.0));
    g.step(5);
    g.caveland.set_money(100);
    for _ in 0..3 {
        g.give(player, C::Stone);
    }
    g.act(player, Action::Interact);
    g.act(player, Action::Choose(0));
    assert_eq!(g.caveland.money(), 100);
    assert_eq!(g.pack(player).len(), 3);
}

// ---- construction ---------------------------------------------------------------------------

/// Build a construction site for `result` with the toolkit where the player stands, then step
/// aside. Returns the site's cell.
fn toolkit_site(g: &mut Game, player: EntityId, result: u8) -> Cell {
    g.give(player, C::Toolkit);
    let cell = g.cell(player);
    g.act(player, Action::UseItem);
    assert_eq!(g.caveland.open_dialog(player).unwrap().title, "Choose construction");
    g.act(player, Action::Choose(result));
    assert_eq!(g.block(cell).id(), ids::CONSTRUCTION_SITE, "no site was built");
    g.teleport(player, (cell.0 + 1, cell.1, cell.2));
    g.step(2);
    cell
}

#[test]
fn the_toolkit_offers_four_machines_and_builds_a_site_where_you_stand() {
    let mut g = Game::new();
    let player = g.player_at(10, 40);
    g.give(player, C::Toolkit);
    let cell = g.cell(player);
    g.act(player, Action::UseItem);
    let dialog = g.caveland.open_dialog(player).unwrap().clone();
    let offered: Vec<u8> = dialog.options.iter().map(|o| o.id).collect();
    assert_eq!(offered, vec![ids::OVEN, ids::ROBOT_FACTORY, ids::POWER_STATION, ids::TURRET]);
    assert_eq!(g.pack(player), vec![C::Toolkit], "the kit stays while choosing");

    g.act(player, Action::Choose(ids::TURRET));
    assert_eq!(g.block(cell), Block::new(ids::CONSTRUCTION_SITE, 4), "a turret site remembers its result in the value");
    assert!(g.pack(player).is_empty(), "the kit is used up");
    assert!(g.saw_extra(|e| matches!(e, ExtraEvent::BlockChanged { block, .. } if block.id() == ids::CONSTRUCTION_SITE)));
    assert_eq!(g.caveland.construction_site(cell).unwrap().result, ids::TURRET);
}

#[test]
fn cancelling_the_toolkit_keeps_it() {
    let mut g = Game::new();
    let player = g.player_at(10, 40);
    g.give(player, C::Toolkit);
    g.act(player, Action::UseItem);
    g.act(player, Action::Cancel);
    assert!(g.caveland.open_dialog(player).is_none());
    assert_eq!(g.pack(player), vec![C::Toolkit]);
}

#[test]
fn a_site_is_built_into_the_machine_once_it_has_everything() {
    let mut g = Game::new();
    let player = g.player_at(10, 40);
    let cell = toolkit_site(&mut g, player, ids::OVEN);

    // Without the items, building does nothing.
    g.act(player, Action::Interact);
    let dialog = g.caveland.open_dialog(player).expect("the site's menu").clone();
    assert_eq!(dialog.title, "Build Oven");
    assert_eq!(dialog.options[0].label, "Add: You have nothing to add");
    assert_eq!(dialog.options[1].label, "Take: Empty");
    assert_eq!(dialog.options[2].label, "Build: 0/2 Stone, 0/1 Wood, ");
    g.act(player, Action::Choose(2));
    assert_eq!(g.block(cell).id(), ids::CONSTRUCTION_SITE);

    for kind in [C::Stone, C::Wood, C::Stone] {
        g.give(player, kind);
        g.act(player, Action::Interact);
        assert!(g.caveland.open_dialog(player).unwrap().options[0].label.starts_with("Add: "));
        g.act(player, Action::Choose(0));
    }
    assert!(g.pack(player).is_empty(), "everything went into the site");
    g.act(player, Action::Interact);
    assert_eq!(g.caveland.open_dialog(player).unwrap().options[2].label, "Build: 2/2 Stone, 1/1 Wood, ");
    g.act(player, Action::Choose(2));

    assert_eq!(g.block(cell), Block::new(ids::OVEN, 0));
    assert!(g.caveland.construction_site(cell).is_none());
    assert!(g.saw_extra(|e| matches!(e, ExtraEvent::Built { block: ids::OVEN, .. })));
    assert!(g.saw(|e| matches!(e, GameEvent::Sound { name: "construct", .. })));
}

#[test]
fn a_site_does_not_take_items_it_does_not_need() {
    let mut g = Game::new();
    let player = g.player_at(10, 40);
    toolkit_site(&mut g, player, ids::OVEN);
    g.give(player, C::Iron);
    g.act(player, Action::Interact);
    assert_eq!(g.caveland.open_dialog(player).unwrap().options[0].label, "Add: You have nothing to add");
    g.act(player, Action::Choose(0));
    assert_eq!(g.pack(player), vec![C::Iron]);
}

#[test]
fn taking_from_a_site_puts_the_last_item_on_the_ground() {
    let mut g = Game::new();
    let player = g.player_at(10, 40);
    let cell = toolkit_site(&mut g, player, ids::OVEN);
    g.give(player, C::Stone);
    g.act(player, Action::Interact);
    g.act(player, Action::Choose(0));
    g.act(player, Action::Interact);
    assert_eq!(g.caveland.open_dialog(player).unwrap().options[1].label, "Take: Stone");
    g.act(player, Action::Choose(1));
    assert_eq!(g.caveland.construction_site(cell).unwrap().contents().len(), 0);
    let lying = g.things(EntityKind::Collectible(C::Stone));
    assert_eq!(lying.len(), 1);
}

#[test]
fn breaking_a_site_gives_back_what_was_in_it() {
    let mut g = Game::new();
    let player = g.player_at(10, 40);
    let cell = toolkit_site(&mut g, player, ids::OVEN);
    g.give(player, C::Stone);
    g.act(player, Action::Interact);
    g.act(player, Action::Choose(0));
    g.world.set(cell.0, cell.1, cell.2, Block::AIR);
    g.seconds(1.0);
    assert!(g.caveland.construction_site(cell).is_none());
    assert_eq!(g.things(EntityKind::Collectible(C::Stone)).len(), 1);
}

#[test]
fn a_lift_site_needs_a_cave_entry_below() {
    let mut g = Game::new();
    let player = g.player_at(10, 40);
    let cell = g.cell(player);
    assert!(!g.caveland.place_construction_site(&mut g.world, cell, ids::LIFT));
    g.put((cell.0, cell.1, cell.2 - 1), ids::ENTRY, 0);
    assert!(g.caveland.place_construction_site(&mut g.world, cell, ids::LIFT));
}

#[test]
fn a_site_cannot_be_placed_in_a_wall() {
    let mut g = Game::new();
    g.put((10, 40, 3), ids::SAND, 0);
    assert!(!g.caveland.place_construction_site(&mut g.world, (10, 40, 3), ids::OVEN));
}

// ---- rails and cables -----------------------------------------------------------------------

#[test]
fn a_rails_kit_lays_three_pieces_and_walks_along() {
    let mut g = Game::new();
    let player = g.player_at(10, 40);
    g.give(player, C::Rails);
    let start = g.cell(player);
    let mut laid = Vec::new();
    for round in 0..3 {
        let here = g.cell(player);
        g.act(player, Action::UseItem);
        let dialog = g.caveland.open_dialog(player).unwrap_or_else(|| panic!("round {round}: no dialog")).clone();
        assert_eq!(dialog.title, "Choose direction");
        assert_eq!(dialog.options.len(), 10);
        // The first offer continues the line (the player faces east on the screen).
        let first = dialog.options[0].clone();
        g.act(player, Action::Choose(first.id));
        assert_eq!(g.block(here), Block::new(ids::RAILS, first.id), "round {round}");
        laid.push(here);
        assert_ne!(g.cell(player), here, "the player moves on to the next cell");
    }
    assert_eq!(laid[0], start);
    assert!(g.pack(player).is_empty(), "three pieces and the kit is gone");
}

#[test]
fn a_cable_kit_lays_cables_with_even_values() {
    let mut g = Game::new();
    let player = g.player_at(10, 40);
    g.give(player, C::Powercable);
    let here = g.cell(player);
    g.act(player, Action::UseItem);
    let dialog = g.caveland.open_dialog(player).unwrap().clone();
    assert!(dialog.options.iter().all(|o| o.id % 2 == 0), "the low bit of a cable's value is its power");
    g.act(player, Action::Choose(dialog.options[0].id));
    assert_eq!(g.block(here).id(), ids::POWER_CABLE);
    assert_eq!(g.caveland.player(player).unwrap().inventory.front().unwrap().charges, 2);
}

#[test]
fn rails_cannot_be_laid_inside_a_wall() {
    let mut g = Game::new();
    let player = g.player_at(10, 40);
    g.give(player, C::Rails);
    let here = g.cell(player);
    g.put(here, ids::SAND, 0);
    g.act(player, Action::UseItem);
    assert!(g.caveland.open_dialog(player).is_none());
    assert!(g.saw(|e| matches!(e, GameEvent::Sound { name: "interactionFail", .. })));
}

#[test]
fn the_next_pieces_are_offered_in_the_order_of_the_last_direction() {
    let mut g = Game::new();
    let player = g.player_at(10, 40);
    g.give(player, C::Rails);
    g.act(player, Action::UseItem);
    g.act(player, Action::Choose(0));
    // The line went on to the north-east (side 1); the next offer starts with the continuing piece.
    g.act(player, Action::UseItem);
    let labels: Vec<String> = g.caveland.open_dialog(player).unwrap().options.iter().map(|o| o.label.clone()).collect();
    assert_eq!(labels[0], "Straight SW-NE");
    assert_eq!(labels[1], "Curved SW-SE");
}

// ---- flags ----------------------------------------------------------------------------------

#[test]
fn touching_a_flag_makes_it_yours_and_sets_where_you_respawn() {
    let mut g = Game::new();
    let player = g.player_at(10, 40);
    assert_eq!(g.caveland.respawn_position(), cell_floor((0, 1, 10)), "the default respawn is north of (0, 0, 10)");
    let (gx, gy) = to_iso(10, 40);
    let flag = g.caveland.spawn_flag(&mut g.entities, Team::Neutral, Vec3::new(gx + 1.0, gy, 3.5));
    g.step(2);
    assert_eq!(g.caveland.kind_of(flag), Some(EntityKind::Flag(Team::Neutral)));
    g.act(player, Action::Interact);
    assert_eq!(g.caveland.kind_of(flag), Some(EntityKind::Flag(Team::Player)));
    assert!(g.saw_extra(|e| matches!(e, ExtraEvent::FlagCaptured { team: Team::Player, .. })));
    let flag_cell = g.cell(flag);
    assert_eq!(g.caveland.respawn_position(), cell_floor((flag_cell.0, flag_cell.1 + 1, flag_cell.2 - 1)));
}

#[test]
fn a_flag_pole_carries_a_flag_above_it_and_takes_it_away_when_broken() {
    let mut g = Game::new();
    let pole = (10, 40, 1);
    g.put(pole, ids::FLAG_POLE, 0);
    g.step(40);
    let flags = g.things(EntityKind::Flag(Team::Neutral));
    assert_eq!(flags.len(), 1);
    let p = g.position(flags[0]);
    assert!((p - (cell_floor(pole) + Vec3::Z * 2.5)).length() < 1e-4, "{p}");
    g.world.set(pole.0, pole.1, pole.2, Block::AIR);
    g.step(40);
    assert!(g.things(EntityKind::Flag(Team::Neutral)).is_empty());
}

#[test]
fn a_drop_space_flag_and_its_kit_turn_into_each_other() {
    let mut g = Game::new();
    let player = g.player_at(10, 40);
    g.give(player, C::DropSpaceFlagConstructionKit);
    g.act(player, Action::UseItem);
    assert!(g.pack(player).is_empty(), "the kit is used up");
    assert_eq!(g.things(EntityKind::DropSpaceFlag).len(), 1);

    g.step(2);
    g.act(player, Action::Interact);
    assert!(g.things(EntityKind::DropSpaceFlag).is_empty());
    g.step(2);
    // The kit appears where the player stands, so it goes straight into the pack again.
    assert_eq!(g.pack(player), vec![C::DropSpaceFlagConstructionKit]);
}

// ---- power and the turret -------------------------------------------------------------------

/// A station with a line of cables to a turret three cells to the south-east.
fn powered_turret(g: &mut Game, origin: Cell) -> Cell {
    let station = origin;
    let c1 = neighbour(station, 3);
    let c2 = neighbour(c1, 3);
    let turret = neighbour(c2, 3);
    g.put(station, ids::POWER_STATION, 0);
    g.put(c1, ids::POWER_CABLE, 2);
    g.put(c2, ids::POWER_CABLE, 2);
    g.put(turret, ids::TURRET, 0);
    turret
}

#[test]
fn power_reaches_the_turret_and_the_cables_show_it() {
    let mut g = Game::new();
    let turret = powered_turret(&mut g, (10, 40, 1));
    g.seconds(1.0);
    assert!(g.caveland.is_powered(turret));
    assert!(g.saw_extra(|e| matches!(e, ExtraEvent::PowerChanged { cell, powered: true } if *cell == turret)));
    let cable = neighbour((10, 40, 1), 3);
    assert_eq!(g.block(cable), Block::new(ids::POWER_CABLE, 3), "a powered cable has the low bit set");
    assert!(g.saw_extra(|e| matches!(e, ExtraEvent::BlockChanged { cell, block } if *cell == cable && block.value() == 3)));
    assert!(g.caveland.turret_online(turret).unwrap() >= 1.0);
}

#[test]
fn breaking_the_station_switches_the_turret_off_again() {
    let mut g = Game::new();
    let turret = powered_turret(&mut g, (10, 40, 1));
    g.seconds(1.0);
    g.world.set(10, 40, 1, Block::AIR);
    g.seconds(1.0);
    assert!(!g.caveland.is_powered(turret));
    assert_eq!(g.caveland.turret_online(turret), Some(0.0));
    assert_eq!(g.block(neighbour((10, 40, 1), 3)), Block::new(ids::POWER_CABLE, 2), "the cables lose their power bit");
}

#[test]
fn a_powered_turret_shoots_an_enemy_robot() {
    let mut g = Game::new();
    let turret = powered_turret(&mut g, (10, 40, 1));
    let near = cell_center((turret.0 + 1, turret.1, 1));
    let robot = g.caveland.spawn_robot(&mut g.entities, Team::Robots, Vec3::new(near.x, near.y, 1.0));
    g.seconds(2.0);
    assert!(g.saw_extra(|e| matches!(e, ExtraEvent::TurretShot { .. })), "the turret never fired");
    assert!(g.saw(|e| matches!(e, GameEvent::Sound { name: "turret", .. })));
    assert!(g.entities.get(robot).is_none() || g.entities.get(robot).unwrap().health() <= 0.0, "two bullets of 50 kill a robot");
    assert!(g.saw(|e| matches!(e, GameEvent::RobotDestroyed { .. })));
}

#[test]
fn an_unpowered_turret_leaves_robots_alone() {
    let mut g = Game::new();
    let turret = (10, 40, 1);
    g.put(turret, ids::TURRET, 0);
    let near = cell_center((turret.0 + 1, turret.1, 1));
    let robot = g.caveland.spawn_robot(&mut g.entities, Team::Robots, Vec3::new(near.x, near.y, 1.0));
    g.seconds(2.0);
    assert_eq!(g.entities.get(robot).unwrap().health(), 100.0);
    assert!(!g.saw_extra(|e| matches!(e, ExtraEvent::TurretShot { .. })));
}

#[test]
fn a_turret_does_not_shoot_friendly_robots() {
    let mut g = Game::new();
    let turret = powered_turret(&mut g, (10, 40, 1));
    let near = cell_center((turret.0 + 1, turret.1, 1));
    let robot = g.caveland.spawn_robot(&mut g.entities, Team::Player, Vec3::new(near.x, near.y, 1.0));
    g.seconds(2.0);
    assert_eq!(g.entities.get(robot).unwrap().health(), 100.0);
    assert!(!g.saw_extra(|e| matches!(e, ExtraEvent::TurretShot { .. })));
}

/// The powered turret of [`powered_turret`], but built by `builder` with the toolkit, so that
/// they own it. Returns the turret's cell.
fn turret_built_by(g: &mut Game, builder: EntityId, origin: Cell) -> Cell {
    let c1 = neighbour(origin, 3);
    let c2 = neighbour(c1, 3);
    let turret = neighbour(c2, 3);
    g.put(origin, ids::POWER_STATION, 0);
    g.put(c1, ids::POWER_CABLE, 2);
    g.put(c2, ids::POWER_CABLE, 2);
    g.teleport(builder, turret);
    g.step(2);
    let site = toolkit_site(g, builder, ids::TURRET);
    assert_eq!(site, turret, "the site is where the builder stood");
    for kind in [C::Iron, C::Iron, C::Wood] {
        g.give(builder, kind);
        g.act(builder, Action::Interact);
        g.act(builder, Action::Choose(0));
    }
    g.act(builder, Action::Interact);
    g.act(builder, Action::Choose(2));
    assert_eq!(g.block(turret).id(), ids::TURRET, "the turret was not built");
    g.seconds(1.0);
    turret
}

/// A player's health; one who was shot dead is gone from the world and counts as 0.
fn health(g: &Game, id: EntityId) -> f32 {
    g.entities.get(id).map_or(0.0, |e| e.health())
}

#[test]
fn a_turret_shoots_strangers_but_not_its_owner() {
    let mut g = Game::new();
    let owner = g.player_at(10, 40);
    let turret = turret_built_by(&mut g, owner, (10, 40, 1));
    let owner_health = health(&g, owner);
    let stranger = g.player_at(10, 40);
    g.teleport(stranger, (turret.0 + 1, turret.1 + 1, 1));
    let stranger_health = health(&g, stranger);
    g.seconds(2.0);
    assert!(health(&g, stranger) < stranger_health, "a stranger near the turret is shot");
    assert_eq!(health(&g, owner), owner_health, "the owner standing next to it is not");
}

#[test]
fn a_turret_spares_the_friends_of_its_owner_until_the_friendship_ends() {
    let mut g = Game::new();
    let owner = g.player_at(10, 40);
    let turret = turret_built_by(&mut g, owner, (10, 40, 1));
    let friend = g.player_at(10, 40);
    g.teleport(friend, (turret.0 + 1, turret.1 + 1, 1));
    g.caveland.set_friends([(friend, owner)]);
    let before = health(&g, friend);
    g.seconds(2.0);
    assert_eq!(health(&g, friend), before, "a friend is spared");
    g.caveland.set_friends([]);
    g.seconds(2.0);
    assert!(health(&g, friend) < before, "after the friendship ends they are shot like anybody");
}

/// Open the turret's menu next to it and pick `answer`.
fn set_turret_mode(g: &mut Game, player: EntityId, answer: u8) {
    g.act(player, Action::Interact);
    assert_eq!(g.caveland.open_dialog(player).expect("the turret's menu").title, "Turret");
    g.act(player, Action::Choose(answer));
}

#[test]
fn a_turret_menu_lists_the_modes_and_says_which_one_is_on() {
    let mut g = Game::new();
    let owner = g.player_at(10, 40);
    let turret = turret_built_by(&mut g, owner, (10, 40, 1));
    assert_eq!(g.caveland.turret_mode(turret), Some(TargetMode::HostilesAndStrangers), "the default");
    g.act(owner, Action::Interact);
    let dialog = g.caveland.open_dialog(owner).expect("the turret's menu").clone();
    assert_eq!(dialog.title, "Turret");
    assert!(dialog.text.contains("Enemies and strangers"), "{}", dialog.text);
    assert_eq!(dialog.options.iter().map(|o| o.label.as_str()).collect::<Vec<_>>(), vec![
        "Enemy robots only",
        "Enemies and strangers, not friends",
        "Everything",
    ]);
    g.act(owner, Action::Choose(0));
    assert_eq!(g.caveland.turret_mode(turret), Some(TargetMode::HostileRobots));
    assert!(g.caveland.open_dialog(owner).is_none(), "the menu closes after choosing");
}

#[test]
fn enemy_robots_only_spares_strangers_and_everything_shoots_friends_too() {
    let mut g = Game::new();
    let owner = g.player_at(10, 40);
    let turret = turret_built_by(&mut g, owner, (10, 40, 1));
    set_turret_mode(&mut g, owner, 0);

    let stranger = g.player_at(10, 40);
    g.teleport(stranger, (turret.0 + 1, turret.1 + 1, 1));
    g.seconds(2.0);
    assert_eq!(health(&g, stranger), 100.0, "enemy robots only leaves players alone");
    let robot = g.caveland.spawn_robot(&mut g.entities, Team::Robots, cell_center((turret.0 + 1, turret.1, 1)));
    g.seconds(2.0);
    assert!(g.entities.get(robot).is_none_or(|e| e.health() <= 0.0), "but still shoots an enemy robot");

    set_turret_mode(&mut g, owner, 2);
    assert_eq!(g.caveland.turret_mode(turret), Some(TargetMode::Everything));
    g.caveland.set_friends([(stranger, owner)]);
    g.seconds(2.0);
    assert!(health(&g, stranger) < 100.0, "everything shoots even a friend of the owner");
    assert_eq!(health(&g, owner), 100.0, "but never the owner");
}

#[test]
fn whoever_built_a_turret_last_owns_it() {
    let mut g = Game::new();
    let first = g.player_at(10, 40);
    let turret = turret_built_by(&mut g, first, (10, 40, 1));
    // The turret is replaced: the second player rebuilds a turret in the same spot.
    let second = g.player_at(40, 80); // far from the turret's sight
    g.teleport(first, (40, 82, 1));
    g.world.set(turret.0, turret.1, turret.2, Block::AIR);
    g.seconds(1.0);
    g.teleport(second, turret);
    g.step(2);
    let site = toolkit_site(&mut g, second, ids::TURRET);
    assert_eq!(site, turret);
    for kind in [C::Iron, C::Iron, C::Wood] {
        g.give(second, kind);
        g.act(second, Action::Interact);
        g.act(second, Action::Choose(0));
    }
    g.act(second, Action::Interact);
    g.act(second, Action::Choose(2));
    g.seconds(1.0);
    g.teleport(first, (turret.0 + 1, turret.1 + 1, 1));
    let before = health(&g, first);
    g.seconds(2.0);
    assert!(health(&g, first) < before, "the first builder is a stranger to the turret now");
    assert_eq!(health(&g, second), 100.0);
}

// ---- robot factory and the robots -----------------------------------------------------------

#[test]
fn a_robot_factory_builds_the_robot_you_choose_for_your_team() {
    let mut g = Game::new();
    let player = g.player_at(10, 40);
    let factory = (11, 40, 1);
    g.put(factory, ids::ROBOT_FACTORY, 0);
    g.step(2);
    g.act(player, Action::Interact);
    let dialog = g.caveland.open_dialog(player).expect("the factory's menu").clone();
    assert_eq!(dialog.title, "What do you want to build?");
    assert_eq!(dialog.options.iter().map(|o| o.label.as_str()).collect::<Vec<_>>(), vec!["Fighter Robot", "Robot", "Drone"]);

    g.act(player, Action::Choose(1));
    let spiders = g.things(EntityKind::SpiderRobot(Team::Player));
    assert_eq!(spiders.len(), 1, "a friendly spider was built");
    assert!(g.saw_extra(|e| matches!(e, ExtraEvent::RobotBuilt { variant: caveland_sim::enemy::RobotVariant::Spider, .. })));
    assert!(g.position(spiders[0]).z >= 2.0 - 1e-3, "it stands on top of the factory");
}

#[test]
fn a_factory_builds_one_robot_at_a_time_and_can_destroy_it() {
    let mut g = Game::new();
    let player = g.player_at(10, 40);
    g.put((11, 40, 1), ids::ROBOT_FACTORY, 0);
    g.step(2);
    g.act(player, Action::Interact);
    g.act(player, Action::Choose(2));
    let drone = g.things(EntityKind::Drone(Team::Player));
    assert_eq!(drone.len(), 1);
    assert!(g.entities.get(drone[0]).unwrap().body.as_ref().unwrap().floating, "drones fly");

    g.act(player, Action::Interact);
    let dialog = g.caveland.open_dialog(player).unwrap().clone();
    assert_eq!(dialog.title, "Robot in use");
    assert_eq!(dialog.mode, DialogMode::Boolean);
    g.act(player, Action::Choose(0));
    assert_eq!(g.things(EntityKind::Drone(Team::Player)).len(), 1, "no destroys the robot only on yes");
    g.act(player, Action::Interact);
    g.act(player, Action::Choose(1));
    g.step(3);
    assert!(g.things(EntityKind::Drone(Team::Player)).is_empty(), "the robot was destroyed");

    g.act(player, Action::Interact);
    assert_eq!(g.caveland.open_dialog(player).unwrap().title, "What do you want to build?", "free again");
}

#[test]
fn a_spider_mines_ore_and_carries_the_loot_to_a_drop_space_flag() {
    let mut g = Game::new();
    let ore = (12, 40, 1);
    g.put(ore, ids::COAL, 0);
    g.world.set_block_health(ore.0, ore.1, ore.2, 9); // nearly gone: the first hit gives a piece
    let flag_cell = (12, 44, 1);
    let flag = cell_floor(flag_cell);
    g.caveland.spawn_drop_space_flag(&mut g.entities, flag);
    let (gx, gy) = to_iso(10, 40);
    let spider = g.caveland.spawn_spider(&mut g.entities, Team::Player, Vec3::new(gx, gy, 1.0));
    assert_eq!(g.caveland.kind_of(spider), Some(EntityKind::SpiderRobot(Team::Player)));

    g.seconds(6.0);
    assert!(g.saw(|e| matches!(e, GameEvent::Sound { name: "impact", .. })), "the spider never hit the ore");
    assert_eq!(g.block(ore).id(), ids::COAL, "one hit is not enough to break it");

    // The piece is carried to the flag and put down there (the ore has 8 hits left, so it is
    // still standing).
    g.seconds(4.0);
    assert_eq!(g.block(ore).id(), ids::COAL);
    let coal = g.things(EntityKind::Collectible(C::Coal));
    assert_eq!(coal.len(), 1, "one piece of coal");
    let d = (g.position(coal[0]) - flag).truncate().length();
    assert!(d < 1.0, "the coal lies {d} blocks from the flag");
}

#[test]
fn a_spider_without_a_flag_keeps_what_it_mined() {
    let mut g = Game::new();
    let ore = (12, 40, 1);
    g.put(ore, ids::COAL, 0);
    g.world.set_block_health(ore.0, ore.1, ore.2, 9);
    let (gx, gy) = to_iso(10, 40);
    let spider = g.caveland.spawn_spider(&mut g.entities, Team::Player, Vec3::new(gx, gy, 1.0));
    g.seconds(15.0);
    let coal = g.things(EntityKind::Collectible(C::Coal));
    assert_eq!(coal.len(), 1);
    let d = g.position(coal[0]).distance(g.position(spider) + Vec3::Z * 0.5);
    assert!(d < 0.5, "it rides along with the spider ({d} away)");
}

#[test]
fn an_idle_spider_wanders_instead_of_standing_still() {
    let mut g = Game::new();
    let (gx, gy) = to_iso(10, 40);
    let spider = g.caveland.spawn_spider(&mut g.entities, Team::Player, Vec3::new(gx, gy, 1.0));
    let start = g.position(spider);
    let mut farthest = 0.0f32;
    for _ in 0..20 {
        g.seconds(1.0);
        farthest = farthest.max((g.position(spider) - start).truncate().length());
    }
    assert!(farthest > 0.3, "it never left home");
}

#[test]
fn a_dead_spider_drops_what_it_carried_for_anyone_to_take() {
    let mut g = Game::new();
    let ore = (12, 40, 1);
    g.put(ore, ids::COAL, 0);
    g.world.set_block_health(ore.0, ore.1, ore.2, 9);
    let (gx, gy) = to_iso(10, 40);
    let spider = g.caveland.spawn_spider(&mut g.entities, Team::Player, Vec3::new(gx, gy, 1.0));
    g.seconds(6.0);
    assert_eq!(g.things(EntityKind::Collectible(C::Coal)).len(), 1);
    g.caveland.damage_entity(&mut g.entities, spider, 1000.0);
    g.step(5);
    let player = g.player_at(10, 40);
    let coal = g.things(EntityKind::Collectible(C::Coal))[0];
    g.teleport(player, g.cell(coal));
    g.seconds(2.0);
    assert!(g.pack(player).contains(&C::Coal), "the dropped piece can be picked up: {:?}", g.pack(player));
}

// ---- birds, drones ----------------------------------------------------------------------------

#[test]
fn a_bird_flies_without_falling() {
    let mut g = Game::new();
    let (gx, gy) = to_iso(10, 40);
    let bird = g.caveland.spawn_bird(&mut g.entities, Vec3::new(gx, gy, 6.0));
    g.seconds(5.0);
    assert!(g.position(bird).z > 1.5, "a bird does not fall to the ground");
    assert_eq!(g.caveland.kind_of(bird), Some(EntityKind::Bird));
}

// ---- the story --------------------------------------------------------------------------------

#[test]
fn the_scenario_is_off_unless_asked_for() {
    let mut g = Game::new();
    g.player_at(10, 40);
    g.seconds(1.0);
    assert!(g.things(EntityKind::Vanya).is_empty());
}

#[test]
fn the_scenario_brings_vanya_to_the_start_once() {
    let mut g = Game::new();
    g.caveland.set_scenario(true);
    g.player_at(10, 40);
    g.step(3);
    let vanya = g.things(EntityKind::Vanya);
    assert_eq!(vanya.len(), 1);
    g.seconds(1.0);
    assert_eq!(g.things(EntityKind::Vanya).len(), 1, "not again");
    let (x, y) = from_iso(g.position(vanya[0]).x, g.position(vanya[0]).y);
    assert!((x + 3).abs() <= 2 && (y - 8).abs() <= 3, "starts near (-3, 8): ({x}, {y})");
    assert_eq!(g.caveland.tutorial_step(&g.entities), Some(0));
}

#[test]
fn going_down_into_the_caves_ends_the_tutorial() {
    let mut g = Game::new();
    g.caveland.set_scenario(true);
    let player = g.player_at(10, 40);
    g.step(3);
    g.teleport(player, (10, 1100, 2));
    g.step(3);
    assert_eq!(g.caveland.tutorial_step(&g.entities), Some(4));
    assert!(g.saw_extra(|e| matches!(e, ExtraEvent::TutorialStep { step: 4 })));
}

#[test]
fn entering_the_end_zone_starts_the_fight_once() {
    let mut g = Game::new();
    g.caveland.set_scenario(true);
    let player = g.player_at(10, 40);
    g.step(3);
    g.teleport(player, (29, 0, 4));
    g.step(3);
    assert!(g.saw_extra(|e| matches!(e, ExtraEvent::EndFightStarted)));
    assert_eq!(g.caveland.tutorial_step(&g.entities), Some(5));
    let robots = g.things(EntityKind::Robot(Team::Neutral));
    assert_eq!(robots.len(), 5, "five robots, without a team like the Java ones");
    g.teleport(player, (10, 40, 2));
    g.step(3);
    g.teleport(player, (29, 0, 4));
    g.step(3);
    assert_eq!(g.things(EntityKind::Robot(Team::Neutral)).len(), 5, "the fight starts only once");
    assert_eq!(g.extra.iter().filter(|e| matches!(e, ExtraEvent::EndFightStarted)).count(), 1);
}

#[test]
fn the_intro_ship_can_start_the_tutorial_from_outside() {
    let mut g = Game::new();
    g.caveland.set_scenario(true);
    g.player_at(10, 40);
    g.step(3);
    g.caveland.set_tutorial_step(&g.entities, 1);
    assert_eq!(g.caveland.tutorial_step(&g.entities), Some(1));
    g.caveland.set_tutorial_step(&g.entities, 0);
    assert_eq!(g.caveland.tutorial_step(&g.entities), Some(1), "never backwards");
}

// ---- saving -----------------------------------------------------------------------------------

#[test]
fn money_the_respawn_point_and_machines_survive_a_save() {
    let mut g = Game::new();
    let player = g.player_at(10, 40);
    g.caveland.set_money(42);
    // An oven that has been fed.
    let oven = (13, 40, 1);
    g.put(oven, ids::OVEN, 0);
    g.teleport(player, (12, 40, 1));
    g.step(2);
    g.give(player, C::Coal);
    g.act(player, Action::Interact);
    assert!(g.caveland.oven(oven).unwrap().is_burning());
    // A construction site with something in it.
    g.teleport(player, (10, 40, 1));
    let site = toolkit_site(&mut g, player, ids::OVEN);
    g.give(player, C::Stone);
    g.act(player, Action::Interact);
    g.act(player, Action::Choose(0));
    // A flag.
    let (gx, gy) = to_iso(8, 40);
    g.caveland.spawn_flag(&mut g.entities, Team::Neutral, Vec3::new(gx, gy, 3.5));
    g.teleport(player, (8, 40, 1));
    g.step(2);
    g.act(player, Action::Interact);
    let respawn = g.caveland.respawn_position();

    let saved = g.caveland.save_state(&g.entities);

    let mut fresh = Game::new();
    fresh.caveland.load_state(&fresh.entities, &saved).unwrap();
    assert_eq!(fresh.caveland.money(), 42);
    assert_eq!(fresh.caveland.respawn_position(), respawn);
    assert!(fresh.caveland.oven(oven).unwrap().is_burning(), "the oven still burns");
    assert_eq!(fresh.caveland.construction_site(site).unwrap().contents().count(C::Stone), 1);
}

#[test]
fn a_broken_save_is_an_error_and_changes_nothing() {
    let mut g = Game::new();
    g.caveland.set_money(5);
    assert!(g.caveland.load_state(&g.entities, "{not json").unwrap_err().contains("unreadable"));
    assert_eq!(g.caveland.money(), 5);
}

#[test]
fn a_save_of_an_empty_game_round_trips() {
    let g = Game::new();
    let saved = g.caveland.save_state(&g.entities);
    let mut again = Game::new();
    again.caveland.load_state(&again.entities, &saved).unwrap();
    assert_eq!(again.caveland.save_state(&again.entities), saved);
}

// ---- dialogs ----------------------------------------------------------------------------------

#[test]
fn a_dialog_closes_when_the_character_goes_away() {
    let mut g = Game::new();
    let player = g.player_at(10, 40);
    let (gx, gy) = to_iso(10, 40);
    let shop = g.caveland.spawn_shopkeeper(&mut g.entities, Vec3::new(gx + 1.0, gy, 1.0));
    g.step(5);
    g.act(player, Action::Interact);
    assert!(g.caveland.open_dialog(player).is_some());
    g.entities.get_mut(shop).unwrap().dispose();
    g.step(3);
    assert!(g.caveland.open_dialog(player).is_none());
    assert!(g.saw_extra(|e| matches!(e, ExtraEvent::DialogClosed { .. })));
}

#[test]
fn choosing_without_a_dialog_does_nothing() {
    let mut g = Game::new();
    let player = g.player_at(10, 40);
    g.act(player, Action::Choose(1));
    g.act(player, Action::Cancel);
    assert!(g.extra.is_empty());
}

#[test]
fn interacting_while_a_dialog_is_open_does_not_stack_dialogs() {
    let mut g = Game::new();
    let player = g.player_at(10, 40);
    let (gx, gy) = to_iso(10, 40);
    g.caveland.spawn_shopkeeper(&mut g.entities, Vec3::new(gx + 1.0, gy, 1.0));
    g.step(5);
    g.act(player, Action::Interact);
    let opened = g.extra.iter().filter(|e| matches!(e, ExtraEvent::DialogOpened { .. })).count();
    g.act(player, Action::Interact);
    assert_eq!(g.extra.iter().filter(|e| matches!(e, ExtraEvent::DialogOpened { .. })).count(), opened);
}

#[test]
fn an_oven_nearer_than_a_shopkeeper_is_still_used() {
    let mut g = Game::new();
    let player = g.player_at(10, 40);
    let (gx, gy) = to_iso(10, 40);
    g.put((10, 41, 1), ids::OVEN, 0);
    g.caveland.spawn_shopkeeper(&mut g.entities, Vec3::new(gx + 1.9, gy, 1.0));
    g.step(5);
    g.give(player, C::Coal);
    g.act(player, Action::Interact);
    assert!(g.caveland.open_dialog(player).is_none(), "the oven took the coal, no shop opened");
    assert!(g.caveland.oven((10, 41, 1)).unwrap().is_burning());
}

// ---- keys -------------------------------------------------------------------------------------

#[test]
fn walking_still_works_while_content_is_around() {
    let mut g = Game::new();
    let player = g.player_at(10, 40);
    g.step(3);
    let before = g.position(player);
    for _ in 0..30 {
        g.caveland.set_controls(&mut g.entities, &g.world, player, Controls { up: true, ..Default::default() });
        g.step(1);
    }
    assert!(g.position(player).distance(before) > 0.5);
}
