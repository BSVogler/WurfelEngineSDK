use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use glam::{Vec2, Vec3};

use super::ai::MoveToAi;
use super::physics::{self, GRAVITY};
use super::*;
use crate::block::id;
use crate::grid::{from_iso, lower_right, to_iso};
use crate::{Block, Generator, World};

const DT: f32 = 1.0 / 60.0;

/// A sand floor at z = 0, so the ground surface is at height 1.
struct Flat;
impl Generator for Flat {
    fn generate(&self, _x: i32, _y: i32, z: i32) -> Block {
        if z == 0 { Block::new(id::SAND, 0) } else { Block::AIR }
    }
}

fn world_with(blocks: &[((i32, i32, i32), u8)]) -> World {
    let mut world = World::new(Flat);
    for &((x, y, z), block) in blocks {
        world.set(x, y, z, Block::new(block, 0));
    }
    world
}

fn at(column: (i32, i32), z: f32) -> Vec3 {
    let (gx, gy) = to_iso(column.0, column.1);
    Vec3::new(gx, gy, z)
}

fn walker(position: Vec3) -> Entity {
    let mut entity = Entity::new("walker", 1).movable().at(position);
    entity.body.as_mut().unwrap().friction = 0.0;
    entity
}

/// Like the Caveland player: 1.4 blocks tall, heavy, jumps at 4.7 blocks/s.
fn player(position: Vec3) -> Entity {
    let mut entity = Entity::new("player", 30).movable().at(position);
    entity.dimension_z = 1.4;
    entity.mass = 60.0;
    let body = entity.body.as_mut().unwrap();
    body.friction = 0.03;
    body.jump_speed = Some(4.7);
    entity
}

fn run(entities: &mut Entities, world: &World, seconds: f32) -> Vec<Event> {
    let mut events = Vec::new();
    for _ in 0..(seconds / DT).round() as usize {
        events.extend(entities.update(world, DT));
    }
    events
}

fn count(events: &[Event], wanted: Event) -> usize {
    events.iter().filter(|e| **e == wanted).count()
}

const START: (i32, i32) = (0, 10);

#[test]
fn a_falling_entity_lands_exactly_on_the_ground_once() {
    let world = world_with(&[]);
    let mut entities = Entities::new();
    let id = entities.spawn(walker(at(START, 4.0)));

    let events = run(&mut entities, &world, 2.0);

    let e = entities.get(id).unwrap();
    assert_eq!(e.position.z, 1.0, "rests on top of the sand block");
    assert_eq!(e.body.as_ref().unwrap().movement.z, 0.0);
    assert_eq!(count(&events, Event::Landed(id)), 1);
    assert!(e.is_on_ground(&world));
}

#[test]
fn free_fall_accelerates_at_gravity() {
    let world = world_with(&[]);
    let mut entities = Entities::new();
    let id = entities.spawn(walker(at(START, 9.5)));
    run(&mut entities, &world, 0.5);
    let vz = entities.get(id).unwrap().body.as_ref().unwrap().movement.z;
    assert!((vz + GRAVITY * 0.5).abs() < 0.2, "vz = {vz}");
}

#[test]
fn a_jump_reaches_the_ballistic_height_and_comes_back() {
    let world = world_with(&[]);
    let mut entities = Entities::new();
    let id = entities.spawn(player(at(START, 1.0)));

    assert!(entities.get_mut(id).unwrap().jump(&world));
    let mut peak = 0.0f32;
    let mut events = Vec::new();
    for frame in 0..120 {
        events.extend(entities.update(&world, DT));
        peak = peak.max(entities.get(id).unwrap().position.z);
        if frame == 5 {
            assert!(!entities.get_mut(id).unwrap().jump(&world), "no jumping in mid-air");
        }
    }
    let expected = 1.0 + 4.7f32.powi(2) / (2.0 * GRAVITY);
    assert!((peak - expected).abs() < 0.07, "peak {peak}, expected {expected}");
    assert_eq!(entities.get(id).unwrap().position.z, 1.0);
    assert_eq!(count(&events, Event::Landed(id)), 1);
}

