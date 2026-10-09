//! Atmosphere and life: the pure maths behind the ambient particles (pollen, fireflies, dust motes,
//! leaves, mist), the weather (rain, snow) and the fog sprites, no graphics.
//!
//! The client draws all of it as instanced quads whose positions the vertex shader computes from a
//! time uniform (`wurfel-web/src/atmosphere.wgsl`), so nothing here runs per particle. What lives
//! here is what has to agree between the CPU and the shader and what is worth a unit test: the wind
//! (the same direction and gusts as the grass, see [`crate::grass`]), how strong each kind of particle
//! is at a given sun height and weather, how many instances a density setting buys, the cycle of a
//! falling drop and its splash ring, and the column map the shader reads the ground from.
//!
//! Frames: positions are in the isometric ground frame `(gx, gy, z)` in blocks (`grid::to_iso`),
//! the sun's height is the `z` of the unit vector towards the sun (`LightState::sun_direction`):
//! 1 at the zenith, 0 on the horizon, negative at night. Time is in seconds.

use std::f32::consts::TAU;
use std::ops::Range;

use crate::block::id;
use crate::grass::{GUST_DIRECTION, GUST_MIN, GUST_PERIOD, GUST_SPEED, WIND_DIRECTION};
use crate::grid::from_iso;
use crate::{World, CHUNK_SIZE_Z};

// ------------------------------------------------------------------------------------------ wind

/// A direction or offset of the Java game space (`x = (gx - gy) * 100`, `y = (gx + gy) * 100`) in
/// the ground frame, scaled to blocks. The map is linear, so it serves for directions as well.
pub fn game_to_ground(v: (f32, f32)) -> (f32, f32) {
    ((v.0 + v.1) / 200.0, (v.1 - v.0) / 200.0)
}

/// Where the wind blows to in the ground frame, a unit vector: the grass's [`WIND_DIRECTION`].
pub fn wind_direction() -> (f32, f32) {
    wind_direction_for(WIND_DIRECTION)
}

/// [`wind_direction`] for a wind that blows to `game` (a direction in game space, e.g. the menu's).
pub fn wind_direction_for(game: (f32, f32)) -> (f32, f32) {
    let (x, y) = game_to_ground(game);
    let length = (x * x + y * y).sqrt();
    (x / length, y / length)
}

/// Radians per second of the gusts (the grass's [`GUST_PERIOD`]).
pub const GUST_RATE: f32 = TAU / GUST_PERIOD;

/// How far the gust's phase moves per block along `gx` and `gy`: the grass delays a gust by
/// `(game position . GUST_DIRECTION) / GUST_SPEED` seconds.
pub fn gust_phase_per_block() -> (f32, f32) {
    let k = GUST_RATE * 100.0 / GUST_SPEED;
    (k * (GUST_DIRECTION.0 + GUST_DIRECTION.1), k * (GUST_DIRECTION.1 - GUST_DIRECTION.0))
}

/// The gust factor at the ground position `at` at `time`: [`GUST_MIN`] between gusts, 1 at the height
/// of one. The same slow wave that scales the sway of the grass (`Wind::at`), so a blade and a drifting
/// leaf feel the same gust.
pub fn gust(time: f32, at: (f32, f32)) -> f32 {
    let (px, py) = gust_phase_per_block();
    let wave = (GUST_RATE * time - (px * at.0 + py * at.1)).sin() * 0.5 + 0.5;
    GUST_MIN + (1.0 - GUST_MIN) * wave
}

/// How far the wind has carried something since time 0, in blocks along [`wind_direction`], when the
/// wind blows at `speed` blocks per second times the gust: the integral of `speed * gust`, so a drifting
/// particle moves smoothly through gusts instead of jumping. `at` is where its gust phase is read.
pub fn drift(time: f32, at: (f32, f32), speed: f32) -> f32 {
    let (px, py) = gust_phase_per_block();
    let phase = px * at.0 + py * at.1;
    let mean = GUST_MIN + (1.0 - GUST_MIN) * 0.5;
    let swing = (1.0 - GUST_MIN) * 0.5 / GUST_RATE;
    speed * (mean * time - swing * ((GUST_RATE * time - phase).cos() - (-phase).cos()))
}

