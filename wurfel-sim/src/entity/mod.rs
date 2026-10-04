//! Entity system, ported from the Java `AbstractEntity` / `MovableEntity`.
//!
//! The Java classes are an inheritance chain; here an [`Entity`] has an optional [`Body`] (the
//! `MovableEntity` part: velocity, gravity, collision) and a list of [`Component`]s. Entities live
//! in [`Entities`], which runs the update loop.
//!
//! Differences from the Java engine, besides the unit change described in [`physics`]:
//! * time steps are in seconds, not milliseconds (friction is still per millisecond, as its
//!   default was tuned for that);
//! * the Java `MessageManager` telegrams (`collided`, `landed`...) become [`Event`]s returned from
//!   [`Entities::update`];
//! * sound, sprite animation and editor handling are not part of the simulation;
//! * components are updated once per step (the Java `MoveToAi` was updated twice);
//! * two Java quirks are kept on purpose: the sideways collision test never checks the head (see
//!   [`physics::collides_with_world`]), and the push between two colliding entities uses the
//!   original impulse formula (see `Entities::resolve_entity_collisions`).

pub mod ai;
pub mod physics;

#[cfg(test)]
mod tests;

use glam::{Vec2, Vec3};

use crate::World;
use physics::{is_in_liquid, is_on_ceil, is_on_ground, collides_with_world, DEFAULT_FRICTION, GRAVITY, UNIT};

pub type EntityId = u32;

/// Entities within this distance of each other (sum of their radii) collide: 50 game units each.
pub const COLLISION_RADIUS: f32 = 50.0 * UNIT;
/// The Java engine tests sideways movement against walls over at most 30 game units per update.
const MAX_PROBE_DISTANCE: f32 = 30.0 * UNIT;
/// Lighter entities do not push others; they only get pushed.
const MIN_PUSHING_MASS: f32 = 0.5;
const ENTITY_IMPULSE_LIMIT: f32 = 20.0;

/// Things that happened during an update, for sound, particles, game rules and networking.
/// Replaces the Java `Events` telegrams.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    /// Hit a wall or the ground, or bumped into another entity.
    Collided(EntityId),
    /// Touched the ground after falling.
    Landed(EntityId),
    EnteredLiquid(EntityId),
    /// Removed from the world (health reached zero, or disposed explicitly).
    Disposed(EntityId),
}

/// Behaviour attached to an entity (the Java `Component`). It gets the parent entity to read and
/// steer, and the world for block queries.
pub trait Component: Send {
    /// Returns `false` when the component is finished and should be removed.
    fn update(&mut self, parent: &mut Entity, world: &World, dt: f32) -> bool;
}

/// Maps the keys of a player to movement (the Java `Controllable`).
pub trait Controllable {
    /// `speed` in blocks per second. Directions are screen directions: `up` is away from the viewer.
    fn walk(&mut self, up: bool, down: bool, left: bool, right: bool, speed: f32);
}

/// Screen-aligned direction (x right, y towards the viewer: the Java engine's frame) to the
/// isometric ground frame used here.
pub fn screen_to_iso(v: Vec2) -> Vec2 {
    let s = std::f32::consts::FRAC_1_SQRT_2;
    Vec2::new(s * (v.x + v.y), s * (v.y - v.x))
}

/// Inverse of [`screen_to_iso`].
pub fn iso_to_screen(v: Vec2) -> Vec2 {
    let s = std::f32::consts::FRAC_1_SQRT_2;
    Vec2::new(s * (v.x - v.y), s * (v.x + v.y))
}

/// The physical part of an entity (the Java `MovableEntity` state).
#[derive(Debug, Clone)]
pub struct Body {
    /// Velocity in blocks per second.
    pub movement: Vec3,
    orientation: Vec2,
    /// Collides with blocks. Without it the entity passes through the world.
    pub collider: bool,
    /// Not affected by gravity.
    pub floating: bool,
    /// How fast the entity slows down on the ground. Per millisecond; 0 disables sliding.
    pub friction: f32,
    /// Jump velocity in blocks per second, if this entity can jump (see [`Entity::jump`]).
    pub jump_speed: Option<f32>,
    in_liquid: bool,
}

impl Default for Body {
    fn default() -> Self {
        Body {
            movement: Vec3::ZERO,
            orientation: screen_to_iso(Vec2::X),
            collider: true,
            floating: false,
            friction: DEFAULT_FRICTION,
            jump_speed: None,
            in_liquid: false,
        }
    }
}

impl Body {
    /// Unit vector of the direction last moved in; kept while standing still.
    pub fn orientation(&self) -> Vec2 {
        self.orientation
    }

    pub fn hor_movement(&self) -> Vec2 {
        self.movement.truncate()
    }

