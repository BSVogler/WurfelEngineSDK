use super::*;
use crate::grid::{lower_left, lower_right};
use crate::{Generator, IslandGenerator};

/// Blocks at a list of block positions, otherwise air.
struct Blocks(Vec<(i32, i32, i32)>);
impl Generator for Blocks {
    fn generate(&self, x: i32, y: i32, z: i32) -> Block {
        if self.0.contains(&(x, y, z)) { Block::new(id::STONE, 0) } else { Block::AIR }
    }
}

fn source(azimuth: f32, height: f32) -> GlobalLightSource {
    let mut s = GlobalLightSource::new(azimuth, height, Vec3::ONE, Vec3::splat(0.5), 1.0, 60.0);
    s.set_fixed_position(true);
    s
}

/// An engine whose sun and moon stand still at the given heights (degrees) and azimuths.
fn fixed_engine(sun: (f32, f32), moon: Option<(f32, f32)>) -> LightEngine {
    let mut e = LightEngine::new(DEFAULT_WORLD_SPIN_ANGLE, DEFAULT_AZIMUTH_SPEED);
    {
        let s = e.sun_mut();
        s.set_fixed_position(true);
        s.set_azimuth(sun.0);
        s.set_height(sun.1);
    }
    match moon {
        Some((azimuth, height)) => {
            let m = e.moon.as_mut().unwrap();
            m.set_fixed_position(true);
            m.set_azimuth(azimuth);
            m.set_height(height);
        }
        None => e.moon = None,
    }
    e.update(0.0);
    e
}

fn approx(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-4
}

// ---------------------------------------------------------------------------------- sun and moon

#[test]
fn power_follows_the_java_curve() {
    let power = |height: f32| power_for(height, 60.0, 1.0);
    assert_eq!(power(-45.0), 0.0, "night");
    assert_eq!(power(-31.0), 0.0, "still night");
    assert!(approx(power(0.0), 0.5), "horizon");
    assert!(approx(power(15.0), 0.5 + (15.0f32 * std::f32::consts::PI / 60.0).sin()), "morning");
    assert!(approx(power(30.0), 1.5), "the upper branch starts at amplitude / 2");
    assert!(approx(power(59.0), 1.5), "day");
    // Java gives -0.5 just above the night limit; that dip is clamped.
    assert_eq!(power(-29.0), 0.0);
    assert_eq!(power(-15.0), 0.0);
}

#[test]
fn azimuth_is_kept_in_range_and_update_moves_the_sun() {
    let mut s = GlobalLightSource::new(-40.0, 0.0, Vec3::ONE, Vec3::ONE, 1.0, 60.0);
    assert_eq!(s.azimuth(), 320.0);
    s.set_azimuth(725.0);
    assert!(approx(s.azimuth(), 5.0));
    s.set_azimuth(f32::NAN);
    assert_eq!(s.azimuth(), 0.0, "bad input does not poison the state");

    let mut s = GlobalLightSource::new(40.0, 0.0, Vec3::ONE, Vec3::ONE, 1.0, 60.0);
    s.update(115_200.0, DEFAULT_AZIMUTH_SPEED, -40); // a quarter of a day
    assert!(approx(s.azimuth(), 130.0), "{}", s.azimuth());
    assert!(approx(s.height(), 60.0), "noon is the highest point, {}", s.height());
    assert!(approx(s.power(), 1.5));

    s.set_fixed_position(true);
    let before = (s.azimuth(), s.height());
    s.update(10_000.0, DEFAULT_AZIMUTH_SPEED, -40);
    assert_eq!((s.azimuth(), s.height()), before, "a fixed light does not move");
}

#[test]
fn the_moon_is_slower_than_the_sun() {
    let mut sun = GlobalLightSource::new(0.0, 0.0, Vec3::ONE, Vec3::ONE, 1.0, 60.0);
    let mut moon = GlobalLightSource::moon(0.0, 0.0, Vec3::ONE, Vec3::ONE, 1.0, 45.0);
    sun.update(100_000.0, DEFAULT_AZIMUTH_SPEED, 0);
    moon.update(100_000.0, DEFAULT_AZIMUTH_SPEED, 0);
    assert!(approx(moon.azimuth() / sun.azimuth(), 0.85));
}

#[test]
fn light_colours_are_clamped_like_libgdx_colors() {
    let s = source(90.0, 59.0); // power 1.5
    assert_eq!(s.light(), Vec3::ONE, "white times 1.5 is still white");
    assert_eq!(s.ambient(), Vec3::splat(0.75));
}

#[test]
fn directions_are_unit_vectors_and_the_normal_matches_the_java_formula() {
    for az in (0..360).step_by(23) {
        for h in (-80..=80).step_by(20) {
            let s = source(az as f32, h as f32);
            assert!(approx(s.direction_game().length(), 1.0));
            assert!(approx(s.direction().length(), 1.0), "the frame change keeps lengths");
            assert!(approx(s.normal().length(), 1.0));
        }
    }
    // Java getNormal: (-cos az, sin az, sin h) normalised.
    let s = source(90.0, 0.0);
    assert!((s.normal_game() - Vec3::new(0.0, 1.0, 0.0)).length() < 1e-5);
    // The Java normal does not scale x and y by cos(h), so straight up at azimuth 0 is not (0, 0, 1).
    let s = source(0.0, 90.0);
    assert!((s.normal_game() - Vec3::new(-1.0, 0.0, 1.0).normalize()).length() < 1e-5);
    // The direction that the per-side formulas use is the unit vector (0, 0, 1) there.
    assert!((s.direction_game() - Vec3::Z).length() < 1e-5);
}

