//! Carts, lifts, portals and the spaceship, played through the public API like a server would.

use caveland_sim::blocks::ids;
use caveland_sim::collectible::{CollectibleType as C, Item};
use caveland_sim::game::cell_floor;
use caveland_sim::minecart::{BOOSTER_SPEED, BOTTOM_HEIGHT, MAX_SPEED};
use caveland_sim::player::Action;
use caveland_sim::{Caveland, EntityKind, GameEvent, Team, Tuning, TransportEvent};
use glam::{Vec2, Vec3};
use wurfel_sim::block::Block;
use wurfel_sim::entity::{Entities, EntityId};
use wurfel_sim::generator::EntitySpawn;
use wurfel_sim::grid::{from_iso, lower_right, to_iso};
use wurfel_sim::{AirGenerator, World};

const DT: f32 = 1.0 / 60.0;

struct Game {
    world: World,
    entities: Entities,
    caveland: Caveland,
    events: Vec<GameEvent>,
    notes: Vec<TransportEvent>,
}

impl Game {
    /// Stone at z = 0 for `x` in -20..60 and `y` in -20..80, and nothing else.
    fn new() -> Game {
        let mut world = World::new(AirGenerator);
        Caveland::install(&mut world);
        for x in -20..60 {
            for y in -20..80 {
                world.set(x, y, 0, Block::new(ids::STONE, 0));
            }
        }
        Game { world, entities: Entities::new(), caveland: Caveland::new(Tuning::default(), 1), events: Vec::new(), notes: Vec::new() }
    }

    fn step(&mut self, steps: usize) {
        for _ in 0..steps {
            let events = self.caveland.tick(&mut self.entities, &mut self.world, DT);
            self.events.extend(events);
            self.notes.extend(self.caveland.drain_transport_events());
        }
    }

    /// Set a block and tell the game, like whatever builds it does.
    fn place(&mut self, cell: (i32, i32, i32), block: Block) {
        self.world.set(cell.0, cell.1, cell.2, block);
        self.caveland.block_changed(&self.world, cell);
    }

    fn seconds(&mut self, s: f32) {
        self.step((s * 60.0).round() as usize);
    }

    fn pos(&self, id: EntityId) -> Vec3 {
        self.entities.get(id).expect("the entity exists").position
    }

    fn speed(&self, id: EntityId) -> f32 {
        self.entities.get(id).and_then(|e| e.body.as_ref()).expect("a body").speed_hor()
    }

    fn player_at(&mut self, cell: (i32, i32, i32)) -> EntityId {
        self.caveland.spawn_player(&mut self.entities, 0, cell_floor(cell) + Vec3::Z * 0.01)
    }

    fn interact(&mut self, player: EntityId) {
        self.caveland.act(&mut self.entities, &mut self.world, player, Action::Interact);
        self.notes.extend(self.caveland.drain_transport_events());
    }

    fn push(&mut self, id: EntityId, hor: Vec2) {
        self.entities.get_mut(id).and_then(|e| e.body.as_mut()).expect("a body").set_hor_movement(hor);
    }

    fn sounds(&self, name: &str) -> usize {
        self.events.iter().filter(|e| matches!(e, GameEvent::Sound { name: n, .. } if *n == name)).count()
    }
}

/// A straight rail of block value 1 (it runs along the isometric x axis), `len` blocks long from
/// `start`. Returns the cells.
fn lay_rails(game: &mut Game, start: (i32, i32), z: i32, len: usize) -> Vec<(i32, i32, i32)> {
    let mut cells = Vec::new();
    let mut at = start;
    for _ in 0..len {
        game.world.set(at.0, at.1, z, Block::new(ids::RAILS, 1));
        cells.push((at.0, at.1, z));
        at = lower_right(at.0, at.1);
    }
    cells
}

// ---- the cart ----------------------------------------------------------------------------