    pub fn speed_hor(&self) -> f32 {
        self.hor_movement().length()
    }

    pub fn speed(&self) -> f32 {
        self.movement.length()
    }

    fn update_orientation(&mut self) {
        // Only update if there is new information, otherwise keep facing the same way.
        if self.hor_movement().length_squared() != 0.0 {
            self.orientation = self.hor_movement().normalize();
        }
    }

    pub fn set_movement(&mut self, movement: Vec3) {
        self.movement = movement;
        self.update_orientation();
    }

    pub fn add_movement(&mut self, movement: Vec3) {
        self.movement += movement;
        self.update_orientation();
    }

    pub fn set_hor_movement(&mut self, movement: Vec2) {
        self.movement.x = movement.x;
        self.movement.y = movement.y;
        self.update_orientation();
    }

    /// Set the horizontal speed along the current orientation.
    pub fn set_speed_horizontal(&mut self, speed: f32) {
        self.movement.x = self.orientation.x * speed;
        self.movement.y = self.orientation.y * speed;
    }

    /// Turn without losing speed. `orientation` must be normalised.
    pub fn set_orientation(&mut self, orientation: Vec2) {
        let speed = self.speed_hor();
        self.orientation = orientation;
        self.movement.x = orientation.x * speed;
        self.movement.y = orientation.y * speed;
    }

    /// Jump with a given vertical velocity, keeping horizontal movement. Works in mid-air.
    pub fn jump_with(&mut self, velocity: f32) {
        self.movement.z = velocity;
    }
}

pub struct Entity {
    id: EntityId,
    pub name: String,
    /// Which sprite represents the entity (the Java `spriteId` / `value`).
    pub sprite_id: u8,
    pub sprite_value: u8,
    /// Position of the feet, in blocks.
    pub position: Vec3,
    /// Height of the entity in blocks.
    pub dimension_z: f32,
    pub mass: f32,
    health: f32,
    pub indestructible: bool,
    /// Present for entities that move and collide.
    pub body: Option<Body>,
    components: Vec<Box<dyn Component>>,
    disposed: bool,
}

impl Entity {
    /// A static entity, e.g. a decoration. Call [`Entity::movable`] for a physical one.
    pub fn new(name: &str, sprite_id: u8) -> Self {
        Entity {
            id: 0,
            name: name.to_string(),
            sprite_id,
            sprite_value: 0,
            position: Vec3::ZERO,
            dimension_z: 1.0,
            mass: 0.4,
            health: 100.0,
            indestructible: false,
            body: None,
            components: Vec::new(),
            disposed: false,
        }
    }

    pub fn movable(mut self) -> Self {
        self.body = Some(Body::default());
        self
    }

    pub fn at(mut self, position: Vec3) -> Self {
        self.position = position;
        self
    }

    /// Assigned when the entity is added to [`Entities`]; 0 before that.
    pub fn id(&self) -> EntityId {
        self.id
    }

    pub fn health(&self) -> f32 {
        self.health
    }

    pub fn set_health(&mut self, health: f32) {
        self.health = health.clamp(0.0, 100.0);
    }

    pub fn take_damage(&mut self, amount: f32) {
        if !self.indestructible {
            self.set_health(self.health - amount);
        }
    }

    pub fn heal(&mut self, amount: f32) {
        self.set_health(self.health + amount);
    }

    /// Mark for removal; [`Entities::update`] removes it and reports [`Event::Disposed`].
    pub fn dispose(&mut self) {
        self.disposed = true;
    }

    pub fn is_disposed(&self) -> bool {
        self.disposed
    }

    pub fn add_component(&mut self, component: Box<dyn Component>) {
        self.components.push(component);
    }

    pub fn component_count(&self) -> usize {
        self.components.len()
    }

    /// Do the collision spheres overlap? (`collidesWith`)
    pub fn collides_with(&self, other: &Entity) -> bool {
        self.position.distance_squared(other.position) < (2.0 * COLLISION_RADIUS).powi(2)
    }

    pub fn is_on_ground(&self, world: &World) -> bool {
        match &self.body {
            Some(_) => is_on_ground(world, self.position, self.dimension_z),
            // Static entities only look at the single point below them.
            None => self.position.z <= 0.0 || world.blocks().is_obstacle(physics::block_at(world, self.position - Vec3::Z * UNIT)),
        }
    }

    pub fn is_in_liquid(&self, world: &World) -> bool {
        is_in_liquid(world, self.position)
    }

    /// Jump if this entity can, and it stands on something. Returns whether it jumped.
    pub fn jump(&mut self, world: &World) -> bool {
        let (position, dimension_z) = (self.position, self.dimension_z);
        let Some(body) = self.body.as_mut() else { return false };
        let Some(speed) = body.jump_speed else { return false };
        if !body.floating && !is_on_ground(world, position, dimension_z) {
            return false;
        }
        body.jump_with(speed);
        true
    }
}