#[test]
fn the_java_per_side_formulas_are_the_lambert_terms_of_the_face_normals() {
    // This ties the Java angles to this project's axes: the face normals in the isometric frame
    // dotted with the direction to the light must equal cos(h) cos(az - 45) etc.
    for az in (0..360).step_by(11) {
        for h in (-85..=85).step_by(17) {
            let s = source(az as f32, h as f32);
            for face in Face::ALL {
                let expected = s.direction().dot(face.normal()).max(0.0);
                assert!(approx(s.lambert(face), expected), "az {az} h {h} {face:?}: {} vs {expected}", s.lambert(face));
            }
        }
    }
}

#[test]
fn specular_light_is_non_negative_finite_and_only_on_top_by_construction() {
    for az in (0..360).step_by(15) {
        for h in (-90..=90).step_by(10) {
            let spec = source(az as f32, h as f32).specular_top();
            assert!(spec.is_finite() && spec >= 0.0, "az {az} h {h}: {spec}");
        }
    }
    // A peak exists: the Java base (sin h sin az - cos h) / sqrt 2 is maximal low over the horizon on the +y side.
    let peak = (0..360).map(|az| source(az as f32, 5.0).specular_top()).fold(0.0f32, f32::max);
    assert!(peak > 0.0);
}

// ----------------------------------------------------------------------------------- the engine

#[test]
fn a_new_engine_starts_at_sunrise_with_the_java_values() {
    let e = LightEngine::new(-40, DEFAULT_AZIMUTH_SPEED);
    assert!(approx(e.time_of_day(), 0.0));
    assert_eq!(e.sun().azimuth(), 40.0);
    // Not opposite the sun (that would be 220): where the moon is in the running day cycle.
    assert!(approx(e.moon().unwrap().azimuth(), 200.5), "{}", e.moon().unwrap().azimuth());
    assert!(approx(e.sun().amplitude(), 60.0) && approx(e.moon().unwrap().amplitude(), 45.0));
}

#[test]
fn a_whole_day_comes_back_to_the_start() {
    let mut e = LightEngine::new(-40, DEFAULT_AZIMUTH_SPEED);
    let full_day_ms = 360.0 / DEFAULT_AZIMUTH_SPEED;
    let mut quarter_noon_seen = false;
    let step = 1000.0;
    let mut t = 0.0;
    while t < full_day_ms {
        e.update(step);
        t += step;
        quarter_noon_seen |= (e.time_of_day() - 0.25).abs() < 0.001 && e.sun().power() > 1.4;
    }
    assert!(quarter_noon_seen, "full power at a quarter of the day");
    let tod = e.time_of_day();
    assert!(tod < 0.01 || tod > 0.99, "back at sunrise, got {tod}");
}

#[test]
fn noon_lights_the_top_most_and_both_sides_equally() {
    let mut e = LightEngine::new(-40, DEFAULT_AZIMUTH_SPEED);
    e.set_to_noon();
    let (top, left, right) = (e.face_light(Face::Top), e.face_light(Face::Left), e.face_light(Face::Right));
    assert!(top.x > left.x && left.x > 0.1, "top {top:?} left {left:?}");
    assert!((left - right).length() < 1e-5, "the sun is straight ahead, so both sides are lit alike");
    // Java numbers: power 1.5 * k_diff * sin(height) with height = 60 sin(50 deg).
    let height = 60.0f32 * 50f32.to_radians().sin();
    assert!(approx(top.x, 1.5 * K_DIFF * height.to_radians().sin() + e.sun().specular_top()));
}

#[test]
fn a_face_turned_away_from_the_sun_is_dark() {
    // Sun low over the left side: the right face is edge-on.
    let e = fixed_engine((45.0, 30.0), None);
    assert!(e.face_light(Face::Left).x > 0.2);
    assert!(e.face_light(Face::Left).x > e.face_light(Face::Right).x * 3.0);
    assert!(approx(e.face_light(Face::Right).x, 0.0), "cos(45 - 135) = 0");
    // Sun behind the block: both side faces are dark, only the top is lit.
    let behind = fixed_engine((225.0, 30.0), None);
    assert!(behind.face_light(Face::Left).max_element() < 1e-6);
    assert!(behind.face_light(Face::Right).max_element() < 1e-6);
    assert!(behind.face_light(Face::Top).x > 0.0);
}

#[test]
fn the_moon_lights_the_world_unlike_in_the_java_engine() {
    // Java overwrote the moon's contribution with the sun's, so a night lit only by the moon was black.
    let e = fixed_engine((90.0, -50.0), Some((45.0, 40.0)));
    assert_eq!(e.sun().power(), 0.0);
    assert!(e.moon().unwrap().power() > 1.0);
    let light = e.face_light(Face::Left);
    assert!(light.x > 0.01 && light.y > light.x, "bluish-green moon light, got {light:?}");
    assert_eq!(e.state().sun_faces, [0.0, 0.0, 0.0]);
    assert!(e.state().moon_faces[0] > 0.1);
}

#[test]
fn ambient_follows_the_power_and_stays_in_range() {
    let day = fixed_engine((90.0, 59.0), Some((270.0, -50.0)));
    assert!((day.ambient() - Vec3::new(0.75, 0.75, 0.6)).length() < 1e-5);
    let night = fixed_engine((90.0, -50.0), Some((45.0, 40.0)));
    assert!(night.ambient().x == 0.0 && night.ambient().z > 0.0 && night.ambient().z < 0.2, "{:?}", night.ambient());
    let both = fixed_engine((90.0, 59.0), Some((45.0, 40.0)));
    assert!(both.ambient().max_element() <= 1.0);
}

