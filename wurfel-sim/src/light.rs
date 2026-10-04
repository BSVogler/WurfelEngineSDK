//! The light engine, ported from the Java `LightEngine`, `GlobalLightSource`, `Moon`,
//! `PointLightSource` and `AmbientOcclusionCalculator`, in a form that works on **vertices** of a
//! flat-coloured mesh: no sprites, normal maps or textures are needed.
//!
//! # What is here
//!
//! * [`GlobalLightSource`]: the sun and the moon. Azimuth and height move with the time of day, the
//!   power follows the Java curve (dark at night, 0.5 at the horizon, `0.5 + brightness` high up).
//! * [`LightEngine`]: the day-night cycle (`update`, `time_of_day`, `set_to_noon`...) and the
//!   per-face light of the three visible sides (left, top, right) plus the top-only specular term,
//!   exactly the Java `I_diff0/1/2` and `I_spec1`.
//! * [`PointLight`]: torches and the like, with the Java falloff `(1 + brightness) / distance²`
//!   and the per-side Lambert term, cut off at the radius, and [`visible`] for the occlusion
//!   the Java engine got from its ray casts.
//! * Ambient occlusion per face vertex ([`face_vertex_ao`]): the classic voxel AO (two sides and a
//!   corner) on the isometric lattice, replacing the Java neighbour flags that selected AO sprites.
//! * [`shade_vertex`]: puts it together into the final colour of one vertex. The web shader
//!   (`wurfel-web/src/shader.wgsl`) evaluates the same formula for everything that changes every
//!   frame; the static part (AO and baked point lights) is computed here while meshing.
//!
//! # Frames
//!
//! Everything is in the isometric ground frame of [`crate::grid`]: `x = gx`, `y = gy`, `z` up.
//! The Java engine used screen-aligned axes (x right, y towards the viewer); [`GlobalLightSource`]
//! keeps its Java angles and converts directions with [`crate::entity::screen_to_iso`]. The left
//! face's normal is `+y`, the right face's `+x`, the top's `+z`.
//!
//! # Differences from Java
//!
//! * The Java engine added the moon's diffuse and specular terms and then **overwrote** them with the
//!   sun's (`I_diff0 = ...` after `I_diff0 += ...`), so the moon never lit anything and nights were
//!   black. Here sun and moon each light the faces and the results are added.
//! * Java's power curve is `0.5 + brightness * sin(height * pi / amplitude)` between the night and
//!   the day branch, which is `-0.5` just above the night limit with the default brightness. The
//!   value is clamped at 0.
//! * The Java non-pixel path never added the ambient colour (only the shader did). It is included.
//! * Point lights are occluded by sampling the line to the light (see [`visible`]) instead of one
//!   ray cast per cell of a 3D cache, and static ones are evaluated per vertex.

use glam::{Vec2, Vec3};

use crate::block::id;
use crate::cvar::CVarSystem;
use crate::entity::screen_to_iso;
use crate::grid::{from_iso, to_iso};
use crate::{Block, World, CHUNK_SIZE_Z};

/// Diffuse reflection factor (`k_diff` in Java: 100/255).
pub const K_DIFF: f32 = 100.0 / 255.0;
/// Specular reflection factor (`k_specular`: `1 - k_diff`).
pub const K_SPEC: f32 = 1.0 - K_DIFF;
/// Specular exponent (`n_spec`).
pub const N_SPEC: i32 = 12;
/// CVar `worldSpinAngle` default.
pub const DEFAULT_WORLD_SPIN_ANGLE: i32 = -40;
/// CVar `LEazimutSpeed` default, in degrees per millisecond.
pub const DEFAULT_AZIMUTH_SPEED: f32 = 0.000_781_25;
/// Java `Moon.getAzimuthSpeed`: the moon moves at 85 % of the sun's speed.
const MOON_SPEED_FACTOR: f32 = 0.85;

/// The three sides of a block the camera sees.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Face {
    /// Towards the lower left of the screen (normal `+y`).
    Left,
    Top,
    /// Towards the lower right of the screen (normal `+x`).
    Right,
}

impl Face {
    pub const ALL: [Face; 3] = [Face::Left, Face::Top, Face::Right];