impl Controllable for Entity {
    fn walk(&mut self, up: bool, down: bool, left: bool, right: bool, speed: f32) {
        let Some(body) = self.body.as_mut() else { return };
        // Left wins over right and up over down, like the original.
        let screen = Vec2::new(
            if left { -1.0 } else if right { 1.0 } else { 0.0 },
            if up { -1.0 } else if down { 1.0 } else { 0.0 },
        )
        .normalize_or_zero();
        if screen == Vec2::ZERO {
            return; // no keys: friction brings the entity to a stop
        }
        let dir = screen_to_iso(screen);
        body.set_orientation(dir);
        body.set_hor_movement(dir * speed);
    }
}

/// All entities of a world, ordered by id so updates are deterministic.
#[derive(Default)]
pub struct Entities {
    entities: Vec<Entity>,
    next_id: EntityId,
}

impl Entities {
    pub fn new() -> Self {
        Entities { entities: Vec::new(), next_id: 1 }
    }

    pub fn spawn(&mut self, mut entity: Entity) -> EntityId {
        entity.id = self.next_id;
        self.next_id += 1;
        let id = entity.id;
        self.entities.push(entity);
        id
    }

    /// Add an entity under a chosen id, so something that was removed (a player who died) can come
    /// back as the same one. Returns `false`, and adds nothing, if the id is in use or is 0.
    pub fn spawn_as(&mut self, id: EntityId, mut entity: Entity) -> bool {
        if id == 0 {
            return false;
        }
        match self.entities.binary_search_by_key(&id, |e| e.id) {
            Ok(_) => false,
            Err(at) => {
                entity.id = id;
                self.entities.insert(at, entity);
                self.next_id = self.next_id.max(id + 1);
                true
            }
        }
    }

    fn index_of(&self, id: EntityId) -> Option<usize> {
        // Ids only grow and removal keeps order, so the vector stays sorted by id.
        self.entities.binary_search_by_key(&id, |e| e.id).ok()
    }

    pub fn get(&self, id: EntityId) -> Option<&Entity> {
        self.index_of(id).map(|i| &self.entities[i])
    }

    pub fn get_mut(&mut self, id: EntityId) -> Option<&mut Entity> {
        self.index_of(id).map(|i| &mut self.entities[i])
    }

    pub fn remove(&mut self, id: EntityId) -> Option<Entity> {
        self.index_of(id).map(|i| self.entities.remove(i))
    }