/// How fast the wind carries light things (pollen, mist), blocks per second at a gust factor of 1.
pub const DRIFT_SPEED: f32 = 1.1;

// ----------------------------------------------------------------------------------- the weather

/// What falls from the sky.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Weather {
    #[default]
    Clear,
    Rain,
    Snow,
}

impl Weather {
    /// `clear`, `rain` or `snow` (also `none`, `off`, `0`), any case. `None` for anything else.
    pub fn parse(text: &str) -> Option<Weather> {
        match text.trim().to_ascii_lowercase().as_str() {
            "clear" | "none" | "off" | "0" | "false" => Some(Weather::Clear),
            "rain" => Some(Weather::Rain),
            "snow" => Some(Weather::Snow),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Weather::Clear => "clear",
            Weather::Rain => "rain",
            Weather::Snow => "snow",
        }
    }

    /// 1 while something falls, 0 under a clear sky: what takes the sun's beams and the pollen away.
    pub fn overcast(self) -> f32 {
        if self == Weather::Clear {
            0.0
        } else {
            1.0
        }
    }
}

/// Rain falls this fast, blocks per second.
pub const RAIN_SPEED: f32 = 12.0;
/// A drop is drawn for this many blocks above the ground it lands on.
pub const RAIN_FALL: f32 = 18.0;
/// How long a splash ring lives, seconds.
pub const RING_LIFE: f32 = 0.5;
/// One cycle of a drop: the fall, then the splash. The drop's column is drawn again for each cycle.
pub const RAIN_CYCLE: f32 = RAIN_FALL / RAIN_SPEED + RING_LIFE;
/// Sideways blocks the wind blows a drop per block it falls, at a gust factor of 1.
pub const RAIN_SLANT: f32 = 0.3;
/// Snow falls this fast, blocks per second.
pub const SNOW_SPEED: f32 = 1.4;
/// A flake is drawn for this many blocks above the ground of the column it starts over.
pub const SNOW_FALL: f32 = 14.0;
/// One cycle of a flake, seconds.
pub const SNOW_CYCLE: f32 = SNOW_FALL / SNOW_SPEED;
/// How fast the wind carries a flake sideways, blocks per second at a gust factor of 1.
pub const SNOW_DRIFT: f32 = 0.9;

/// Where in its cycle a drop (or flake) is at `time`: `(cycle number, seconds into the cycle)`. The
/// `phase` in `0..1` (a hash of the instance) offsets drops against each other. The cycle number picks
/// the column it falls over, so the rain does not fall in the same places forever.
pub fn cycle(time: f32, phase: f32, length: f32) -> (u32, f32) {
    let u = time / length + phase;
    let n = u.floor();
    (n.max(0.0) as u32, (u - n) * length)
}

/// Height above the ground of a drop `seconds` into its cycle, or `None` once it has landed.
pub fn rain_height(seconds: f32) -> Option<f32> {
    let h = RAIN_FALL - RAIN_SPEED * seconds;
    (h > 0.0).then_some(h)
}

/// Age of the splash ring `seconds` into the cycle, or `None` while the drop still falls or after the
/// ring has gone.
pub fn splash_age(seconds: f32) -> Option<f32> {
    let age = seconds - RAIN_FALL / RAIN_SPEED;
    (0.0..RING_LIFE).contains(&age).then_some(age)
}

/// Radius in blocks of a splash ring of that age; water throws bigger rings than ground does.
pub fn ring_radius(age: f32, water: bool) -> f32 {
    let t = (age / RING_LIFE).clamp(0.0, 1.0);
    if water {
        0.08 + 0.5 * t
    } else {
        0.05 + 0.22 * t
    }
}

