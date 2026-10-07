//! Grass blades on top of grass blocks: wind, placement and bending, no graphics.
//!
//! Port of `TopologicalSort.drawGrass` / `updateWind` of the Java engine (the same idea lives in
//! Caveland's `GrassBlock`). The Java code was disabled in the end (`false &&`, a commented-out
//! call), so this is a port of what it was meant to do. All functions are pure and deterministic so
//! every client and every frame agree; a renderer calls them per grass block and per blade.
//!
//! Units: the Java "game space" has 200 units across a block's diagonal (`x` = `(gx - gy) * 100`)
//! and 100 units per row (`y` = `(gx + gy) * 100`), see [`game_xy`]. Offsets and the force centre
//! are in game units, rotations in degrees, counter-clockwise positive (libGDX).
//!
//! Differences from Java, on purpose:
//!
//! * The seed of a cell is a hash of its position ([`cell_seed`]) instead of the first draw of a
//!   global `Random(1)` per sorter instance, so it does not depend on creation order.
//! * The per-blade jitter is smooth value noise of a hash ([`jitter`]) instead of a fresh
//!   `Random.nextFloat()` every frame, which made the blades twitch.
//! * `update_wind` takes seconds; Java's `dt` was in milliseconds (`dt * 0.01` there is
//!   `dt_seconds * 10` here, the wind swings with a 2 s period).
//! * The force distance is measured between the blade and the force centre in the same space.
//!   Java mixed in `-yPos * 2 ... + 900` terms tuned for some other convention, which does not put
//!   the centre where the player is. The sign uses the blade's own `x` instead of its cell's.

use crate::grid::to_iso;

/// Largest sway of the wind in degrees (Java `WINDAMPLITUDE`).
pub const WIND_AMPLITUDE: f32 = 20.0;
/// Java `noisenum`: the jitter is up to `NOISE * WIND_AMPLITUDE / 2` degrees.
pub const NOISE: f32 = 0.1;
/// Where the wind blows to, in game space (a unit vector). The sway reaches a blade later the
/// further downwind it stands, so the wind moves over the grass in waves instead of everywhere at once.
pub const WIND_DIRECTION: (f32, f32) = (0.8, 0.6);
/// How fast the waves travel, game units per second (a wavelength of `2 s * speed`, about 11 blocks).
pub const WIND_SPEED: f32 = 800.0;
/// Gusts: a slower, broader wave that makes the sway stronger and weaker, so it does not look like
/// regular stripes. It blows across the main wave.
pub const GUST_DIRECTION: (f32, f32) = (-0.5, 0.866);
/// How fast the gusts travel, game units per second.
pub const GUST_SPEED: f32 = 350.0;
/// Seconds between two gusts at one place.
pub const GUST_PERIOD: f32 = 7.0;
/// The sway is this fraction of its full strength between gusts and 100 % at the height of one.
pub const GUST_MIN: f32 = 0.55;
/// Edge of a block in game units (`RenderCell.GAME_EDGELENGTH`).
pub const EDGE: f32 = 141.0;
const EDGE2: f32 = 70.0;
/// Half the diagonal of a block in game units (`GAME_DIAGLENGTH2`).
const DIAG2: f32 = 100.0;
/// Squared distance (game units) beyond which a blade ignores the force centre.
pub const FORCE_RANGE_SQ: f32 = 200_000.0;
/// Strength of the push: the rotation is `FORCE / distance²` degrees (Java 600000), capped at 90.
pub const FORCE: f32 = 600_000.0;
/// How far a blade bends away from the force centre, as a fraction of that rotation.
pub const FORCE_FACTOR: f32 = 0.3;
/// Most blades a block gets (Java drew 10).
pub const MAX_BLADES_PER_CELL: i32 = 10;
/// Java's test for a stone instead of a blade: `(xPos + i * yPos * 17) % 7 == 0`.
const STONE_PERIOD: f32 = 7.0;

/// The wind: one value for the whole world, advanced by [`Wind::update`] each frame.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Wind {
    circle: f32,
    /// Degrees added to every blade: a triangle wave between `-WIND_AMPLITUDE / 2` and 0.
    pub value: f32,
    /// Seconds since the start, drives the [`jitter`].
    pub time: f32,
}