#[test]
fn a_cart_that_is_pushed_runs_along_the_rails_at_full_speed() {
    let mut game = Game::new();
    let cells = lay_rails(&mut game, (2, 10), 1, 12);
    let start = cell_floor(cells[0]);
    let cart = game.caveland.spawn_minecart(&mut game.entities, start);
    assert_eq!(game.caveland.kind_of(cart), Some(EntityKind::MineCart));

    game.step(2); // settles; on rails but not moving: stays
    assert_eq!(game.speed(cart), 0.0);
    game.push(cart, Vec2::new(1.0, 0.0));
    game.step(60);

    let p = game.pos(cart);
    assert!((game.speed(cart) - MAX_SPEED).abs() < 1e-3, "a pushed cart rolls at {MAX_SPEED} b/s, not {}", game.speed(cart));
    assert!(p.x - start.x > 5.5 && p.x - start.x < 6.3, "a second at full speed is about six blocks, got {}", p.x - start.x);
    assert!((p.y - start.y).abs() < 1e-3, "it stays on the line, off by {}", p.y - start.y);
    assert!(game.caveland.transport().cart(cart).unwrap().on_rails(), "the lamp is lit on rails");
    assert_eq!(game.sounds("wagon"), 1, "the rolling sound starts once");
}

#[test]
fn a_cart_that_leaves_the_rails_slows_down_and_stops() {
    let mut game = Game::new();
    let cells = lay_rails(&mut game, (2, 10), 1, 4);
    let cart = game.caveland.spawn_minecart(&mut game.entities, cell_floor(cells[0]));
    game.step(2);
    game.push(cart, Vec2::new(1.0, 0.0));
    game.step(30);
    assert!(game.speed(cart) > 5.9);
    let end_of_rails = cell_floor(*cells.last().unwrap()).x + 0.5;

    game.seconds(3.0);
    assert_eq!(game.speed(cart), 0.0, "friction stops it off the rails");
    let x = game.pos(cart).x;
    assert!(x > end_of_rails && x < end_of_rails + 4.0, "it coasted a few blocks past the rails: {x} vs {end_of_rails}");
    assert!(!game.caveland.transport().cart(cart).unwrap().on_rails());
    assert!(
        game.notes.contains(&TransportEvent::SoundStopped { name: "wagon", entity: cart }),
        "the rolling sound is stopped when the rails end"
    );
}

#[test]
fn a_booster_shoots_the_cart_on_when_it_has_power_and_stops_it_without() {
    for powered in [false, true] {
        let mut game = Game::new();
        let cells = lay_rails(&mut game, (2, 10), 1, 10);
        let booster = cells[4];
        game.world.set(booster.0, booster.1, booster.2, Block::new(ids::BOOSTER_RAILS, 1));
        if powered {
            // A cable on side 1 of the booster that has a power station at its far end. The cable
            // is of the type that runs along that side (value 0 without power, 1 with).
            let cable = caveland_sim::cells::neighbour(booster, 1);
            let station = caveland_sim::cells::neighbour(cable, 1);
            game.place(cable, Block::new(ids::POWER_CABLE, 0));
            game.place(station, Block::new(ids::POWER_STATION, 0));
        }
        game.step(10); // the network settles
        let cart = game.caveland.spawn_minecart(&mut game.entities, cell_floor(cells[0]));
        game.step(2);
        game.push(cart, Vec2::new(1.0, 0.0));

        let mut fastest = 0.0f32;
        for _ in 0..180 {
            game.step(1);
            fastest = fastest.max(game.speed(cart));
        }
        let booster_x = cell_floor(booster).x;
        if powered {
            assert!((fastest - BOOSTER_SPEED).abs() < 1e-3, "powered boosters give {BOOSTER_SPEED} b/s, got {fastest}");
            assert!(game.pos(cart).x > booster_x + 3.0, "and carry the cart on");
        } else {
            assert!(fastest <= MAX_SPEED + 1e-3, "no power, no boost: {fastest}");
            assert!(game.pos(cart).x < booster_x + 1.0, "an unpowered booster stops the cart on it");
            assert_eq!(game.speed(cart), 0.0);
        }
    }
}