#[test]
fn only_entities_with_a_jump_speed_can_jump() {
    let world = world_with(&[]);
    assert!(!walker(at(START, 1.0)).jump(&world));
    assert!(!Entity::new("rock", 2).jump(&world));
}

#[test]
fn a_wall_stops_a_walker_and_reports_a_collision() {
    let c1 = lower_right(START.0, START.1);
    let c2 = lower_right(c1.0, c1.1);
    let world = world_with(&[((c2.0, c2.1, 1), id::STONE), ((c2.0, c2.1, 2), id::STONE)]);
    let mut entities = Entities::new();
    let start = at(START, 1.0);
    let mut e = walker(start);
    e.body.as_mut().unwrap().set_movement(Vec3::new(3.0, 0.0, 0.0));
    let id = entities.spawn(e);

    let events = run(&mut entities, &world, 2.0);

    let e = entities.get(id).unwrap();
    // The wall's near face is at gx = 7 - 0.5; the front corner of the body sticks out 0.25.
    assert!(e.position.x > start.x + 1.0, "it should have walked up to the wall, x = {}", e.position.x);
    assert!(e.position.x <= 6.25 + 1e-3, "it must not enter the wall, x = {}", e.position.x);
    assert_eq!(e.body.as_ref().unwrap().speed_hor(), 0.0);
    assert!(count(&events, Event::Collided(id)) >= 1);
}

#[test]
fn walking_into_a_one_block_step_needs_a_jump() {
    let c1 = lower_right(START.0, START.1);
    let c2 = lower_right(c1.0, c1.1);
    let world = world_with(&[((c2.0, c2.1, 1), id::STONE)]);

    // Without jumping the step is a wall.
    let mut entities = Entities::new();
    let mut e = walker(at(START, 1.0));
    e.body.as_mut().unwrap().set_movement(Vec3::new(3.0, 0.0, 0.0));
    let id = entities.spawn(e);
    run(&mut entities, &world, 2.0);
    assert!(entities.get(id).unwrap().position.x <= 6.25 + 1e-3);

    // With a jump the player gets up to the step's height, over it and on beyond.
    let mut entities = Entities::new();
    let mut e = player(at(START, 1.0));
    e.body.as_mut().unwrap().friction = 0.0;
    e.body.as_mut().unwrap().set_movement(Vec3::new(3.0, 0.0, 0.0));
    let id = entities.spawn(e);
    assert!(entities.get_mut(id).unwrap().jump(&world));
    let mut highest_while_over_the_step = 0.0f32;
    for _ in 0..120 {
        entities.update(&world, DT);
        let p = entities.get(id).unwrap().position;
        if p.x > 6.4 && p.x < 7.6 {
            highest_while_over_the_step = highest_while_over_the_step.max(p.z);
        }
    }
    assert!(highest_while_over_the_step >= 2.0, "never got above the step: {highest_while_over_the_step}");
    assert!(entities.get(id).unwrap().position.x > 7.0, "x = {}", entities.get(id).unwrap().position.x);
}

#[test]
fn a_low_ceiling_cuts_a_jump_short() {
    // The player is 1.4 tall. With a block in cell 3 the head touches it at z = 3 - 1.4 = 1.6.
    let world = world_with(&[((START.0, START.1, 3), id::STONE)]);
    let mut entities = Entities::new();
    let id = entities.spawn(player(at(START, 1.0)));
    assert!(entities.get_mut(id).unwrap().jump(&world));

    let mut peak = 0.0f32;
    for _ in 0..90 {
        entities.update(&world, DT);
        peak = peak.max(entities.get(id).unwrap().position.z);
    }
    assert!(peak > 1.4 && peak < 1.6, "peak {peak}");
}