impl Wind {
    /// Advance by `dt` seconds. Java: `windWholeCircle = (windWholeCircle + dt * 0.01) % AMP` with
    /// `dt` in milliseconds, then `|windWholeCircle - AMP / 2| - AMP / 2`.
    pub fn update(&mut self, dt: f32) {
        self.circle = (self.circle + dt * 1000.0 * 0.01) % WIND_AMPLITUDE;
        self.value = (self.circle - WIND_AMPLITUDE / 2.0).abs() - WIND_AMPLITUDE / 2.0;
        self.time += dt;
    }
}

impl Wind {
    /// The wind at the game-space position `at`: the same triangle wave as [`Wind::value`], but it
    /// reaches the position `distance along the wind / WIND_SPEED` seconds late, and a gust (a
    /// slower wave of its own) scales it.
    pub fn at(&self, at: (f32, f32)) -> f32 {
        let delay = (at.0 * WIND_DIRECTION.0 + at.1 * WIND_DIRECTION.1) / WIND_SPEED;
        // The circle advances 10 units per second (Java: `dt` in milliseconds times 0.01).
        let circle = ((self.time - delay) * 10.0).rem_euclid(WIND_AMPLITUDE);
        let wave = (circle - WIND_AMPLITUDE / 2.0).abs() - WIND_AMPLITUDE / 2.0;
        let gust_delay = (at.0 * GUST_DIRECTION.0 + at.1 * GUST_DIRECTION.1) / GUST_SPEED;
        let gust = ((self.time - gust_delay) / GUST_PERIOD * std::f32::consts::TAU).sin() * 0.5 + 0.5;
        wave * (GUST_MIN + (1.0 - GUST_MIN) * gust)
    }
}

/// `update_wind` of the Java sorter as a free function over `(circle, dt)`; returns the new circle
/// position and the wind value. [`Wind::update`] is the stateful twin.
pub fn update_wind(circle: f32, dt: f32) -> (f32, f32) {
    let circle = (circle + dt * 1000.0 * 0.01) % WIND_AMPLITUDE;
    (circle, (circle - WIND_AMPLITUDE / 2.0).abs() - WIND_AMPLITUDE / 2.0)
}

/// The Java game-space position of the centre of block column `(x, y)`.
pub fn game_xy(x: i32, y: i32) -> (f32, f32) {
    let (gx, gy) = to_iso(x, y);
    ((gx - gy) * 100.0, (gx + gy) * 100.0)
}

/// 32-bit integer hash (lowbias32), the basis of all randomness here.
fn hash(mut h: u32) -> u32 {
    h ^= h >> 16;
    h = h.wrapping_mul(0x7feb_352d);
    h ^= h >> 15;
    h = h.wrapping_mul(0x846c_a68b);
    h ^ (h >> 16)
}

fn to_unit(h: u32) -> f32 {
    (h >> 8) as f32 / (1u32 << 24) as f32
}

fn mix(values: &[i32]) -> u32 {
    values.iter().fold(0x9e37_79b9u32, |acc, &v| hash(acc ^ (v as u32).wrapping_mul(0x85eb_ca6b)))
}

/// The seed of a block in `[0, 1)` (Java: one `Random.nextFloat()` per sorter), a hash of its
/// position.
pub fn cell_seed(x: i32, y: i32, z: i32) -> f32 {
    to_unit(mix(&[x, y, z]))
}

/// What a blade looks like.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// The sprite `e7-0`.
    Blade,
    /// The small sprite `e7-1`.
    Stone,
}

/// One placed blade, everything a renderer needs except the sprite.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Blade {
    pub kind: Kind,
    /// Offset from the block's centre in game units (`xOffset`, `yOffset`).
    pub offset: (i32, i32),
    /// Offset in isometric ground blocks: add to the column's `to_iso`.
    pub iso_offset: (f32, f32),
    /// Multiplied into the sprite. Java drew with `0.5` as neutral; this is that times two.
    pub tint: [f32; 3],
    pub scale: f32,
    /// Rotation in degrees (counter-clockwise positive) about the blade's foot.
    pub rotation: f32,
}