#[test]
fn a_player_gets_in_rides_along_and_is_left_behind_when_they_jump_high() {
    let mut game = Game::new();
    let cells = lay_rails(&mut game, (2, 10), 1, 14);
    let cart = game.caveland.spawn_minecart(&mut game.entities, cell_floor(cells[1]));
    let player = game.player_at(cells[1]);
    game.step(5);

    game.interact(player);
    assert!(game.notes.contains(&TransportEvent::Boarded { cart, passenger: player }));
    assert_eq!(game.caveland.transport().cart(cart).unwrap().passenger(), Some(player));
    assert_eq!(game.caveland.nearest_interactable(&game.world, game.pos(player)), None, "a full cart cannot be used again");

    game.push(cart, Vec2::new(1.0, 0.0));
    game.step(90);
    let (c, p) = (game.pos(cart), game.pos(player));
    assert!(c.x > cell_floor(cells[1]).x + 6.0, "the cart rolled");
    assert!(Vec2::new(c.x - p.x, c.y - p.y).length() < 0.01, "the passenger is in the middle of the cart");
    assert!((p.z - (c.z + BOTTOM_HEIGHT)).abs() < 0.01, "and sits on its floor: {} vs {}", p.z, c.z);
    assert!(game.entities.get(player).unwrap().body.as_ref().unwrap().floating);

    // Jumping out: far enough above the cart and the passenger lets go.
    game.entities.get_mut(player).unwrap().position.z += 1.0;
    game.step(1);
    assert!(game.notes.contains(&TransportEvent::Left { cart, passenger: player }));
    assert_eq!(game.caveland.transport().cart(cart).unwrap().passenger(), None);
    assert!(!game.entities.get(player).unwrap().body.as_ref().unwrap().floating, "gravity works again");
}

#[test]
fn an_empty_cart_loads_what_falls_into_it_up_to_five_items() {
    let mut game = Game::new();
    let cells = lay_rails(&mut game, (2, 10), 1, 4);
    let cart = game.caveland.spawn_minecart(&mut game.entities, cell_floor(cells[1]));
    game.step(2);
    let mut items = Vec::new();
    for _ in 0..6 {
        let at = game.pos(cart) + Vec3::Z * 0.3;
        items.push(game.caveland.spawn_collectible(&mut game.entities, Item::new(C::Iron), at));
        game.step(3);
    }
    let cart_state = game.caveland.transport().cart(cart).unwrap();
    assert_eq!(cart_state.content().len(), 5, "the cart takes five");
    assert!(cart_state.content().iter().all(|i| i.kind == C::Iron));
    let loaded = items.iter().filter(|&&i| game.caveland.kind_of(i).is_none()).count();
    assert_eq!(loaded, 5, "five items left the world");
    assert!(game.caveland.kind_of(*items.last().unwrap()).is_some(), "the sixth is still there");
}

#[test]
fn a_cart_on_a_curve_stays_on_its_circle() {
    let mut game = Game::new();
    // Value 5: a circle of half a block around the point one half diagonal right of the block's middle.
    game.world.set(5, 10, 1, Block::new(ids::RAILS, 5));
    let (gx, gy) = to_iso(5, 10);
    let anchor = Vec2::new(gx, gy) + wurfel_sim::entity::screen_to_iso(Vec2::new(0.707_106_8, 0.0));
    // Start a little off the circle, rolling down the screen.
    let start = Vec3::new(gx, gy, 1.0) + wurfel_sim::entity::screen_to_iso(Vec2::new(0.1, -0.3)).extend(0.0);
    let cart = game.caveland.spawn_minecart(&mut game.entities, start);
    game.step(1);
    game.push(cart, wurfel_sim::entity::screen_to_iso(Vec2::new(0.0, 1.0)));
    for tick in 0..5 {
        game.step(1);
        let p = game.pos(cart);
        let radius = Vec2::new(p.x, p.y).distance(anchor);
        assert!((radius - 0.5).abs() < 1e-3, "tick {tick}: {radius} from the circle's middle");
        assert!((p.z - 1.0).abs() < 1e-3, "the height is kept: {}", p.z);
    }
}

