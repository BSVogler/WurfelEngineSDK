//! The catapult and the cannon: aiming, gunpowder, flights, shells.

use caveland_sim::blocks::ids;
use caveland_sim::collectible::{CollectibleType as C, Item};
use caveland_sim::game::Cell;
use caveland_sim::launcher::{simulate_flight, Launcher, CANNON_CAPACITY, CATAPULT_RELOAD};
use caveland_sim::player::{Action, Controls};
use caveland_sim::{Caveland, EntityKind, ExtraEvent, GameEvent, Tuning};
use glam::Vec3;
use wurfel_sim::entity::{Entities, EntityId};
use wurfel_sim::grid::to_iso;

const MACHINE: Cell = (10, 40, 1);

mod common;
use common::Game;

impl Game {
    /// A game with a machine at [`MACHINE`] and a player one block away.
    fn with(block: u8) -> (Game, EntityId) {
        let mut g = Game::new();
        assert!(g.caveland.place_machine(&mut g.world, MACHINE, block));
        let player = g.player_at(10, 42);
        g.step(2);
        (g, player)
    }

    /// Open the machine's menu and pick an option (0 launch me, 1 launch item, 2 shell, 3 load).
    fn menu(&mut self, player: EntityId, option: u8) {
        if self.caveland.open_dialog(player).is_none() {
            self.act(player, Action::Interact);
        }
        self.act(player, Action::Choose(option));
    }

    fn velocity(&self, id: EntityId) -> Vec3 {
        self.entities.get(id).unwrap().body.as_ref().unwrap().movement
    }

    fn health(&self, id: EntityId) -> f32 {
        self.entities.get(id).map_or(0.0, |e| e.health())
    }

    fn dialog_text(&self, player: EntityId) -> String {
        self.caveland.open_dialog(player).map(|d| d.text.clone()).unwrap_or_default()
    }

    /// Back to where the player started, next to the machine.
    fn walk_back(&mut self, player: EntityId) {
        let (gx, gy) = to_iso(10, 42);
        let e = self.entities.get_mut(player).unwrap();
        e.position = Vec3::new(gx, gy, 1.0);
        e.body.as_mut().unwrap().movement = Vec3::ZERO;
    }

    fn load_and_fire_me(&mut self, player: EntityId) {
        self.give(player, C::Gunpowder);
        self.menu(player, 3);
        self.menu(player, 0);
    }
}

#[test]
fn the_cannon_refuses_to_fire_without_gunpowder() {
    let (mut g, player) = Game::with(ids::CANNON);
    g.menu(player, 0);
    assert!(g.dialog_text(player).contains("no gunpowder"), "{}", g.dialog_text(player));
    g.step(2);
    assert_eq!(g.velocity(player).z, 0.0, "nobody was thrown");
    assert!(!g.extra.iter().any(|e| matches!(e, ExtraEvent::Launched { .. })));
    // The same for an item, and for a shell.
    g.give(player, C::Explosives);
    g.menu(player, 1);
    g.menu(player, 2);
    assert_eq!(g.pack(player), vec![C::Explosives], "nothing was used up");
    assert!(g.caveland.things().is_empty(), "no item or shell was fired");
}

#[test]
fn loading_takes_the_gunpowder_from_the_pack_and_each_shot_burns_one() {
    let (mut g, player) = Game::with(ids::CANNON);
    g.give(player, C::Gunpowder);
    g.give(player, C::Gunpowder);
    g.menu(player, 3);
    g.menu(player, 3);
    assert!(g.pack(player).is_empty(), "both were loaded");
    assert_eq!(g.caveland.launcher(MACHINE).unwrap().loaded, 2);
    g.menu(player, 0);
    assert_eq!(g.caveland.launcher(MACHINE).unwrap().loaded, 1, "one unit per shot");
    assert!(g.extra.iter().any(|e| matches!(e, ExtraEvent::Launched { entity, .. } if *entity == player)));
    // Full barrel: the rest stays in the pack.
    for _ in 0..CANNON_CAPACITY {
        g.caveland.launcher_mut(MACHINE).unwrap().load_gunpowder();
    }
    g.give(player, C::Gunpowder);
    g.step(400); // land
    g.walk_back(player);
    g.menu(player, 3);
    assert_eq!(g.pack(player), vec![C::Gunpowder]);
    assert!(g.dialog_text(player).contains("full"));
}