/// Run the engine for `days` days in 16 ms steps, calling `check` after every step.
fn run_days(e: &mut LightEngine, days: f32, mut check: impl FnMut(&LightEngine)) {
    let day_ms = 360.0 / DEFAULT_AZIMUTH_SPEED;
    for _ in 0..(days * day_ms / 16.0) as u32 {
        e.update(16.0);
        check(e);
    }
}

#[test]
fn a_body_is_always_in_the_sky() {
    let mut e = LightEngine::new(DEFAULT_WORLD_SPIN_ANGLE, DEFAULT_AZIMUTH_SPEED);
    let mut dark = 0;
    run_days(&mut e, 12.0, |e| {
        // Up means above the horizon: the sun at night is down and the moon is not yet risen.
        if e.sun().height() < 0.0 && e.moon().unwrap().height() < 0.0 {
            dark += 1;
        }
    });
    assert_eq!(dark, 0, "frames without sun and moon in the sky");
}

#[test]
fn the_moon_jumps_unseen_and_slower_than_the_sun() {
    let mut e = LightEngine::new(DEFAULT_WORLD_SPIN_ANGLE, DEFAULT_AZIMUTH_SPEED);
    let mut jumps = 0;
    let mut last = e.moon().unwrap().azimuth();
    run_days(&mut e, 6.0, |e| {
        let moon = e.moon().unwrap();
        let step = (moon.azimuth() - last + 540.0).rem_euclid(360.0) - 180.0;
        if step.abs() > 1.0 {
            jumps += 1;
            assert_eq!(moon.power(), 0.0, "the jump must not change the light");
        }
        last = moon.azimuth();
    });
    // About once a day, never more.
    assert!((4..=7).contains(&jumps), "{jumps} jumps in 6 days");
    // The moon is slower: after a day it has turned less than the sun.
    // From sunrise, before the sun reaches its setting phase (where the moon jumps).
    let mut e = LightEngine::new(DEFAULT_WORLD_SPIN_ANGLE, DEFAULT_AZIMUTH_SPEED);
    let (sun0, moon0) = (e.sun().azimuth(), e.moon().unwrap().azimuth());
    e.update(100_000.0);
    let turned = |from: f32, to: f32| (to - from).rem_euclid(360.0);
    assert!(turned(moon0, e.moon().unwrap().azimuth()) < turned(sun0, e.sun().azimuth()));
}

#[test]
fn the_moon_rises_as_the_sun_goes_down() {
    let mut e = LightEngine::new(-40, DEFAULT_AZIMUTH_SPEED);
    // Phase = azimuth + spin. Sun at phase 130 (still high), moon at phase 260 (well below).
    e.sun_mut().set_azimuth(130.0 + 40.0);
    e.moon.as_mut().unwrap().set_azimuth(260.0 + 40.0);
    e.update(16.0);
    assert!(approx(e.moon().unwrap().azimuth(), 300.0 + 0.85 * DEFAULT_AZIMUTH_SPEED * 16.0), "not before the sun sets");
    // Sun at phase 160: setting. The moon jumps to phase 342 = azimuth 382 = 22.
    e.sun_mut().set_azimuth(160.0 + 40.0);
    e.update(16.0);
    let moon = e.moon().unwrap();
    assert!(approx(moon.azimuth(), 22.0 + 0.85 * DEFAULT_AZIMUTH_SPEED * 16.0), "{}", moon.azimuth());
    assert!(moon.height() < 0.0 && moon.power() == 0.0, "{} {}", moon.height(), moon.power());
}

#[test]
fn set_to_noon_and_night_use_the_java_azimuths() {
    let mut e = LightEngine::new(-40, DEFAULT_AZIMUTH_SPEED);
    e.set_to_night();
    assert_eq!(e.sun().azimuth(), 270.0);
    assert!(approx(e.moon().unwrap().azimuth(), 90.0), "{}", e.moon().unwrap().azimuth());
    assert!(e.moon().unwrap().height() > 0.0, "the moon is up at night");
    assert_eq!(e.sun().power(), 0.0, "the sun is below the horizon at night");
    e.set_to_noon();
    assert_eq!(e.sun().azimuth(), 90.0);
    assert!(approx(e.moon().unwrap().azimuth(), 243.0), "not opposite (270): {}", e.moon().unwrap().azimuth());
    assert!(e.sun().power() > 1.4);
}

#[test]
fn the_engine_is_configured_from_cvars() {
    let mut root = CVarSystem::root();
    root.set_i32("worldSpinAngle", 10).unwrap();
    root.set_f32("LEazimutSpeed", 0.5).unwrap();
    let mut save = CVarSystem::save_slot();
    save.set_f32("LEsunAzimuth", 123.0).unwrap();
    save.set_f32("LEmoonAzimuth", 321.0).unwrap();
    let e = LightEngine::from_cvars(&root, &save);
    assert_eq!(e.world_spin_angle(), 10);
    assert!(approx(e.sun().azimuth(), 123.0) && approx(e.moon().unwrap().azimuth(), 321.0));
    let mut e = e;
    e.sun_mut().set_fixed_position(false);
    let before = e.sun().azimuth();
    e.update(10.0);
    assert!(approx(e.sun().azimuth(), before + 5.0), "speed 0.5 degrees per millisecond");

    // Defaults when nothing is configured.
    let e = LightEngine::from_cvars(&CVarSystem::root(), &CVarSystem::save_slot());
    assert_eq!(e.world_spin_angle(), -40);
    assert!(approx(e.sun().azimuth(), 0.0), "the save slot default for LEsunAzimuth is 0");
}

