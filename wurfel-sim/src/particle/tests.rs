use glam::Vec3;

use super::*;
use crate::block::id;
use crate::entity::Entity;
use crate::{Block, Generator, World};

const DT: f32 = 1.0 / 60.0;

/// Sand floor at z = 0: the surface is at height 1.
struct Flat;
impl Generator for Flat {
    fn generate(&self, _x: i32, _y: i32, z: i32) -> Block {
        if z == 0 { Block::new(id::SAND, 0) } else { Block::AIR }
    }
}

fn flat() -> World {
    World::new(Flat)
}

fn run(particles: &mut Particles, world: &World, seconds: f32) {
    for _ in 0..(seconds / DT).round() as usize {
        particles.update(world, DT);
    }
}

fn one(spec: ParticleSpec, position: Vec3, velocity: Vec3) -> Particles {
    let mut particles = Particles::new(16, 7);
    assert!(particles.spawn(&spec, position, velocity, Vec3::ZERO));
    particles
}

#[test]
fn rng_is_deterministic_and_in_range() {
    let mut a = Rng::new(42);
    let mut b = Rng::new(42);
    let xs: Vec<f32> = (0..200).map(|_| a.next_f32()).collect();
    assert_eq!(xs, (0..200).map(|_| b.next_f32()).collect::<Vec<_>>());
    assert!(xs.iter().all(|&x| (0.0..1.0).contains(&x)));
    assert!(xs.iter().any(|&x| x < 0.1) && xs.iter().any(|&x| x > 0.9));
    assert_ne!(Rng::new(1).next_u64(), Rng::new(2).next_u64());
    assert_ne!(Rng::new(0).next_u64(), 0);
}

#[test]
fn fade_curve_is_smootherstep() {
    assert_eq!(fade(0.0), 0.0);
    assert_eq!(fade(1.0), 1.0);
    assert!((fade(0.5) - 0.5).abs() < 1e-6);
    assert!((fade(0.25) - 0.103_515_6).abs() < 1e-5);
    assert_eq!(fade(2.0), 1.0);
}

#[test]
fn particle_dies_after_its_ttl() {
    let world = flat();
    let mut spec = ParticleSpec::regular();
    spec.ttl = 1.0;
    let mut particles = one(spec, Vec3::new(0.0, 0.0, 3.0), Vec3::ZERO);
    run(&mut particles, &world, 0.9);
    assert_eq!(particles.len(), 1);
    run(&mut particles, &world, 0.2);
    assert_eq!(particles.len(), 0);
}

#[test]
fn regular_particle_floats_and_keeps_its_look() {
    let world = flat();
    let mut particles = one(ParticleSpec::regular(), Vec3::new(1.0, 1.0, 3.0), Vec3::new(1.0, 0.0, 0.0));
    run(&mut particles, &world, 1.0);
    let p = particles.iter().next().unwrap();
    assert!((p.position.x - 2.0).abs() < 0.02, "moved {}", p.position.x);
    assert_eq!(p.position.z, 3.0);
    assert_eq!(p.color(), [0.5, 0.5, 0.5, 1.0]);
    assert_eq!(p.scale(), 0.3);
    assert!((p.size() - 0.3).abs() < 1e-6);
}

#[test]
fn rotation_turns_100_degrees_per_second_in_the_chosen_direction() {
    let world = flat();
    let mut particles = one(ParticleSpec::regular(), Vec3::new(0.0, 0.0, 3.0), Vec3::ZERO);
    let before = particles.iter().next().unwrap().rotation();
    run(&mut particles, &world, 0.5);
    let after = particles.iter().next().unwrap().rotation();
    assert!(((after - before).abs() - 50.0).abs() < 0.1, "turned {}", after - before);
}

#[test]
fn smoke_grows_and_fades_to_nothing() {
    let world = flat();
    let mut spec = ParticleSpec::smoke();
    spec.ttl = 2.0;
    spec.color = [0.4, 0.4, 0.4, 0.8];
    let mut particles = one(spec, Vec3::new(0.0, 0.0, 5.0), Vec3::ZERO);
    assert_eq!(particles.iter().next().unwrap().scale(), 0.0);
    run(&mut particles, &world, 1.0);
    let p = particles.iter().next().unwrap();
    assert!((p.scale() - 1.25).abs() < 0.03, "scale {}", p.scale());
    // Half way through: alpha = start alpha * fade(0.5), colour untouched.
    assert!((p.color()[3] - 0.4).abs() < 0.02, "alpha {}", p.color()[3]);
    assert_eq!(&p.color()[..3], &[0.4, 0.4, 0.4]);
}

