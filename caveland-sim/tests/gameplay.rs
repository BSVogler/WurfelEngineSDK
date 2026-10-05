//! The Caveland rules played through the public API, the way a server would.

use caveland_sim::blocks::ids;
use caveland_sim::collectible::{CollectibleType as C, Item};
use caveland_sim::crafting::RecipeResult;
use caveland_sim::game::{cell_center, cell_floor};
use caveland_sim::player::{Action, Controls, DROP_PICKUP_BLOCK};
use caveland_sim::{Caveland, EntityKind, GameEvent, Team, Tuning};
use glam::Vec3;
use wurfel_sim::block::Block;
use wurfel_sim::entity::physics::is_on_ground;
use wurfel_sim::entity::{Entities, EntityId};
use wurfel_sim::grid::{from_iso, to_iso};
use wurfel_sim::{AirGenerator, World};

const DT: f32 = 1.0 / 60.0;

struct Game {
    world: World,
    entities: Entities,
    caveland: Caveland,
    events: Vec<GameEvent>,
}

impl Game {
    /// A sand floor at z = 0 around block (10, 40).
    fn new() -> Game {
        Game::with_floor_at(40)
    }

    fn with_floor_at(y_mid: i32) -> Game {
        let mut world = World::new(AirGenerator);
        Caveland::install(&mut world);
        for x in -10..40 {
            for y in y_mid - 40..y_mid + 40 {
                world.set(x, y, 0, Block::new(ids::SAND, 0));
            }
        }
        Game { world, entities: Entities::new(), caveland: Caveland::new(Tuning::default(), 1), events: Vec::new() }
    }

    fn spawn_player_at(&mut self, x: i32, y: i32) -> EntityId {
        let (gx, gy) = to_iso(x, y);
        self.caveland.spawn_player(&mut self.entities, 0, Vec3::new(gx, gy, 1.0))
    }

    fn step(&mut self, steps: usize) {
        for _ in 0..steps {
            let events = self.caveland.tick(&mut self.entities, &mut self.world, DT);
            self.events.extend(events);
        }
    }

    fn seconds(&mut self, s: f32) {
        self.step((s * 60.0).round() as usize);
    }

    fn give(&mut self, player: EntityId, kind: C) {
        assert!(self.caveland.player_mut(player).unwrap().inventory.add(Item::new(kind)));
    }

    fn act(&mut self, player: EntityId, action: Action) {
        self.caveland.act(&mut self.entities, &mut self.world, player, action);
        self.events.extend(self.caveland.drain_events());
    }

    fn controls(&mut self, player: EntityId, c: Controls) {
        self.caveland.set_controls(&mut self.entities, &self.world, player, c);
        self.events.extend(self.caveland.drain_events());
    }

    fn inventory_types(&self, player: EntityId) -> Vec<C> {
        self.caveland.player(player).unwrap().inventory.items().iter().map(|i| i.kind).collect()
    }

    fn health(&self, id: EntityId) -> f32 {
        self.entities.get(id).unwrap().health()
    }

    fn position(&self, id: EntityId) -> Vec3 {
        self.entities.get(id).unwrap().position
    }

    fn saw(&self, f: impl Fn(&GameEvent) -> bool) -> bool {
        self.events.iter().any(f)
    }

    fn collectibles(&self, kind: C) -> usize {
        self.entities.iter().filter(|e| self.caveland.kind_of(e.id()) == Some(EntityKind::Collectible(kind))).count()
    }
}

#[test]
fn the_player_is_ejira_and_stands_on_the_floor() {
    let mut g = Game::new();
    let id = g.spawn_player_at(10, 40);
    g.seconds(1.0);
    assert_eq!(g.entities.get(id).unwrap().name, "Ejira");
    assert_eq!(g.caveland.kind_of(id), Some(EntityKind::Player { number: 0 }));
    assert_eq!(g.caveland.team_of(id), Team::Player);
    assert_eq!(g.health(id), 100.0);
    assert!((g.position(id).z - 1.0).abs() < 1e-3, "{:?}", g.position(id));
}