#[test]
fn a_ramp_launches_a_cart_that_drives_up_it_and_lets_a_standing_one_roll_down() {
    // Value 6 climbs towards the top right of the screen: the isometric -y direction. A line of
    // ramp blocks along it: the cells whose middle is (8, 5), (8, 4), ...
    let mut game = Game::new();
    for k in 0..6 {
        let (x, y) = from_iso(8.0, 5.0 - k as f32);
        game.world.set(x, y, 1, Block::new(ids::RAILS, 6));
    }
    let cart = game.caveland.spawn_minecart(&mut game.entities, Vec3::new(8.0, 5.3, 1.0));
    game.step(2);
    game.push(cart, Vec2::new(0.0, -1.0));
    let mut highest = 0.0f32;
    for _ in 0..40 {
        game.step(1);
        highest = highest.max(game.pos(cart).z);
    }
    assert!(highest > 1.15, "the cart took off from the ramp, got to {highest}");

    // A cart at rest on the ramp rolls down: towards the lower left of the screen, iso +y.
    let mut game = Game::new();
    let (x, y) = from_iso(8.0, 5.0);
    game.world.set(x, y, 1, Block::new(ids::RAILS, 6));
    let cart = game.caveland.spawn_minecart(&mut game.entities, Vec3::new(8.0, 5.0, 1.0));
    game.step(3);
    let movement = game.entities.get(cart).unwrap().body.as_ref().unwrap().movement;
    assert!(movement.y > 5.0 && movement.x.abs() < 0.1, "it rolls down the slope at full speed: {movement}");
}

#[test]
fn a_cart_throws_things_in_its_way_aside() {
    let mut game = Game::new();
    let cells = lay_rails(&mut game, (2, 10), 1, 10);
    let cart = game.caveland.spawn_minecart(&mut game.entities, cell_floor(cells[0]));
    let robot_at = cell_floor(cells[3]) + Vec3::new(0.0, 0.0, 0.2);
    let robot = game.caveland.spawn_robot(&mut game.entities, Team::Robots, robot_at);
    game.step(2);
    game.push(cart, Vec2::new(1.0, 0.0));
    game.seconds(1.5);
    let moved = game.pos(robot);
    assert!(moved.x > robot_at.x + 0.5 || moved.z > robot_at.z + 0.05, "the robot was hit and went flying: {moved} from {robot_at}");
}

#[test]
fn a_destroyed_cart_drops_iron_and_lets_its_passenger_go() {
    let mut game = Game::new();
    let cells = lay_rails(&mut game, (2, 10), 1, 4);
    let cart = game.caveland.spawn_minecart(&mut game.entities, cell_floor(cells[1]));
    let player = game.player_at(cells[1]);
    game.step(3);
    game.interact(player);
    game.step(2);

    game.caveland.damage_entity(&mut game.entities, cart, 500.0);
    game.step(2);
    assert_eq!(game.caveland.kind_of(cart), None, "the cart is gone");
    assert!(game.notes.contains(&TransportEvent::Left { cart, passenger: player }));
    assert!(!game.entities.get(player).unwrap().body.as_ref().unwrap().floating);
    assert_eq!(game.sounds("robot1destroy"), 1);
    // The bar of iron jumps out; the player standing right there picks it up.
    let in_the_world = game.caveland.things().into_iter().filter(|(_, k)| matches!(k, EntityKind::Collectible(C::Iron))).count();
    let in_the_pack = game.caveland.player(player).unwrap().inventory.items().iter().filter(|i| i.kind == C::Iron).count();
    assert_eq!(in_the_world + in_the_pack, 1, "a bar of iron comes out");
}

// ---- portals -----------------------------------------------------------------------------