#[test]
fn fire_darkens_with_age() {
    let world = flat();
    let mut spec = ParticleSpec::fire();
    spec.ttl = 1.0;
    spec.color = [1.0, 0.5, 0.2, 1.0];
    let mut particles = one(spec, Vec3::new(0.0, 0.0, 5.0), Vec3::ZERO);
    run(&mut particles, &world, 0.5);
    let c = particles.iter().next().unwrap().color();
    let left = particles.iter().next().unwrap().life_fraction();
    assert!((left - 0.5).abs() < 0.02);
    assert!((c[0] - left).abs() < 1e-5);
    assert!((c[1] - 0.5 * left).abs() < 1e-5);
    assert!((c[2] - 0.2 * left).abs() < 1e-5);
}

#[test]
fn falling_debris_rests_on_the_floor_and_bounces_less_each_time() {
    let world = flat();
    let mut particles = one(ParticleSpec::debris([0.6, 0.4, 0.2]), Vec3::new(2.0, 2.0, 3.0), Vec3::ZERO);
    // Fall from 2 blocks above the surface: v = sqrt(2 g 2) = 6.3 b/s, reached after 0.64 s.
    run(&mut particles, &world, 0.7);
    let p = particles.iter().next().unwrap();
    assert!(p.velocity.z > 0.0, "should be bouncing up, vz = {}", p.velocity.z);
    assert!(p.velocity.z < 0.4 * 6.5, "bounce keeps 40 %, vz = {}", p.velocity.z);
    assert!(p.position.z >= 1.0);
    run(&mut particles, &world, 3.0);
    let p = particles.iter().next().unwrap();
    assert!(p.position.z >= 1.0 && p.position.z < 1.05, "rests at {}", p.position.z);
    assert_eq!(p.velocity.z, 0.0);
}

#[test]
fn debris_stops_at_a_wall_instead_of_passing_through() {
    let mut world = flat();
    // A stone wall one block high over the whole width, in front of gx = 4.
    for y in -40..40 {
        for x in -10..10 {
            let (gx, _) = crate::grid::to_iso(x, y);
            if gx >= 4.0 {
                world.set(x, y, 1, Block::new(id::STONE, 0));
            }
        }
    }
    let mut spec = ParticleSpec::debris([1.0, 1.0, 1.0]);
    spec.gravity = 0.0;
    spec.bounce = 0.0;
    spec.ttl = 10.0;
    // Flying along +x (gx) at height 1.5, i.e. inside the wall row, towards gx = 4.
    let mut particles = one(spec, Vec3::new(1.0, 0.0, 1.5), Vec3::new(3.0, 0.0, 0.0));
    run(&mut particles, &world, 2.0);
    let p = particles.iter().next().unwrap();
    assert!(p.position.x < 4.0 && p.position.x > 3.0, "stopped at gx = {}", p.position.x);
    assert_eq!(p.velocity.x, 0.0);
}

#[test]
fn burst_stops_at_capacity() {
    let mut particles = Particles::new(5, 1);
    assert_eq!(particles.burst(&ParticleSpec::regular(), Vec3::ZERO, 8, Vec3::ZERO, Vec3::ZERO), 5);
    assert_eq!(particles.len(), 5);
    assert!(!particles.spawn(&ParticleSpec::regular(), Vec3::ZERO, Vec3::ZERO, Vec3::ZERO));
}

#[test]
fn spawn_noise_stays_within_the_spread_and_is_reproducible() {
    let make = || {
        let mut p = Particles::new(100, 99);
        p.burst(&ParticleSpec::regular(), Vec3::ZERO, 100, Vec3::new(1.0, 2.0, 3.0), Vec3::new(0.5, 0.0, 1.0));
        p
    };
    let (a, b) = (make(), make());
    assert_eq!(a.iter().map(|p| p.velocity).collect::<Vec<_>>(), b.iter().map(|p| p.velocity).collect::<Vec<_>>());
    for p in a.iter() {
        assert!((p.velocity.x - 1.0).abs() <= 0.5);
        assert_eq!(p.velocity.y, 2.0);
        assert!((p.velocity.z - 3.0).abs() <= 1.0);
    }
    let xs: Vec<f32> = a.iter().map(|p| p.velocity.x).collect();
    assert!(xs.iter().cloned().fold(f32::MAX, f32::min) < 0.7 && xs.iter().cloned().fold(f32::MIN, f32::max) > 1.3);
}

#[test]
fn debris_sizes_vary_between_half_and_full() {
    let mut particles = Particles::new(200, 5);
    particles.block_break(Vec3::new(0.0, 0.0, 2.0), [0.5, 0.5, 0.5]);
    assert_eq!(particles.len(), 8);
    for p in particles.iter() {
        assert!(p.scale() <= 0.3 && p.scale() > 0.15 - 1e-6, "scale {}", p.scale());
    }
    let first = particles.iter().next().unwrap().scale();
    assert!(particles.iter().any(|p| p.scale() != first));
}