#[test]
fn caveland_blocks_follow_caveland_rules_not_the_engines() {
    let mut g = Game::new();
    // A torch is not an obstacle in Caveland (the engine would treat every id but air and water as
    // solid), an oven is.
    g.world.set(10, 40, 1, Block::new(ids::TORCH, 0));
    g.world.set(11, 40, 1, Block::new(ids::OVEN, 0));
    let torch = cell_center((10, 40, 1));
    let oven = cell_center((11, 40, 1));
    assert!(!g.world.blocks().is_obstacle(g.world.get(10, 40, 1)));
    assert!(g.world.blocks().is_obstacle(g.world.get(11, 40, 1)));
    // Standing on a torch cell: the player falls through it to the floor, an oven holds them.
    let id = g.caveland.spawn_player(&mut g.entities, 0, torch + Vec3::Z * 0.5);
    g.seconds(1.0);
    assert!((g.position(id).z - 1.0).abs() < 0.1, "fell to the floor, got {:?}", g.position(id));
    let id2 = g.caveland.spawn_player(&mut g.entities, 1, oven + Vec3::new(0.0, 0.0, 0.5));
    g.seconds(1.0);
    assert!(g.position(id2).z >= 2.0 - 1e-3, "stands on the oven, got {:?}", g.position(id2));
}

#[test]
fn jumping_from_the_ground_uses_the_caveland_jump_speed() {
    let mut g = Game::new();
    let id = g.spawn_player_at(10, 40);
    g.seconds(0.5);
    g.controls(id, Controls { jump: true, ..Default::default() });
    let vz = g.entities.get(id).unwrap().body.as_ref().unwrap().movement.z;
    assert!((vz - 4.7).abs() < 1e-4, "{vz}");
    g.step(1);
    assert!(g.position(id).z > 1.0);
    // Holding the key does not jump again.
    g.controls(id, Controls { jump: true, ..Default::default() });
    g.seconds(1.0);
    assert!(is_on_ground(&g.world, g.position(id), 1.4));
}

#[test]
fn the_jetpack_burns_while_the_key_is_held_in_the_air_and_refills_on_landing() {
    let mut g = Game::new();
    let id = g.caveland.spawn_player(&mut g.entities, 0, cell_floor((10, 40, 6)));
    g.step(1);
    let falling = g.entities.get(id).unwrap().body.as_ref().unwrap().movement.z;
    g.controls(id, Controls { jump: true, ..Default::default() });
    assert!(g.caveland.player(id).unwrap().jetpack_on, "the jump key in the air lights the jetpack");
    g.seconds(0.2);
    let rising = g.entities.get(id).unwrap().body.as_ref().unwrap().movement.z;
    assert!(rising > falling + 2.0, "thrust should beat gravity: {falling} -> {rising}");
    assert!(rising <= 5.0 + 0.6, "capped near jetpackMaxSpeed, got {rising}");
    g.seconds(0.4);
    assert!(!g.caveland.player(id).unwrap().jetpack_on, "out of fuel after 0.4 s");
    g.controls(id, Controls::default());
    g.seconds(3.0);
    assert!(is_on_ground(&g.world, g.position(id), 1.4));
    assert!((g.caveland.player(id).unwrap().jetpack_time - 0.4).abs() < 1e-4, "refilled on the ground");
}

#[test]
fn releasing_the_jump_key_stops_the_jetpack() {
    let mut g = Game::new();
    let id = g.caveland.spawn_player(&mut g.entities, 0, cell_floor((10, 40, 6)));
    g.controls(id, Controls { jump: true, ..Default::default() });
    assert!(g.caveland.player(id).unwrap().jetpack_on);
    g.step(3);
    g.controls(id, Controls::default());
    assert!(!g.caveland.player(id).unwrap().jetpack_on);
}

#[test]
fn walking_over_a_collectible_picks_it_up() {
    let mut g = Game::new();
    let id = g.spawn_player_at(10, 40);
    let (gx, gy) = to_iso(10, 40);
    let coal = g.caveland.spawn_collectible(&mut g.entities, Item::new(C::Coal), Vec3::new(gx + 0.1, gy, 1.0));
    g.seconds(0.2);
    assert_eq!(g.inventory_types(id), vec![C::Coal]);
    assert!(g.entities.get(coal).is_none(), "the world entity is gone, it is in the pack now");
    assert!(g.saw(|e| matches!(e, GameEvent::ItemPicked { kind: C::Coal, .. })));
    assert!(g.saw(|e| matches!(e, GameEvent::Sound { name: "collect", .. })));
}

#[test]
fn a_full_pack_leaves_items_lying() {
    let mut g = Game::new();
    let id = g.spawn_player_at(10, 40);
    for _ in 0..3 {
        g.give(id, C::Stone);
    }
    let (gx, gy) = to_iso(10, 40);
    g.caveland.spawn_collectible(&mut g.entities, Item::new(C::Coal), Vec3::new(gx, gy, 1.0));
    g.seconds(0.5);
    assert_eq!(g.inventory_types(id), vec![C::Stone; 3]);
    assert_eq!(g.collectibles(C::Coal), 1);
}