#[test]
fn a_portal_takes_whatever_lands_in_it_and_the_exit_does_not_send_it_back() {
    let mut game = Game::new();
    let portal = game.caveland.transport_mut().spawn_portal(&mut game.entities, (5, 10, 1), (12, 20, 1), false);
    // The far end: an exit portal that leads back up to the block above the entrance.
    let exit = game.caveland.transport_mut().spawn_portal(&mut game.entities, (12, 20, 1), (5, 10, 2), true);
    assert_eq!(game.caveland.kind_of(portal), Some(EntityKind::Portal));
    assert_eq!(game.caveland.kind_of(exit), Some(EntityKind::ExitPortal));
    assert!(game.caveland.things().iter().any(|&(id, _)| id == exit), "exit portals are drawn");
    assert!(!game.caveland.things().iter().any(|&(id, _)| id == portal), "plain portals are invisible");

    // Falls into the portal's block.
    let player = game.caveland.spawn_player(&mut game.entities, 0, cell_floor((5, 10, 1)) + Vec3::Z * 3.0);
    game.seconds(2.0);
    assert!(game.notes.iter().any(|n| matches!(n, TransportEvent::Teleported { entity, .. } if *entity == player)));
    let at = game.pos(player);
    let target = cell_floor((12, 20, 1));
    assert!(Vec2::new(at.x - target.x, at.y - target.y).length() < 0.5, "arrived at the exit: {at} vs {target}");
    let teleports = game.notes.iter().filter(|n| matches!(n, TransportEvent::Teleported { .. })).count();
    assert_eq!(teleports, 1, "the closed exit portal does not bounce it back");

    // Using the exit portal by hand goes back, to the block above the entrance.
    game.interact(player);
    let back = game.pos(player);
    let above = cell_floor((5, 10, 2));
    assert!((back - above).length() < 0.2, "back above the entrance: {back} vs {above}");
}

#[test]
fn a_closed_portal_teleports_nothing_and_the_target_can_be_changed() {
    let mut game = Game::new();
    // An exit portal is closed.
    let closed = game.caveland.transport_mut().spawn_portal(&mut game.entities, (8, 10, 1), (30, 30, 1), true);
    let waiting = game.player_at((8, 10, 1));
    game.seconds(1.0);
    let at = game.pos(waiting);
    let here = cell_floor((8, 10, 1));
    assert!(Vec2::new(at.x - here.x, at.y - here.y).length() < 0.5, "stayed at the closed portal: {at}");
    assert!(!game.notes.iter().any(|n| matches!(n, TransportEvent::Teleported { .. })));
    assert!(!game.caveland.transport().portal(closed).unwrap().active);

    // An open one follows `portaltarget`.
    let open = game.caveland.transport_mut().spawn_portal(&mut game.entities, (5, 10, 1), (12, 20, 1), false);
    assert!(game.caveland.transport_mut().set_portal_target(&[open], (30, 30, 1)));
    let player = game.player_at((5, 10, 1));
    game.seconds(1.0);
    let at = game.pos(player);
    let target = cell_floor((30, 30, 1));
    assert!(Vec2::new(at.x - target.x, at.y - target.y).length() < 0.5, "sent to the new target: {at} vs {target}");
}

#[test]
fn the_generator_creates_portals_and_a_cave_exit_sends_robots() {
    let mut game = Game::new();
    // The portal of cave 0 as the Caveland generator places it.
    let spawn = EntitySpawn {
        kind: "ExitPortal",
        cell: (21, 1212, 4),
        target: Some((0, 0, 5)),
        extras: vec![("enemy_spawner", "true".to_string())],
    };
    game.world.set(21, 1212, 0, Block::new(ids::STONE, 0)); // loads the chunk
    let id = game.caveland.spawn_from_generator(&mut game.entities, &spawn).expect("a portal");
    let portal = game.caveland.transport().portal(id).unwrap();
    assert!(portal.exit && portal.is_spawner());
    assert_eq!(portal.target, (0, 0, 5));
    assert!(game
        .caveland
        .spawn_from_generator(&mut game.entities, &EntitySpawn { kind: "Bird", cell: (0, 0, 0), target: None, extras: vec![] })
        .is_none());

    let robots = |game: &Game| game.caveland.things().iter().filter(|(_, k)| matches!(k, EntityKind::Robot(Team::Robots))).count();
    game.step(2);
    assert_eq!(robots(&game), 3, "three robots arrive");
    game.seconds(6.0);
    assert_eq!(robots(&game), 3, "never more than three");
}