    pub fn iter(&self) -> impl Iterator<Item = &Entity> {
        self.entities.iter()
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut Entity> {
        self.entities.iter_mut()
    }

    pub fn len(&self) -> usize {
        self.entities.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entities.is_empty()
    }

    /// Entities whose collision sphere overlaps `id`'s (`getCollidingEntities`).
    pub fn colliding(&self, id: EntityId) -> Vec<EntityId> {
        let Some(entity) = self.get(id) else { return Vec::new() };
        self.entities.iter().filter(|o| o.id != id && entity.collides_with(o)).map(|o| o.id).collect()
    }

    /// Advance every entity by `dt` seconds. Entities that died are removed afterwards.
    pub fn update(&mut self, world: &World, dt: f32) -> Vec<Event> {
        let mut events = Vec::new();
        for i in 0..self.entities.len() {
            self.update_entity(i, world, dt, &mut events);
        }
        self.entities.retain(|e| !e.disposed);
        events
    }

    fn update_entity(&mut self, i: usize, world: &World, dt: f32, events: &mut Vec<Event>) {
        let id = self.entities[i].id;
        if self.entities[i].disposed {
            return;
        }
        if self.entities[i].health <= 0.0 && !self.entities[i].indestructible {
            self.entities[i].disposed = true;
            events.push(Event::Disposed(id));
            return;
        }

        // Components first, like `super.update` in the Java `MovableEntity.update`. Components
        // added while running are kept.
        let mut components = std::mem::take(&mut self.entities[i].components);
        components.retain_mut(|c| c.update(&mut self.entities[i], world, dt));
        components.append(&mut self.entities[i].components);
        self.entities[i].components = components;

        if self.entities[i].body.is_some() {
            self.update_physics(i, world, dt, events);
        }
    }

    /// The body of `MovableEntity.update`.
    fn update_physics(&mut self, i: usize, world: &World, dt: f32, events: &mut Vec<Event>) {
        let id = self.entities[i].id;
        let dim = self.entities[i].dimension_z;

        let (new_position, old_height) = {
            let entity = &mut self.entities[i];
            let position = entity.position;
            let body = entity.body.as_mut().expect("checked by the caller");

            // Horizontal: stop dead if the next step would end inside a wall.
            let step = Vec3::new(body.movement.x * dt, body.movement.y * dt, 0.0).clamp_length_max(MAX_PROBE_DISTANCE);
            if body.collider && collides_with_world(world, position + step, dim) {
                body.set_hor_movement(Vec2::ZERO);
                events.push(Event::Collided(id));
            }

            // Vertical: gravity, then a ceiling check.
            let old_height = position.z;
            if !body.floating && !is_on_ground(world, position, dim) {
                body.add_movement(Vec3::new(0.0, 0.0, -GRAVITY * dt));
            }
            let mut new_position = position + body.movement * dt;
            if body.collider && body.movement.z > 0.0 && is_on_ceil(world, new_position, dim) {
                body.movement.z = 0.0;
                new_position = position + body.movement * dt;
            }
            (new_position, old_height)
        };

        let pushes = {
            let e = &self.entities[i];
            e.mass > MIN_PUSHING_MASS && e.body.as_ref().is_some_and(|b| b.collider)
        };
        if pushes {
            self.resolve_entity_collisions(i, events);
        }

        let entity = &mut self.entities[i];
        entity.position = new_position;
        let position = entity.position;
        let body = entity.body.as_mut().expect("checked by the caller");
        // (set_movement/add_movement keep the orientation current; this covers direct writes.)
        body.update_orientation();

        // Landing: the move has been applied, now see whether it ended on the ground.
        if body.movement.z < 0.0 && is_on_ground(world, position, dim) {
            if body.collider {
                body.movement.z = 0.0;
            }
            events.push(Event::Collided(id));
            if !body.floating {
                events.push(Event::Landed(id));
            }
            if body.collider {
                // Rest on the floor of the cell the entity occupied before this step.
                entity.position.z = old_height.trunc();
            }
        }

        let body = entity.body.as_mut().expect("checked by the caller");
        let now_in_liquid = is_in_liquid(world, entity.position);
        if !body.in_liquid && now_in_liquid {
            events.push(Event::EnteredLiquid(id));
        }
        body.in_liquid = now_in_liquid;

        // Friction slows walking down, but only on the ground.
        if is_on_ground(world, entity.position, dim) {
            let body = entity.body.as_mut().expect("checked by the caller");
            if body.movement.x * body.movement.x + body.movement.y * body.movement.y > 0.1 {
                let factor = 1.0 / (dt * 1000.0 * body.friction + 1.0); // always below 1
                let hor = body.hor_movement() * factor;
                body.set_hor_movement(hor);
            } else {
                body.set_hor_movement(Vec2::ZERO);
            }
        }
    }

    /// Push `i` and whatever it overlaps apart (`checkEntColl`). Each heavy entity does this for
    /// itself, so two heavy entities resolve the same contact twice, and the original impulse
    /// formula (including the factor 2 and the way the second entity's share is scaled) is kept.
    fn resolve_entity_collisions(&mut self, i: usize, events: &mut Vec<Event>) {
        for j in 0..self.entities.len() {
            if j == i {
                continue;
            }
            let (a, b) = two_mut(&mut self.entities, i, j);
            if !b.body.as_ref().is_some_and(|body| body.collider) || !a.collides_with(b) {
                continue;
            }

            let delta = a.position - b.position;
            let horizontal = Vec2::new(delta.x, delta.y);
            let distance = horizontal.length();
            // Direction pointing from `b` to `a`. The Java code divides by zero when two entities
            // sit at exactly the same spot; pick a direction instead.
            let n = if distance > 1e-6 { horizontal / distance } else { Vec2::X };

            let (body_a, body_b) = (a.body.as_mut().unwrap(), b.body.as_mut().unwrap());
            let approach_speed = (body_a.hor_movement() - body_b.hor_movement()).dot(n);
            // Only act on entities that move towards each other.
            if approach_speed <= 0.0 {
                let inv_a = 1.0 / a.mass.max(1e-3);
                let inv_b = 1.0 / b.mass.max(1e-3);
                let impulse = n * ((-2.0 * approach_speed) / (inv_a + inv_b)) * inv_a;
                let impulse = impulse.clamp_length_max(ENTITY_IMPULSE_LIMIT);
                body_a.add_movement(impulse.extend(0.0));
                body_b.add_movement((-impulse * inv_b).extend(0.0));
                events.push(Event::Collided(a.id));
            }
        }
    }
}

fn two_mut<T>(slice: &mut [T], i: usize, j: usize) -> (&mut T, &mut T) {
    assert_ne!(i, j);
    if i < j {
        let (left, right) = slice.split_at_mut(j);
        (&mut left[i], &mut right[0])
    } else {
        let (left, right) = slice.split_at_mut(i);
        (&mut right[0], &mut left[j])
    }
}