#[test]
fn a_dropped_item_cannot_be_picked_up_again_right_away() {
    let mut g = Game::new();
    let id = g.spawn_player_at(10, 40);
    g.give(id, C::Wood);
    g.act(id, Action::Drop);
    assert!(g.inventory_types(id).is_empty());
    assert_eq!(g.collectibles(C::Wood), 1);
    g.seconds(DROP_PICKUP_BLOCK - 0.2);
    assert!(g.inventory_types(id).is_empty(), "still blocked for its owner");
    g.seconds(0.6);
    assert_eq!(g.inventory_types(id), vec![C::Wood], "after the block it is picked up again");
}

#[test]
fn another_player_may_take_a_dropped_item_immediately() {
    let mut g = Game::new();
    let a = g.spawn_player_at(10, 40);
    let (gx, gy) = to_iso(10, 40);
    let b = g.caveland.spawn_player(&mut g.entities, 1, Vec3::new(gx + 0.5, gy, 1.0));
    g.give(a, C::Wood);
    g.act(a, Action::Drop);
    g.seconds(0.3);
    assert!(g.inventory_types(a).is_empty());
    assert_eq!(g.inventory_types(b), vec![C::Wood]);
}

#[test]
fn throwing_needs_the_pose_and_launches_the_item() {
    let mut g = Game::new();
    let id = g.spawn_player_at(10, 40);
    g.give(id, C::Stone);
    g.act(id, Action::Throw);
    assert_eq!(g.inventory_types(id), vec![C::Stone], "no throw without preparing");
    assert!(g.saw(|e| matches!(e, GameEvent::Sound { name: "interactionFail", .. })));

    g.act(id, Action::PrepareThrow);
    assert!(g.caveland.player(id).unwrap().movement_locked());
    g.act(id, Action::Throw);
    assert!(g.inventory_types(id).is_empty());
    assert!(!g.caveland.player(id).unwrap().prepare_throw);
    let thrown = g
        .entities
        .iter()
        .find(|e| g.caveland.kind_of(e.id()) == Some(EntityKind::Collectible(C::Stone)))
        .expect("a stone flies");
    assert!(thrown.body.as_ref().unwrap().speed() > 2.0);
}

#[test]
fn a_prepared_throw_stops_walking() {
    let mut g = Game::new();
    let id = g.spawn_player_at(10, 40);
    g.give(id, C::Stone);
    g.act(id, Action::PrepareThrow);
    g.controls(id, Controls { right: true, ..Default::default() });
    g.seconds(0.3);
    assert!(g.entities.get(id).unwrap().body.as_ref().unwrap().speed_hor() < 0.1);
}

#[test]
fn torches_are_placed_where_the_player_stands() {
    let mut g = Game::new();
    let id = g.spawn_player_at(10, 40);
    g.give(id, C::Torch);
    g.seconds(0.2);
    let cell = {
        let p = g.position(id);
        let (x, y) = from_iso(p.x, p.y);
        (x, y, p.z.floor() as i32)
    };
    g.act(id, Action::UseItem);
    assert_eq!(g.world.get(cell.0, cell.1, cell.2).id(), ids::TORCH);
    assert!(g.inventory_types(id).is_empty(), "used up");
}

#[test]
fn a_torch_is_not_placed_inside_a_wall() {
    let mut g = Game::new();
    let id = g.spawn_player_at(10, 40);
    g.seconds(0.2);
    let p = g.position(id);
    let (x, y) = from_iso(p.x, p.y);
    g.world.set(x, y, 1, Block::new(ids::INDESTRUCTIBLE_OBSTACLE, 0));
    g.give(id, C::Torch);
    g.act(id, Action::UseItem);
    assert_eq!(g.world.get(x, y, 1).id(), ids::INDESTRUCTIBLE_OBSTACLE);
    assert_eq!(g.inventory_types(id), vec![C::Torch], "keeps the torch");
}

#[test]
fn a_lit_explosive_goes_off_after_two_seconds_and_hurts() {
    let mut g = Game::new();
    let id = g.spawn_player_at(10, 40);
    g.give(id, C::Explosives);
    g.act(id, Action::UseItem);
    assert!(g.caveland.player(id).unwrap().inventory.front().unwrap().is_lit());
    g.seconds(1.5);
    assert!(!g.saw(|e| matches!(e, GameEvent::Explosion { .. })));
    g.seconds(0.7);
    assert!(g.saw(|e| matches!(e, GameEvent::Explosion { radius: 3, damage: 150, .. })));
    // 150 damage at the centre: lighting one in your pocket is fatal.
    assert!(g.saw(|e| matches!(e, GameEvent::PlayerDied { player } if *player == id)));
}