#[test]
fn the_state_matches_the_engine() {
    let mut e = LightEngine::new(-40, DEFAULT_AZIMUTH_SPEED);
    e.set_to_noon();
    let s = e.state();
    assert_eq!(s.sun_color, e.sun().light());
    assert_eq!(s.ambient, e.ambient());
    assert!(approx(s.time_of_day, e.time_of_day()));
    assert_eq!(s.night_mix, 0.0, "no night grading at noon");
    assert!(approx(s.sun_direction.length(), 1.0));
    for face in Face::ALL {
        let from_state = s.sun_color * (s.sun_faces[face.index()] + if face == Face::Top { s.sun_spec } else { 0.0 })
            + s.moon_color * (s.moon_faces[face.index()] + if face == Face::Top { s.moon_spec } else { 0.0 });
        assert!((from_state - e.face_light(face)).length() < 1e-5);
    }
    e.set_to_night();
    assert_eq!(e.state().night_mix, 1.0);
}

// ------------------------------------------------------------------------------------ point lights

#[test]
fn point_light_falloff_is_inverse_square_and_zero_at_the_radius() {
    let light = PointLight::new(Vec3::new(0.0, 0.0, 10.0), Vec3::ONE, 6.0, 1.0);
    let at = |d: f32| light.intensity_at(Vec3::new(0.0, 0.0, 10.0 - d), Face::Top);
    assert!(approx(at(1.0), 2.0 * POINT_SIDE_FACTORS[1]), "(1 + brightness) / d^2 * side factor");
    assert!(approx(at(2.0), at(1.0) / 4.0), "inverse square");
    assert!(at(3.0) > at(5.9) && at(5.9) > 0.0);
    assert_eq!(at(6.0), 0.0, "zero at the radius");
    assert_eq!(at(7.0), 0.0, "and beyond");
    assert!(approx(at(0.1), at(0.5)), "the falloff is capped close to the light");
    assert!(at(0.0).is_finite() && at(0.0) > 0.0);
}

#[test]
fn point_light_only_lights_faces_that_face_it() {
    let light = PointLight::new(Vec3::new(5.0, 5.0, 5.0), Vec3::new(1.0, 0.5, 0.2), 20.0, 1.0);
    // The light is above and to the +x, +y side of the origin: the three visible faces all see it.
    for face in Face::ALL {
        assert!(light.intensity_at(Vec3::ZERO, face) > 0.0, "{face:?}");
    }
    // From the other side the same faces are turned away.
    let behind = PointLight::new(Vec3::new(-5.0, -5.0, -5.0), Vec3::ONE, 20.0, 1.0);
    for face in Face::ALL {
        assert_eq!(behind.intensity_at(Vec3::ZERO, face), 0.0, "{face:?}");
    }
    let c = light.contribution(Vec3::ZERO, Face::Top);
    assert!(c.x > c.y && c.y > c.z, "the colour is kept: {c:?}");
}

#[test]
fn point_light_survives_degenerate_input() {
    let nan = PointLight::new(Vec3::splat(f32::NAN), Vec3::ONE, 5.0, 1.0);
    let inf = PointLight::new(Vec3::ZERO, Vec3::splat(f32::INFINITY), f32::INFINITY, 1.0);
    let none = PointLight::new(Vec3::ZERO, Vec3::ONE, 0.0, 1.0);
    for face in Face::ALL {
        assert_eq!(nan.intensity_at(Vec3::ONE, face), 0.0);
        assert!(nan.contribution(Vec3::ONE, face).is_finite());
        assert!(inf.contribution(Vec3::ONE, face).is_finite());
        assert_eq!(none.intensity_at(Vec3::ZERO, face), 0.0, "a radius of zero lights nothing");
    }
}

#[test]
fn blocks_stop_light_but_water_and_air_do_not() {
    let (gx, gy) = to_iso(5, 10);
    let from = Vec3::new(gx, gy, 3.5);
    let to = Vec3::new(gx + 4.0, gy, 3.5);
    let (wx, wy) = from_iso(gx + 2.0, gy);
    let open = World::new(Blocks(vec![]));
    assert!(visible(&open, from, to));

    let walled = World::new(Blocks(vec![(wx, wy, 3)]));
    assert!(!visible(&walled, from, to), "a stone block in the way");
    assert!(visible(&walled, from, Vec3::new(gx, gy + 4.0, 3.5)), "but not in another direction");
    assert!(visible(&walled, from, from), "no distance, nothing in between");

    let mut watery = World::new(Blocks(vec![]));
    watery.set(wx, wy, 3, Block::new(id::WATER, 0));
    assert!(visible(&watery, from, to), "water is transparent");
}

#[test]
fn baked_point_light_respects_occlusion() {
    let (gx, gy) = to_iso(5, 10);
    let lamp = PointLight::new(Vec3::new(gx, gy, 6.0), Vec3::ONE, 10.0, 1.0);
    let open = World::new(Blocks(vec![]));
    let surface = Vec3::new(gx, gy, 2.0);
    let lit = bake_point_lights(&open, &[lamp], surface, Face::Top);
    assert!(lit.x > 0.0);

    let cover = World::new(Blocks(vec![(5, 10, 4)]));
    assert_eq!(bake_point_lights(&cover, &[lamp], surface, Face::Top), Vec3::ZERO, "a roof in between");
    assert_eq!(bake_point_lights(&open, &[], surface, Face::Top), Vec3::ZERO, "no lights, no light");
    let sum = bake_point_lights(&open, &[lamp, lamp], surface, Face::Top);
    assert!(approx(sum.x, 2.0 * lit.x), "lights add up");
}