    /// 0 left, 1 top, 2 right (the Java `Side` codes).
    pub fn index(self) -> usize {
        match self {
            Face::Left => 0,
            Face::Top => 1,
            Face::Right => 2,
        }
    }

    pub fn from_index(index: usize) -> Option<Face> {
        Face::ALL.get(index).copied()
    }

    /// Outward unit normal in the isometric ground frame.
    pub fn normal(self) -> Vec3 {
        match self {
            Face::Left => Vec3::Y,
            Face::Top => Vec3::Z,
            Face::Right => Vec3::X,
        }
    }
}

fn clamp01(v: Vec3) -> Vec3 {
    v.clamp(Vec3::ZERO, Vec3::ONE)
}

/// Replace NaN and infinities by 0 so one bad input cannot spread through the lighting.
fn finite_or_zero(v: Vec3) -> Vec3 {
    if v.is_finite() { v } else { Vec3::ZERO }
}

// ------------------------------------------------------------------------------------- sun and moon

/// A light that is infinitely far away: the sun or the moon (Java `GlobalLightSource`).
#[derive(Debug, Clone)]
pub struct GlobalLightSource {
    power: f32,
    tone: Vec3,
    ambient: Vec3,
    /// Degrees above the horizon.
    height: f32,
    /// Degrees, 0..360.
    azimuth: f32,
    /// The highest the light climbs, in degrees.
    amplitude: f32,
    fixed_position: bool,
    brightness_factor: f32,
    speed_factor: f32,
}

/// Aliases for the two instances the engine uses.
pub type Sun = GlobalLightSource;
pub type Moon = GlobalLightSource;

/// The Java brightness curve, with the negative dip just above the night limit clamped away.
fn power_for(height: f32, amplitude: f32, brightness_factor: f32) -> f32 {
    if height < -amplitude / 2.0 {
        0.0 // night
    } else if height < amplitude / 2.0 {
        (0.5 + brightness_factor * (height * std::f32::consts::PI / amplitude).sin()).max(0.0) // morning and evening
    } else {
        0.5 + brightness_factor
    }
}

impl GlobalLightSource {
    pub fn new(azimuth: f32, height: f32, tone: Vec3, ambient: Vec3, brightness_factor: f32, amplitude: f32) -> Self {
        let mut source = GlobalLightSource {
            power: 0.0,
            tone,
            ambient,
            height,
            azimuth: 0.0,
            amplitude,
            fixed_position: false,
            brightness_factor,
            speed_factor: 1.0,
        };
        source.set_azimuth(azimuth);
        source.power = power_for(source.height, amplitude, brightness_factor);
        source
    }

    /// The Java `Moon`: the same, moving at 85 % of the speed.
    pub fn moon(azimuth: f32, height: f32, tone: Vec3, ambient: Vec3, brightness_factor: f32, amplitude: f32) -> Self {
        let mut moon = Self::new(azimuth, height, tone, ambient, brightness_factor, amplitude);
        moon.speed_factor = MOON_SPEED_FACTOR;
        moon
    }

    pub fn power(&self) -> f32 {
        self.power
    }

    pub fn tone(&self) -> Vec3 {
        self.tone
    }

    pub fn height(&self) -> f32 {
        self.height
    }

    pub fn azimuth(&self) -> f32 {
        self.azimuth
    }

    pub fn amplitude(&self) -> f32 {
        self.amplitude
    }

    pub fn set_azimuth(&mut self, azimuth: f32) {
        self.azimuth = if azimuth.is_finite() { azimuth.rem_euclid(360.0) } else { 0.0 };
    }

    /// Set the height in degrees. Wrapped into -180..180 so that a light below the horizon can be
    /// set explicitly (the Java setter wrapped into 0..360, which turned -50 into 310 and made a
    /// light below the horizon count as being high up).
    pub fn set_height(&mut self, height: f32) {
        self.height = if height.is_finite() { (height + 180.0).rem_euclid(360.0) - 180.0 } else { 0.0 };
        self.power = power_for(self.height, self.amplitude, self.brightness_factor);
    }

    pub fn set_tone(&mut self, tone: Vec3) {
        self.tone = tone;
    }

