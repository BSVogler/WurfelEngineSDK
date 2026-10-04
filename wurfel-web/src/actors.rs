//! Players and the things of a game mode as sprites: which picture, which way it faces, where it
//! stands. Pure CPU code (the vertices go into the dynamic buffer in `web.rs`), so it is tested
//! natively. When there is no atlas, or no art for a kind, the callers keep drawing coloured boxes.

use std::collections::HashMap;
use std::rc::Rc;

use glam::Vec3;
use wurfel_sim::protocol::ThingState;

use crate::mesh::Vertex;
use crate::sprites::{self, Anim, Sprites};

/// Blocks of travel that make one walking cycle of the player (8 frames, two steps).
const PLAYER_STRIDE: f32 = 1.6;
const ROBOT_STRIDE: f32 = 1.2;
/// How much of the player's chosen colour tints the sprite: the art stays recognisable, the colour
/// tells players apart.
const TINT_STRENGTH: f32 = 0.45;
/// Things arrive in snapshots, not every frame: without a new position for this long they stand still.
const THING_STILL_AFTER: f32 = 0.2;

#[derive(Default)]
pub struct Actors {
    sprites: Option<Rc<Sprites>>,
    players: HashMap<u32, Anim>,
    things: HashMap<u32, ThingAnim>,
}

#[derive(Default)]
struct ThingAnim {
    anim: Anim,
    /// Time since the position last changed.
    still_for: f32,
    /// Time since `anim` was last advanced, so a change is judged over the real interval.
    pending: f32,
    last: Option<Vec3>,
}

impl Actors {
    pub fn set_sprites(&mut self, sprites: Option<Rc<Sprites>>) {
        self.sprites = sprites;
    }

    /// Advance every animation by `dt` seconds. `players` are everybody drawn this frame; whoever
    /// is missing is forgotten.
    pub fn update(&mut self, dt: f32, players: impl IntoIterator<Item = (u32, Vec3)>, things: &[ThingState]) {
        let mut seen = Vec::new();
        for (id, pos) in players {
            self.players.entry(id).or_default().update(pos, dt, PLAYER_STRIDE);
            seen.push(id);
        }
        self.players.retain(|id, _| seen.contains(id));

        for thing in things {
            let pos = Vec3::from(thing.pos);
            let entry = self.things.entry(thing.id).or_default();
            entry.pending += dt;
            if entry.last != Some(pos) {
                let interval = entry.pending;
                entry.anim.update(pos, interval, ROBOT_STRIDE);
                entry.last = Some(pos);
                entry.pending = 0.0;
                entry.still_for = 0.0;
            } else {
                entry.still_for += dt;
                if entry.still_for > THING_STILL_AFTER {
                    let interval = entry.pending;
                    entry.anim.update(pos, interval, ROBOT_STRIDE);
                    entry.pending = 0.0;
                }
            }
        }
        self.things.retain(|id, _| things.iter().any(|t| t.id == *id));
    }

    /// Draw a player standing at `pos` (the middle of the feet), tinted with its colour. `false`
    /// when there is no sprite for it and the caller should draw its box.
    pub fn push_player(&self, out: &mut Vec<Vertex>, id: u32, pos: Vec3, color: [f32; 3]) -> bool {
        let Some(sprites) = &self.sprites else { return false };
        let anim = self.players.get(&id).cloned().unwrap_or_default();
        let Some(region) = sprites.player(sprites::player_frame(anim.facing, anim.cycle)) else { return false };
        let tint = color.map(|c| 1.0 + (c - 1.0) * TINT_STRENGTH);
        sprites::billboard(out, &sprites.atlas, region, pos, sprites::PLAYER_BOX_BOTTOM, false, tint);
        true
    }