#[test]
fn nothing_spawns_while_a_player_is_near_the_cave_exit() {
    let mut game = Game::new();
    let spawn = EntitySpawn {
        kind: "ExitPortal",
        cell: (21, 1212, 4),
        target: Some((0, 0, 5)),
        extras: vec![("enemy_spawner", "true".to_string())],
    };
    game.world.set(21, 1212, 0, Block::new(ids::STONE, 0));
    game.caveland.spawn_from_generator(&mut game.entities, &spawn).unwrap();
    let _player = game.caveland.spawn_player(&mut game.entities, 0, cell_floor((21, 1212, 5)));
    game.seconds(2.0);
    let robots = game.caveland.things().iter().filter(|(_, k)| matches!(k, EntityKind::Robot(_))).count();
    assert_eq!(robots, 0, "a player within 13 blocks keeps the robots away");
}

// ---- the cave entry and the lift ---------------------------------------------------------

/// A hole in the surface at (3, 10, 1) (even row), the way down to cave 0 at (21, 1266, 7), a floor
/// in that cave.
fn hole_game() -> Game {
    let mut game = Game::new();
    game.place((3, 10, 1), Block::new(ids::ENTRY, 0));
    for x in 15..30 {
        for y in 1260..1272 {
            game.world.set(x, y, 0, Block::new(ids::STONE, 0));
        }
    }
    game
}

#[test]
fn a_hole_in_the_ground_takes_a_player_to_the_caves() {
    let mut game = hole_game();
    game.step(3);
    // The block shows the open hole (value 0) and a portal is in it.
    assert_eq!(game.world.get(3, 10, 1), Block::new(ids::ENTRY, 0));
    let player = game.caveland.spawn_player(&mut game.entities, 0, cell_floor((3, 10, 1)) + Vec3::Z * 2.0);
    game.seconds(2.0);
    let at = game.pos(player);
    let (gx, gy) = to_iso(21, 1266);
    assert!(Vec2::new(at.x - gx, at.y - gy).length() < 1.0, "in cave 0, below its way up: {at}");
    assert!(at.z < 7.5, "and falling, or standing on its floor: {}", at.z);
}

#[test]
fn something_built_over_the_hole_closes_it() {
    let mut game = hole_game();
    game.step(3);
    game.world.set(3, 10, 2, Block::new(ids::STONE, 0));
    game.step(2);
    assert_eq!(game.world.get(3, 10, 1), Block::new(ids::ENTRY, 1), "covered: shut (value 1, an obstacle)");
    game.world.set(3, 10, 2, Block::AIR);
    game.step(2);
    assert_eq!(game.world.get(3, 10, 1), Block::new(ids::ENTRY, 0), "uncovered: open again");
}

#[test]
fn using_the_hole_offers_a_lift_and_confirming_places_a_construction_site() {
    let mut game = hole_game();
    game.step(3);
    let player = game.player_at((3, 11, 1));
    // Standing next to the hole, which is the nearest thing to use.
    game.interact(player);
    let offered = game.notes.iter().find_map(|n| match n {
        TransportEvent::LiftSiteOffered { player: p, entry, site } if *p == player => Some((*entry, *site)),
        _ => None,
    });
    assert_eq!(offered, Some(((3, 10, 1), (3, 10, 2))));

    assert!(game.caveland.confirm_lift_site(&mut game.world, (3, 10, 2)));
    assert_eq!(game.world.get(3, 10, 2), Block::new(ids::CONSTRUCTION_SITE, 0));
    assert_eq!(game.caveland.transport().site_result((3, 10, 2)), Some(ids::LIFT));
    assert!(!game.caveland.confirm_lift_site(&mut game.world, (3, 10, 2)), "not twice");
    game.step(2);
    game.interact(player);
    let again = game.notes.iter().filter(|n| matches!(n, TransportEvent::LiftSiteOffered { .. })).count();
    assert_eq!(again, 1, "a covered hole offers nothing");
}