    /// A light with a fixed position does not follow the day cycle.
    pub fn set_fixed_position(&mut self, fixed: bool) {
        self.fixed_position = fixed;
    }

    /// Advance by `dt_ms` milliseconds. `azimuth_speed` is the CVar `LEazimutSpeed` (degrees per
    /// millisecond) and `world_spin_angle` the CVar `worldSpinAngle`.
    pub fn update(&mut self, dt_ms: f32, azimuth_speed: f32, world_spin_angle: i32) {
        let speed = azimuth_speed * self.speed_factor;
        if !self.fixed_position && speed != 0.0 {
            self.set_azimuth(self.azimuth + speed * dt_ms);
            self.height = self.amplitude * ((self.azimuth + world_spin_angle as f32).to_radians()).sin();
        }
        self.power = power_for(self.height, self.amplitude, self.brightness_factor);
    }

    /// Colour and intensity of the light, clamped to 0..1 like the libGDX `Color` maths.
    pub fn light(&self) -> Vec3 {
        clamp01(self.tone * self.power)
    }

    pub fn ambient(&self) -> Vec3 {
        clamp01(self.ambient * self.power)
    }

    /// Java `getNormal`: `(-cos az, sin az, sin h)` normalised, in the Java screen-aligned frame.
    /// (The Java code does not scale the first two by `cos h`; this is what its shaders use.)
    pub fn normal_game(&self) -> Vec3 {
        let (az, h) = (self.azimuth.to_radians(), self.height.to_radians());
        Vec3::new(-az.cos(), az.sin(), h.sin()).normalize_or_zero()
    }

    /// The unit vector towards the light that the Java per-side formulas
    /// (`cos h * cos(az - 45)` and friends) are the Lambert terms of, in the Java frame.
    pub fn direction_game(&self) -> Vec3 {
        let (az, h) = (self.azimuth.to_radians(), self.height.to_radians());
        Vec3::new(-h.cos() * az.cos(), h.cos() * az.sin(), h.sin())
    }

    /// [`direction_game`](Self::direction_game) in the isometric ground frame.
    pub fn direction(&self) -> Vec3 {
        to_iso_frame(self.direction_game())
    }

    /// [`normal_game`](Self::normal_game) in the isometric ground frame.
    pub fn normal(&self) -> Vec3 {
        to_iso_frame(self.normal_game())
    }

    /// Lambert term for a face, the Java `I_diff0/1/2` before the power and `k_diff` factors.
    pub fn lambert(&self, face: Face) -> f32 {
        let (az, h) = (self.azimuth.to_radians(), self.height.to_radians());
        let value = match face {
            Face::Left => h.cos() * (az - 45f32.to_radians()).cos(),
            Face::Top => (h - 90f32.to_radians()).cos(),
            Face::Right => h.cos() * (az - 135f32.to_radians()).cos(),
        };
        value.max(0.0)
    }

    /// The Java `I_spec1`: the specular highlight, which only the top face can show.
    pub fn specular_top(&self) -> f32 {
        let (az, h) = (self.azimuth.to_radians(), self.height.to_radians());
        let base = (h.sin() * az.sin() / 2f32.sqrt()) + ((h - 90f32.to_radians()).sin() / 2f32.sqrt());
        let value = self.power * K_SPEC * base.powi(N_SPEC) * (N_SPEC as f32 + 2.0) / (2.0 * std::f32::consts::PI);
        if value.is_finite() { value.max(0.0) } else { 0.0 }
    }
}

fn to_iso_frame(game: Vec3) -> Vec3 {
    let xy = screen_to_iso(Vec2::new(game.x, game.y));
    Vec3::new(xy.x, xy.y, game.z)
}

// ------------------------------------------------------------------------------------- the engine

/// Everything the shader needs about the global light for one frame. Plain data.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LightState {
    /// Ambient colour (sun's plus moon's, clamped).
    pub ambient: Vec3,
    pub sun_color: Vec3,
    pub moon_color: Vec3,
    /// Diffuse intensity per face (left, top, right), `power * k_diff * lambert`.
    pub sun_faces: [f32; 3],
    pub moon_faces: [f32; 3],
    /// Specular intensity of the top face.
    pub sun_spec: f32,
    pub moon_spec: f32,
    /// Unit directions towards the light, isometric frame.
    pub sun_direction: Vec3,
    pub moon_direction: Vec3,
    /// 0 by day, 1 at night: how far the night colour grading is applied.
    pub night_mix: f32,
    /// `[0, 1)`: 0 sunrise, 0.25 noon, 0.5 sunset, 0.75 midnight (with the default spin angle).
    pub time_of_day: f32,
    /// Blue channel of the moon light, which raises the night contrast like in the Java shader.
    pub moon_blue: f32,
}

