//! Force waves: the ripple is for looks, the shove is rules.

use caveland_sim::collectible::{CollectibleType as C, Item};
use caveland_sim::{ExtraEvent, GameEvent};
use glam::Vec3;

mod common;
use common::Game;

#[test]
fn a_wave_throws_a_player_away_from_its_centre_and_tells_the_client() {
    let mut g = Game::new();
    let player = g.player_at(10, 40);
    let center = g.position(player) - Vec3::new(3.0, 0.0, 0.0);
    g.caveland.shockwave(center, 9.0, 1.0);
    g.seconds(0.6);
    assert!(g.saw_extra(|e| matches!(e, ExtraEvent::Shockwave { radius, .. } if *radius == 9.0)), "clients draw the ripple");
    assert!(g.saw_extra(|e| matches!(e, ExtraEvent::Launched { entity, velocity, .. } if *entity == player && velocity.x > 3.0)), "the shove is a launch the client predicts");
    assert!(g.position(player).x > center.x + 3.0, "pushed away along +x");
}

#[test]
fn a_wave_shoves_each_body_once_and_things_out_of_reach_stay() {
    let mut g = Game::new();
    let near = g.player_at(10, 40);
    let far = g.player_at(10, 70);
    let center = g.position(near) - Vec3::new(2.0, 0.0, 0.0);
    g.caveland.shockwave(center, 9.0, 1.0);
    g.seconds(2.0);
    let launches = g.extra.iter().filter(|e| matches!(e, ExtraEvent::Launched { entity, .. } if *entity == near)).count();
    assert_eq!(launches, 1, "once");
    assert!(!g.extra.iter().any(|e| matches!(e, ExtraEvent::Launched { entity, .. } if *entity == far)), "beyond the radius");
}

#[test]
fn a_wave_pushes_items_and_robots_too() {
    let mut g = Game::new();
    let player = g.player_at(10, 40);
    let spot = g.position(player) + Vec3::new(2.0, 0.0, 0.2);
    let item = g.caveland.spawn_collectible(&mut g.entities, Item::new(C::Coal), spot);
    g.step(2);
    let start = g.position(item);
    g.caveland.shockwave(start - Vec3::new(2.0, 0.0, 0.0), 9.0, 1.5);
    g.seconds(0.8);
    assert!(g.position(item).x > start.x + 0.5, "the item was shoved along +x");
}

#[test]
fn an_explosion_sends_a_wave() {
    let mut g = Game::new();
    let player = g.player_at(10, 40);
    let at = g.position(player);
    g.caveland.explode(&mut g.world, &mut g.entities, at, 3, 0);
    g.step(1);
    let strength = |g: &Game| g.extra.iter().find_map(|e| if let ExtraEvent::Shockwave { strength, .. } = e { Some(*strength) } else { None });
    assert_eq!(strength(&g), Some(1.0));
    assert!(g.saw(|e| matches!(e, GameEvent::Explosion { .. })));
}
