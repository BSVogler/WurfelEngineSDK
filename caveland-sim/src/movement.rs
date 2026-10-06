//! How much a player can steer, by how fast they move (not in the Java game).
//!
//! A launch from a catapult or cannon only sets a velocity. What follows is normal movement, with
//! one rule on top: the walking input is scaled by a control factor that depends on the speed. Up to
//! [`cutoff_speed`] the player has full control, so ordinary running and jumping feel as always.
//! Above it control falls along a smoothstep to [`MIN_CONTROL`] at [`top_speed`]; a flying player
//! can still nudge their path a little but cannot walk through the air. On the ground friction slows
//! the player and control comes back by itself; in the air only [`AIR_DRAG_PER_SECOND`] slows them.
//!
//! Fast things also bounce ([`bounce`]): a collision at or above [`bounce_threshold`] reflects the
//! velocity about the surface and keeps [`RESTITUTION`] of it, never with damage. Speeds below the
//! threshold are left to the engine (stop or slide), and a bounce that would leave less than
//! [`REST_SPEED`] along the normal ends in rest, so nothing jitters forever. The bounce loses speed,
//! so control returns by itself as the speed falls.
//!
//! Speed is measured relative to what the player stands on (a cart, a lift basket), so riding at the
//! cart's speed is not "fast". Everything here is a pure function of numbers and runs in the fixed
//! step of the shared rules, so a client predicts the same as the server.

use glam::{Vec2, Vec3};
use wurfel_sim::entity::physics::{collides_with_world, is_on_ceil};
use wurfel_sim::entity::{Body, Entity};
use wurfel_sim::grid::{from_iso, to_iso};
use wurfel_sim::player::JUMP_SPEED;
use wurfel_sim::World;

use crate::blocks::ids;

/// The cutoff is this much above the fastest speed a normal run plus jump reaches.
pub const CUTOFF_MARGIN: f32 = 1.1;
/// Control stops falling at this multiple of the cutoff speed.
pub const TOP_SPEED_FACTOR: f32 = 3.0;
/// The share of the steering that is left at the top speed and above: small but never zero.
pub const MIN_CONTROL: f32 = 0.05;
/// Share of the horizontal speed an airborne body loses per second.
pub const AIR_DRAG_PER_SECOND: f32 = 0.2;
/// Collisions at this multiple of the cutoff speed or more bounce; slower ones do not, so ordinary
/// running, jumping and landing never bounce.
pub const BOUNCE_THRESHOLD_FACTOR: f32 = 1.0;
/// The share of the velocity a bounce keeps.
pub const RESTITUTION: f32 = 0.4;
/// A bounce that would leave less than this speed (blocks per second) along the surface normal
/// does not happen: the thing comes to rest against the surface instead.
pub const REST_SPEED: f32 = 3.0;
/// The swept collision looks at the path every this many blocks, which is less than any wall is thick.
pub const SWEEP_STEP: f32 = 0.2;

/// The fastest a normal run plus jump from base level gets: the running speed and the jump's
/// upward speed together (a jump comes back down at the speed it left with).
pub fn normal_top_speed(walking_speed: f32) -> f32 {
    (walking_speed * walking_speed + JUMP_SPEED * JUMP_SPEED).sqrt()
}

/// Up to this speed (blocks per second, relative to the platform) the player has full control.
pub fn cutoff_speed(walking_speed: f32) -> f32 {
    normal_top_speed(walking_speed) * CUTOFF_MARGIN
}

/// From this speed on, control is at its minimum.
pub fn top_speed(walking_speed: f32) -> f32 {
    cutoff_speed(walking_speed) * TOP_SPEED_FACTOR
}