#[derive(Debug, Clone)]
pub struct LightEngine {
    sun: GlobalLightSource,
    moon: Option<GlobalLightSource>,
    world_spin_angle: i32,
    azimuth_speed: f32,
}

impl LightEngine {
    /// The Java constructor with its sun and moon. `world_spin_angle` and `azimuth_speed` are the
    /// CVars of the same name.
    pub fn new(world_spin_angle: i32, azimuth_speed: f32) -> Self {
        let spin = world_spin_angle as f32;
        LightEngine {
            sun: GlobalLightSource::new(-spin, 0.0, Vec3::new(1.0, 1.0, 1.0), Vec3::new(0.5, 0.5, 0.4), 1.0, 60.0),
            moon: Some(GlobalLightSource::moon(
                180.0 - spin,
                0.0,
                Vec3::new(0.4, 0.9, 0.9),
                Vec3::new(0.0, 0.0, 0.1),
                1.0,
                45.0,
            )),
            world_spin_angle,
            azimuth_speed,
        }
    }

    /// Read the engine CVars (`worldSpinAngle`, `LEazimutSpeed`) from the root system and restore the
    /// positions saved with the map (`LEsunAzimuth`, `LEmoonAzimuth`) from the save-slot system.
    /// Anything missing falls back to the defaults.
    pub fn from_cvars(root: &CVarSystem, save: &CVarSystem) -> Self {
        let spin = root.get_i32("worldSpinAngle").unwrap_or(DEFAULT_WORLD_SPIN_ANGLE);
        let speed = root.get_f32("LEazimutSpeed").unwrap_or(DEFAULT_AZIMUTH_SPEED);
        let mut engine = LightEngine::new(spin, speed);
        if let Ok(azimuth) = save.get_f32("LEsunAzimuth") {
            engine.sun.set_azimuth(azimuth);
        }
        if let (Ok(azimuth), Some(moon)) = (save.get_f32("LEmoonAzimuth"), engine.moon.as_mut()) {
            moon.set_azimuth(azimuth);
        }
        engine.update(0.0);
        engine
    }

    pub fn sun(&self) -> &GlobalLightSource {
        &self.sun
    }

    pub fn moon(&self) -> Option<&GlobalLightSource> {
        self.moon.as_ref()
    }

    pub fn sun_mut(&mut self) -> &mut GlobalLightSource {
        &mut self.sun
    }

    pub fn world_spin_angle(&self) -> i32 {
        self.world_spin_angle
    }

    /// Advance the day by `dt_ms` milliseconds (`LightEngine.update`).
    pub fn update(&mut self, dt_ms: f32) {
        self.sun.update(dt_ms, self.azimuth_speed, self.world_spin_angle);
        let time_of_day = self.time_of_day();
        if let Some(moon) = self.moon.as_mut() {
            // The moon rises as the sun sets, and is pulled back to its rising point if it drifted.
            if time_of_day > 0.25
                && time_of_day < 0.3
                && ((moon.azimuth() - 210.0 - self.world_spin_angle as f32) % 360.0).abs() > 10.0
            {
                moon.set_azimuth(210.0 + self.world_spin_angle as f32);
            }
            moon.update(dt_ms, self.azimuth_speed, self.world_spin_angle);
        }
    }

    /// 0 sunrise, 0.25 noon, 0.5 sunset, 0.75 midnight (for the default spin angle).
    pub fn time_of_day(&self) -> f32 {
        (self.sun.azimuth() + self.world_spin_angle as f32).rem_euclid(360.0) / 360.0
    }

    pub fn set_to_noon(&mut self) {
        self.sun.set_azimuth(90.0);
        if let Some(moon) = self.moon.as_mut() {
            moon.set_azimuth(270.0);
        }
        self.refresh();
    }