#[test]
fn a_lift_takes_a_player_down_to_the_cave_and_back_up() {
    let mut game = hole_game();
    // The lift stands on the hole.
    game.place((3, 10, 2), Block::new(ids::LIFT, 0));
    game.step(5);

    // The lift set itself up: a basket in the shaft, an exit portal in the cave that leads back to
    // the lift, and a lift ground at the foot of it.
    let baskets: Vec<_> = game.caveland.things().into_iter().filter(|(_, k)| *k == EntityKind::LiftBasket).collect();
    assert_eq!(baskets.len(), 1, "one basket");
    let basket = baskets[0].0;
    let exit = game
        .caveland
        .things()
        .into_iter()
        .find(|(_, k)| *k == EntityKind::ExitPortal)
        .expect("an exit portal in the cave")
        .0;
    assert_eq!(game.caveland.transport().portal(exit).unwrap().target, (3, 10, 2));
    assert_eq!(game.world.get(21, 1266, 0).id(), ids::LIFT_GROUND);
    let stand = cell_floor((3, 10, 2));
    assert!(Vec2::new(game.pos(basket).x - stand.x, game.pos(basket).y - stand.y).length() < 0.1);

    // Down.
    let player = game.player_at((3, 11, 2));
    game.entities.get_mut(player).unwrap().position = stand + Vec3::new(0.3, 0.0, 0.0);
    game.interact(player);
    assert_eq!(game.caveland.transport().basket(basket).unwrap().passenger(), Some(player));
    assert_eq!(game.caveland.transport().basket(basket).unwrap().movement_dir(), -1);
    game.seconds(4.0);
    let (gx, gy) = to_iso(21, 1266);
    let basket_at = game.pos(basket);
    assert!(Vec2::new(basket_at.x - gx, basket_at.y - gy).length() < 0.1, "the basket is in the cave: {basket_at}");
    assert!(basket_at.z < 1.2, "on the lift ground: {}", basket_at.z);
    assert_eq!(game.caveland.transport().basket(basket).unwrap().movement_dir(), 0, "it stopped");
    assert_eq!(game.caveland.transport().basket(basket).unwrap().passenger(), None, "and let the player out");
    let down = game.pos(player);
    assert!(Vec2::new(down.x - gx, down.y - gy).length() < 3.0 && down.z < 1.5, "the player is in the cave: {down}");
    assert!(game.entities.get(player).unwrap().health() > 99.0, "unhurt");

    // Up again: the one who just got out may only get back in after a block's walk away.
    game.entities.get_mut(player).unwrap().position = cell_floor((21, 1266, 1)) + Vec3::new(1.2, 0.0, 0.0);
    game.step(2);
    game.interact(player);
    assert_eq!(game.caveland.transport().basket(basket).unwrap().passenger(), Some(player));
    assert_eq!(game.caveland.transport().basket(basket).unwrap().movement_dir(), 1);
    game.seconds(5.0);
    let top = game.pos(player);
    assert!(Vec2::new(top.x - stand.x, top.y - stand.y).length() < 3.0, "back on the surface: {top}");
    assert!(top.z > 1.5 && top.z < 3.5, "at the lift's height: {}", top.z);
    assert_eq!(game.caveland.transport().basket(basket).unwrap().movement_dir(), 0);
}

#[test]
fn a_lift_that_is_dug_out_takes_its_basket_with_it() {
    let mut game = hole_game();
    game.place((3, 10, 2), Block::new(ids::LIFT, 0));
    game.step(5);
    assert!(game.caveland.things().iter().any(|(_, k)| *k == EntityKind::LiftBasket));
    game.world.set(3, 10, 2, Block::AIR);
    game.step(3);
    assert!(!game.caveland.things().iter().any(|(_, k)| *k == EntityKind::LiftBasket), "no lift, no basket");
}