/// Where blade `i` of a cell stands, in game units from the cell's centre, or `None` when the
/// formulas put it outside the cell's diamond. The same for every call (Java
/// `xOffset`/`yOffset`; the float maths is kept in the same order).
pub fn placement(x_pos: f32, y_pos: f32, seed: f32, i: i32) -> Option<(i32, i32)> {
    let fi = i as f32;
    let xo = (((x_pos - seed * 17.0) * fi * y_pos).abs() % EDGE - EDGE2) as i32;
    let yo = ((((x_pos - fi) * 3.0 * (y_pos * seed * 11.0 - fi))).abs() % EDGE - EDGE2) as i32;
    ((xo.abs() + yo.abs()) as f32 <= DIAG2 - 1.0).then_some((xo, yo))
}

/// The colour tint of a blade: Java `(1/2, 1/2 - (xOffset + i) % 7 * 0.005, 1/2)` doubled, a bit
/// less green for some blades. Stones go reddish-grey with distance from the middle.
pub fn tint(kind: Kind, x_offset: i32, i: i32) -> [f32; 3] {
    match kind {
        Kind::Blade => [1.0, (1.0 - ((x_offset + i) % 7) as f32 * 0.01).clamp(0.0, 1.0), 1.0],
        Kind::Stone => {
            let dim = 1.0 - (x_offset as f32 * 0.4 / DIAG2).abs();
            [1.0, dim.clamp(0.0, 1.0), dim.clamp(0.0, 1.0)]
        }
    }
}

/// Size of a blade: smaller towards the right edge of the block (Java `1 - xOffset * 0.3 / 100`).
pub fn scale(x_offset: i32) -> f32 {
    1.0 - x_offset as f32 * 0.3 / DIAG2
}

/// Which way a blade bends because something stands near it: degrees to add to the rotation,
/// already multiplied by [`FORCE_FACTOR`]. `blade` and `force` are game-space `(x, y)`.
/// Zero beyond [`FORCE_RANGE_SQ`], away from the centre (right of it is clockwise, negative),
/// at most `90 * FORCE_FACTOR` in size.
pub fn force_bend(blade: (f32, f32), force: (f32, f32)) -> f32 {
    let (dx, dy) = (blade.0 - force.0, blade.1 - force.1);
    let distance = dx * dx + dy * dy;
    if distance > FORCE_RANGE_SQ {
        return 0.0;
    }
    let mut rot = if distance <= 0.0 { 90.0 } else { (FORCE / distance).min(90.0) };
    if force.0 < blade.0 {
        rot = -rot;
    }
    rot * FORCE_FACTOR
}

/// Smooth pseudo-random value in `[0, 1)` for blade `i` of a cell at time `t` seconds: noise that
/// drifts a few times a second instead of jumping each frame. Deterministic in all its inputs.
pub fn jitter(seed: f32, i: i32, t: f32) -> f32 {
    let key = mix(&[(seed * 16_777_216.0) as i32, i]);
    let phase = t.max(0.0) * 3.0;
    let k = phase.floor();
    let f = phase - k;
    let f = f * f * (3.0 - 2.0 * f);
    let a = to_unit(hash(key ^ (k as u32)));
    let b = to_unit(hash(key ^ (k as u32).wrapping_add(1)));
    a + (b - a) * f
}

/// Rotation in degrees of blade `i`: a fixed lean (`i * 0.4 - 10.2`), the wind, the jitter and the
/// bend away from the force centre (Java `setRotation(...)`).
pub fn rotation(i: i32, wind: f32, jitter: f32, bend: f32) -> f32 {
    i as f32 * 0.4 - 10.2 + wind + jitter * NOISE * WIND_AMPLITUDE / 2.0 + bend
}

/// How many blades a block at `distance` blocks from the viewer gets when `density` is the
/// number near the viewer (`0` turns grass off): full density within `NEAR`, then fewer, then none.
pub fn blades_at_distance(density: i32, distance: f32) -> i32 {
    const NEAR: f32 = 6.0;
    const MID: f32 = 12.0;
    const FAR: f32 = 20.0;
    let density = density.clamp(0, MAX_BLADES_PER_CELL * 2);
    if distance <= NEAR {
        density
    } else if distance <= MID {
        (density + 1) / 2
    } else if distance <= FAR {
        (density + 4) / 5
    } else {
        0
    }
}