    pub fn set_to_night(&mut self) {
        self.sun.set_azimuth(270.0);
        if let Some(moon) = self.moon.as_mut() {
            moon.set_azimuth(90.0);
        }
        self.refresh();
    }

    /// Recompute heights and powers for the current azimuths without moving time.
    fn refresh(&mut self) {
        self.update(0.0);
    }

    /// Ambient colour: sun's plus moon's, clamped.
    pub fn ambient(&self) -> Vec3 {
        clamp01(self.sun.ambient() + self.moon.as_ref().map_or(Vec3::ZERO, |m| m.ambient()))
    }

    /// Diffuse intensity of one source on one face.
    fn diffuse(source: &GlobalLightSource, face: Face) -> f32 {
        source.power() * K_DIFF * source.lambert(face)
    }

    /// Light of the sun and the moon on one face: `colour * (diffuse [+ specular on the top])`.
    pub fn face_light(&self, face: Face) -> Vec3 {
        let mut total = Vec3::ZERO;
        for source in std::iter::once(&self.sun).chain(self.moon.as_ref()) {
            let mut intensity = Self::diffuse(source, face);
            if face == Face::Top {
                intensity += source.specular_top();
            }
            total += source.light() * intensity;
        }
        finite_or_zero(total)
    }

    pub fn state(&self) -> LightState {
        let moon = self.moon.as_ref();
        let faces = |source: Option<&GlobalLightSource>| {
            source.map_or([0.0; 3], |s| [Face::Left, Face::Top, Face::Right].map(|f| Self::diffuse(s, f)))
        };
        LightState {
            ambient: self.ambient(),
            sun_color: self.sun.light(),
            moon_color: moon.map_or(Vec3::ZERO, |m| m.light()),
            sun_faces: faces(Some(&self.sun)),
            moon_faces: faces(moon),
            sun_spec: self.sun.specular_top(),
            moon_spec: moon.map_or(0.0, |m| m.specular_top()),
            sun_direction: self.sun.direction(),
            moon_direction: moon.map_or(Vec3::ZERO, |m| m.direction()),
            // The Java shader: clamp(u_sunNormal.z / -0.2, 0, 1).
            night_mix: (self.sun.normal_game().z / -0.2).clamp(0.0, 1.0),
            time_of_day: self.time_of_day(),
            moon_blue: moon.map_or(0.0, |m| m.light().z),
        }
    }
}

// ------------------------------------------------------------------------------------ point lights

/// Per-side constants of the Java `PointLightSource`: `0.15 + k * 0.005` with k 0.1, 0.2, 0.25.
const POINT_SIDE_FACTORS: [f32; 3] = [0.15 + 0.1 * 0.005, 0.15 + 0.2 * 0.005, 0.15 + 0.25 * 0.005];
/// Nearer than this (in blocks) a point light stops getting brighter, so the `1 / d²` falloff stays finite.
const MIN_DISTANCE: f32 = 0.5;

/// A torch, lamp or other local light (Java `PointLightSource`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PointLight {
    /// Isometric ground frame, in blocks.
    pub position: Vec3,
    pub color: Vec3,
    /// Nothing is lit farther away than this, in blocks.
    pub radius: f32,
    /// The Java `brightness`: the light is `(1 + brightness) / distance²`.
    pub brightness: f32,
}

impl PointLight {
    pub fn new(position: Vec3, color: Vec3, radius: f32, brightness: f32) -> Self {
        PointLight { position, color, radius, brightness }
    }

    /// Light arriving at `point` on a face, as a factor before the colour. Zero at and beyond the
    /// radius, for faces turned away from the light, and for non-finite input.
    pub fn intensity_at(&self, point: Vec3, face: Face) -> f32 {
        let to_light = self.position - point;
        let distance = to_light.length();
        if !distance.is_finite() || !self.radius.is_finite() || distance >= self.radius {
            return 0.0;
        }
        let falloff = (1.0 + self.brightness) / distance.max(MIN_DISTANCE).powi(2);
        let lambert = if distance < 1e-4 { 1.0 } else { (to_light / distance).dot(face.normal()) };
        if lambert <= 0.0 {
            return 0.0;
        }
        let value = falloff * lambert * POINT_SIDE_FACTORS[face.index()];
        if value.is_finite() { value.max(0.0) } else { 0.0 }
    }