/// Opacity of a ring of that age: bright at first, then gone.
pub fn ring_alpha(age: f32, water: bool) -> f32 {
    let t = 1.0 - (age / RING_LIFE).clamp(0.0, 1.0);
    t * t * if water { 0.6 } else { 0.35 }
}

// ------------------------------------------------------------------------------ what is going on

/// The kinds of instanced things the client draws. The numbers are the `kind` of `atmosphere.wgsl`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Pollen = 0,
    Firefly = 1,
    Mote = 2,
    Leaf = 3,
    Mist = 4,
    Rain = 5,
    Splash = 6,
    Snow = 7,
    Fog = 8,
    GodRay = 9,
}

/// The density setting goes up to this; the instance cap of every kind is reached there.
pub const MAX_DENSITY: f32 = 2.0;

impl Kind {
    pub const ALL: [Kind; 10] = [
        Kind::Pollen,
        Kind::Firefly,
        Kind::Mote,
        Kind::Leaf,
        Kind::Mist,
        Kind::Rain,
        Kind::Splash,
        Kind::Snow,
        Kind::Fog,
        Kind::GodRay,
    ];

    /// The most instances of the kind, whatever the setting says. The whole lot is a few thousand
    /// quads, the vertex shader does the rest.
    pub fn cap(self) -> u32 {
        match self {
            Kind::Pollen => 480,
            Kind::Firefly => 120,
            Kind::Mote => 300,
            Kind::Leaf => 200,
            Kind::Mist => 64,
            Kind::Rain | Kind::Splash => 4000,
            Kind::Snow => 3000,
            Kind::Fog => 32,
            Kind::GodRay => 24,
        }
    }

    /// How many instances `density` (0 to [`MAX_DENSITY`], 1 is the default) draws: none at 0, half the
    /// cap at 1, all of it at 2. A value that is not a number counts as 0.
    pub fn count(self, density: f32) -> u32 {
        if !density.is_finite() {
            return 0;
        }
        let share = density.clamp(0.0, MAX_DENSITY) / MAX_DENSITY;
        (self.cap() as f32 * share).round() as u32
    }
}

