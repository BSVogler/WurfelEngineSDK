//! The client side of the Caveland game mode: what the keys do, how the things of the mode look,
//! how the local player is predicted with Caveland's rules, and what the HUD shows.
//!
//! The rules themselves are the `caveland-sim` crate, the same code the server runs. Like the
//! server, the client keeps the engine out of Caveland's way: this file is the only place in the
//! client that knows about the mode.

use caveland_sim::{Caveland, Controls, Tuning};
use glam::Vec3;
use serde_json::Value;
use wurfel_sim::entity::Entities;
use wurfel_sim::player::{PlayerInput, TICK_DT};
use wurfel_sim::protocol::ThingState;
use wurfel_sim::World;

use crate::mesh::{self, Vertex};

/// The name of the mode in `Welcome::gamemode`.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))] // only the browser build joins worlds
pub const MODE: &str = "caveland";

pub fn controls(input: PlayerInput) -> Controls {
    Controls { up: input.up, down: input.down, left: input.left, right: input.right, jump: input.jump }
}

/// A fresh ruleset for the local player's prediction, with the world following Caveland's blocks.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub fn start(world: &mut World) -> Caveland {
    Caveland::install(world);
    Caveland::new(Tuning::default(), 1)
}

/// The action a key does. `pressed` is false for the key coming up. The keys avoid the ones the
/// camera and the console use. `None` if the key is not a Caveland key.
pub fn key_action(key: &str, pressed: bool) -> Option<(&'static str, i32)> {
    match (key, pressed) {
        ("f", true) => Some(("attack", 0)),
        ("f", false) => Some(("release_attack", 0)),
        ("c", true) => Some(("prepare_throw", 0)),
        ("c", false) => Some(("throw", 0)),
        ("g", true) => Some(("use", 0)),
        ("r", true) => Some(("interact", 0)),
        ("x", true) => Some(("drop", 0)),
        ("z", true) => Some(("switch_left", 0)),
        ("v", true) => Some(("switch_right", 0)),
        (digit, true) => craft_index(digit).map(|i| ("craft", i)),
        _ => None,
    }
}

/// `1` to `9` pick the first to ninth recipe of the list the HUD shows.
pub fn craft_index(key: &str) -> Option<i32> {
    let n: i32 = key.parse().ok()?;
    (1..=9).contains(&n).then_some(n - 1)
}

/// The mouse does the same as the attack key: the left button swings, holding it charges.
pub fn mouse_action(button: i16, pressed: bool) -> Option<(&'static str, i32)> {
    (button == 0).then_some(if pressed { ("attack", 0) } else { ("release_attack", 0) })
}

/// How a thing is drawn: no sprites yet, so a coloured block of this size.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ThingStyle {
    pub color: [f32; 3],
    /// Half the side of the square it stands on, in blocks.
    pub half: f32,
    pub height: f32,
}

pub fn thing_style(kind: &str) -> ThingStyle {
    let item = |color: [f32; 3]| ThingStyle { color, half: 0.15, height: 0.3 };
    match kind {
        "robot" => ThingStyle { color: [0.85, 0.15, 0.15], half: 0.25, height: 1.1 },
        "friendly_robot" => ThingStyle { color: [0.2, 0.7, 0.75], half: 0.25, height: 1.1 },
        "money" => ThingStyle { color: [0.95, 0.8, 0.15], half: 0.1, height: 0.2 },
        "minecart" => ThingStyle { color: [0.45, 0.45, 0.5], half: 0.35, height: 0.5 },
        "Wood" => item([0.55, 0.36, 0.18]),
        "Coal" => item([0.12, 0.12, 0.14]),
        "Torch" => item([1.0, 0.6, 0.1]),
        "Explosives" => item([0.8, 0.1, 0.1]),
        "Iron" => item([0.78, 0.78, 0.82]),
        "Ironore" => item([0.6, 0.35, 0.25]),
        "Cristall" => item([0.4, 0.9, 0.95]),
        "Sulfur" => item([0.9, 0.9, 0.2]),
        "Stone" => item([0.55, 0.55, 0.55]),
        "Toolkit" => item([0.25, 0.4, 0.85]),
        "Rails" => item([0.3, 0.3, 0.35]),
        "Powercable" => item([0.6, 0.3, 0.8]),
        _ => item([0.9, 0.5, 0.9]), // something new the client does not know yet
    }
}