#[test]
fn a_lift_without_a_hole_under_it_does_nothing() {
    let mut game = Game::new();
    game.place((3, 10, 2), Block::new(ids::LIFT, 0));
    game.step(5);
    assert!(game.caveland.things().is_empty());
}

#[test]
fn a_cart_rolls_into_the_basket_and_rides_down() {
    let mut game = hole_game();
    game.place((3, 10, 2), Block::new(ids::LIFT, 0));
    game.step(5);
    let stand = cell_floor((3, 10, 2));
    let cart = game.caveland.spawn_minecart(&mut game.entities, stand + Vec3::new(0.2, 0.0, 0.0));
    game.step(3);
    let basket = game.caveland.things().into_iter().find(|(_, k)| *k == EntityKind::LiftBasket).unwrap().0;
    assert_eq!(game.caveland.transport().basket(basket).unwrap().passenger(), Some(cart), "the basket took the cart");
    game.seconds(4.0);
    let (gx, gy) = to_iso(21, 1266);
    let at = game.pos(cart);
    assert!(Vec2::new(at.x - gx, at.y - gy).length() < 3.0 && at.z < 2.0, "the cart is in the cave: {at}");
}

// ---- the spaceship -----------------------------------------------------------------------

#[test]
fn the_spaceship_flies_in_with_the_players_hidden_and_crashes() {
    let mut game = Game::new();
    let target = (10, 40, 1);
    let start = cell_floor(target) + Vec3::new(-40.0, 0.0, 29.0);
    let ship = game.caveland.transport_mut().spawn_spaceship(&mut game.entities, start);
    let player = game.caveland.spawn_player(&mut game.entities, 0, start);
    game.caveland.transport_mut().enable_crash(&mut game.entities, ship, target);
    game.caveland.transport_mut().add_ship_content(&mut game.entities, ship, player);
    assert!(game.caveland.is_hidden(player), "on board, nobody sees the player");
    assert_eq!(game.caveland.kind_of(ship), Some(EntityKind::Spaceship));

    // On its way: the player moves with the ship.
    game.seconds(0.5);
    let (s, p) = (game.pos(ship), game.pos(player));
    assert!(s.x > start.x + 3.0, "the ship flies towards the crash site: {s}");
    assert!((s - p).length() < 0.3, "the player is inside it: {s} vs {p}");
    assert!(game.caveland.is_hidden(player));
    assert!(!game.caveland.transport().ship(ship).unwrap().crashed);

    game.seconds(8.0);
    let ship_state = game.caveland.transport().ship(ship).unwrap();
    assert!(ship_state.crashed, "it crashed");
    assert!(game.notes.iter().any(|n| matches!(n, TransportEvent::ShipCrashed { ship: s, .. } if *s == ship)));
    assert!(game.notes.contains(&TransportEvent::IntroCutsceneCompleted));
    assert!(game.caveland.transport().intro_crashed());
    assert!(!game.caveland.is_hidden(player), "the player got out");
    assert_eq!(game.entities.get(player).unwrap().health(), 100.0, "and is unhurt: the explosion spares the passengers");
    let explosions = game.events.iter().filter(|e| matches!(e, GameEvent::Explosion { .. })).count();
    assert_eq!(explosions, 2, "a harmless one in the air, a real one on the ground");
    assert!(game.pos(ship).z < 2.0, "the wreck is on the ground");
    let before = game.notes.len();
    game.seconds(2.0);
    assert_eq!(game.notes.len(), before, "it crashes once");
}

#[test]
fn a_ship_without_a_crash_site_just_floats_where_it_is() {
    let mut game = Game::new();
    let start = cell_floor((10, 40, 1)) + Vec3::Z * 20.0;
    let ship = game.caveland.transport_mut().spawn_spaceship(&mut game.entities, start);
    game.seconds(1.0);
    assert!(game.pos(ship).z < start.z, "without enable_crash it is an ordinary falling body");
    assert!(!game.caveland.transport().ship(ship).unwrap().crashed);
}