fn smoothstep(lo: f32, hi: f32, x: f32) -> f32 {
    let t = ((x - lo) / (hi - lo)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// How present each kind of ambient thing is, 0..1, for a sun height and weather. The shader fades its
/// instances in and out with these, so dusk brings the fireflies out one by one.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Strengths {
    pub pollen: f32,
    pub fireflies: f32,
    pub motes: f32,
    pub leaves: f32,
    pub mist: f32,
    /// The large soft fog banks.
    pub fog: f32,
    /// The slanting beams of light.
    pub god_rays: f32,
}

impl Strengths {
    /// `sun_z`: the height of the sun (see the module doc).
    pub fn at(sun_z: f32, weather: Weather) -> Strengths {
        let wet = weather.overcast();
        let day = smoothstep(0.12, 0.5, sun_z);
        // Low sun: the time the beams and the motes in them show. Noon and night have none.
        let low_sun = smoothstep(0.05, 0.22, sun_z) * (1.0 - smoothstep(0.5, 0.85, sun_z));
        let gloom = 1.0 - smoothstep(0.1, 0.6, sun_z);
        Strengths {
            pollen: day * (1.0 - 0.9 * wet),
            fireflies: (1.0 - smoothstep(0.0, 0.3, sun_z)) * (1.0 - wet),
            motes: low_sun * (1.0 - 0.8 * wet),
            leaves: (0.25 + 0.75 * smoothstep(-0.05, 0.3, sun_z)) * (1.0 + 0.8 * wet) / 1.8,
            mist: (0.3 + 0.7 * gloom).max(0.8 * wet),
            fog: (0.2 + 0.6 * gloom + 0.4 * wet).min(1.0),
            god_rays: low_sun * (1.0 - wet),
        }
    }
}

// --------------------------------------------------------------------------------- the column map

/// The ground the shader looks up: one `u16` per block column, indexed by the lattice point
/// `(gx, gy)` of the ground frame (every integer `(gx, gy)` is the centre of exactly one column).
/// The low byte is the height of the column's surface in blocks (the top of its highest block, 0
/// when it has none); the flags say what the top block is.
pub const HEIGHT_MASK: u16 = 0xff;
pub const COLUMN_WATER: u16 = 1 << 8;
pub const COLUMN_GRASS: u16 = 1 << 9;
pub const COLUMN_TREE: u16 = 1 << 10;
/// The column is not loaded: nothing is drawn over it.
pub const COLUMN_UNKNOWN: u16 = 1 << 15;

/// The entry of the column whose centre is the ground lattice point `(gx, gy)`.
pub fn column(world: &World, gx: i32, gy: i32) -> u16 {
    let (x, y) = from_iso(gx as f32, gy as f32);
    if !world.is_loaded_at(x, y) {
        return COLUMN_UNKNOWN;
    }
    for z in (0..CHUNK_SIZE_Z).rev() {
        let block = world.get(x, y, z);
        if block.is_air() {
            continue;
        }
        let flag = match block.id() {
            id::WATER => COLUMN_WATER,
            id::GRASS => COLUMN_GRASS,
            id::TREE => COLUMN_TREE,
            _ => 0,
        };
        return (z as u16 + 1).min(HEIGHT_MASK) | flag;
    }
    0
}

/// Fill the rows `rows` of a square map of `size` x `size` columns whose texel `(0, 0)` is the lattice
/// point `origin`: `out[row * size + col]` is [`column`] at `(origin.0 + col, origin.1 + row)`. Meant to be
/// called with a few rows per frame.
pub fn fill_rows(world: &World, origin: (i32, i32), size: usize, rows: Range<usize>, out: &mut [u16]) {
    for row in rows.start..rows.end.min(size) {
        for col in 0..size {
            out[row * size + col] = column(world, origin.0 + col as i32, origin.1 + row as i32);
        }
    }
}

/// The origin of a map of `size` columns that holds `centre` with at least `margin` columns on every
/// side, moving in steps of `step` so the map is rebuilt only now and then.
pub fn map_origin(centre: (f32, f32), size: usize, step: i32) -> (i32, i32) {
    let snap = |v: f32| (v / step as f32).round() as i32 * step - size as i32 / 2;
    (snap(centre.0), snap(centre.1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grass::Wind;
    use crate::grid::to_iso;
    use crate::{AirGenerator, Block};

    #[test]
    fn the_wind_blows_the_way_the_grass_leans() {
        let (x, y) = wind_direction();
        assert!((x * x + y * y - 1.0).abs() < 1e-5);
        // Game space (0.8, 0.6): right and down the screen, which is mostly +gx in the ground frame.
        assert!(x > 0.9 && y < 0.0, "{x} {y}");
        // The same direction the other way round: ground -> game must give the grass's direction back.
        let game = ((x - y) * 100.0, (x + y) * 100.0);
        let n = (game.0 * game.0 + game.1 * game.1).sqrt();
        assert!((game.0 / n - WIND_DIRECTION.0).abs() < 1e-4 && (game.1 / n - WIND_DIRECTION.1).abs() < 1e-4);
    }

    #[test]
    fn the_gust_is_the_factor_the_grass_scales_its_sway_with() {
        // `Wind::at` is the triangle wave times this very gust factor; rebuild the wave to isolate it.
        let mut wind = Wind::default();
        wind.update(3.7);
        for cell in [(0, 0), (5, 6), (-8, 3), (12, -9)] {
            let game = crate::grass::game_xy(cell.0, cell.1);
            let delay = (game.0 * WIND_DIRECTION.0 + game.1 * WIND_DIRECTION.1) / crate::grass::WIND_SPEED;
            let circle = ((wind.time - delay) * 10.0).rem_euclid(crate::grass::WIND_AMPLITUDE);
            let wave = (circle - crate::grass::WIND_AMPLITUDE / 2.0).abs() - crate::grass::WIND_AMPLITUDE / 2.0;
            if wave.abs() < 1.0 {
                continue;
            }
            let ground = to_iso(cell.0, cell.1);
            let ours = gust(wind.time, ground);
            assert!((wind.at(game) / wave - ours).abs() < 1e-3, "{cell:?}: {} vs {ours}", wind.at(game) / wave);
        }
    }

    #[test]
    fn a_gust_stays_between_its_floor_and_one_and_does_not_stand_still() {
        let mut lo = f32::MAX;
        let mut hi = f32::MIN;
        for step in 0..2000 {
            let g = gust(step as f32 * 0.05, (3.0, -2.0));
            lo = lo.min(g);
            hi = hi.max(g);
        }
        assert!(lo >= GUST_MIN - 1e-5 && hi <= 1.0 + 1e-5);
        assert!(lo < GUST_MIN + 0.02 && hi > 0.98, "it reaches both ends: {lo} {hi}");
    }

    #[test]
    fn the_drift_is_the_integral_of_the_gusty_wind() {
        let at = (4.0, 9.0);
        let speed = 1.5;
        for t in [0.0f32, 1.3, 7.9, 30.0] {
            let h = 0.01;
            let slope = (drift(t + h, at, speed) - drift(t - h, at, speed)) / (2.0 * h);
            assert!((slope - speed * gust(t, at)).abs() < 2e-3, "t={t}: {slope} vs {}", speed * gust(t, at));
        }
        assert_eq!(drift(0.0, at, speed), 0.0);
        // Over a long time it advances at the mean speed.
        let mean = GUST_MIN + (1.0 - GUST_MIN) * 0.5;
        let far = drift(700.0, at, speed);
        assert!((far / 700.0 - speed * mean).abs() < 0.01, "{far}");
    }

    #[test]
    fn weather_names_parse_and_unknown_ones_do_not() {
        assert_eq!(Weather::parse("rain"), Some(Weather::Rain));
        assert_eq!(Weather::parse(" SNOW "), Some(Weather::Snow));
        assert_eq!(Weather::parse("off"), Some(Weather::Clear));
        assert_eq!(Weather::parse("hail"), None);
        for w in [Weather::Clear, Weather::Rain, Weather::Snow] {
            assert_eq!(Weather::parse(w.name()), Some(w));
        }
    }

    #[test]
    fn a_drop_falls_then_splashes_then_the_cycle_starts_again() {
        assert!((RAIN_CYCLE - (1.5 + 0.5)).abs() < 1e-5);
        assert_eq!(rain_height(0.0), Some(RAIN_FALL));
        assert!(rain_height(1.0).unwrap() < RAIN_FALL);
        assert_eq!(rain_height(RAIN_FALL / RAIN_SPEED + 0.001), None);
        // The ring starts exactly when the drop lands, and the two never overlap or leave a gap.
        let landing = RAIN_FALL / RAIN_SPEED;
        assert_eq!(splash_age(landing - 0.01), None);
        assert!(splash_age(landing + 0.01).unwrap() < 0.02);
        assert_eq!(splash_age(RAIN_CYCLE + 0.01), None, "the ring is over before the next cycle");
        for i in 0..200 {
            let t = i as f32 * RAIN_CYCLE / 200.0;
            assert!(rain_height(t).is_some() ^ splash_age(t).is_some(), "{t}");
        }
    }

    #[test]
    fn the_cycle_counts_up_and_phases_offset_the_drops() {
        let (n0, t0) = cycle(0.0, 0.0, RAIN_CYCLE);
        assert_eq!((n0, t0), (0, 0.0));
        let (n1, t1) = cycle(RAIN_CYCLE * 2.5, 0.0, RAIN_CYCLE);
        assert_eq!(n1, 2);
        assert!((t1 - RAIN_CYCLE * 0.5).abs() < 1e-4);
        let (_, a) = cycle(1.0, 0.0, RAIN_CYCLE);
        let (_, b) = cycle(1.0, 0.4, RAIN_CYCLE);
        assert_ne!(a, b);
        for i in 0..100 {
            let (_, s) = cycle(i as f32 * 0.37, 0.77, RAIN_CYCLE);
            assert!((0.0..RAIN_CYCLE).contains(&s));
        }
    }

    #[test]
    fn rings_grow_and_fade_and_water_throws_bigger_ones() {
        for water in [false, true] {
            let mut last = (ring_radius(0.0, water), ring_alpha(0.0, water));
            for i in 1..=10 {
                let age = RING_LIFE * i as f32 / 10.0;
                let now = (ring_radius(age, water), ring_alpha(age, water));
                assert!(now.0 > last.0 && now.1 < last.1, "{age}: {now:?} after {last:?}");
                last = now;
            }
            assert_eq!(last.1, 0.0);
        }
        assert!(ring_radius(0.3, true) > ring_radius(0.3, false));
        assert!(ring_alpha(0.0, true) > ring_alpha(0.0, false));
    }

    #[test]
    fn density_buys_instances_up_to_a_hard_cap() {
        for kind in Kind::ALL {
            assert_eq!(kind.count(0.0), 0);
            assert_eq!(kind.count(-3.0), 0);
            assert_eq!(kind.count(f32::NAN), 0);
            assert_eq!(kind.count(MAX_DENSITY), kind.cap());
            assert_eq!(kind.count(1000.0), kind.cap(), "never above the cap");
            assert!(kind.count(1.0) * 2 <= kind.cap() + 1);
            assert!(kind.count(0.5) < kind.count(1.0));
        }
        let all: u32 = Kind::ALL.iter().map(|k| k.cap()).sum();
        assert!(all < 15_000, "the whole show stays a few thousand quads: {all}");
        // The shader knows the kinds by number.
        for (i, kind) in Kind::ALL.iter().enumerate() {
            assert_eq!(*kind as usize, i);
        }
    }

    #[test]
    fn fireflies_come_out_at_dusk_pollen_and_motes_belong_to_the_day() {
        let noon = Strengths::at(0.9, Weather::Clear);
        let dusk = Strengths::at(0.12, Weather::Clear);
        let night = Strengths::at(-0.6, Weather::Clear);
        assert!(night.fireflies > 0.99 && dusk.fireflies > 0.3 && noon.fireflies < 0.01);
        assert!(noon.pollen > 0.99 && night.pollen < 0.01 && dusk.pollen < noon.pollen);
        // Motes and beams are about the low sun: neither at noon nor at night.
        assert!(dusk.motes > 0.3 && noon.motes < 0.01 && night.motes < 0.01);
        assert!(dusk.god_rays > 0.3 && noon.god_rays < 0.01 && night.god_rays < 0.01);
        // Mist and fog thicken towards the morning and the evening and never vanish.
        assert!(night.mist > noon.mist && noon.mist > 0.1);
        assert!(dusk.fog > noon.fog);
    }

    #[test]
    fn weather_takes_the_sun_and_the_flying_things_away() {
        for sun in [0.9, 0.3, 0.12] {
            let clear = Strengths::at(sun, Weather::Clear);
            for weather in [Weather::Rain, Weather::Snow] {
                let wet = Strengths::at(sun, weather);
                assert!(wet.pollen < clear.pollen * 0.2 + 1e-6, "{sun} {weather:?}");
                assert_eq!(wet.god_rays, 0.0);
                assert!(wet.fog >= clear.fog && wet.mist >= clear.mist);
                assert_eq!(wet.fireflies, 0.0);
            }
        }
        for sun in [-1.0f32, -0.2, 0.0, 0.2, 0.5, 1.0] {
            for weather in [Weather::Clear, Weather::Rain, Weather::Snow] {
                let s = Strengths::at(sun, weather);
                for v in [s.pollen, s.fireflies, s.motes, s.leaves, s.mist, s.fog, s.god_rays] {
                    assert!((0.0..=1.0).contains(&v), "{sun} {weather:?}: {s:?}");
                }
            }
        }
    }

    fn world_with(blocks: &[((i32, i32, i32), Block)]) -> World {
        let mut world = World::new(AirGenerator);
        for &((x, y, z), block) in blocks {
            world.set(x, y, z, block);
        }
        world
    }

    #[test]
    fn a_column_remembers_the_height_and_kind_of_its_top_block() {
        let world = world_with(&[
            ((3, 4, 0), Block::new(id::STONE, 0)),
            ((3, 4, 1), Block::new(id::STONE, 0)),
            ((3, 4, 2), Block::new(id::GRASS, 0)),
            ((5, 4, 0), Block::new(id::WATER, 0)),
            ((6, 4, 5), Block::new(id::TREE, 0)),
        ]);
        let at = |x: i32, y: i32| {
            let (gx, gy) = to_iso(x, y);
            column(&world, gx as i32, gy as i32)
        };
        assert_eq!(at(3, 4), 3 | COLUMN_GRASS, "the top of the grass block is at 3");
        assert_eq!(at(5, 4), 1 | COLUMN_WATER);
        assert_eq!(at(6, 4), 6 | COLUMN_TREE);
        assert_eq!(at(7, 4), 0, "loaded but empty");
        let (gx, gy) = to_iso(50_000, 50_000);
        assert_eq!(column(&World::remote(), gx as i32, gy as i32), COLUMN_UNKNOWN, "nothing is known of a chunk that has not arrived");
    }

    #[test]
    fn every_lattice_point_is_one_column_and_rows_fill_a_map() {
        let world = world_with(&[((0, 0, 0), Block::new(id::GRASS, 0))]);
        let size = 8;
        let origin = (-4, -4);
        let mut map = vec![0xffffu16; size * size];
        fill_rows(&world, origin, size, 0..3, &mut map);
        assert!(map[..3 * size].iter().all(|&c| c != 0xffff));
        assert!(map[3 * size..].iter().all(|&c| c == 0xffff), "only the asked rows are written");
        fill_rows(&world, origin, size, 3..size + 5, &mut map);
        assert!(map.iter().all(|&c| c != 0xffff), "rows beyond the map are ignored");
        // The lattice point of block (0, 0) holds its grass.
        let (gx, gy) = to_iso(0, 0);
        let texel = (gy as i32 - origin.1) as usize * size + (gx as i32 - origin.0) as usize;
        assert_eq!(map[texel], 1 | COLUMN_GRASS);
        for gx in -3..4 {
            for gy in -3..4 {
                let (x, y) = from_iso(gx as f32, gy as f32);
                assert_eq!(to_iso(x, y), (gx as f32, gy as f32), "the lattice and the columns are one to one");
            }
        }
    }

    #[test]
    fn the_map_follows_the_viewer_in_steps_and_always_holds_it() {
        let size = 96;
        let step = 16;
        let mut last = map_origin((0.0, 0.0), size, step);
        let mut moves = 0;
        for i in 0..400 {
            let c = (i as f32 * 0.5, -(i as f32) * 0.25);
            let o = map_origin(c, size, step);
            assert!(c.0 >= o.0 as f32 + 40.0 && c.0 <= o.0 as f32 + size as f32 - 40.0, "{c:?} {o:?}");
            assert!(c.1 >= o.1 as f32 + 40.0 && c.1 <= o.1 as f32 + size as f32 - 40.0, "{c:?} {o:?}");
            if o != last {
                moves += 1;
                last = o;
            }
        }
        assert!(moves > 0 && moves < 20, "rebuilt only now and then: {moves}");
    }
}