#[test]
fn gunpowder_is_an_ordinary_item() {
    assert_eq!(C::from_name("Gunpowder"), Some(C::Gunpowder));
    let mut inv = caveland_sim::Inventory::new();
    assert!(inv.add(Item::new(C::Gunpowder)));
    let recipes = caveland_sim::crafting::recipes();
    let r = recipes.iter().find(|r| r.name == "Gunpowder").unwrap();
    assert_eq!(r.ingredients, vec![C::Sulfur, C::Coal, C::Coal]);
}

#[test]
fn a_launch_only_sets_the_velocity_and_the_flight_matches_the_shared_arc() {
    let (mut g, player) = Game::with(ids::CANNON);
    g.caveland.launcher_mut(MACHINE).unwrap().heading = 3;
    g.load_and_fire_me(player);
    // Straight after the launch: the muzzle's position, the aim's velocity, nothing else.
    let expected = g.caveland.launcher(MACHINE).unwrap().velocity();
    assert_eq!(g.velocity(player), expected);
    assert_eq!(g.position(player), Launcher::muzzle(MACHINE));

    let flight = simulate_flight(&g.world, g.position(player), expected);
    assert!(flight.landed);
    for (tick, point) in flight.points.iter().enumerate().skip(1) {
        g.step(1);
        assert!(g.position(player).distance(*point) < 1e-4, "tick {tick}: {} vs {point}", g.position(player));
    }
}

#[test]
fn landing_and_the_launch_never_hurt() {
    let (mut g, player) = Game::with(ids::CANNON);
    {
        let l = g.caveland.launcher_mut(MACHINE).unwrap();
        l.power = 10;
        l.elevation = 12; // the steepest, highest throw
    }
    g.load_and_fire_me(player);
    let mut high = 0.0f32;
    for _ in 0..900 {
        g.step(1);
        high = high.max(g.position(player).z);
    }
    assert!(high > 10.0, "a long fall: {high}");
    assert_eq!(g.health(player), 100.0);
    assert!(!g.events.iter().any(|e| matches!(e, GameEvent::PlayerDamaged { .. } | GameEvent::PlayerDied { .. })));
    assert_eq!(g.velocity(player).z, 0.0, "standing again");
}

#[test]
fn a_flying_player_has_less_control_the_faster_they_go() {
    let steer = |power: u8| {
        let (mut g, player) = Game::with(ids::CANNON);
        g.caveland.launcher_mut(MACHINE).unwrap().power = power;
        g.load_and_fire_me(player);
        g.step(10);
        let before = g.velocity(player);
        g.caveland.set_controls(&mut g.entities, &g.world, player, Controls { right: true, ..Default::default() });
        (g.velocity(player) - before).truncate().length()
    };
    let (slow, fast) = (steer(1), steer(10));
    assert!(fast < slow, "the faster flight changes less: {fast} vs {slow}");
}

#[test]
fn walking_on_the_ground_keeps_full_control() {
    let (mut g, player) = Game::with(ids::CATAPULT);
    g.caveland.set_controls(&mut g.entities, &g.world, player, Controls { right: true, ..Default::default() });
    let v = g.velocity(player).truncate().length();
    assert!((v - 4.0).abs() < 1e-3, "full walking speed: {v}");
}