// -------------------------------------------------------------------------------- ambient occlusion

/// The block column at lattice position `(ix, iy)`.
fn column(ix: i32, iy: i32) -> (i32, i32) {
    from_iso(ix as f32, iy as f32)
}

/// A flat floor at z = 0 plus extra blocks given in lattice coordinates.
fn floor_with(extra: &[(i32, i32, i32)]) -> (World, (i32, i32, i32), (i32, i32)) {
    let (x, y) = (5, 10);
    let (gx, gy) = to_iso(x, y);
    let (ix, iy) = (gx as i32, gy as i32);
    let mut blocks = Vec::new();
    for dx in -3..=3 {
        for dy in -3..=3 {
            let (cx, cy) = column(ix + dx, iy + dy);
            blocks.push((cx, cy, 0));
        }
    }
    for &(dx, dy, z) in extra {
        let (cx, cy) = column(ix + dx, iy + dy);
        blocks.push((cx, cy, z));
    }
    (World::new(Blocks(blocks)), (x, y, 0), (ix, iy))
}

/// One block at `(5, 10, 0)` plus extra blocks given in lattice offsets from it. Unlike a floor, all
/// three faces of the block are exposed, so side-face occlusion can be tested.
fn pillar_with(extra: &[(i32, i32, i32)]) -> (World, (i32, i32, i32)) {
    let (x, y) = (5, 10);
    let (gx, gy) = to_iso(x, y);
    let (ix, iy) = (gx as i32, gy as i32);
    let mut blocks = vec![(x, y, 0)];
    for &(dx, dy, z) in extra {
        let (cx, cy) = column(ix + dx, iy + dy);
        blocks.push((cx, cy, z));
    }
    (World::new(Blocks(blocks)), (x, y, 0))
}

#[test]
fn lattice_neighbours_agree_with_the_staggered_grid() {
    for (x, y) in [(5, 10), (5, 11), (-3, -7), (0, 0)] {
        let (ix, iy) = lattice((x, y));
        assert_eq!(column(ix + 1, iy), lower_right(x, y));
        assert_eq!(column(ix, iy + 1), lower_left(x, y));
        assert_eq!(column(ix, iy), (x, y));
    }
}

#[test]
fn ambient_occlusion_is_zero_in_the_open() {
    let (world, cell) = pillar_with(&[]);
    for (du, dv) in [(-1, -1), (-1, 1), (1, -1), (1, 1)] {
        for face in Face::ALL {
            assert_eq!(face_vertex_ao(&world, cell, face, du, dv), 0, "{face:?} {du},{dv}");
        }
    }
}

#[test]
fn a_wall_beside_the_top_face_darkens_the_corners_next_to_it() {
    // A block on the +x side, one layer up.
    let (world, cell, _) = floor_with(&[(1, 0, 1)]);
    assert_eq!(face_vertex_ao(&world, cell, Face::Top, 1, 1), 1);
    assert_eq!(face_vertex_ao(&world, cell, Face::Top, 1, -1), 1);
    assert_eq!(face_vertex_ao(&world, cell, Face::Top, -1, 1), 0);
    assert_eq!(face_vertex_ao(&world, cell, Face::Top, -1, -1), 0);
}

#[test]
fn an_inside_corner_is_fully_occluded() {
    let (world, cell, _) = floor_with(&[(1, 0, 1), (0, 1, 1)]);
    assert_eq!(face_vertex_ao(&world, cell, Face::Top, 1, 1), 3, "both sides solid: closed");
    assert_eq!(face_vertex_ao(&world, cell, Face::Top, 1, -1), 1);
    assert_eq!(face_vertex_ao(&world, cell, Face::Top, -1, 1), 1);
    assert_eq!(face_vertex_ao(&world, cell, Face::Top, -1, -1), 0);
}

#[test]
fn only_the_corner_cell_counts_for_one_level() {
    let (world, cell, _) = floor_with(&[(1, 1, 1)]);
    assert_eq!(face_vertex_ao(&world, cell, Face::Top, 1, 1), 1);
    assert_eq!(face_vertex_ao(&world, cell, Face::Top, 1, -1), 0);
    assert_eq!(face_vertex_ao(&world, cell, Face::Top, -1, 1), 0);
}

#[test]
fn the_side_faces_get_occlusion_from_the_row_in_front_of_them() {
    // Look at the left face (+y plane) of the block: a block above the cell in front of it.
    let (world, cell) = pillar_with(&[(0, 1, 1)]);
    for du in [-1, 1] {
        assert_eq!(face_vertex_ao(&world, cell, Face::Left, du, 1), 1, "top vertices touch it");
        assert_eq!(face_vertex_ao(&world, cell, Face::Left, du, -1), 0, "bottom vertices do not");
    }
    // The right face (+x plane): a block above the cell in front of it.
    let (world, cell) = pillar_with(&[(1, 0, 1)]);
    for du in [-1, 1] {
        assert_eq!(face_vertex_ao(&world, cell, Face::Right, du, 1), 1);
        assert_eq!(face_vertex_ao(&world, cell, Face::Right, du, -1), 0);
    }
    // A block beside the left face's cell darkens the vertices on that side.
    let (world, cell) = pillar_with(&[(1, 1, 0)]);
    assert_eq!(face_vertex_ao(&world, cell, Face::Left, 1, 1), 1, "side cell in the row in front, same height");
    assert_eq!(face_vertex_ao(&world, cell, Face::Left, 1, -1), 1);
    assert_eq!(face_vertex_ao(&world, cell, Face::Left, -1, 1), 0);
}