#[test]
fn explosions_dig_but_cannot_break_the_indestructible() {
    let mut g = Game::new();
    for x in 8..13 {
        g.world.set(x, 40, 1, Block::new(ids::DIRT, 0));
    }
    g.world.set(10, 41, 1, Block::new(ids::INDESTRUCTIBLE_OBSTACLE, 0));
    g.world.set(10, 50, 1, Block::new(ids::DIRT, 0)); // five blocks away
    let center = cell_center((10, 40, 1));
    g.caveland.explode(&mut g.world, &mut g.entities, center, 3, 150);
    assert_eq!(g.world.get(10, 40, 1).id(), ids::AIR, "the centre is gone");
    assert_eq!(g.world.get(10, 41, 1).id(), ids::INDESTRUCTIBLE_OBSTACLE);
    assert_eq!(g.world.get(10, 50, 1).id(), ids::DIRT, "outside the radius nothing happens");
    let events = g.caveland.drain_events();
    assert!(events.iter().any(|e| matches!(e, GameEvent::Explosion { radius: 3, .. })));
    assert!(events.iter().any(|e| matches!(e, GameEvent::BlockDestroyed { id: ids::DIRT, .. })));
}

#[test]
fn damage_to_a_block_is_gradual_and_destroying_it_drops_loot() {
    let mut g = Game::new();
    g.world.set(10, 40, 1, Block::new(ids::COAL, 0));
    let cell = (10, 40, 1);
    assert!(g.caveland.damage_block(&mut g.world, &mut g.entities, cell, 40));
    assert_eq!(g.world.get(10, 40, 1).id(), ids::COAL);
    assert_eq!(g.world.block_health(10, 40, 1), 60);
    assert!(g.caveland.damage_block(&mut g.world, &mut g.entities, cell, 40));
    assert_eq!(g.world.block_health(10, 40, 1), 20);
    assert!(g.caveland.damage_block(&mut g.world, &mut g.entities, cell, 40));
    assert_eq!(g.world.get(10, 40, 1).id(), ids::AIR);
    assert_eq!(g.collectibles(C::Coal), 1, "coal drops coal");
    let events = g.caveland.drain_events();
    assert!(events.iter().any(|e| matches!(e, GameEvent::BlockDestroyed { id: ids::COAL, .. })));
    // Air cannot be damaged.
    assert!(!g.caveland.damage_block(&mut g.world, &mut g.entities, cell, 40));
}

#[test]
fn an_indestructible_block_takes_hits_without_breaking() {
    let mut g = Game::new();
    g.world.set(10, 40, 1, Block::new(ids::INDESTRUCTIBLE_OBSTACLE, 0));
    for _ in 0..5 {
        assert!(g.caveland.damage_block(&mut g.world, &mut g.entities, (10, 40, 1), 100));
    }
    assert_eq!(g.world.get(10, 40, 1).id(), ids::INDESTRUCTIBLE_OBSTACLE);
}

#[test]
fn a_tree_falls_in_two_halves_and_drops_wood_for_each() {
    let mut g = Game::new();
    g.world.set(10, 40, 1, Block::new(ids::TREE, 0));
    g.world.set(10, 40, 2, Block::new(ids::TREE, 8)); // top half
    g.caveland.damage_block(&mut g.world, &mut g.entities, (10, 40, 2), 100);
    assert_eq!(g.world.get(10, 40, 2).id(), ids::AIR);
    assert_eq!(g.world.get(10, 40, 1).id(), ids::AIR, "the trunk goes with the crown");
    assert_eq!(g.collectibles(C::Wood), 2);
}

#[test]
fn a_swing_digs_into_the_blocks_in_front() {
    let mut g = Game::new();
    // A ring of coal around the player's cell: whichever way the character faces there is coal.
    for dx in -2..=2 {
        for dy in -4..=4 {
            if (dx, dy) != (0, 0) {
                g.world.set(10 + dx, 40 + dy, 1, Block::new(ids::COAL, 0));
            }
        }
    }
    let (gx, gy) = to_iso(10, 40);
    let id = g.caveland.spawn_player(&mut g.entities, 0, Vec3::new(gx + 0.15, gy - 0.15, 1.0));
    g.seconds(0.3);
    g.events.clear();
    g.act(id, Action::Attack);
    g.act(id, Action::ReleaseAttack);
    assert!(g.saw(|e| matches!(e, GameEvent::Sound { name: "sword", .. })));
    g.seconds(0.4);
    assert!(
        g.saw(|e| matches!(e, GameEvent::BlockDamaged { id: ids::COAL, .. } | GameEvent::BlockDestroyed { id: ids::COAL, .. })),
        "events: {:?}",
        g.events
    );
}

