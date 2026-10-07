//! The game harness the integration tests share: a world, its entities and the Caveland rules,
//! stepped and acted on like a server would. Each test file adds its own helpers in an `impl Game`.

// Every test file is its own crate and uses a different subset.
#![allow(dead_code)]

use std::ops::Range;

use caveland_sim::blocks::ids;
use caveland_sim::collectible::{CollectibleType, Item};
use caveland_sim::player::Action;
use caveland_sim::{Caveland, ExtraEvent, GameEvent, TransportEvent, Tuning};
use glam::Vec3;
use wurfel_sim::block::Block;
use wurfel_sim::entity::{Entities, EntityId};
use wurfel_sim::grid::to_iso;
use wurfel_sim::{AirGenerator, World};

pub const DT: f32 = 1.0 / 60.0;

pub struct Game {
    pub world: World,
    pub entities: Entities,
    pub caveland: Caveland,
    pub events: Vec<GameEvent>,
    pub extra: Vec<ExtraEvent>,
    pub notes: Vec<TransportEvent>,
}

impl Game {
    /// A sand floor at z = 0 for `x` in -10..60 and `y` in -20..100.
    pub fn new() -> Game {
        Game::with_floor(ids::SAND, -10..60, -20..100)
    }

    /// A floor of `block` at z = 0 over `xs` × `ys`, and nothing else.
    pub fn with_floor(block: u8, xs: Range<i32>, ys: Range<i32>) -> Game {
        let mut world = World::new(AirGenerator);
        Caveland::install(&mut world);
        for x in xs {
            for y in ys.clone() {
                world.set(x, y, 0, Block::new(block, 0));
            }
        }
        Game {
            world,
            entities: Entities::new(),
            caveland: Caveland::new(Tuning::default(), 1),
            events: Vec::new(),
            extra: Vec::new(),
            notes: Vec::new(),
        }
    }

    /// A player standing on the floor at block `(x, y)`.
    pub fn player_at(&mut self, x: i32, y: i32) -> EntityId {
        let (gx, gy) = to_iso(x, y);
        self.caveland.spawn_player(&mut self.entities, 0, Vec3::new(gx, gy, 1.0))
    }

    pub fn step(&mut self, steps: usize) {
        for _ in 0..steps {
            let events = self.caveland.tick(&mut self.entities, &mut self.world, DT);
            self.events.extend(events);
            self.extra.extend(self.caveland.drain_extra_events());
            self.notes.extend(self.caveland.drain_transport_events());
        }
    }

    pub fn seconds(&mut self, s: f32) {
        self.step((s * 60.0).round() as usize);
    }

    pub fn act(&mut self, player: EntityId, action: Action) {
        self.caveland.act(&mut self.entities, &mut self.world, player, action);
        self.events.extend(self.caveland.drain_events());
        self.extra.extend(self.caveland.drain_extra_events());
    }

    pub fn give(&mut self, player: EntityId, kind: CollectibleType) {
        assert!(self.caveland.player_mut(player).unwrap().inventory.add(Item::new(kind)));
    }

    /// The kinds in the player's inventory, in order.
    pub fn pack(&self, player: EntityId) -> Vec<CollectibleType> {
        self.caveland.player(player).unwrap().inventory.items().iter().map(|i| i.kind).collect()
    }

    pub fn position(&self, id: EntityId) -> Vec3 {
        self.entities.get(id).unwrap().position
    }

    pub fn saw(&self, f: impl Fn(&GameEvent) -> bool) -> bool {
        self.events.iter().any(f)
    }

    pub fn saw_extra(&self, f: impl Fn(&ExtraEvent) -> bool) -> bool {
        self.extra.iter().any(f)
    }
}