#[test]
fn emitter_spawns_on_a_timer_and_can_catch_up() {
    let mut particles = Particles::new(100, 3);
    let mut emitter = ParticleEmitter::new(Vec3::new(0.0, 0.0, 5.0));
    emitter.interval = 0.1;
    assert_eq!(emitter.update(0.05, &mut particles), 0);
    assert_eq!(emitter.update(0.06, &mut particles), 1);
    // A long frame spawns several, like the Java while loop.
    assert_eq!(emitter.update(0.35, &mut particles), 3);
    assert_eq!(particles.len(), 4);
    assert!(particles.iter().all(|p| p.position == Vec3::new(0.0, 0.0, 5.0)));
}

#[test]
fn inactive_emitter_spawns_nothing_and_toggle_restarts_it() {
    let mut particles = Particles::new(100, 3);
    let mut emitter = ParticleEmitter::new(Vec3::ZERO);
    emitter.toggle();
    assert_eq!(emitter.update(1.0, &mut particles), 0);
    emitter.toggle();
    assert_eq!(emitter.update(0.1, &mut particles), 1);
    emitter.interval = 0.0;
    assert_eq!(emitter.update(1.0, &mut particles), 0, "zero interval must not loop forever");
}

fn entity_at(entities: &mut Entities, position: Vec3) -> u32 {
    entities.spawn(Entity::new("e", 1).movable().at(position))
}

#[test]
fn landing_event_makes_dust_at_the_feet() {
    let world = flat();
    let mut entities = Entities::new();
    let id = entity_at(&mut entities, Vec3::new(3.0, 3.0, 1.0));
    let mut particles = Particles::new(100, 1);
    particles.handle_events(&[Event::Landed(id)], &entities, &world);
    assert_eq!(particles.len(), 6);
    for p in particles.iter() {
        assert_eq!(p.kind(), ParticleType::Smoke);
        assert_eq!((p.position.x, p.position.y), (3.0, 3.0));
        assert!((p.position.z - 1.05).abs() < 1e-6);
    }
}

#[test]
fn entering_water_splashes_from_the_surface() {
    let mut world = flat();
    // A pool: water at z = 1 and 2 around column (5, 5); the entity is two blocks deep.
    for z in 1..=2 {
        world.set(5, 5, z, Block::new(id::WATER, 0));
    }
    let (gx, gy) = crate::grid::to_iso(5, 5);
    let mut entities = Entities::new();
    let id = entity_at(&mut entities, Vec3::new(gx, gy, 1.2));
    let mut particles = Particles::new(100, 1);
    particles.handle_events(&[Event::EnteredLiquid(id)], &entities, &world);
    assert_eq!(particles.len(), 10);
    assert!(particles.iter().all(|p| p.position.z == 3.0 && p.kind() == ParticleType::Regular));
    assert!(particles.iter().all(|p| p.velocity.z > 1.4), "thrown upwards");
}

#[test]
fn collision_and_disposal_events_and_unknown_entities_make_no_particles() {
    let world = flat();
    let mut entities = Entities::new();
    let id = entity_at(&mut entities, Vec3::new(1.0, 1.0, 1.0));
    let mut particles = Particles::new(100, 1);
    particles.handle_events(&[Event::Collided(id), Event::Disposed(id), Event::Landed(999), Event::EnteredLiquid(999)], &entities, &world);
    assert!(particles.is_empty());
}

#[test]
fn droplets_fall_and_die_on_the_floor_or_by_time() {
    let world = flat();
    let mut particles = Particles::new(100, 1);
    particles.splash(Vec3::new(2.0, 2.0, 1.0));
    run(&mut particles, &world, 0.5);
    assert!(particles.iter().any(|p| p.position.z > 1.0));
    run(&mut particles, &world, 0.5);
    assert!(particles.is_empty(), "ttl 0.9 s");
}

// ---------------------------------------------------------------------------- emitter light

#[test]
fn an_emitter_has_no_light_until_it_gets_a_brightness() {
    let emitter = ParticleEmitter::new(Vec3::new(1.0, 2.0, 3.0));
    assert_eq!(emitter.light(), None);
}

#[test]
fn the_emitter_light_is_the_java_yellow_radius_5_light_at_the_emitter() {
    let mut emitter = ParticleEmitter::new(Vec3::new(1.0, 2.0, 3.0));
    emitter.set_brightness(11.0);
    let light = emitter.light().expect("brightness gives a light");
    assert_eq!(light.position, Vec3::new(1.0, 2.0, 3.0));
    assert_eq!(light.color, Vec3::new(1.0, 1.0, 0.0));
    assert_eq!(light.radius, 5.0);
    assert_eq!(light.brightness, 11.0);
}

#[test]
fn the_light_follows_the_emitter() {
    let mut emitter = ParticleEmitter::new(Vec3::ZERO);
    emitter.set_brightness(2.0);
    emitter.position = Vec3::new(4.0, 5.0, 6.0);
    assert_eq!(emitter.light().unwrap().position, Vec3::new(4.0, 5.0, 6.0));
}