    /// Coloured light arriving at `point` on a face.
    pub fn contribution(&self, point: Vec3, face: Face) -> Vec3 {
        finite_or_zero(self.color * self.intensity_at(point, face))
    }
}

/// Does light pass between two points? Blocks that are air, water or the invisible obstacle (the
/// Java `isTransparent`) let it through; anything else stops it. The segment is sampled every
/// quarter block, which is enough because blocks are a block wide.
pub fn visible(world: &World, from: Vec3, to: Vec3) -> bool {
    visible_with(&|x, y, z| is_opaque(world.get(x, y, z)), from, to)
}

/// [`visible`] for any source of block data. `opaque(x, y, z)` answers for block coordinates (the
/// staggered grid, see [`crate::grid`]); heights outside the world are never asked.
pub fn visible_with(opaque: &dyn Fn(i32, i32, i32) -> bool, from: Vec3, to: Vec3) -> bool {
    let delta = to - from;
    let length = delta.length();
    if !length.is_finite() || length < 1e-4 {
        return true;
    }
    let steps = (length / 0.25).ceil() as usize;
    // Skip the very ends: they lie on the surfaces of the blocks being lit and of the lamp.
    (1..steps).all(|i| {
        let p = from + delta * (i as f32 / steps as f32);
        if p.z < 0.0 || p.z >= CHUNK_SIZE_Z as f32 {
            return true;
        }
        let (x, y) = from_iso(p.x, p.y);
        !opaque(x, y, p.z.floor() as i32)
    })
}

/// Blocks that cast shadows and occlude light: not air, water or the invisible obstacle (id 4).
pub fn is_opaque(block: Block) -> bool {
    !matches!(block.id(), id::AIR | id::WATER | 4)
}

/// Total light from `lights` at a vertex on `face`, with the occlusion test. `point` is the vertex
/// position.
pub fn bake_point_lights(world: &World, lights: &[PointLight], point: Vec3, face: Face) -> Vec3 {
    bake_point_lights_with(&|x, y, z| is_opaque(world.get(x, y, z)), lights, point, face)
}

/// [`bake_point_lights`] for any source of block data, see [`visible_with`].
pub fn bake_point_lights_with(
    opaque: &dyn Fn(i32, i32, i32) -> bool,
    lights: &[PointLight],
    point: Vec3,
    face: Face,
) -> Vec3 {
    // Nudge off the surface so the face does not shadow itself.
    let origin = point + face.normal() * 0.05;
    let mut total = Vec3::ZERO;
    for light in lights {
        if light.intensity_at(point, face) > 0.0 && visible_with(opaque, origin, light.position) {
            total += light.contribution(point, face);
        }
    }
    total
}

// ------------------------------------------------------------------------------ ambient occlusion

/// The lattice coordinates of a block column: block centres sit on integer `(gx, gy)` points.
fn lattice((x, y): (i32, i32)) -> (i32, i32) {
    let (gx, gy) = to_iso(x, y);
    (gx as i32, gy as i32)
}

fn opaque_lattice(opaque: &dyn Fn(i32, i32, i32) -> bool, ix: i32, iy: i32, z: i32) -> bool {
    if !(0..CHUNK_SIZE_Z).contains(&z) {
        return false;
    }
    let (x, y) = from_iso(ix as f32, iy as f32);
    opaque(x, y, z)
}

/// Ambient occlusion at one corner of a face of the block at `(x, y, z)`, 0 (open) to 3 (a
/// crevice), with the classic voxel rule: two solid sides close the corner completely, otherwise
/// the sides and the corner cell add up.
///
/// `du` and `dv` (each -1 or 1) pick the corner of the face as the sign of the vertex offset from
/// the face centre along its two axes: for the top face `du` is along `x` and `dv` along `y`; for
/// the left face `du` is along `x` and `dv` along `z`; for the right face `du` is along `y` and
/// `dv` along `z`.
pub fn face_vertex_ao(world: &World, cell: (i32, i32, i32), face: Face, du: i32, dv: i32) -> u8 {
    face_vertex_ao_with(&|x, y, z| is_opaque(world.get(x, y, z)), cell, face, du, dv)
}