/// Draw a thing as a small block standing on its position.
pub fn push_thing(out: &mut Vec<Vertex>, thing: &ThingState) {
    let style = thing_style(&thing.kind);
    let [x, y, z] = thing.pos;
    mesh::cuboid(out, style.color, [x - style.half, x + style.half, y - style.half, y + style.half], [z, z + style.height]);
}

/// What the local player's HUD shows (`Rules` `state`, our own entry).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Hud {
    pub health: f32,
    pub jetpack: f32,
    pub items: Vec<String>,
    pub recipes: Vec<(String, bool)>,
}

/// Our entry of a `state` message: `{"<id>": {health, jetpack, items, recipes}, ...}`.
pub fn parse_state(data: &Value, my_id: u32) -> Option<Hud> {
    let mine = data.get(my_id.to_string())?;
    let items = mine.get("items")?.as_array()?.iter().filter_map(|v| v.as_str().map(str::to_string)).collect();
    let recipes = mine
        .get("recipes")?
        .as_array()?
        .iter()
        .filter_map(|r| Some((r.get(0)?.as_str()?.to_string(), r.get(1)?.as_bool()?)))
        .collect();
    Some(Hud {
        health: mine.get("health")?.as_f64()? as f32,
        jetpack: mine.get("jetpack")?.as_f64()? as f32,
        items,
        recipes,
    })
}

/// The HUD as the page's `wurfelHud.update` takes it.
pub fn hud_json(hud: &Hud) -> String {
    serde_json::json!({
        "health": hud.health,
        "jetpack": hud.jetpack,
        "items": hud.items,
        "recipes": hud.recipes.iter().map(|(name, ok)| serde_json::json!([name, ok])).collect::<Vec<_>>(),
    })
    .to_string()
}

/// A happening of the `events` message, as far as the client reacts to it.
#[derive(Debug, Clone, PartialEq)]
pub enum Happening {
    Sound { name: String, pos: Vec3 },
    Dust { pos: Vec3 },
    Explosion { pos: Vec3 },
    /// A line for the HUD to show.
    Toast(String),
    /// We died and are back at the start.
    Died,
}

fn position(v: &Value) -> Option<Vec3> {
    let a = v.as_array()?;
    Some(Vec3::new(a.first()?.as_f64()? as f32, a.get(1)?.as_f64()? as f32, a.get(2)?.as_f64()? as f32))
}

/// Read an `events` message. Things the client does not understand are skipped, so a newer server
/// does not break an older client.
pub fn parse_events(data: &Value, my_id: u32) -> Vec<Happening> {
    let mut out = Vec::new();
    for event in data.as_array().into_iter().flatten() {
        let mine = event.get("player").and_then(Value::as_u64) == Some(u64::from(my_id));
        let text = |key: &str| event.get(key).and_then(Value::as_str).unwrap_or_default();
        let happening = match event.get("t").and_then(Value::as_str).unwrap_or_default() {
            "sound" => position(&event["pos"]).map(|pos| Happening::Sound { name: text("name").to_string(), pos }),
            "dust" => position(&event["pos"]).map(|pos| Happening::Dust { pos }),
            "explosion" => position(&event["pos"]).map(|pos| Happening::Explosion { pos }),
            "picked" if mine => Some(Happening::Toast(format!("Picked up {}", text("item")))),
            "crafted" if mine => Some(Happening::Toast(format!("Crafted {}", text("item")))),
            "money" if mine => Some(Happening::Toast(format!("Money: {}", event.get("total").and_then(Value::as_u64).unwrap_or(0)))),
            "died" if mine => Some(Happening::Died),
            _ => None,
        };
        out.extend(happening);
    }
    out
}