#[test]
fn ambient_occlusion_stays_in_range_across_chunk_borders_and_negative_coordinates() {
    let world = World::new(IslandGenerator::new(1));
    let mut darkest = 0;
    for x in -12..22 {
        for y in -45..85 {
            for z in 0..10 {
                if world.get(x, y, z).is_air() {
                    continue;
                }
                for face in Face::ALL {
                    for (du, dv) in [(-1, -1), (-1, 1), (1, -1), (1, 1)] {
                        let ao = face_vertex_ao(&world, (x, y, z), face, du, dv);
                        assert!(ao <= 3);
                        darkest = darkest.max(ao);
                    }
                }
            }
        }
    }
    assert!(darkest >= 2, "a stepped mountain has crevices");
}

// ----------------------------------------------------------------------------------------- shading

fn noon() -> LightState {
    let mut e = LightEngine::new(-40, DEFAULT_AZIMUTH_SPEED);
    e.set_to_noon();
    e.state()
}

#[test]
fn at_noon_a_top_face_keeps_roughly_its_own_colour_and_sides_are_darker() {
    let state = noon();
    let white = Vec3::ONE;
    let shading = Shading::default();
    let top = shade_vertex(&state, &shading, white, Face::Top, 0.0, Vec3::ZERO);
    let left = shade_vertex(&state, &shading, white, Face::Left, 0.0, Vec3::ZERO);
    let right = shade_vertex(&state, &shading, white, Face::Right, 0.0, Vec3::ZERO);
    assert!(top.x > 0.9 && top.x < 1.15, "top {top:?}");
    assert!(left.x > 0.5 && left.x < top.x, "left {left:?}");
    assert!((left - right).length() < 1e-5);
}

#[test]
fn occlusion_darkens_by_the_configured_strength() {
    let state = noon();
    let mut shading = Shading::default();
    let open = shade_vertex(&state, &shading, Vec3::ONE, Face::Top, 0.0, Vec3::ZERO);
    let closed = shade_vertex(&state, &shading, Vec3::ONE, Face::Top, 1.0, Vec3::ZERO);
    assert!(approx(closed.x, open.x * 0.5), "strength 0.5 halves a fully occluded vertex");
    shading.ao_strength = 0.0;
    assert_eq!(shade_vertex(&state, &shading, Vec3::ONE, Face::Top, 1.0, Vec3::ZERO), open, "AO off");
    shading.ao_strength = 1.0;
    assert_eq!(shade_vertex(&state, &shading, Vec3::ONE, Face::Top, 1.0, Vec3::ZERO), Vec3::ZERO);
}

#[test]
fn night_is_much_darker_than_noon() {
    let mut e = LightEngine::new(-40, DEFAULT_AZIMUTH_SPEED);
    e.set_to_night();
    let night = e.state();
    let shading = Shading::default();
    let albedo = Vec3::new(0.36, 0.64, 0.25);
    let day = shade_vertex(&noon(), &shading, albedo, Face::Top, 0.0, Vec3::ZERO);
    let dark = shade_vertex(&night, &shading, albedo, Face::Top, 0.0, Vec3::ZERO);
    assert!(dark.length() < day.length() * 0.8, "day {day:?}, night {dark:?}");
}

#[test]
fn a_moonless_night_is_dim_but_not_black() {
    // Like the Java shader's max(light, 1): colours stay recognisable in the dark.
    let e = fixed_engine((90.0, -50.0), None);
    let shading = Shading::default();
    let grass = Vec3::new(0.36, 0.64, 0.25);
    let c = shade_vertex(&e.state(), &shading, grass, Face::Top, 0.0, Vec3::ZERO);
    assert!(c.min_element() > 0.2 && c.max_element() < 0.5, "{c:?}");
    assert!(c.y > c.x && c.y > c.z, "still green, {c:?}");
    let mut black = shading;
    black.min_light = 0.0;
    black.night_grading = false;
    assert_eq!(shade_vertex(&e.state(), &black, grass, Face::Top, 0.0, Vec3::ZERO), Vec3::ZERO);
    // The floor never darkens daylight.
    assert_eq!(shade_vertex(&noon(), &shading, grass, Face::Top, 0.0, Vec3::ZERO).x, shade_vertex(&noon(), &black, grass, Face::Top, 0.0, Vec3::ZERO).x);
}

#[test]
fn point_light_brightens_a_dark_night_vertex() {
    let mut e = LightEngine::new(-40, DEFAULT_AZIMUTH_SPEED);
    e.set_to_night();
    let shading = Shading::default();
    let albedo = Vec3::splat(0.6);
    let dark = shade_vertex(&e.state(), &shading, albedo, Face::Top, 0.0, Vec3::ZERO);
    let lamp = PointLight::new(Vec3::new(0.0, 0.0, 2.0), Vec3::new(1.0, 0.8, 0.4), 8.0, 1.0);
    let torch = lamp.contribution(Vec3::ZERO, Face::Top);
    let lit = shade_vertex(&e.state(), &shading, albedo, Face::Top, 0.0, torch);
    assert!(lit.x > dark.x + 0.1, "dark {dark:?} lit {lit:?}");
    assert!(lit.x > lit.z, "warm light stays warm");
}