#[test]
fn an_inactive_emitter_gives_no_light_and_toggling_brings_it_back() {
    let mut emitter = ParticleEmitter::new(Vec3::ZERO);
    emitter.set_brightness(5.0);
    emitter.toggle();
    assert_eq!(emitter.light(), None);
    emitter.toggle();
    assert_eq!(emitter.light().unwrap().brightness, 5.0);
}

#[test]
fn a_negative_brightness_removes_the_light() {
    let mut emitter = ParticleEmitter::new(Vec3::ZERO);
    emitter.set_brightness(5.0);
    emitter.set_brightness(-1.0);
    assert_eq!(emitter.light(), None);
    // Zero is a valid (dim) light, as in Java where only `< 0` disposes.
    emitter.set_brightness(0.0);
    assert_eq!(emitter.light().unwrap().brightness, 0.0);
}

// ---------------------------------------------------------------------------- debris shades

fn drifting(cycle: f32) -> Particles {
    let spec = ParticleSpec {
        gravity: 0.0,
        collides: false,
        ttl: 100.0,
        cycle_distance: cycle,
        color: [0.8, 0.6, 0.4, 1.0],
        ..ParticleSpec::regular()
    };
    one(spec, Vec3::new(0.0, 0.0, 5.0), Vec3::new(1.0, 0.0, 0.0))
}

#[test]
fn the_shade_stays_until_the_particle_has_travelled_the_cycle_distance() {
    let world = flat();
    let mut particles = drifting(0.06);
    // 1 block/s for 3 frames of 1/60 s is 0.05 blocks: not yet.
    run(&mut particles, &world, 3.0 * DT);
    let p = particles.iter().next().unwrap();
    assert_eq!(p.variant(), 0);
    assert_eq!(p.color()[..3], [0.8, 0.6, 0.4]);
}

#[test]
fn a_travelling_particle_cycles_through_shades_and_darkens_its_colour() {
    let world = flat();
    let mut particles = drifting(0.06);
    let mut seen = [false; 3];
    for _ in 0..600 {
        particles.update(&world, DT);
        let p = particles.iter().next().unwrap();
        let shade = SHADES[p.variant() as usize];
        assert!((p.color()[0] - 0.8 * shade).abs() < 1e-6);
        assert!((p.color()[2] - 0.4 * shade).abs() < 1e-6);
        seen[p.variant() as usize] = true;
    }
    assert_eq!(seen, [true; 3], "ten seconds of travel should have used all three shades");
}

#[test]
fn a_particle_that_does_not_move_keeps_its_shade() {
    let world = flat();
    let spec = ParticleSpec { cycle_distance: 0.06, collides: false, gravity: 0.0, ..ParticleSpec::regular() };
    let mut particles = one(spec, Vec3::new(0.0, 0.0, 5.0), Vec3::ZERO);
    run(&mut particles, &world, 1.0);
    assert_eq!(particles.iter().next().unwrap().variant(), 0);
}

#[test]
fn only_debris_cycles_by_default() {
    assert_eq!(ParticleSpec::debris([1.0; 3]).cycle_distance, 0.06);
    for spec in [ParticleSpec::regular(), ParticleSpec::smoke(), ParticleSpec::fire(), ParticleSpec::dust(), ParticleSpec::droplet()] {
        assert_eq!(spec.cycle_distance, 0.0);
    }
}

// ---------------------------------------------------------------------------- jetpack

#[test]
fn the_jetpack_emitter_is_off_until_lit_and_then_sprays_flame_downwards() {
    let mut particles = Particles::new(64, 1);
    let mut emitter = ParticleEmitter::jetpack();
    assert_eq!(emitter.update(1.0, &mut particles), 0, "off while the jetpack is not burning");
    assert_eq!(emitter.spec.ttl, 1.8, "Ejira's flame lives 1.8 s");
    assert_eq!(emitter.spec.color, [1.0, 0.8, 0.2, 0.7], "yellow-orange, 70 % opaque");
    emitter.active = true;
    emitter.velocity.z = -4.5; // rising at 3 blocks/s: the exhaust goes the other way, 1.5 times as fast
    emitter.position = Vec3::new(3.0, 4.0, 5.0);
    assert!(emitter.update(0.1, &mut particles) >= 3);
    let world = World::new(crate::generator::AirGenerator);
    particles.update(&world, 0.1);
    for p in particles.iter() {
        assert_eq!(p.kind(), ParticleType::Fire);
        assert!(p.velocity.z < 0.0, "flame goes down");
        assert!(p.size() > 0.0 && p.color()[3] > 0.0, "visible after a frame: size {} alpha {}", p.size(), p.color()[3]);
    }
}