#[test]
fn a_swing_at_hard_rock_only_makes_dust() {
    let mut g = Game::new();
    for dx in -2..=2 {
        for dy in -4..=4 {
            if (dx, dy) != (0, 0) {
                g.world.set(10 + dx, 40 + dy, 1, Block::new(ids::STONE, 0));
            }
        }
    }
    let (gx, gy) = to_iso(10, 40);
    let id = g.caveland.spawn_player(&mut g.entities, 0, Vec3::new(gx + 0.15, gy - 0.15, 1.0));
    g.seconds(0.3);
    g.act(id, Action::Attack);
    g.act(id, Action::ReleaseAttack);
    g.seconds(0.4);
    assert!(g.saw(|e| matches!(e, GameEvent::HardHit { .. })), "events: {:?}", g.events);
    assert!(!g.saw(|e| matches!(e, GameEvent::BlockDestroyed { .. })));
}

#[test]
fn a_second_swing_is_ignored_while_the_first_is_under_way() {
    let mut g = Game::new();
    let id = g.spawn_player_at(10, 40);
    g.seconds(0.2);
    g.act(id, Action::Attack);
    let before = g.caveland.player(id).unwrap().time_till_impact;
    g.step(3);
    g.act(id, Action::Attack);
    let after = g.caveland.player(id).unwrap().time_till_impact;
    assert!(after < before, "the countdown was not restarted");
}

#[test]
fn a_swing_hurts_and_shoves_an_enemy_robot() {
    let mut g = Game::new();
    let id = g.spawn_player_at(10, 40);
    g.seconds(0.3);
    let me = g.position(id);
    let aim = g.caveland.player(id).unwrap().aim.extend(0.0);
    // The robot stands right where the swing lands.
    let robot = g.caveland.spawn_robot(&mut g.entities, Team::Robots, me + aim * 1.0);
    g.act(id, Action::Attack);
    g.act(id, Action::ReleaseAttack);
    g.seconds(0.3);
    assert!(g.entities.get(robot).map_or(true, |r| r.health() < 100.0), "robot untouched");
}

#[test]
fn holding_attack_charges_a_power_attack_that_dashes() {
    let mut g = Game::new();
    let id = g.spawn_player_at(10, 40);
    g.seconds(0.3);
    g.act(id, Action::Attack);
    g.seconds(1.2);
    // Held past the full charge time the power attack fires by itself.
    assert!(g.saw(|e| matches!(e, GameEvent::Sound { name: "release", .. })), "events: {:?}", g.events);
}

#[test]
fn a_short_press_does_not_release_a_power_attack() {
    let mut g = Game::new();
    let id = g.spawn_player_at(10, 40);
    g.act(id, Action::Attack);
    g.seconds(0.1);
    g.act(id, Action::ReleaseAttack);
    g.seconds(0.5);
    assert!(!g.saw(|e| matches!(e, GameEvent::Sound { name: "release", .. })));
    assert!(g.caveland.player(id).unwrap().load_attack.is_none());
}

#[test]
fn crafting_uses_up_ingredients() {
    let mut g = Game::new();
    let id = g.spawn_player_at(10, 40);
    g.give(id, C::Stone);
    g.give(id, C::Wood);
    g.give(id, C::Coal);
    // The torch is the only one that can be made; the index is the fixed list's.
    let torch = caveland_sim::crafting::recipes().iter().position(|r| r.name == "Torch").unwrap();
    g.act(id, Action::Craft(torch));
    assert_eq!(g.inventory_types(id), vec![C::Stone, C::Torch]);
    assert!(g.saw(|e| matches!(e, GameEvent::Crafted { result: RecipeResult::Item(C::Torch), .. })));
}

#[test]
fn crafting_a_minecart_puts_one_in_the_world() {
    let mut g = Game::new();
    let id = g.spawn_player_at(10, 40);
    g.give(id, C::Iron);
    g.give(id, C::Iron);
    g.give(id, C::Wood);
    let index = caveland_sim::crafting::recipes().iter().position(|r| r.result == RecipeResult::MineCart).unwrap();
    g.act(id, Action::Craft(index));
    assert!(g.inventory_types(id).is_empty());
    assert!(g.entities.iter().any(|e| g.caveland.kind_of(e.id()) == Some(EntityKind::MineCart)));
}