#[test]
fn night_grading_desaturates() {
    let mut e = LightEngine::new(-40, DEFAULT_AZIMUTH_SPEED);
    e.set_to_night();
    let mut state = e.state();
    let colourful = Vec3::new(0.9, 0.2, 0.1);
    let mut shading = Shading::default();
    shading.night_grading = false;
    let plain = shade_vertex(&state, &shading, colourful, Face::Top, 0.0, Vec3::splat(0.1));
    shading.night_grading = true;
    let graded = shade_vertex(&state, &shading, colourful, Face::Top, 0.0, Vec3::splat(0.1));
    let spread = |c: Vec3| c.max_element() - c.min_element();
    assert!(spread(graded) < spread(plain), "{plain:?} -> {graded:?}");
    state.night_mix = 0.0;
    assert_eq!(shade_vertex(&state, &shading, colourful, Face::Top, 0.0, Vec3::splat(0.1)), plain, "no grading by day");
}

#[test]
fn shading_is_deterministic_and_never_returns_nan() {
    let state = noon();
    let shading = Shading::default();
    let a = shade_vertex(&state, &shading, Vec3::new(0.3, 0.5, 0.7), Face::Left, 0.3, Vec3::splat(0.2));
    let b = shade_vertex(&state, &shading, Vec3::new(0.3, 0.5, 0.7), Face::Left, 0.3, Vec3::splat(0.2));
    assert_eq!(a, b);
    for bad in [f32::NAN, f32::INFINITY, -1.0, 1e30] {
        let c = shade_vertex(&state, &shading, Vec3::splat(bad), Face::Top, bad, Vec3::splat(bad));
        assert!(c.is_finite() && c.min_element() >= 0.0, "{bad}: {c:?}");
    }
    assert_eq!(shade_vertex(&state, &shading, Vec3::ZERO, Face::Top, 0.0, Vec3::ZERO), Vec3::ZERO);
}

#[test]
fn faces_index_round_trip() {
    for face in Face::ALL {
        assert_eq!(Face::from_index(face.index()), Some(face));
    }
    assert_eq!(Face::from_index(3), None);
    assert_eq!(Face::Left.normal(), Vec3::Y);
    assert_eq!(Face::Right.normal(), Vec3::X);
    assert_eq!(Face::Top.normal(), Vec3::Z);
}

#[test]
fn the_sun_is_golden_like_in_caveland() {
    let e = LightEngine::new(-40, DEFAULT_AZIMUTH_SPEED);
    assert_eq!(e.sun().tone(), Vec3::new(1.0, 0.8, 0.3));
    let mut low = fixed_engine((90.0, 15.0), None);
    low.sun_mut().set_tone(Vec3::new(1.0, 0.8, 0.3));
    let c = low.state().sun_color;
    assert!(c.x > c.y && c.y > c.z, "golden hour is warm, {c:?}");
}

#[test]
fn the_night_is_bluish_not_black() {
    let mut e = LightEngine::new(-40, DEFAULT_AZIMUTH_SPEED);
    e.set_to_night();
    let c = shade_vertex(&e.state(), &Shading::default(), Vec3::splat(0.5), Face::Top, 0.0, Vec3::ZERO);
    assert!(c.z > c.x * 1.1, "blue dominates, {c:?}");
    assert!(c.min_element() > 0.2, "the base colour stays visible, {c:?}");
}

#[test]
fn fog_only_starts_behind_the_centre_and_is_capped() {
    assert_eq!(fog_mix(-500.0), 0.0, "nearer than the centre: no fog");
    assert_eq!(fog_mix(40.8), 0.0);
    assert!(fog_mix(300.0) > 0.0 && fog_mix(300.0) < fog_mix(600.0));
    assert!((fog_mix(1e6) - FOG_MAX).abs() < 1e-6, "capped");
}

#[test]
fn fog_pulls_a_colour_towards_a_light_haze_instead_of_darkening_or_brightening_it_blindly() {
    let fog = Vec3::new(0.3, 0.4, 1.0);
    let haze = fog_haze(fog, 0.0);
    let saturation = |c: Vec3| c.max_element() - c.min_element();
    assert!(haze.dot(LUMA) > fog.dot(LUMA), "haze is lighter than the raw fog colour: {haze:?}");
    assert!(haze.z > haze.x, "still bluish: {haze:?}");
    for grass in [Vec3::new(0.36, 0.64, 0.25), Vec3::new(0.2, 0.35, 0.15)] {
        let far = grass.lerp(haze, fog_mix(600.0));
        assert!(saturation(far) < saturation(grass), "less saturated: {far:?}");
        assert!(far.dot(LUMA) >= grass.dot(LUMA) - 0.02, "not darker: {far:?} from {grass:?}");
    }
    assert!(fog_haze(fog, 1.0).dot(LUMA) < 0.4 * haze.dot(LUMA), "dim at night");
}

#[test]
fn lambert_toward_agrees_with_the_three_known_faces_and_lights_the_opposite_ones() {
    for (azimuth, height) in [(90.0, 50.0), (30.0, 20.0), (200.0, 60.0), (300.0, 10.0)] {
        let sun = source(azimuth, height);
        for (face, normal) in [(Face::Left, Vec3::Y), (Face::Right, Vec3::X), (Face::Top, Vec3::Z)] {
            assert!((sun.lambert(face) - sun.lambert_toward(normal)).abs() < 1e-4, "{face:?} at {azimuth}/{height}");
        }
        // A face and its opposite are never both lit.
        assert!(sun.lambert_toward(Vec3::Y) * sun.lambert_toward(Vec3::NEG_Y) == 0.0);
        assert!(sun.lambert_toward(Vec3::X) * sun.lambert_toward(Vec3::NEG_X) == 0.0);
    }
}