/// [`face_vertex_ao`] for any source of block data (the render storage uses its own cell cache).
/// `opaque(x, y, z)` answers for block coordinates; heights outside the world are never asked.
pub fn face_vertex_ao_with(
    opaque: &dyn Fn(i32, i32, i32) -> bool,
    (x, y, z): (i32, i32, i32),
    face: Face,
    du: i32,
    dv: i32,
) -> u8 {
    let (ix, iy) = lattice((x, y));
    let o = |dx: i32, dy: i32, dz: i32| opaque_lattice(opaque, ix + dx, iy + dy, z + dz);
    let (side1, side2, corner) = match face {
        Face::Top => (o(du, 0, 1), o(0, dv, 1), o(du, dv, 1)),
        Face::Left => (o(du, 1, 0), o(0, 1, dv), o(du, 1, dv)),
        Face::Right => (o(1, du, 0), o(1, 0, dv), o(1, du, dv)),
    };
    if side1 && side2 { 3 } else { side1 as u8 + side2 as u8 + corner as u8 }
}

// ----------------------------------------------------------------------------------------- shading

/// Tuning of the final colour: how the Java light values map to display brightness. The Java
/// sprites used a tint where 0.5 means "unchanged"; for flat colours the defaults are chosen so
/// that, at the default noon, the top of a block shows about its own colour (the Java numbers there
/// are 0.42 diffuse on top and 0.29 on the sides, plus ambient 0.75 * 0.35, times 1.45).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Shading {
    /// Multiplies the sum of ambient and global light.
    pub exposure: f32,
    /// How much of the ambient colour is added before exposure.
    pub ambient_weight: f32,
    /// CVar `ambientOcclusion`: how dark a full occlusion makes a vertex (0 off, 1 black).
    pub ao_strength: f32,
    /// Scales the (small) Java point-light values into the visible range.
    pub point_gain: f32,
    /// The least light a surface gets from the global lights. The Java shader computes
    /// `albedo * max(light * 3.5, 1.0)`, so even a moonless night shows the colours dimly instead of
    /// going black; this is the same floor.
    pub min_light: f32,
    /// Night colour grading (desaturation and contrast) like the Java shader.
    pub night_grading: bool,
}

impl Default for Shading {
    fn default() -> Self {
        Shading { exposure: 1.45, ambient_weight: 0.35, ao_strength: 0.5, point_gain: 25.0, min_light: 0.15, night_grading: true }
    }
}

const LUMA: Vec3 = Vec3::new(0.222, 0.707, 0.071);

/// Final colour of one vertex: the CPU reference for what the web shader does per frame.
///
/// * `albedo`: the block's flat colour.
/// * `ao_level`: occlusion 0..1 (the AO count divided by 3).
/// * `point`: light from point lights, already including their colour.
pub fn shade_vertex(state: &LightState, shading: &Shading, albedo: Vec3, face: Face, ao_level: f32, point: Vec3) -> Vec3 {
    let i = face.index();
    let mut sun = state.sun_faces[i];
    let mut moon = state.moon_faces[i];
    if face == Face::Top {
        sun += state.sun_spec;
        moon += state.moon_spec;
    }
    let lit = ((state.ambient * shading.ambient_weight + state.sun_color * sun + state.moon_color * moon) * shading.exposure)
        .max(Vec3::splat(shading.min_light));
    let ao = 1.0 - shading.ao_strength * ao_level.clamp(0.0, 1.0);
    let color = albedo * ((lit + point * shading.point_gain) * ao);

    let graded = if shading.night_grading && state.night_mix > 0.0 {
        let grey = color.dot(LUMA);
        let desaturated = color - 0.6 * (color - Vec3::splat(grey));
        let contrast = (1.0 + 0.4 * state.moon_blue).max(0.0);
        let night = (desaturated - Vec3::splat(0.5)) * contrast + Vec3::splat(0.5);
        color.lerp(night, state.night_mix)
    } else {
        color
    };
    finite_or_zero(graded).max(Vec3::ZERO)
}

#[cfg(test)]
mod tests;