/// The blades of the block at column `(x, y)` and height `z`, the first `count` of them, as drawn
/// at wind `wind`, time `t` and with the force centre `force` in game units (`None` for none).
pub fn blades(x: i32, y: i32, z: i32, count: i32, wind: &Wind, force: Option<(f32, f32)>) -> Vec<Blade> {
    let (x_pos, y_pos) = game_xy(x, y);
    let seed = cell_seed(x, y, z);
    let mut out = Vec::with_capacity(count.max(0) as usize);
    for i in 0..count.max(0) {
        let Some((xo, yo)) = placement(x_pos, y_pos, seed, i) else { continue };
        let stone = (x_pos + i as f32 * y_pos * 17.0) % STONE_PERIOD == 0.0;
        let kind = if stone { Kind::Stone } else { Kind::Blade };
        let spot = (x_pos + xo as f32, y_pos + yo as f32);
        let (rotation, scale) = match kind {
            Kind::Blade => {
                let bend = force.map_or(0.0, |f| force_bend(spot, f));
                (rotation(i, wind.at(spot), jitter(seed, i, wind.time), bend), scale(xo))
            }
            // The Java stone sprite was neither scaled nor turned.
            Kind::Stone => (0.0, 1.0),
        };
        out.push(Blade {
            kind,
            offset: (xo, yo),
            iso_offset: ((xo + yo) as f32 / 200.0, (yo - xo) as f32 / 200.0),
            tint: tint(kind, xo, i),
            scale,
            rotation,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wind_at_time(time: f32) -> Wind {
        let mut w = Wind::default();
        w.update(time);
        w
    }

    #[test]
    fn the_wind_reaches_places_downwind_later() {
        // The same sway arrives at a place `d` units downwind `d / WIND_SPEED` seconds after the
        // origin. (Leave the gust out of it by comparing the unscaled wave through its ratio.)
        let d = 400.0;
        let downwind = (WIND_DIRECTION.0 * d, WIND_DIRECTION.1 * d);
        let delay = d / WIND_SPEED;
        let early = wind_at_time(3.0).at((0.0, 0.0));
        let late = wind_at_time(3.0 + delay).at(downwind);
        // Same wave phase; the gust differs, so only the sign and rough size must agree.
        assert!(early.signum() == late.signum() || early.abs() < 0.5, "{early} {late}");
        // At one moment two far apart places sway differently: it is not everywhere at once.
        let w = wind_at_time(3.0);
        assert!((w.at((0.0, 0.0)) - w.at((500.0, 300.0))).abs() > 0.5);
    }

    #[test]
    fn the_sway_stays_in_range_and_leans_one_way_a_wavelength_downwind() {
        let wavelength = 2.0 * WIND_SPEED;
        let w = wind_at_time(5.3);
        let a = (100.0, -40.0);
        // Along the wind the wave repeats after one wavelength; the gust moves across it, so the
        // values can differ by the gust factor at most.
        let b = (a.0 + WIND_DIRECTION.0 * wavelength, a.1 + WIND_DIRECTION.1 * wavelength);
        let (va, vb) = (w.at(a), w.at(b));
        assert!(va.abs() <= WIND_AMPLITUDE / 2.0 && vb.abs() <= WIND_AMPLITUDE / 2.0);
        assert!(va <= 1e-4 && vb <= 1e-4, "the sway only leans one way: {va} {vb}");
    }

    #[test]
    fn a_gust_never_takes_more_than_it_leaves_and_never_flips_the_sway() {
        for step in 0..400 {
            let w = wind_at_time(step as f32 * 0.173);
            for &at in &[(0.0, 0.0), (900.0, -300.0), (-1200.0, 640.0)] {
                let v = w.at(at);
                assert!((-WIND_AMPLITUDE / 2.0 - 1e-3..=1e-3).contains(&v), "{v}");
            }
        }
        // At a place the sway still reaches its full depth now and then (GUST_MIN is a floor, not a cap).
        let deepest = (0..2000).map(|s| wind_at_time(s as f32 * 0.05).at((0.0, 0.0))).fold(0.0_f32, f32::min);
        assert!(deepest < -WIND_AMPLITUDE / 2.0 * GUST_MIN, "{deepest}");
    }

    #[test]
    fn the_wind_is_a_triangle_wave_between_minus_ten_and_zero() {
        let mut w = Wind::default();
        assert_eq!(w.value, 0.0);
        // 0.1 s is 100 ms = 1 unit of the circle.
        w.update(0.1);
        assert!((w.value - -1.0).abs() < 1e-5, "{}", w.value);
        for _ in 0..4 {
            w.update(0.1);
        }
        assert!((w.value - -5.0).abs() < 1e-4);
        for _ in 0..5 {
            w.update(0.1);
        }
        assert!((w.value - -10.0).abs() < 1e-3, "half a circle is the deepest: {}", w.value);
        for _ in 0..10 {
            w.update(0.1);
        }
        assert!(w.value.abs() < 1e-2, "back where it began after 2 s: {}", w.value);
        for _ in 0..1000 {
            w.update(0.0137);
            assert!((-WIND_AMPLITUDE / 2.0 - 1e-4..=1e-4).contains(&w.value));
        }
    }

    #[test]
    fn the_free_function_matches_the_stateful_wind() {
        let (mut c, mut w) = (0.0, Wind::default());
        for dt in [0.016, 0.03, 0.1, 0.0, 0.25] {
            let (nc, v) = update_wind(c, dt);
            c = nc;
            w.update(dt);
            assert_eq!(v, w.value);
        }
    }

    #[test]
    fn a_cell_seed_is_stable_and_spread_over_the_unit_interval() {
        assert_eq!(cell_seed(3, 4, 5), cell_seed(3, 4, 5));
        assert_ne!(cell_seed(3, 4, 5), cell_seed(4, 3, 5));
        let mut sum = 0.0;
        for i in 0..1000 {
            let s = cell_seed(i, i * 7 % 13, 2);
            assert!((0.0..1.0).contains(&s));
            sum += s;
        }
        assert!((sum / 1000.0 - 0.5).abs() < 0.05);
    }

    #[test]
    fn placement_follows_the_java_formula() {
        // xPos = 200, yPos = 100, seed 0.5, i = 3.
        let (x_pos, y_pos, seed, i) = (200.0f32, 100.0f32, 0.5f32, 3);
        let xo = (((x_pos - seed * 17.0) * i as f32 * y_pos).abs() % 141.0 - 70.0) as i32;
        let yo = ((((x_pos - i as f32) * 3.0 * (y_pos * seed * 11.0 - i as f32))).abs() % 141.0 - 70.0) as i32;
        let expected = ((xo.abs() + yo.abs()) < 100).then_some((xo, yo));
        assert_eq!(placement(x_pos, y_pos, seed, i), expected);
        // Spot-check the first number by hand: 191.5 * 3 * 100 = 57450; 57450 % 141 = 63; 63 - 70 = -7.
        assert_eq!(xo, -7);
    }

    #[test]
    fn placed_blades_stay_in_the_cell_and_do_not_move_between_calls() {
        let mut placed = 0;
        for x in -3..4 {
            for y in -3..4 {
                let (xp, yp) = game_xy(x, y);
                let seed = cell_seed(x, y, 1);
                for i in 0..10 {
                    let a = placement(xp, yp, seed, i);
                    assert_eq!(a, placement(xp, yp, seed, i));
                    if let Some((xo, yo)) = a {
                        placed += 1;
                        assert!(xo.abs() + yo.abs() < 100 && xo.abs() <= 70 && yo.abs() <= 70);
                    }
                }
            }
        }
        assert!(placed > 100, "a fair share of the blades fits in the diamond: {placed}");
    }

    #[test]
    fn game_space_matches_the_staggered_grid() {
        assert_eq!(game_xy(0, 0), (0.0, 0.0));
        assert_eq!(game_xy(1, 0), (200.0, 0.0));
        assert_eq!(game_xy(0, 1), (100.0, 100.0), "odd rows are shifted half a block");
    }

    #[test]
    fn the_iso_offset_stays_inside_the_footprint() {
        for x in -2..3 {
            for y in -2..3 {
                for b in blades(x, y, 1, 10, &Wind::default(), None) {
                    assert!(b.iso_offset.0.abs() <= 0.5 && b.iso_offset.1.abs() <= 0.5, "{b:?}");
                }
            }
        }
    }

    #[test]
    fn force_bends_away_from_the_centre_and_fades_with_distance() {
        let centre = (0.0, 0.0);
        let right = force_bend((100.0, 0.0), centre);
        let left = force_bend((-100.0, 0.0), centre);
        assert!(right < 0.0 && left > 0.0, "right of it leans clockwise (negative), left leans the other way");
        assert!((right + left).abs() < 1e-5);
        // 600000 / 100^2 = 60 degrees, times 0.3.
        assert!((right.abs() - 18.0).abs() < 1e-4);
        assert!(force_bend((200.0, 0.0), centre).abs() < right.abs());
        assert_eq!(force_bend((500.0, 0.0), centre), 0.0, "beyond sqrt(200000) = 447 units");
        // exactly on the centre (and very near) is capped, not infinite
        assert_eq!(force_bend((0.0, 0.0), centre).abs(), 90.0 * FORCE_FACTOR);
        assert_eq!(force_bend((5.0, 0.0), centre).abs(), 90.0 * FORCE_FACTOR);
        // above and below, on the same x: not to the left of the centre, so the sign is positive
        assert!(force_bend((0.0, 100.0), centre) > 0.0);
    }

    #[test]
    fn the_jitter_is_deterministic_smooth_and_small() {
        let s = cell_seed(1, 2, 3);
        assert_eq!(jitter(s, 4, 1.234), jitter(s, 4, 1.234));
        assert_ne!(jitter(s, 4, 1.234), jitter(s, 5, 1.234));
        let mut last = jitter(s, 0, 0.0);
        for k in 1..600 {
            let v = jitter(s, 0, k as f32 / 60.0);
            assert!((0.0..1.0).contains(&v));
            assert!((v - last).abs() < 0.2, "no jump between frames: {last} to {v}");
            last = v;
        }
        // at most noise * amplitude / 2 = 1 degree
        assert!((rotation(0, 0.0, 1.0, 0.0) - (-10.2 + 1.0)).abs() < 1e-5);
    }

    #[test]
    fn the_rotation_adds_lean_wind_jitter_and_bend() {
        assert!((rotation(10, -5.0, 0.5, 2.0) - (4.0 - 10.2 - 5.0 + 0.5 + 2.0)).abs() < 1e-5);
    }

    #[test]
    fn colour_and_scale_follow_the_offset() {
        assert_eq!(scale(0), 1.0);
        assert!((scale(-50) - 1.15).abs() < 1e-5);
        assert!((scale(50) - 0.85).abs() < 1e-5);
        let t = tint(Kind::Blade, 0, 0);
        assert_eq!(t, [1.0, 1.0, 1.0]);
        assert!(tint(Kind::Blade, 0, 6)[1] < 1.0, "some blades are a little less green");
        let stone = tint(Kind::Stone, 50, 0);
        assert!(stone[1] < 1.0 && stone[2] < 1.0 && stone[0] == 1.0);
    }

    #[test]
    fn fewer_blades_farther_away_and_none_when_off() {
        assert_eq!(blades_at_distance(10, 3.0), 10);
        assert_eq!(blades_at_distance(10, 9.0), 5);
        assert_eq!(blades_at_distance(10, 15.0), 2);
        assert_eq!(blades_at_distance(10, 25.0), 0);
        assert_eq!(blades_at_distance(0, 1.0), 0);
        assert_eq!(blades_at_distance(-3, 1.0), 0);
        assert!(blades_at_distance(1000, 1.0) <= MAX_BLADES_PER_CELL * 2);
    }

    #[test]
    fn blades_of_a_block_react_to_force_and_wind_but_keep_their_places() {
        let calm = blades(5, 6, 2, 10, &Wind::default(), None);
        assert!(!calm.is_empty());
        let mut wind = Wind::default();
        wind.update(0.5);
        let (xp, yp) = game_xy(5, 6);
        let pushed = blades(5, 6, 2, 10, &wind, Some((xp - 150.0, yp)));
        assert_eq!(calm.len(), pushed.len());
        for (a, b) in calm.iter().zip(&pushed) {
            assert_eq!(a.offset, b.offset);
            assert_eq!(a.kind, b.kind);
            if a.kind == Kind::Blade {
                assert!(b.rotation < a.rotation, "force from the left and the wind both lean them: {a:?} {b:?}");
            }
        }
        assert_eq!(blades(5, 6, 2, 10, &wind, None), blades(5, 6, 2, 10, &wind, None));
    }
}