    /// Draw a thing of the game mode; `false` when its kind has no art.
    pub fn push_thing(&self, out: &mut Vec<Vertex>, thing: &ThingState) -> bool {
        let Some(sprites) = &self.sprites else { return false };
        let Some(art) = sprites::entity_art(&thing.kind) else { return false };
        let anim = self.things.get(&thing.id).map(|t| t.anim.clone()).unwrap_or_default();
        let value = if art.walks { sprites::robot_value(anim.facing, anim.cycle, art.steps) } else { 0 };
        let Some(region) = sprites.entity(art.id, value).or_else(|| sprites.entity(art.id, 0)) else { return false };
        sprites::billboard(out, &sprites.atlas, region, Vec3::from(thing.pos), sprites::FOOTPRINT_TIP, false, [1.0; 3]);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::atlas::Atlas;
    use crate::mesh::{FACE_SPRITE, NO_SPRITE};

    fn actors() -> Actors {
        let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/sprites/sprites.atlas")).unwrap();
        let mut actors = Actors::default();
        actors.set_sprites(Some(Rc::new(Sprites::new(Atlas::parse(&text).unwrap()))));
        actors
    }

    fn thing(id: u32, kind: &str, pos: [f32; 3]) -> ThingState {
        ThingState { id, kind: kind.into(), pos }
    }

    #[test]
    fn without_an_atlas_nothing_is_drawn_so_the_caller_falls_back_to_boxes() {
        let actors = Actors::default();
        let mut out = Vec::new();
        assert!(!actors.push_player(&mut out, 1, Vec3::ZERO, [1.0; 3]));
        assert!(!actors.push_thing(&mut out, &thing(1, "Wood", [0.0; 3])));
        assert!(out.is_empty());
    }

    #[test]
    fn a_player_is_one_sprite_quad_tinted_with_the_players_colour() {
        let actors = actors();
        let mut out = Vec::new();
        assert!(actors.push_player(&mut out, 1, Vec3::new(3.0, 2.0, 1.0), [1.0, 0.0, 0.0]));
        assert_eq!(out.len(), 6);
        assert!(out.iter().all(|v| v.layer >= 0.0 && v.shade[0] == FACE_SPRITE));
        let tint = out[0].color;
        assert_eq!(tint[0], 1.0, "red stays");
        assert!((tint[1] - (1.0 - TINT_STRENGTH)).abs() < 1e-6 && tint[1] == tint[2], "the other channels are pulled down: {tint:?}");
    }

    #[test]
    fn a_player_walking_screen_left_shows_the_west_frames_and_the_cycle_changes_frames() {
        let mut actors = actors();
        let mut pos = Vec3::ZERO;
        let mut frames = std::collections::HashSet::new();
        for _ in 0..90 {
            pos += Vec3::new(-0.05, 0.05, 0.0);
            actors.update(1.0 / 60.0, [(1, pos)], &[]);
            let mut out = Vec::new();
            actors.push_player(&mut out, 1, pos, [1.0; 3]);
            frames.insert(out[0].uv.map(f32::to_bits));
        }
        assert!(frames.len() >= 6, "{} different frames over 3 cycles", frames.len());
        let anim = &actors.players[&1];
        assert_eq!(sprites::player_frame(anim.facing, 0.0), 6 * 8 + 1, "west");
    }

    #[test]
    fn players_that_left_are_forgotten() {
        let mut actors = actors();
        actors.update(0.016, [(1, Vec3::ZERO), (2, Vec3::ZERO)], &[]);
        actors.update(0.016, [(2, Vec3::ZERO)], &[]);
        assert_eq!(actors.players.keys().copied().collect::<Vec<_>>(), vec![2]);
    }

    #[test]
    fn items_robots_and_unknown_kinds() {
        let mut actors = actors();
        let things = [thing(1, "Wood", [1.0, 1.0, 0.0]), thing(2, "robot", [2.0, 1.0, 0.0]), thing(3, "mystery", [0.0; 3])];
        actors.update(0.016, [], &things);
        let mut out = Vec::new();
        assert!(actors.push_thing(&mut out, &things[0]));
        assert!(actors.push_thing(&mut out, &things[1]));
        assert!(!actors.push_thing(&mut out, &things[2]), "no art: the caller draws a box");
        assert_eq!(out.len(), 12);
        assert!(out.iter().all(|v| v.layer != NO_SPRITE));
    }

    #[test]
    fn a_robot_between_snapshots_does_not_flicker_back_to_standing() {
        let mut actors = actors();
        // A snapshot every 2 frames (30 Hz at 60 fps), 0.3 blocks each: a brisk walk.
        let mut x = 0.0;
        let mut cycles = Vec::new();
        for frame in 0..40 {
            if frame % 2 == 0 {
                x += 0.1;
            }
            actors.update(1.0 / 60.0, [], &[thing(7, "robot", [x, 0.0, 0.0])]);
            cycles.push(actors.things[&7].anim.cycle);
        }
        let standing_frames = cycles.iter().skip(4).filter(|&&c| c == 0.0).count();
        assert!(standing_frames <= 2, "{standing_frames} frames reset to standing: {cycles:?}");
        // and it does come to rest when the snapshots stop changing
        for _ in 0..30 {
            actors.update(1.0 / 60.0, [], &[thing(7, "robot", [x, 0.0, 0.0])]);
        }
        assert_eq!(actors.things[&7].anim.cycle, 0.0);
    }
}