#[test]
fn crafting_without_the_ingredients_fails_quietly() {
    let mut g = Game::new();
    let id = g.spawn_player_at(10, 40);
    g.give(id, C::Stone);
    g.act(id, Action::Craft(0));
    assert_eq!(g.inventory_types(id), vec![C::Stone]);
    assert!(!g.saw(|e| matches!(e, GameEvent::Crafted { .. })));
    assert!(g.saw(|e| matches!(e, GameEvent::Sound { name: "interactionFail", .. })), "crafting without ingredients fails audibly");
    g.act(id, Action::Craft(999)); // out of range: ignored
    assert_eq!(g.inventory_types(id), vec![C::Stone]);
}

#[test]
fn an_oven_smelts_iron_and_the_bar_pops_out() {
    let mut g = Game::new();
    g.world.set(11, 40, 1, Block::new(ids::OVEN, 0));
    let (gx, gy) = to_iso(10, 40);
    let id = g.caveland.spawn_player(&mut g.entities, 0, Vec3::new(gx, gy, 1.0));
    g.give(id, C::Coal);
    g.give(id, C::Ironore);
    g.seconds(0.2);
    // The item in hand goes in first: the coal; then the ore.
    g.act(id, Action::Interact);
    g.act(id, Action::Interact);
    assert!(g.inventory_types(id).is_empty());
    let oven = g.caveland.oven((11, 40, 1)).expect("the oven is in use");
    assert!(oven.is_burning());
    g.seconds(4.0);
    assert!(g.saw(|e| matches!(e, GameEvent::OvenProduced { kind: C::Iron, .. })), "events: {:?}", g.events);
    assert!(g.collectibles(C::Iron) + g.caveland.player(id).unwrap().inventory.count(C::Iron) >= 1);
}

#[test]
fn interacting_far_from_any_machine_does_nothing() {
    let mut g = Game::new();
    g.world.set(30, 40, 1, Block::new(ids::OVEN, 0));
    let id = g.spawn_player_at(10, 40);
    g.give(id, C::Coal);
    g.act(id, Action::Interact);
    assert_eq!(g.inventory_types(id), vec![C::Coal]);
    assert!(g.caveland.oven((30, 40, 1)).is_none());
}

#[test]
fn breaking_an_oven_returns_what_was_inside() {
    let mut g = Game::new();
    g.world.set(11, 40, 1, Block::new(ids::OVEN, 0));
    let (gx, gy) = to_iso(10, 40);
    let id = g.caveland.spawn_player(&mut g.entities, 0, Vec3::new(gx, gy, 1.0));
    g.give(id, C::Ironore);
    g.act(id, Action::Interact);
    assert!(g.caveland.oven((11, 40, 1)).is_some());
    g.world.set(11, 40, 1, Block::AIR);
    g.seconds(0.1);
    assert!(g.caveland.oven((11, 40, 1)).is_none());
    assert_eq!(g.collectibles(C::Ironore), 1);
}

#[test]
fn an_evil_robot_hunts_the_player_and_hurts() {
    let mut g = Game::new();
    let id = g.spawn_player_at(10, 40);
    let (gx, gy) = to_iso(10, 40);
    g.caveland.spawn_robot(&mut g.entities, Team::Robots, Vec3::new(gx + 3.0, gy, 1.0));
    g.seconds(8.0);
    assert!(g.health(id) < 100.0, "the robot never attacked");
    assert!(g.saw(|e| matches!(e, GameEvent::PlayerDamaged { .. })));
}

#[test]
fn godmode_makes_the_player_immune() {
    let mut g = Game::new();
    g.caveland.tuning.godmode = true;
    let id = g.spawn_player_at(10, 40);
    g.caveland.damage_entity(&mut g.entities, id, 60.0);
    assert_eq!(g.health(id), 100.0);
}

#[test]
fn damage_delays_health_regeneration() {
    let mut g = Game::new();
    let id = g.spawn_player_at(10, 40);
    g.caveland.damage_entity(&mut g.entities, id, 60.0);
    assert_eq!(g.health(id), 40.0);
    g.seconds(3.5);
    assert_eq!(g.health(id), 40.0, "no healing within four seconds of the hit");
    g.seconds(1.5);
    assert!(g.health(id) > 40.0);
    g.seconds(3.0);
    assert_eq!(g.health(id), 100.0);
}

#[test]
fn friendly_robots_and_neutral_robots_leave_the_player_alone() {
    let mut g = Game::new();
    let id = g.spawn_player_at(10, 40);
    let (gx, gy) = to_iso(10, 40);
    g.caveland.spawn_robot(&mut g.entities, Team::Player, Vec3::new(gx + 1.5, gy, 1.0));
    g.caveland.spawn_robot(&mut g.entities, Team::Neutral, Vec3::new(gx - 1.5, gy, 1.0));
    g.seconds(6.0);
    assert_eq!(g.health(id), 100.0);
}