#[test]
fn the_catapult_reloads_over_time_and_needs_nothing_loaded() {
    let (mut g, player) = Game::with(ids::CATAPULT);
    g.menu(player, 0);
    assert!(g.extra.iter().any(|e| matches!(e, ExtraEvent::Launched { .. })));
    g.step(60); // still winding up
    g.walk_back(player);
    g.extra.clear();
    g.menu(player, 0);
    assert!(g.dialog_text(player).to_lowercase().contains("reloading"),"{}", g.dialog_text(player));
    assert!(!g.extra.iter().any(|e| matches!(e, ExtraEvent::Launched { .. })), "nothing was thrown");
    g.seconds(CATAPULT_RELOAD);
    g.walk_back(player);
    g.menu(player, 0);
    assert!(g.extra.iter().any(|e| matches!(e, ExtraEvent::Launched { .. })), "thrown again after the wind-up");
}

#[test]
fn an_item_in_hand_is_thrown_along_the_arc() {
    let (mut g, player) = Game::with(ids::CATAPULT);
    g.give(player, C::Stone);
    g.menu(player, 1);
    assert!(g.pack(player).is_empty());
    let things = g.caveland.things();
    assert_eq!(things.len(), 1);
    assert_eq!(things[0].1, EntityKind::Collectible(C::Stone));
    let item = things[0].0;
    g.seconds(0.2);
    assert!(g.position(item).z > 1.5, "in the air");
    g.seconds(4.0);
    assert!(g.position(item).distance(Launcher::muzzle(MACHINE)) > 3.0, "it came down far away");
}

#[test]
fn a_shell_explodes_where_it_lands_and_spares_friends_by_default() {
    for friendly_fire in [false, true] {
        let (mut g, player) = Game::with(ids::CANNON);
        g.caveland.tuning.friendly_fire = friendly_fire;
        {
            let l = g.caveland.launcher_mut(MACHINE).unwrap();
            l.power = 3;
            l.loaded = 1;
        }
        g.give(player, C::Explosives);
        g.menu(player, 2);
        assert!(g.pack(player).is_empty(), "the shell used the explosives");
        // Stand where the shell will come down.
        let velocity = g.caveland.launcher(MACHINE).unwrap().velocity();
        let landing = *simulate_flight(&g.world, Launcher::muzzle(MACHINE), velocity).points.last().unwrap();
        g.entities.get_mut(player).unwrap().position = landing;
        g.seconds(4.0);
        assert!(g.events.iter().any(|e| matches!(e, GameEvent::Explosion { radius: 3, damage: 150, .. })), "no explosion");
        if friendly_fire {
            assert!(g.health(player) < 100.0, "friendly fire is on");
        } else {
            assert_eq!(g.health(player), 100.0, "friendly fire is off");
        }
    }
}

#[test]
fn the_aim_and_the_powder_are_saved() {
    let (mut g, _) = Game::with(ids::CANNON);
    {
        let l = g.caveland.launcher_mut(MACHINE).unwrap();
        l.heading = 9;
        l.power = 7;
        l.loaded = 3;
    }
    let json = g.caveland.save_state(&g.entities);
    let mut again = Caveland::new(Tuning::default(), 1);
    again.load_state(&Entities::new(), &json).unwrap();
    assert_eq!(again.launcher(MACHINE), g.caveland.launcher(MACHINE));
}

#[test]
fn both_are_built_at_a_construction_site() {
    for (block, items) in [(ids::CATAPULT, vec![C::Wood, C::Wood, C::Wood, C::Stone]), (ids::CANNON, vec![C::Iron, C::Iron, C::Iron, C::Wood])] {
        let mut g = Game::new();
        let (gx, gy) = to_iso(10, 40);
        let player = g.caveland.spawn_player(&mut g.entities, 0, Vec3::new(gx, gy, 1.0));
        g.give(player, C::Toolkit);
        g.act(player, Action::UseItem);
        g.act(player, Action::Choose(block));
        for kind in items {
            g.give(player, kind);
            g.act(player, Action::Interact);
            g.act(player, Action::Choose(0));
        }
        g.act(player, Action::Interact);
        g.act(player, Action::Choose(2));
        assert_eq!(g.world.get(10, 40, 1).id(), block);
        g.step(5);
        assert!(g.caveland.launcher((10, 40, 1)).is_some(), "the machine knows its aim");
    }
}
