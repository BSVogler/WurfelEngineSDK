//! Force waves in Caveland (not in the Java game): the rules side of [`wurfel_sim::shockwave`]. A wave
//! starts at a point (an explosion, the `forcewave` command), spreads, and shoves the players, robots and
//! items it overtakes. A player is not moved directly: the shove is a launch (`ExtraEvent::Launched`), so
//! the client predicts the flight with the same movement rules as for a catapult. The ripple in the ground
//! is only drawn; clients get [`ExtraEvent::Shockwave`] to draw it.

use glam::Vec3;
use wurfel_sim::entity::Entities;
use wurfel_sim::shockwave::Shockwave;

use crate::extras::ExtraEvent;
use crate::game::{Caveland, Kind};

/// Waves that run at once; an older one is dropped for a new one beyond this.
const MAX_WAVES: usize = 6;

impl Caveland {
    /// Start a force wave at `center` that reaches `radius` blocks. Strength 1 is a dynamite blast.
    pub fn shockwave(&mut self, center: Vec3, radius: f32, strength: f32) {
        if strength <= 0.0 || radius <= 0.0 {
            return;
        }
        self.x.waves.push((Shockwave::new(center, radius, strength), Default::default()));
        if self.x.waves.len() > MAX_WAVES {
            self.x.waves.remove(0);
        }
        self.x.events.push(ExtraEvent::Shockwave { position: center, radius, strength });
    }

    pub(crate) fn update_shockwaves(&mut self, entities: &mut Entities, dt: f32) {
        if self.x.waves.is_empty() {
            return;
        }
        let mut waves = std::mem::take(&mut self.x.waves);
        let mut ids: Vec<_> = self.kinds.iter().filter_map(|(&id, kind)| shoved(kind).then_some(id)).collect();
        ids.sort_unstable(); // deterministic
        for (wave, hit) in &mut waves {
            let before = wave.front();
            wave.age += dt;
            for &id in &ids {
                // Pushed once per wave: a body thrown outward slower than the front would be overtaken again.
                if hit.contains(&id) {
                    continue;
                }
                let Some(entity) = entities.get(id) else { continue };
                let Some(push) = wave.push(entity.position, before) else { continue };
                hit.insert(id);
                let is_player = matches!(self.kinds.get(&id), Some(Kind::Player(_)));
                if is_player {
                    let velocity = entity.body.as_ref().map_or(Vec3::ZERO, |b| b.movement) + push;
                    self.begin_launch(entities, id, None, velocity);
                } else if let Some(body) = entities.get_mut(id).and_then(|e| e.body.as_mut()) {
                    body.add_movement(push);
                }
            }
        }
        waves.retain(|(w, _)| !w.finished());
        // A wave started while this ran (a blast of a shoved shell) is kept.
        waves.append(&mut self.x.waves);
        self.x.waves = waves;
    }
}

/// What a force wave moves: the players, the robots and the things lying around.
fn shoved(kind: &Kind) -> bool {
    matches!(kind, Kind::Player(_) | Kind::Robot(_) | Kind::Collectible(_) | Kind::Money)
}