#[test]
fn water_does_not_block_and_is_reported_once() {
    let c1 = lower_right(START.0, START.1);
    let c2 = lower_right(c1.0, c1.1);
    let world = world_with(&[((c1.0, c1.1, 1), id::WATER), ((c2.0, c2.1, 1), id::WATER)]);
    let mut entities = Entities::new();
    let mut e = walker(at(START, 1.0));
    e.body.as_mut().unwrap().set_movement(Vec3::new(3.0, 0.0, 0.0));
    let id = entities.spawn(e);

    let events = run(&mut entities, &world, 1.2);

    let e = entities.get(id).unwrap();
    assert!(e.position.x > 6.5, "walked through the water, x = {}", e.position.x);
    assert_eq!(count(&events, Event::EnteredLiquid(id)), 1);
    assert!(physics::is_liquid(physics::block_at(&world, at(c1, 1.0))));
}

#[test]
fn friction_stops_a_walker_on_the_ground_but_not_in_the_air() {
    let world = world_with(&[]);

    let mut entities = Entities::new();
    let mut e = player(at(START, 1.0));
    e.body.as_mut().unwrap().set_movement(Vec3::new(3.0, 0.0, 0.0));
    let ground = entities.spawn(e);
    run(&mut entities, &world, 0.5);
    assert_eq!(entities.get(ground).unwrap().body.as_ref().unwrap().speed_hor(), 0.0);

    let mut entities = Entities::new();
    let mut e = player(at(START, 8.0));
    e.body.as_mut().unwrap().set_movement(Vec3::new(3.0, 0.0, 0.0));
    let air = entities.spawn(e);
    run(&mut entities, &world, 0.3);
    let speed = entities.get(air).unwrap().body.as_ref().unwrap().speed_hor();
    assert!((speed - 3.0).abs() < 1e-3, "air speed {speed}");
}

#[test]
fn heavy_entities_moving_towards_each_other_are_pushed_back() {
    let world = world_with(&[]);
    let mut entities = Entities::new();
    let mut a = player(at(START, 1.0) + Vec3::new(-0.25, 0.0, 0.0));
    let mut b = player(at(START, 1.0) + Vec3::new(0.25, 0.0, 0.0));
    a.body.as_mut().unwrap().friction = 0.0;
    b.body.as_mut().unwrap().friction = 0.0;
    a.body.as_mut().unwrap().set_movement(Vec3::new(1.0, 0.0, 0.0));
    b.body.as_mut().unwrap().set_movement(Vec3::new(-1.0, 0.0, 0.0));
    let (ia, ib) = (entities.spawn(a), entities.spawn(b));
    assert!(entities.get(ia).unwrap().collides_with(entities.get(ib).unwrap()));
    assert_eq!(entities.colliding(ia), vec![ib]);

    let events = run(&mut entities, &world, DT);

    let va = entities.get(ia).unwrap().body.as_ref().unwrap().movement.x;
    assert!(va < 1.0, "the left entity was pushed back, vx = {va}");
    assert!(events.contains(&Event::Collided(ia)));
}

#[test]
fn entities_moving_apart_or_too_light_to_push_are_left_alone() {
    let world = world_with(&[]);
    let apart = |mass: f32, left_vx: f32, right_vx: f32| {
        let mut entities = Entities::new();
        let mut a = player(at(START, 1.0) + Vec3::new(-0.25, 0.0, 0.0));
        let mut b = player(at(START, 1.0) + Vec3::new(0.25, 0.0, 0.0));
        for e in [&mut a, &mut b] {
            e.mass = mass;
            e.body.as_mut().unwrap().friction = 0.0;
        }
        a.body.as_mut().unwrap().set_movement(Vec3::new(left_vx, 0.0, 0.0));
        b.body.as_mut().unwrap().set_movement(Vec3::new(right_vx, 0.0, 0.0));
        let ia = entities.spawn(a);
        run(&mut entities, &world, DT);
        entities.get(ia).unwrap().body.as_ref().unwrap().movement.x
    };
    assert_eq!(apart(60.0, -1.0, 1.0), -1.0, "already separating");
    assert_eq!(apart(0.4, 1.0, -1.0), 1.0, "light entities do not push");
}