fn smoothstep(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// How much of the player's steering applies at this speed, from 1.0 (all of it) down to
/// [`MIN_CONTROL`].
pub fn control_factor(relative_speed: f32, walking_speed: f32) -> f32 {
    let (cutoff, top) = (cutoff_speed(walking_speed), top_speed(walking_speed));
    let t = (relative_speed - cutoff) / (top - cutoff);
    1.0 - (1.0 - MIN_CONTROL) * smoothstep(t)
}

/// Move the horizontal velocity `current` towards what the keys ask for, `desired`, by `control`.
pub fn steer(current: Vec2, desired: Vec2, control: f32) -> Vec2 {
    current + (desired - current) * control
}

/// The light drag of the air on the horizontal speed for one step of `dt` seconds.
pub fn air_drag(body: &mut Body, dt: f32) {
    let keep = (1.0 - AIR_DRAG_PER_SECOND * dt).max(0.0);
    let hor = body.hor_movement() * keep;
    body.set_hor_movement(hor);
}

/// From this speed on, a collision bounces.
pub fn bounce_threshold(walking_speed: f32) -> f32 {
    cutoff_speed(walking_speed) * BOUNCE_THRESHOLD_FACTOR
}

/// The way a ramp piece of rails (block values 6 to 9) rises across its cell, as a unit vector in the
/// ground frame: the ramps climb along an axis of the frame, one block up over one block.
fn ramp_uphill(value: u8) -> Option<Vec2> {
    Some(match value {
        6 => Vec2::new(0.0, -1.0),
        7 => Vec2::new(-1.0, 0.0),
        8 => Vec2::new(0.0, 1.0),
        9 => Vec2::new(1.0, 0.0),
        _ => return None,
    })
}

/// The unit normal of a ramp's surface: up, and against the way it climbs.
pub fn ramp_normal(value: u8) -> Option<Vec3> {
    let u = ramp_uphill(value)?;
    Some(Vec3::new(-u.x, -u.y, 1.0).normalize())
}

/// What stops a body at `p`: `Some(None)` is a solid block (the normal is found from the way the
/// body came), `Some(Some(n))` a ramp's slope with its normal. Ramps are not obstacles to the
/// engine; for the bounce their surface is solid.
fn hit(world: &World, p: Vec3, height: f32, rising: bool) -> Option<Option<Vec3>> {
    if collides_with_world(world, p, height) || (rising && is_on_ceil(world, p, height)) {
        return Some(None);
    }
    let (x, y) = from_iso(p.x, p.y);
    let z = p.z.floor() as i32;
    let block = world.get(x, y, z);
    if matches!(block.id(), ids::RAILS | ids::BOOSTER_RAILS) {
        let u = ramp_uphill(block.value())?;
        let (cx, cy) = to_iso(x, y);
        let surface = z as f32 + 0.5 + (p.x - cx) * u.x + (p.y - cy) * u.y;
        if p.z < surface {
            return Some(ramp_normal(block.value()));
        }
    }
    None
}

/// Bounce `entity` if it is about to hit something at bounce speed during this step of `dt` seconds.
/// Call it before the engine moves the entity. The path of the step is swept in pieces of
/// [`SWEEP_STEP`], so a fast shot cannot pass through a thin wall; the entity is put at the point of
/// contact and gets the reflected velocity. Returns whether it bounced. Slower collisions are left
/// to the engine.
pub fn bounce(world: &World, entity: &mut Entity, walking_speed: f32, dt: f32) -> bool {
    let height = entity.dimension_z;
    let Some(body) = entity.body.as_mut() else { return false };
    let v = body.movement;
    let threshold = bounce_threshold(walking_speed);
    if !body.collider || body.floating || v.length() < threshold {
        return false;
    }
    let start = entity.position;
    if hit(world, start, height, v.z > 0.0).is_some() {
        return false; // already inside something: the engine sorts that out
    }
    let step = v * dt;
    let pieces = (step.length() / SWEEP_STEP).ceil().max(1.0) as u32;
    let mut free = start;
    let mut found = None;
    for i in 1..=pieces {
        let p = start + step * (i as f32 / pieces as f32);
        match hit(world, p, height, v.z > 0.0) {
            Some(kind) => {
                found = Some((p, kind));
                break;
            }
            None => free = p,
        }
    }
    let Some((mut blocked, mut kind)) = found else { return false };
    // Narrow the contact down between the last free point and the first blocked one.
    for _ in 0..8 {
        let mid = (free + blocked) * 0.5;
        match hit(world, mid, height, v.z > 0.0) {
            Some(k) => {
                blocked = mid;
                kind = k;
            }
            None => free = mid,
        }
    }
    let normal = kind.unwrap_or_else(|| block_normal(world, free, height, v));
    let impact = -v.dot(normal);
    if impact < threshold {
        return false;
    }
    let mut out = (v - normal * (2.0 * v.dot(normal))) * RESTITUTION;
    if out.dot(normal) < REST_SPEED {
        out -= normal * out.dot(normal); // too slow to bounce: it slides or rests
    }
    entity.position = free;
    if let Some(body) = entity.body.as_mut() {
        body.set_movement(out);
    }
    true
}

/// The normal of a block face the body ran into at `at`: against every axis it cannot go on.
fn block_normal(world: &World, at: Vec3, height: f32, v: Vec3) -> Vec3 {
    const PROBE: f32 = 0.02;
    let mut n = Vec3::ZERO;
    for axis in 0..3 {
        if v[axis] == 0.0 {
            continue;
        }
        let mut probe = Vec3::ZERO;
        probe[axis] = PROBE * v[axis].signum();
        if hit(world, at + probe, height, v.z > 0.0).is_some() {
            n[axis] = -v[axis].signum();
        }
    }
    if n == Vec3::ZERO {
        // Only the corner of something: take the way it moves fastest.
        let axis = if v.x.abs() >= v.y.abs() && v.x.abs() >= v.z.abs() { 0 } else if v.y.abs() >= v.z.abs() { 1 } else { 2 };
        n[axis] = -v[axis].signum();
    }
    n.normalize()
}

#[cfg(test)]
mod tests {
    use super::*;
    use wurfel_sim::player::WALKING_SPEED;

    #[test]
    fn the_cutoff_is_just_above_the_fastest_run_and_jump() {
        let fastest = normal_top_speed(WALKING_SPEED);
        assert!(cutoff_speed(WALKING_SPEED) > fastest && cutoff_speed(WALKING_SPEED) < fastest * 1.5);
        // Derived: a faster runner moves the cutoff along.
        assert!(cutoff_speed(WALKING_SPEED + 2.0) > cutoff_speed(WALKING_SPEED));
    }

    #[test]
    fn control_is_full_up_to_the_cutoff() {
        let cutoff = cutoff_speed(WALKING_SPEED);
        for s in [0.0, 1.0, WALKING_SPEED, normal_top_speed(WALKING_SPEED), cutoff] {
            assert_eq!(control_factor(s, WALKING_SPEED), 1.0, "speed {s}");
        }
    }

    #[test]
    fn control_falls_smoothly_and_never_rises_above_the_cutoff() {
        let (cutoff, top) = (cutoff_speed(WALKING_SPEED), top_speed(WALKING_SPEED));
        let mut last = 1.0;
        for i in 0..=200 {
            let s = cutoff + (top - cutoff) * i as f32 / 200.0;
            let c = control_factor(s, WALKING_SPEED);
            assert!(c <= last + 1e-6, "monotonic at {s}");
            last = c;
        }
        // Smoothstep, not linear: a gentle start, so a quarter of the way in most control is left.
        let quarter = control_factor(cutoff + (top - cutoff) * 0.25, WALKING_SPEED);
        assert!(quarter > 1.0 - 0.25 * (1.0 - MIN_CONTROL), "{quarter}");
        let three_quarters = control_factor(cutoff + (top - cutoff) * 0.75, WALKING_SPEED);
        assert!(three_quarters < 1.0 - 0.75 * (1.0 - MIN_CONTROL), "{three_quarters}");
    }

    #[test]
    fn control_never_drops_below_the_minimum() {
        let top = top_speed(WALKING_SPEED);
        assert!((control_factor(top, WALKING_SPEED) - MIN_CONTROL).abs() < 1e-6);
        assert_eq!(control_factor(top * 10.0, WALKING_SPEED), control_factor(top, WALKING_SPEED));
        assert!(MIN_CONTROL > 0.0);
    }

    #[test]
    fn steering_moves_the_velocity_by_the_control() {
        let (cur, want) = (Vec2::new(10.0, 0.0), Vec2::new(0.0, 4.0));
        assert_eq!(steer(cur, want, 1.0), want);
        assert_eq!(steer(cur, want, 0.0), cur);
        assert_eq!(steer(cur, want, 0.5), Vec2::new(5.0, 2.0));
    }

    #[test]
    fn speed_is_measured_against_the_platform() {
        // A cart rolling at 15 blocks per second with the rider standing still in it: no penalty.
        let cart = glam::Vec3::new(15.0, 0.0, 0.0);
        let rider = glam::Vec3::new(15.0, 0.0, 0.0);
        assert_eq!(control_factor((rider - cart).length(), WALKING_SPEED), 1.0);
        assert!(control_factor(rider.length(), WALKING_SPEED) < 1.0, "the same speed over the ground is fast");
    }

    #[test]
    fn the_air_drag_is_light() {
        let mut body = Body::default();
        body.set_movement(glam::Vec3::new(10.0, 0.0, 3.0));
        for _ in 0..60 {
            air_drag(&mut body, 1.0 / 60.0);
        }
        let kept = body.movement.x / 10.0;
        assert!(kept > 0.75 && kept < 0.85, "a second of drag keeps about 80 %: {kept}");
        assert_eq!(body.movement.z, 3.0, "gravity's business, not the drag's");
    }

    // ---- bouncing ----------------------------------------------------------------------------

    use wurfel_sim::block::Block;
    use wurfel_sim::player::new_player;
    use wurfel_sim::AirGenerator;

    const DT: f32 = 1.0 / 60.0;

    /// A stone floor (surface at height 1) and a wall three blocks high at ground x = 5.
    fn arena() -> World {
        let mut world = World::new(AirGenerator);
        crate::game::Caveland::install(&mut world);
        for gx in -10..=10 {
            for gy in -20..=20 {
                let (x, y) = from_iso(gx as f32, gy as f32);
                world.set(x, y, 0, Block::new(wurfel_sim::block::id::STONE, 0));
                if gx == 5 {
                    for z in 1..4 {
                        world.set(x, y, z, Block::new(wurfel_sim::block::id::STONE, 0));
                    }
                }
            }
        }
        world
    }

    fn mover(x: f32, vx: f32) -> Entity {
        let mut e = new_player(Vec3::new(x, 0.0, 1.0));
        e.body.as_mut().unwrap().set_movement(Vec3::new(vx, 0.0, 0.0));
        e
    }

    #[test]
    fn below_the_threshold_nothing_bounces() {
        let world = arena();
        let slow = bounce_threshold(WALKING_SPEED) - 0.5;
        let mut e = mover(4.0, slow);
        assert!(!bounce(&world, &mut e, WALKING_SPEED, DT));
        assert_eq!(e.body.as_ref().unwrap().movement.x, slow, "the engine stops it as before");
        assert_eq!(e.position, Vec3::new(4.0, 0.0, 1.0));
    }

    #[test]
    fn above_the_threshold_the_velocity_is_reflected_and_scaled() {
        let world = arena();
        let mut e = mover(4.0, 20.0);
        e.body.as_mut().unwrap().movement.y = 3.0;
        assert!(bounce(&world, &mut e, WALKING_SPEED, DT));
        let v = e.body.as_ref().unwrap().movement;
        assert!((v.x + 20.0 * RESTITUTION).abs() < 1e-4, "bounced back with the restitution: {v}");
        assert!((v.y - 3.0 * RESTITUTION).abs() < 1e-4, "{v}");
        assert!(e.position.x < 4.5, "stopped in front of the wall: {}", e.position);
    }

    #[test]
    fn a_bounce_that_would_leave_less_than_the_rest_speed_is_a_rest() {
        let world = arena();
        let speed = bounce_threshold(WALKING_SPEED) + 0.01;
        assert!(speed * RESTITUTION < REST_SPEED, "the test needs a slow bounce");
        let mut e = mover(4.2, speed);
        assert!(bounce(&world, &mut e, WALKING_SPEED, DT));
        assert_eq!(e.body.as_ref().unwrap().movement.x, 0.0, "no jitter: it just stops");
    }

    #[test]
    fn nothing_gets_through_the_wall_at_any_speed() {
        let world = arena();
        for speed in [10.0, 20.0, 40.0, 80.0, 150.0] {
            let mut entities = wurfel_sim::entity::Entities::new();
            let id = entities.spawn(mover(1.0, speed));
            let mut passed = false;
            for _ in 0..120 {
                if let Some(e) = entities.get_mut(id) {
                    bounce(&world, e, WALKING_SPEED, DT);
                }
                entities.update(&world, DT);
                passed |= entities.get(id).unwrap().position.x > 5.5;
            }
            assert!(!passed, "speed {speed} went through a one block wall");
        }
    }

    #[test]
    fn the_floor_bounces_a_fall_from_a_great_height_and_settles() {
        let world = arena();
        let mut entities = wurfel_sim::entity::Entities::new();
        let mut e = new_player(Vec3::new(0.0, 0.0, 20.0));
        e.body.as_mut().unwrap().set_movement(Vec3::ZERO);
        let id = entities.spawn(e);
        let (mut bounces, mut health_low) = (0, 100.0f32);
        for _ in 0..1200 {
            let e = entities.get_mut(id).unwrap();
            if bounce(&world, e, WALKING_SPEED, DT) {
                bounces += 1;
            }
            entities.update(&world, DT);
            health_low = health_low.min(entities.get(id).unwrap().health());
        }
        let e = entities.get(id).unwrap();
        assert!(bounces >= 1 && bounces < 6, "a few bounces, not forever: {bounces}");
        assert!(e.body.as_ref().unwrap().movement.length() < 0.5 && (e.position.z - 1.0).abs() < 0.05, "at rest on the floor: {}", e.position);
        assert_eq!(health_low, 100.0, "never any damage");
    }

    #[test]
    fn a_ramp_gives_the_slope_normal() {
        // Ramp 6 climbs towards -y: the surface faces up and towards +y.
        let n = ramp_normal(6).unwrap();
        assert!((n - Vec3::new(0.0, 1.0, 1.0).normalize()).length() < 1e-6);
        assert!(ramp_normal(5).is_none());
        let mut world = arena();
        let (x, y) = from_iso(0.0, 0.0);
        world.set(x, y, 1, Block::new(ids::RAILS, 6));
        // Coming down fast on the ramp: reflected about the slope, not straight up.
        let mut e = new_player(Vec3::new(0.0, 0.0, 1.8));
        e.body.as_mut().unwrap().set_movement(Vec3::new(0.0, 0.0, -20.0));
        assert!(bounce(&world, &mut e, WALKING_SPEED, DT));
        let v = e.body.as_ref().unwrap().movement;
        assert!(v.y > 1.0 && v.z.abs() < 1e-3, "a straight fall on a 45 degree slope goes off sideways, down the slope: {v}");
        let expected = {
            let v0 = Vec3::new(0.0, 0.0, -20.0);
            (v0 - n * (2.0 * v0.dot(n))) * RESTITUTION
        };
        assert!((v - expected).length() < 1e-3, "{v} vs {expected}");
        assert!(e.position.z < 1.8 && e.position.z > 1.4, "stopped on the surface: {}", e.position);
    }

    #[test]
    fn the_bounce_is_deterministic() {
        let world = arena();
        let run = || {
            let mut e = mover(0.0, 33.0);
            e.body.as_mut().unwrap().movement.z = 5.0;
            let mut entities = wurfel_sim::entity::Entities::new();
            let id = entities.spawn(e);
            let mut trail = Vec::new();
            for _ in 0..300 {
                let e = entities.get_mut(id).unwrap();
                bounce(&world, e, WALKING_SPEED, DT);
                entities.update(&world, DT);
                trail.push(entities.get(id).unwrap().position);
            }
            trail
        };
        assert_eq!(run(), run());
    }
}