/// Where the server would have our player after `plan` (the inputs it has not acknowledged yet),
/// starting from the state it reported: the same rules, run on a scratch copy so the live player's
/// jetpack and attack state are not played twice. Returns position and velocity.
///
/// The scratch player starts with the first input already held, because that input was already
/// being applied when the server reported its state: a held jump is not a new jump.
pub fn replay(world: &mut World, server_pos: Vec3, server_vel: Vec3, plan: &[(PlayerInput, u32)]) -> Option<(Vec3, Vec3)> {
    let mut caveland = Caveland::new(Tuning::default(), 1);
    let mut entities = Entities::new();
    let id = caveland.spawn_player(&mut entities, 0, server_pos);
    entities.get_mut(id)?.body.as_mut()?.movement = server_vel;
    if let Some(&(first, _)) = plan.first() {
        caveland.assume_held(id, controls(first));
    }
    for &(input, steps) in plan {
        for _ in 0..steps {
            caveland.set_controls(&mut entities, world, id, controls(input));
            caveland.tick(&mut entities, world, TICK_DT);
        }
    }
    let entity = entities.get(id)?;
    Some((entity.position, entity.body.as_ref()?.movement))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use wurfel_sim::block::id;
    use wurfel_sim::{AirGenerator, Block};

    #[test]
    fn keys_map_to_actions_and_holding_keys_have_a_release() {
        assert_eq!(key_action("f", true), Some(("attack", 0)));
        assert_eq!(key_action("f", false), Some(("release_attack", 0)));
        assert_eq!(key_action("c", true), Some(("prepare_throw", 0)));
        assert_eq!(key_action("c", false), Some(("throw", 0)));
        assert_eq!(key_action("g", true), Some(("use", 0)));
        assert_eq!(key_action("g", false), None, "using is a press, not a hold");
        assert_eq!(key_action("1", true), Some(("craft", 0)));
        assert_eq!(key_action("7", true), Some(("craft", 6)));
        assert_eq!(key_action("0", true), None);
        assert_eq!(key_action("e", true), None, "e and q belong to the camera zoom");
        assert_eq!(key_action("w", true), None, "walking is not an action");
        assert_eq!(mouse_action(0, true), Some(("attack", 0)));
        assert_eq!(mouse_action(0, false), Some(("release_attack", 0)));
        assert_eq!(mouse_action(2, true), None);
    }

    #[test]
    fn every_key_action_is_one_the_server_understands() {
        // The names the server's `CavelandMode::act` accepts.
        let known = ["attack", "release_attack", "prepare_throw", "throw", "drop", "use", "interact", "switch_left", "switch_right", "craft"];
        for key in ["f", "c", "g", "r", "x", "z", "v", "1", "5", "9"] {
            for pressed in [true, false] {
                if let Some((name, _)) = key_action(key, pressed) {
                    assert!(known.contains(&name), "{name}");
                }
            }
        }
    }

    #[test]
    fn things_have_distinct_looks_and_unknown_ones_still_draw() {
        assert_ne!(thing_style("robot").color, thing_style("friendly_robot").color);
        assert!(thing_style("robot").height > thing_style("Torch").height, "robots are taller than items");
        let mut out = Vec::new();
        push_thing(&mut out, &ThingState { id: 1, kind: "never-heard-of".into(), pos: [1.0, 2.0, 3.0] });
        assert_eq!(out.len(), 18, "the three faces that can be seen, two triangles each");
        let mut again = Vec::new();
        push_thing(&mut again, &ThingState { id: 1, kind: "never-heard-of".into(), pos: [1.0, 2.0, 3.0] });
        assert_eq!(out.len(), again.len());
    }

    #[test]
    fn our_entry_of_a_state_message_becomes_the_hud() {
        let data = json!({
            "4": {"health": 80.0, "jetpack": 0.5, "items": ["Wood", "Coal"], "recipes": [["Torch", true], ["Minecart", false]]},
            "9": {"health": 10.0, "jetpack": 0.0, "items": [], "recipes": []},
        });
        let hud = parse_state(&data, 4).unwrap();
        assert_eq!(hud.health, 80.0);
        assert_eq!(hud.items, vec!["Wood".to_string(), "Coal".to_string()]);
        assert_eq!(hud.recipes, vec![("Torch".to_string(), true), ("Minecart".to_string(), false)]);
        assert!(parse_state(&data, 5).is_none(), "somebody else's state is not ours");
        assert!(parse_state(&json!({"4": {"health": 1}}), 4).is_none(), "a broken entry is ignored, not guessed");
        let round: Value = serde_json::from_str(&hud_json(&hud)).unwrap();
        assert_eq!(round["items"], json!(["Wood", "Coal"]));
        assert_eq!(round["recipes"][1], json!(["Minecart", false]));
    }

    #[test]
    fn events_are_read_and_only_our_own_news_becomes_a_toast() {
        let data = json!([
            {"t": "sound", "name": "collect", "pos": [1.0, 2.0, 3.0]},
            {"t": "picked", "player": 4, "item": "Torch"},
            {"t": "picked", "player": 9, "item": "Coal"},
            {"t": "crafted", "player": 4, "item": "Torch"},
            {"t": "explosion", "pos": [0.0, 0.0, 1.0], "radius": 3},
            {"t": "dust", "pos": [5.0, 5.0, 5.0]},
            {"t": "died", "player": 9},
            {"t": "died", "player": 4},
            {"t": "from-the-future", "pos": [0, 0, 0]},
            {"t": "sound", "name": "x"},
        ]);
        assert_eq!(
            parse_events(&data, 4),
            vec![
                Happening::Sound { name: "collect".into(), pos: Vec3::new(1.0, 2.0, 3.0) },
                Happening::Toast("Picked up Torch".into()),
                Happening::Toast("Crafted Torch".into()),
                Happening::Explosion { pos: Vec3::new(0.0, 0.0, 1.0) },
                Happening::Dust { pos: Vec3::new(5.0, 5.0, 5.0) },
                Happening::Died,
            ]
        );
        assert!(parse_events(&json!("not a list"), 4).is_empty());
    }

    /// A stone floor, the surface at height 1.
    fn floor() -> World {
        let mut world = World::new(AirGenerator);
        Caveland::install(&mut world);
        for x in -30..30 {
            for y in -60..60 {
                world.set(x, y, 0, Block::new(id::STONE, 0));
            }
        }
        world
    }

    #[test]
    fn replaying_walks_the_player_on_with_the_servers_rules() {
        let mut world = floor();
        let walk = PlayerInput { right: true, ..Default::default() };
        let (pos, vel) = replay(&mut world, Vec3::new(0.0, 0.0, 1.0), Vec3::ZERO, &[(walk, 30)]).unwrap();
        assert!(pos.distance(Vec3::new(0.0, 0.0, 1.0)) > 0.5, "half a second of walking moves: {pos:?}");
        assert!(vel.length() > 0.5);
        assert_eq!(pos.z, 1.0, "still on the ground");
        let (stay, _) = replay(&mut world, Vec3::new(0.0, 0.0, 1.0), Vec3::ZERO, &[]).unwrap();
        assert_eq!(stay, Vec3::new(0.0, 0.0, 1.0), "nothing to replay: the server state stands");
    }

    #[test]
    fn a_jump_that_was_already_held_when_the_server_reported_is_not_jumped_again() {
        let mut world = floor();
        let hold = PlayerInput { jump: true, ..Default::default() };
        // The server says we are on the ground with the jump key down for a while already.
        let (held, _) = replay(&mut world, Vec3::new(0.0, 0.0, 1.0), Vec3::ZERO, &[(hold, 6)]).unwrap();
        assert_eq!(held.z, 1.0, "no new jump from a key that was already down");
        // A jump pressed later in the plan is a real new one.
        let release = PlayerInput::default();
        let (jumped, vel) = replay(&mut world, Vec3::new(0.0, 0.0, 1.0), Vec3::ZERO, &[(release, 2), (hold, 6)]).unwrap();
        assert!(jumped.z > 1.0 && vel.z != 0.0, "{jumped:?} {vel:?}");
    }

    #[test]
    fn replay_does_not_touch_the_world() {
        let mut world = floor();
        let before = world.get(0, 0, 0);
        replay(&mut world, Vec3::new(0.0, 0.0, 1.0), Vec3::ZERO, &[(PlayerInput { up: true, jump: true, ..Default::default() }, 120)]).unwrap();
        assert_eq!(world.get(0, 0, 0), before);
    }
}