#[test]
fn entities_on_the_same_spot_do_not_produce_nan() {
    let world = world_with(&[]);
    let mut entities = Entities::new();
    let a = entities.spawn(player(at(START, 1.0)));
    let b = entities.spawn(player(at(START, 1.0)));
    run(&mut entities, &world, 0.1);
    for id in [a, b] {
        let e = entities.get(id).unwrap();
        assert!(e.position.is_finite() && e.body.as_ref().unwrap().movement.is_finite());
    }
}

#[test]
fn dead_entities_are_removed_unless_indestructible() {
    let world = world_with(&[]);
    let mut entities = Entities::new();
    let mortal = entities.spawn(walker(at(START, 1.0)));
    let mut tough = walker(at(START, 1.0));
    tough.indestructible = true;
    let tough = entities.spawn(tough);

    entities.get_mut(mortal).unwrap().take_damage(30.0);
    entities.get_mut(tough).unwrap().take_damage(1000.0);
    assert_eq!(entities.get(mortal).unwrap().health(), 70.0);
    assert_eq!(entities.get(tough).unwrap().health(), 100.0);
    entities.get_mut(mortal).unwrap().take_damage(1000.0);
    assert_eq!(entities.get(mortal).unwrap().health(), 0.0, "health never goes below 0");

    let events = entities.update(&world, DT);
    assert!(events.contains(&Event::Disposed(mortal)));
    assert!(entities.get(mortal).is_none());
    assert!(entities.get(tough).is_some());
    assert_eq!(entities.len(), 1);
}

struct CountingComponent {
    updates: Arc<AtomicUsize>,
    limit: usize,
}

impl Component for CountingComponent {
    fn update(&mut self, _parent: &mut Entity, _world: &World, _dt: f32) -> bool {
        let n = self.updates.fetch_add(1, Ordering::SeqCst) + 1;
        n < self.limit
    }
}

#[test]
fn a_component_runs_once_per_update_until_it_finishes() {
    let world = world_with(&[]);
    let updates = Arc::new(AtomicUsize::new(0));
    let mut entities = Entities::new();
    let mut e = walker(at(START, 1.0));
    e.add_component(Box::new(CountingComponent { updates: updates.clone(), limit: 3 }));
    let id = entities.spawn(e);
    assert_eq!(entities.get(id).unwrap().component_count(), 1);

    for _ in 0..10 {
        entities.update(&world, DT);
    }
    assert_eq!(updates.load(Ordering::SeqCst), 3);
    assert_eq!(entities.get(id).unwrap().component_count(), 0);
}

#[test]
fn move_to_ai_walks_to_the_goal_and_finishes() {
    let world = world_with(&[]);
    let mut entities = Entities::new();
    let mut e = walker(at(START, 1.0));
    let goal = e.position + Vec3::new(4.0, 0.0, 0.0);
    e.add_component(Box::new(MoveToAi::new(goal)));
    let id = entities.spawn(e);

    run(&mut entities, &world, 4.0);

    let e = entities.get(id).unwrap();
    assert!(Vec2::new(e.position.x - goal.x, e.position.y - goal.y).length() < 0.1, "at {:?}", e.position);
    assert_eq!(e.component_count(), 0, "the AI removes itself on arrival");
    assert_eq!(e.body.as_ref().unwrap().speed_hor(), 0.0);
}

#[test]
fn move_to_ai_jumps_when_stuck_against_a_step() {
    let c1 = lower_right(START.0, START.1);
    let world = world_with(&[((c1.0, c1.1, 1), id::STONE)]);
    let mut entities = Entities::new();
    let mut e = player(at(START, 1.0));
    e.body.as_mut().unwrap().friction = 0.0;
    let goal = e.position + Vec3::new(3.0, 0.0, 0.0);
    e.add_component(Box::new(MoveToAi::new(goal)));
    let id = entities.spawn(e);

    run(&mut entities, &world, 4.0);

    let e = entities.get(id).unwrap();
    assert!(e.position.x > goal.x - 0.3, "got over the step, x = {}", e.position.x);
}