#[test]
fn robots_fight_each_other_across_teams() {
    let mut g = Game::new();
    let (gx, gy) = to_iso(10, 40);
    let evil = g.caveland.spawn_robot(&mut g.entities, Team::Robots, Vec3::new(gx, gy, 1.0));
    let good = g.caveland.spawn_robot(&mut g.entities, Team::Player, Vec3::new(gx + 1.2, gy, 1.0));
    g.seconds(6.0);
    assert!(g.health(evil) < 100.0 && g.health(good) < 100.0);
}

#[test]
fn a_destroyed_enemy_robot_drops_money_which_the_player_collects() {
    let mut g = Game::new();
    let id = g.spawn_player_at(10, 40);
    g.seconds(0.2);
    let me = g.position(id);
    let robot = g.caveland.spawn_robot(&mut g.entities, Team::Robots, me + Vec3::new(0.0, 0.0, 0.0));
    g.entities.get_mut(robot).unwrap().set_health(0.0);
    g.seconds(0.5);
    assert!(g.saw(|e| matches!(e, GameEvent::RobotDestroyed { .. })));
    assert!(g.caveland.money() >= 1, "money was not picked up");
    assert!(g.saw(|e| matches!(e, GameEvent::MoneyPicked { .. })));
}

#[test]
fn a_friendly_robot_drops_no_money() {
    let mut g = Game::new();
    let (gx, gy) = to_iso(10, 40);
    let robot = g.caveland.spawn_robot(&mut g.entities, Team::Player, Vec3::new(gx, gy, 1.0));
    g.entities.get_mut(robot).unwrap().set_health(0.0);
    g.seconds(0.2);
    assert!(g.saw(|e| matches!(e, GameEvent::RobotDestroyed { .. })));
    assert!(!g.entities.iter().any(|e| g.caveland.kind_of(e.id()) == Some(EntityKind::Money)));
}

#[test]
fn a_dead_player_is_reported_and_forgotten() {
    let mut g = Game::new();
    let id = g.spawn_player_at(10, 40);
    g.entities.get_mut(id).unwrap().set_health(0.0);
    g.step(2);
    assert!(g.saw(|e| matches!(e, GameEvent::PlayerDied { player } if *player == id)));
    assert!(g.caveland.player(id).is_none());
    assert!(g.entities.get(id).is_none());
}

#[test]
fn the_barrier_block_walls_the_player_in_at_every_height() {
    let mut g = Game::new();
    for y in 30..50 {
        g.world.set(14, y, 8, Block::new(ids::INDESTRUCTIBLE_OBSTACLE, 0));
    }
    let id = g.spawn_player_at(10, 40);
    for _ in 0..300 {
        g.controls(id, Controls { right: true, ..Default::default() });
        g.step(1);
    }
    // Screen-right is +x - y in the ground frame: the player ends up against the wall column.
    let p = g.position(id);
    let (x, _) = from_iso(p.x, p.y);
    assert!(x < 14, "walked through the wall: {p:?}");
}

#[test]
fn caves_have_a_ceiling() {
    let mut g = Game::with_floor_at(1100);
    let (gx, gy) = to_iso(10, 1100);
    let id = g.caveland.spawn_player(&mut g.entities, 0, Vec3::new(gx, gy, 9.8));
    g.step(1);
    assert!(g.position(id).z <= 9.0 + 1e-3, "{:?}", g.position(id));
    // On the surface the world is not capped by this rule.
    let mut s = Game::new();
    let (gx, gy) = to_iso(10, 40);
    let id = s.caveland.spawn_player(&mut s.entities, 0, Vec3::new(gx, gy, 9.8));
    s.step(1);
    assert!(s.position(id).z > 9.0);
}

#[test]
fn collectibles_do_not_fall_through_caveland_floors() {
    let mut g = Game::new();
    let (gx, gy) = to_iso(20, 40);
    let c = g.caveland.spawn_collectible(&mut g.entities, Item::new(C::Coal), Vec3::new(gx, gy, 5.0));
    g.seconds(2.0);
    assert!((g.position(c).z - 1.0).abs() < 1e-3);
}