#[test]
fn set_azimuth_keeps_the_offset_between_sun_and_moon() {
    let mut e = LightEngine::new(DEFAULT_WORLD_SPIN_ANGLE, DEFAULT_AZIMUTH_SPEED);
    e.sun_mut().set_azimuth(100.0);
    e.moon.as_mut().unwrap().set_azimuth(250.0); // 150 degrees ahead, not opposite
    e.set_azimuth(10.0);
    assert!(approx(e.sun().azimuth(), 10.0));
    assert!(approx(e.moon().unwrap().azimuth(), 160.0), "{}", e.moon().unwrap().azimuth());
    e.set_azimuth(350.0); // across the wrap
    assert!(approx(e.moon().unwrap().azimuth(), 140.0), "{}", e.moon().unwrap().azimuth());
}

#[test]
fn the_low_sun_is_golden_and_the_high_sun_is_not() {
    let high = fixed_engine((90.0, 60.0), None);
    let low = fixed_engine((90.0, 8.0), None);
    let night = fixed_engine((90.0, -50.0), None);
    let (h, l) = (high.state().sun_color, low.state().sun_color);
    assert_eq!(h, high.sun().light(), "no tint at noon");
    // Gold means a high red to blue ratio, much more than at noon, and the light is not dimmer.
    assert!(l.x / l.z > 3.0 * (h.x / h.z), "{h:?} {l:?}");
    assert!(l.x >= h.x * 0.9, "{h:?} {l:?}");
    assert_eq!(night.state().sun_color, Vec3::ZERO);
    assert!(golden_hour(60.0) == 0.0 && golden_hour(8.0) == 1.0 && golden_hour(-50.0) == 0.0);
    // It builds up smoothly while the sun goes down.
    let mut last = 0.0;
    for h in (0..=40).rev() {
        let g = golden_hour(h as f32);
        assert!(g >= last - 1e-6, "not monotonic at {h}");
        last = g;
    }
}

#[test]
fn the_start_matches_the_running_day_cycle() {
    // Placing the moon for a sun position gives what the engine reaches by running to it.
    let mut run = LightEngine::new(DEFAULT_WORLD_SPIN_ANGLE, DEFAULT_AZIMUTH_SPEED);
    run_days(&mut run, 3.0, |_| {});
    let mut placed = LightEngine::new(DEFAULT_WORLD_SPIN_ANGLE, DEFAULT_AZIMUTH_SPEED);
    placed.sun_mut().set_azimuth(run.sun().azimuth());
    placed.place_moon();
    let diff = (placed.moon().unwrap().azimuth() - run.moon().unwrap().azimuth() + 540.0).rem_euclid(360.0) - 180.0;
    assert!(diff.abs() < 3.0, "{diff}");
}

// ---------------------------------------------------------------------------------------- tone map

#[test]
fn the_tone_map_leaves_ordinary_colours_alone() {
    for c in [Vec3::ZERO, Vec3::new(0.2, 0.5, 0.1), Vec3::splat(TONE_KNEE), Vec3::new(0.8, 0.3, 0.0)] {
        assert_eq!(tone_map(c), c);
    }
    assert_eq!(tone_map(Vec3::new(-1.0, 0.5, 0.2)), Vec3::new(0.0, 0.5, 0.2), "negatives are floored");
}

#[test]
fn the_tone_map_never_exceeds_one_and_keeps_getting_brighter() {
    let mut last = 0.0;
    for i in 0..=200 {
        let v = i as f32 * 0.05; // 0 to 10
        let out = tone_map(Vec3::splat(v));
        assert!(out.max_element() <= 1.0, "{v} -> {out:?}");
        assert!(out.x >= last - 1e-6, "dimmer at {v}");
        last = out.x;
    }
    assert!(last > 0.99, "a very bright colour gets close to white: {last}");
}

#[test]
fn the_tone_map_is_continuous_at_the_knee() {
    let below = tone_map(Vec3::splat(TONE_KNEE - 1e-4));
    let above = tone_map(Vec3::splat(TONE_KNEE + 1e-4));
    assert!((above - below).length() < 1e-3, "{below:?} {above:?}");
    // And it does not slow down abruptly: the slope just above the knee is close to 1.
    let slope = (tone_map(Vec3::splat(TONE_KNEE + 0.02)).x - tone_map(Vec3::splat(TONE_KNEE)).x) / 0.02;
    assert!(slope > 0.9 && slope <= 1.0, "{slope}");
}

#[test]
fn a_bright_gold_stays_gold_instead_of_clipping_to_yellow() {
    // The golden hour sun on a lit side: red above 1, green about half, blue low.
    let gold = Vec3::new(1.6, 0.7, 0.15);
    let clipped = gold.min(Vec3::ONE); // what the 8-bit pipeline did
    let mapped = tone_map(gold);
    let ratio = |c: Vec3| c.y / c.x;
    // Clipping pulls green/red from 0.44 up to 0.70 (yellow); the tone map keeps it much nearer the
    // original (a little is lost on purpose: very bright colours turn towards white).
    let original = ratio(gold);
    assert!(ratio(mapped) - original < 0.5 * (ratio(clipped) - original), "green/red: original {original} mapped {} clipped {}", ratio(mapped), ratio(clipped));
    assert!(mapped.x > mapped.y && mapped.y > mapped.z, "{mapped:?}");
}

#[test]
fn the_tone_map_shader_uses_the_same_constants() {
    let wgsl = include_str!("../../../wurfel-web/src/tonemap.wgsl");
    assert!(wgsl.contains(&format!("const KNEE = {TONE_KNEE:?};")), "KNEE differs");
    assert!(wgsl.contains(&format!("const DESATURATION = {TONE_DESATURATION:?};")), "DESATURATION differs");
}