#[test]
fn walk_turns_screen_keys_into_isometric_movement() {
    let mut e = walker(Vec3::ZERO);
    e.walk(false, false, false, true, 4.0);
    let m = e.body.as_ref().unwrap().hor_movement();
    let screen = iso_to_screen(m);
    assert!((screen - Vec2::new(4.0, 0.0)).length() < 1e-4, "right on screen, got {screen:?}");

    e.walk(false, false, true, true, 4.0); // left wins over right
    assert!(iso_to_screen(e.body.as_ref().unwrap().hor_movement()).x < 0.0);

    e.walk(true, false, false, true, 4.0); // diagonal is not faster
    assert!((e.body.as_ref().unwrap().speed_hor() - 4.0).abs() < 1e-4);

    let before = e.body.as_ref().unwrap().movement;
    e.walk(false, false, false, false, 4.0); // no keys: unchanged
    assert_eq!(e.body.as_ref().unwrap().movement, before);
}

#[test]
fn screen_and_iso_frames_convert_back_and_forth() {
    for v in [Vec2::new(1.0, 0.0), Vec2::new(-0.3, 2.0), Vec2::new(0.0, -1.0)] {
        assert!((iso_to_screen(screen_to_iso(v)) - v).length() < 1e-5);
        assert!((screen_to_iso(v).length() - v.length()).abs() < 1e-5, "rotation keeps lengths");
    }
}

#[test]
fn chunks_that_have_not_arrived_are_a_wall() {
    // A client world only has the chunk it was sent. Walking towards one it has not got stops at its edge.
    let mut server = World::new(Flat);
    server.load_chunk(0, 0);
    let mut world = World::remote();
    world.insert_chunk(server.chunk(0, 0).unwrap().clone());

    let mut entities = Entities::new();
    let mut e = walker(at((1, 10), 1.0));
    // Screen-left decreases the block column x; chunk -1 is not loaded.
    e.body.as_mut().unwrap().set_hor_movement(screen_to_iso(Vec2::new(-4.0, 0.0)));
    let id = entities.spawn(e);

    run(&mut entities, &world, 3.0);

    let p = entities.get(id).unwrap().position;
    let (x, y) = from_iso(p.x, p.y);
    assert!(world.is_loaded_at(x, y), "ended up in a chunk it does not have: {x},{y}");
    assert!(x >= 0, "stopped at the edge of the loaded chunk, x = {x}");
}

#[test]
fn a_world_with_a_generator_has_no_edge() {
    let world = world_with(&[]);
    let mut entities = Entities::new();
    let mut e = walker(at((0, 10), 1.0));
    e.body.as_mut().unwrap().set_hor_movement(screen_to_iso(Vec2::new(-4.0, 0.0)));
    let id = entities.spawn(e);
    run(&mut entities, &world, 3.0);
    let p = entities.get(id).unwrap().position;
    assert!(from_iso(p.x, p.y).0 < -5, "it should keep walking into ungenerated terrain: {p:?}");
}

#[test]
fn ground_height_ignores_water_and_air() {
    let world = world_with(&[
        ((0, 10, 1), id::STONE),
        ((0, 10, 2), id::STONE),
        ((2, 10, 1), id::WATER),
    ]);
    assert_eq!(physics::ground_height(&world, 1, 10), 1.0);
    assert_eq!(physics::ground_height(&world, 0, 10), 3.0);
    assert_eq!(physics::ground_height(&world, 2, 10), 1.0, "water is not ground");
}

#[test]
fn occupied_cells_cover_the_whole_body() {
    // Centred in a column, 1.4 tall standing on z = 1: two cells in one column.
    let cells = physics::occupied_cells(at(START, 1.0), 1.4);
    assert_eq!(cells.len(), 2);
    assert!(cells.contains(&(START.0, START.1, 1)) && cells.contains(&(START.0, START.1, 2)));

    // Straddling the border between two columns: both are occupied.
    let mut straddling = at(START, 1.0);
    straddling.x += 0.5;
    let columns: std::collections::HashSet<_> = physics::occupied_cells(straddling, 1.0).into_iter().map(|c| (c.0, c.1)).collect();
    assert!(columns.len() >= 2, "{columns:?}");
}