#[test]
fn the_same_inputs_give_the_same_world() {
    let run = || {
        let mut g = Game::new();
        let id = g.spawn_player_at(10, 40);
        let (gx, gy) = to_iso(10, 40);
        g.caveland.spawn_robot(&mut g.entities, Team::Robots, Vec3::new(gx + 3.0, gy, 1.0));
        g.caveland.spawn_robot(&mut g.entities, Team::Neutral, Vec3::new(gx - 3.0, gy, 1.0));
        for i in 0..300 {
            g.controls(id, Controls { right: i % 100 < 50, jump: i % 70 == 0, ..Default::default() });
            g.step(1);
        }
        g.entities.iter().map(|e| (e.id(), e.position, e.health())).collect::<Vec<_>>()
    };
    assert_eq!(run(), run());
}

#[test]
fn a_player_can_come_back_under_the_same_id_with_a_fresh_pack() {
    let mut g = Game::new();
    let id = g.spawn_player_at(10, 40);
    g.give(id, C::Torch);
    g.caveland.damage_entity(&mut g.entities, id, 100.0);
    g.step(2);
    assert!(g.entities.get(id).is_none(), "dead players are removed");
    assert!(g.saw(|e| matches!(e, GameEvent::PlayerDied { player } if *player == id)));

    let (gx, gy) = to_iso(12, 40);
    let again = g.caveland.spawn_player_as(&mut g.entities, id, 0, Vec3::new(gx, gy, 1.0));
    assert_eq!(again, Some(id));
    assert!(g.inventory_types(id).is_empty());
    assert_eq!(g.health(id), 100.0);
    assert_eq!(g.caveland.spawn_player_as(&mut g.entities, id, 0, Vec3::ZERO), None, "the id is taken now");
}

#[test]
fn things_lists_everything_but_players_and_the_view_shows_the_pack() {
    let mut g = Game::new();
    let id = g.spawn_player_at(10, 40);
    let torch = g.caveland.spawn_collectible(&mut g.entities, Item::new(C::Torch), cell_floor((14, 40, 1)));
    let money = g.caveland.spawn_money(&mut g.entities, cell_floor((16, 40, 1)));
    let robot = g.caveland.spawn_robot(&mut g.entities, Team::Robots, cell_floor((18, 40, 1)));
    let names: Vec<(EntityId, String)> = g.caveland.things().iter().map(|(i, k)| (*i, k.name())).collect();
    assert_eq!(names, vec![(torch, "Torch".to_string()), (money, "money".to_string()), (robot, "robot".to_string())]);

    g.give(id, C::Wood);
    g.give(id, C::Coal);
    let view = g.caveland.player_view(&g.entities, id).unwrap();
    assert_eq!(view.health, 100.0);
    assert_eq!(view.jetpack, 1.0);
    assert_eq!(view.items, vec!["Wood", "Coal"]);
    let all = caveland_sim::crafting::recipes();
    assert_eq!(view.recipes.len(), all.len());
    assert!(view.recipes.iter().zip(&all).all(|(v, r)| v.name == r.name), "the fixed order of the recipe list");
    let torch_recipe = view.recipes.iter().find(|r| r.name == "Torch").unwrap();
    assert!(torch_recipe.can_craft);
    assert_eq!(torch_recipe.ingredients, vec!["Wood", "Coal"]);
    assert_eq!(view.recipes.iter().filter(|r| r.can_craft).count(), 1);
    assert!(g.caveland.player_view(&g.entities, torch).is_none(), "only players have a view");
}

#[test]
fn the_engine_events_of_the_last_tick_stay_available_for_clients() {
    use wurfel_sim::entity::Event;
    let mut g = Game::new();
    let (gx, gy) = to_iso(10, 40);
    let id = g.caveland.spawn_player(&mut g.entities, 0, Vec3::new(gx, gy, 3.0)); // falls onto the floor
    let mut landed = false;
    for _ in 0..120 {
        g.step(1);
        landed |= g.caveland.engine_events().iter().any(|e| matches!(e, Event::Landed(who) if *who == id));
    }
    assert!(landed, "the fall ended with a Landed event");
    assert!(g.caveland.engine_events().is_empty(), "each tick replaces the list: a quiet tick has none");
}

#[test]
fn a_key_assumed_held_is_not_a_fresh_press() {
    let mut g = Game::new();
    let id = g.spawn_player_at(10, 40);
    g.step(2);
    let hold = Controls { jump: true, ..Default::default() };
    g.caveland.assume_held(id, hold);
    g.controls(id, hold);
    g.step(10);
    assert!(g.position(id).z <= 1.0 + 1e-3, "no jump from a key that was already down");
    g.controls(id, Controls::default());
    g.controls(id, hold);
    g.step(3);
    assert!(g.position(id).z > 1.0, "a real press jumps");
}
