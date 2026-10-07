//! The lighting uniform and the controller that drives it.
//!
//! `wurfel_sim::light` is the Java light engine. The vertex shader (`shader.wgsl`) applies it to
//! every vertex each frame, and this module is the glue: a [`Lighting`] uniform that mirrors the
//! WGSL struct field by field, and a [`LightingController`] that owns the engine, the day cycle,
//! the settings (lighting on or off, ambient occlusion) and the dynamic point lights.
//!
//! Static lighting (ambient occlusion, baked point lights) lives in the vertices and is computed
//! while meshing (`mesh.rs`), so the controller only has to produce one small uniform per frame.

// web.rs wires this up (see the integration notes in the report); until then nothing but the tests uses it.

use bytemuck::{Pod, Zeroable};
use glam::Vec3;
use wurfel_sim::light::{LightEngine, LightState, PointLight, Shading, DEFAULT_AZIMUTH_SPEED, DEFAULT_WORLD_SPIN_ANGLE};

use crate::clouds;
use crate::mesh::FLAT_SHADES;

/// How many moving point lights the shader handles at once.
pub const MAX_POINT_LIGHTS: usize = 8;

/// The CVar `ambientOcclusion` default: how dark a fully occluded vertex gets.
pub const DEFAULT_AO_STRENGTH: f32 = 0.5;

/// Mirror of the `Lighting` struct in `shader.wgsl`. Everything is a `vec4`, so there is no
/// padding to get wrong: WGSL aligns a `vec3` to 16 bytes, which would have to be matched by hand.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub struct Lighting {
    /// rgb: ambient colour, already multiplied by its weight. w: 1 when lit, 0 for the flat look.
    pub ambient: [f32; 4],
    /// rgb: sun light colour. w: specular intensity on top faces.
    pub sun_color: [f32; 4],
    /// rgb: moon light colour. w: specular intensity on top faces.
    pub moon_color: [f32; 4],
    /// x left, y top, z right: sun diffuse intensity. w: night mix (0 day, 1 night).
    pub sun_faces: [f32; 4],
    /// x left, y top, z right: moon diffuse intensity. w: blue channel of the moon light.
    pub moon_faces: [f32; 4],
    /// x exposure, y ambient occlusion strength, z point light gain, w night grading (1 or 0).
    pub grading: [f32; 4],
    /// x left, y top, z right: brightness of the flat look. w: 1 when the scene is drawn in linear light.
    pub flat_shades: [f32; 4],
    /// x: number of dynamic point lights. y: time of day. z: minimum light. w: 1 when the sprites are
    /// lit per pixel with their normal map (`fragment_NM.fs`), 0 for the vertex lighting alone.
    pub misc: [f32; 4],
    /// rgb: fog colour. w: 1 when fog is on.
    pub fog: [f32; 4],
    /// x: sun diffuse intensity on the -y face, y: on the -x face (the free camera's sides).
    pub sun_back: [f32; 4],
    pub moon_back: [f32; 4],
    /// xyz: the Java `u_sunNormal` / `u_moonNormal` (the Java screen-aligned frame), for the normal maps.
    pub sun_normal: [f32; 4],
    pub moon_normal: [f32; 4],
    /// xyz: the Java `u_ambientColor`, unweighted (`ambient` above is scaled for the vertex path).
    pub pixel_ambient: [f32; 4],
    /// xyz: the Java `u_localLightPos` and `u_playerpos` (the focus entity, in blocks), w: 1 when there is one.
    pub local_light: [f32; 4],
    /// xyz: the unit vector towards the sun in the world's ground frame (where the cloud shadows come from).
    pub sun_dir: [f32; 4],
    /// x: seconds the clouds have drifted, y: shadow strength (0 = no clouds), z: blocks per texture
    /// repeat, w: height of the cloud layer in blocks.
    pub clouds: [f32; 4],
    /// xyz: position in blocks, w: radius.
    pub lights: [[f32; 4]; MAX_POINT_LIGHTS],
    /// rgb: colour, w: brightness.
    pub light_colors: [[f32; 4]; MAX_POINT_LIGHTS],
}

impl Lighting {
    /// The uniform for one frame.
    ///
    /// `lit` false gives the old flat look (a fixed brightness per face, no light engine). At most
    /// [`MAX_POINT_LIGHTS`] lights are used, the first ones; see
    /// [`LightingController::set_dynamic_lights`] for choosing the nearest.
    pub fn new(state: &LightState, shading: &Shading, lit: bool, lights: &[PointLight]) -> Self {
        let rgb = |v: Vec3, w: f32| [v.x, v.y, v.z, w];
        let ambient = state.ambient * shading.ambient_weight;
        let mut uniform = Lighting {
            ambient: rgb(ambient, if lit { 1.0 } else { 0.0 }),
            sun_color: rgb(state.sun_color, state.sun_spec),
            moon_color: rgb(state.moon_color, state.moon_spec),
            sun_faces: [state.sun_faces[0], state.sun_faces[1], state.sun_faces[2], state.night_mix],
            moon_faces: [state.moon_faces[0], state.moon_faces[1], state.moon_faces[2], state.moon_blue],
            grading: [
                shading.exposure,
                shading.ao_strength,
                shading.point_gain,
                if shading.night_grading { 1.0 } else { 0.0 },
            ],
            flat_shades: [FLAT_SHADES[0], FLAT_SHADES[1], FLAT_SHADES[2], 0.0],
            misc: [0.0, state.time_of_day, shading.min_light, 0.0],
            fog: rgb(shading.fog_color, if shading.fog && lit { 1.0 } else { 0.0 }),
            sun_back: [state.sun_back[0], state.sun_back[1], 0.0, 0.0],
            moon_back: [state.moon_back[0], state.moon_back[1], 0.0, 0.0],
            sun_normal: rgb(state.sun_normal_game, 0.0),
            moon_normal: rgb(state.moon_normal_game, 0.0),
            pixel_ambient: rgb(state.ambient, 0.0),
            local_light: [0.0; 4],
            sun_dir: rgb(state.sun_direction, 0.0),
            clouds: [0.0, 0.0, clouds::TILE_BLOCKS, clouds::HEIGHT],
            lights: [[0.0; 4]; MAX_POINT_LIGHTS],
            light_colors: [[0.0; 4]; MAX_POINT_LIGHTS],
        };
        let used = lights.len().min(MAX_POINT_LIGHTS);
        uniform.misc[0] = used as f32;
        for (i, light) in lights.iter().take(used).enumerate() {
            uniform.lights[i] = [light.position.x, light.position.y, light.position.z, light.radius];
            uniform.light_colors[i] = [light.color.x, light.color.y, light.color.z, light.brightness];
        }
        uniform
    }
}

/// Owns the light engine and everything that changes how a frame is lit.
pub struct LightingController {
    pub engine: LightEngine,
    pub shading: Shading,
    /// Off shows the old flat look: a fixed brightness per face.
    pub enabled: bool,
    /// The settings menu's `ambientOcclusion` switch. Off sets the AO strength to 0.
    pub ambient_occlusion: bool,
    /// The strength used while ambient occlusion is on (the CVar `ambientOcclusion`).
    pub ao_strength: f32,
    /// Light the sprites per pixel with their normal maps. Only has an effect while the normal pages
    /// are loaded (`texture::load`), which `web.rs` records here.
    pub normal_maps: bool,
    /// The focus entity (the local player): the one light the normal map shader lights per pixel.
    pub local_light: Option<Vec3>,
    /// The scene is drawn in linear light (`post.rs`) instead of display colours.
    pub linear_light: bool,
    /// Cloud shadows drift over the world (see `clouds.rs`).
    pub clouds: bool,
    /// How dark the heart of a cloud's shadow is, 0..1.
    pub cloud_strength: f32,
    /// How fast the clouds drift, 1 = the normal wind, 0 = they stand still.
    pub cloud_speed: f32,
    /// Seconds of drift so far (real time times the speed, so a change of speed does not make the
    /// shadows jump); they move even while the day clock is stopped.
    cloud_time: f32,
    /// Time passes this many times faster than the Java day length (7.7 minutes). 0 stops the clock.
    pub time_scale: f32,
    dynamic: Vec<PointLight>,
}

impl Default for LightingController {
    fn default() -> Self {
        Self::new()
    }
}

impl LightingController {
    /// Starts at noon, which is what the flat colours of the palette are tuned for.
    pub fn new() -> Self {
        let mut engine = LightEngine::new(DEFAULT_WORLD_SPIN_ANGLE, DEFAULT_AZIMUTH_SPEED);
        engine.set_to_noon();
        let mut controller = LightingController {
            engine,
            shading: Shading::default(),
            enabled: true,
            ambient_occlusion: true,
            ao_strength: DEFAULT_AO_STRENGTH,
            normal_maps: false,
            local_light: None,
            linear_light: false,
            clouds: true,
            cloud_strength: clouds::DEFAULT_STRENGTH,
            cloud_speed: 1.0,
            cloud_time: 0.0,
            time_scale: 1.0,
            dynamic: Vec::new(),
        };
        controller.apply_settings();
        controller
    }

    /// Advance the day. `dt_ms` is real time in milliseconds.
    pub fn update(&mut self, dt_ms: f32) {
        if dt_ms.is_finite() && dt_ms > 0.0 {
            self.cloud_time = (self.cloud_time + dt_ms / 1000.0 * self.cloud_speed.clamp(0.0, 10.0)) % 100_000.0;
        }
        if self.time_scale > 0.0 && dt_ms.is_finite() && dt_ms > 0.0 {
            self.engine.update(dt_ms * self.time_scale);
        }
    }

    /// Apply `ambient_occlusion` and `ao_strength` to the shading. Call after changing them.
    pub fn apply_settings(&mut self) {
        self.shading.ao_strength = if self.ambient_occlusion { self.ao_strength.clamp(0.0, 1.0) } else { 0.0 };
    }

    /// The moving lights for the next frames (torches carried by players, projectiles...). Only the
    /// [`MAX_POINT_LIGHTS`] nearest to `focus` (normally the camera target) are kept.
    pub fn set_dynamic_lights(&mut self, lights: impl IntoIterator<Item = PointLight>, focus: Vec3) {
        self.dynamic = lights.into_iter().collect();
        self.dynamic.sort_by(|a, b| {
            let (da, db) = (a.position.distance_squared(focus), b.position.distance_squared(focus));
            da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
        });
        self.dynamic.truncate(MAX_POINT_LIGHTS);
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn dynamic_lights(&self) -> &[PointLight] {
        &self.dynamic
    }

    /// The uniform for this frame.
    pub fn uniform(&self) -> Lighting {
        let mut uniform = Lighting::new(&self.engine.state(), &self.shading, self.enabled, &self.dynamic);
        // Without the light engine there is nothing to light the pixels with.
        uniform.misc[3] = if self.normal_maps && self.enabled { 1.0 } else { 0.0 };
        uniform.flat_shades[3] = if self.linear_light { 1.0 } else { 0.0 };
        if self.clouds && self.enabled {
            uniform.clouds[0] = self.cloud_time;
            uniform.clouds[1] = self.cloud_strength.clamp(0.0, 1.0);
        }
        if let Some(p) = self.local_light.filter(|p| p.is_finite()) {
            uniform.local_light = [p.x, p.y, p.z, 1.0];
        }
        uniform
    }

    #[cfg_attr(not(test), allow(dead_code))]
    /// One line for a debug overlay.
    pub fn describe(&self) -> String {
        let state = self.engine.state();
        let sun = self.engine.sun();
        format!(
            "light {} · time {:.2} ({}) · sun {:.0}° power {:.2} · AO {:.2} · {} point light(s)",
            if self.enabled { "on" } else { "off" },
            state.time_of_day,
            time_name(state.time_of_day),
            sun.height(),
            sun.power(),
            self.shading.ao_strength,
            self.dynamic.len(),
        )
    }
}

#[cfg_attr(not(test), allow(dead_code))]
fn time_name(time_of_day: f32) -> &'static str {
    match time_of_day {
        t if !(0.0..1.0).contains(&t) => "?",
        t if t < 0.05 => "sunrise",
        t if t < 0.45 => "day",
        t if t < 0.55 => "sunset",
        _ => "night",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::{offset_of, size_of};

    fn noon_uniform() -> Lighting {
        LightingController::new().uniform()
    }

    #[test]
    fn the_uniform_is_all_vec4_so_it_has_no_padding() {
        assert_eq!(size_of::<Lighting>(), 17 * 16 + 2 * MAX_POINT_LIGHTS * 16);
        assert_eq!(size_of::<Lighting>() % 16, 0, "uniform buffers want 16-byte multiples");
        for offset in [
            offset_of!(Lighting, ambient),
            offset_of!(Lighting, sun_color),
            offset_of!(Lighting, moon_color),
            offset_of!(Lighting, sun_faces),
            offset_of!(Lighting, moon_faces),
            offset_of!(Lighting, grading),
            offset_of!(Lighting, flat_shades),
            offset_of!(Lighting, misc),
            offset_of!(Lighting, fog),
            offset_of!(Lighting, sun_back),
            offset_of!(Lighting, moon_back),
            offset_of!(Lighting, sun_normal),
            offset_of!(Lighting, moon_normal),
            offset_of!(Lighting, pixel_ambient),
            offset_of!(Lighting, local_light),
            offset_of!(Lighting, sun_dir),
            offset_of!(Lighting, clouds),
            offset_of!(Lighting, lights),
            offset_of!(Lighting, light_colors),
        ] {
            assert_eq!(offset % 16, 0);
        }
    }

    // ---- the shader

    fn parse_shader() -> naga::Module {
        naga::front::wgsl::parse_str(include_str!("shader.wgsl")).unwrap_or_else(|e| panic!("shader.wgsl does not parse:\n{}", e.emit_to_string(include_str!("shader.wgsl"))))
    }

    #[test]
    fn the_shader_parses_and_validates() {
        let module = parse_shader();
        // No optional capabilities: what has to work everywhere, including the WebGL2 fallback.
        naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::empty())
            .validate(&module)
            .unwrap_or_else(|e| panic!("shader.wgsl is not valid:\n{}", e.emit_to_string(include_str!("shader.wgsl"))));
        let entry_points: Vec<_> = module.entry_points.iter().map(|e| (e.name.as_str(), e.stage)).collect();
        assert!(entry_points.contains(&("vs_main", naga::ShaderStage::Vertex)), "{entry_points:?}");
        assert!(entry_points.contains(&("fs_main", naga::ShaderStage::Fragment)), "{entry_points:?}");
    }

    #[test]
    fn the_shader_binds_camera_and_lighting_in_group_zero_the_atlas_in_one_and_the_peel_state_in_two() {
        let module = parse_shader();
        let mut bindings: Vec<(u32, u32, String)> = module
            .global_variables
            .iter()
            .filter_map(|(_, g)| {
                let b = g.binding.as_ref()?;
                Some((b.group, b.binding, g.name.clone().unwrap_or_default()))
            })
            .collect();
        bindings.sort();
        assert_eq!(
            bindings,
            vec![
                (0, 0, "camera".to_string()),
                (0, 1, "lighting".to_string()),
                (0, 2, "cloud_map".to_string()),
                (0, 3, "cloud_sampler".to_string()),
                (0, 4, "sun_shadow".to_string()),
                (0, 5, "shadow_map".to_string()),
                (1, 0, "atlas".to_string()),
                (1, 1, "atlas_sampler".to_string()),
                (1, 2, "normals".to_string()),
                (2, 0, "peel".to_string()),
                (2, 1, "previous_depth".to_string()),
            ]
        );
    }

    /// The members of a struct in the shader: `(name, offset)` and its total size.
    fn wgsl_struct(module: &naga::Module, name: &str) -> (Vec<(String, u32)>, u32) {
        let (_, ty) = module.types.iter().find(|(_, t)| t.name.as_deref() == Some(name)).unwrap_or_else(|| panic!("no struct {name}"));
        match &ty.inner {
            naga::TypeInner::Struct { members, span } => (
                members.iter().map(|m| (m.name.clone().unwrap_or_default(), m.offset)).collect(),
                *span,
            ),
            other => panic!("{name} is not a struct: {other:?}"),
        }
    }

    #[test]
    fn the_wgsl_lighting_struct_has_the_same_layout_as_the_rust_one() {
        let (members, size) = wgsl_struct(&parse_shader(), "Lighting");
        let expected = [
            ("ambient", offset_of!(Lighting, ambient)),
            ("sun_color", offset_of!(Lighting, sun_color)),
            ("moon_color", offset_of!(Lighting, moon_color)),
            ("sun_faces", offset_of!(Lighting, sun_faces)),
            ("moon_faces", offset_of!(Lighting, moon_faces)),
            ("grading", offset_of!(Lighting, grading)),
            ("flat_shades", offset_of!(Lighting, flat_shades)),
            ("misc", offset_of!(Lighting, misc)),
            ("fog", offset_of!(Lighting, fog)),
            ("sun_back", offset_of!(Lighting, sun_back)),
            ("moon_back", offset_of!(Lighting, moon_back)),
            ("sun_normal", offset_of!(Lighting, sun_normal)),
            ("moon_normal", offset_of!(Lighting, moon_normal)),
            ("pixel_ambient", offset_of!(Lighting, pixel_ambient)),
            ("local_light", offset_of!(Lighting, local_light)),
            ("sun_dir", offset_of!(Lighting, sun_dir)),
            ("clouds", offset_of!(Lighting, clouds)),
            ("lights", offset_of!(Lighting, lights)),
            ("light_colors", offset_of!(Lighting, light_colors)),
        ];
        let got: Vec<(&str, usize)> = members.iter().map(|(n, o)| (n.as_str(), *o as usize)).collect();
        assert_eq!(got, expected.to_vec(), "field order and offsets");
        assert_eq!(size as usize, size_of::<Lighting>(), "total size");
    }

    #[test]
    fn the_camera_struct_is_48_bytes_like_camera_uniform_in_web_rs() {
        let (members, size) = wgsl_struct(&parse_shader(), "Camera");
        assert_eq!(size, 48);
        assert_eq!(members.iter().map(|(n, o)| (n.as_str(), *o)).take(3).collect::<Vec<_>>(), [("center", 0), ("scale", 8), ("center_depth", 16)]);
    }

    #[test]
    fn the_vertex_inputs_match_the_vertex_struct() {
        use crate::mesh::Vertex;
        let module = parse_shader();
        let entry = module.entry_points.iter().find(|e| e.name == "vs_main").unwrap();
        let (_, vertex_in) = module.types.iter().find(|(_, t)| t.name.as_deref() == Some("VertexIn")).unwrap();
        let naga::TypeInner::Struct { members, .. } = &vertex_in.inner else { panic!("VertexIn is not a struct") };
        let formats: Vec<(u32, usize)> = members
            .iter()
            .map(|m| {
                let Some(naga::Binding::Location { location, .. }) = m.binding else { panic!("member without a location") };
                let components = match &module.types[m.ty].inner {
                    naga::TypeInner::Vector { size, .. } => *size as usize,
                    naga::TypeInner::Scalar(_) => 1,
                    other => panic!("unexpected input type {other:?}"),
                };
                (location, components)
            })
            .collect();
        // position (3 floats), colour (3), shade (2), baked point light (3), atlas uv (2), page (1)
        assert_eq!(formats, vec![(0, 3), (1, 3), (2, 2), (3, 3), (4, 2), (5, 1)]);
        let floats: usize = formats.iter().map(|f| f.1).sum();
        assert_eq!(floats * 4, size_of::<Vertex>());
        assert_eq!(entry.function.arguments.len(), 1);
    }

    // ---- the uniform

    #[test]
    fn the_flat_look_is_selected_by_the_ambient_w_component() {
        let state = LightingController::new().engine.state();
        let lit = Lighting::new(&state, &Shading::default(), true, &[]);
        let flat = Lighting::new(&state, &Shading::default(), false, &[]);
        assert_eq!(lit.ambient[3], 1.0);
        assert_eq!(flat.ambient[3], 0.0);
        assert_eq!(flat.flat_shades, [0.78, 1.0, 0.58, 0.0]);
    }

    #[test]
    fn the_uniform_carries_the_light_state() {
        let controller = LightingController::new();
        let state = controller.engine.state();
        let u = noon_uniform();
        assert_eq!(&u.sun_color[..3], &state.sun_color.to_array());
        assert_eq!(u.sun_color[3], state.sun_spec);
        assert_eq!(&u.sun_faces[..3], &state.sun_faces);
        assert_eq!(u.sun_faces[3], state.night_mix);
        assert_eq!(u.misc[1], state.time_of_day);
        assert_eq!(u.grading, [1.45, 0.5, 25.0, 1.0], "exposure, AO, point gain, night grading");
        assert_eq!(u.misc[2], 0.15, "the minimum light");
        let weight = Shading::default().ambient_weight;
        assert!((u.ambient[0] - state.ambient.x * weight).abs() < 1e-6, "the ambient colour is pre-weighted");
        assert!(u.sun_faces[1] > u.sun_faces[0], "the top is lit most at noon");
    }

    #[test]
    fn the_normal_map_inputs_are_the_java_uniforms() {
        let mut controller = LightingController::new();
        let state = controller.engine.state();
        let off = controller.uniform();
        assert_eq!(off.misc[3], 0.0, "off until the normal pages have loaded");
        assert_eq!(off.local_light, [0.0; 4]);
        assert_eq!(&off.pixel_ambient[..3], &state.ambient.to_array(), "unweighted, like u_ambientColor");
        assert_eq!(&off.sun_normal[..3], &state.sun_normal_game.to_array());
        assert!((Vec3::from_slice(&off.sun_normal[..3]).length() - 1.0).abs() < 1e-5, "u_sunNormal is a unit vector");

        controller.normal_maps = true;
        controller.local_light = Some(Vec3::new(3.0, 4.0, 5.0));
        let on = controller.uniform();
        assert_eq!(on.misc[3], 1.0);
        assert_eq!(on.local_light, [3.0, 4.0, 5.0, 1.0]);

        controller.local_light = Some(Vec3::splat(f32::NAN));
        assert_eq!(controller.uniform().local_light, [0.0; 4], "a bad position is no light");
        controller.enabled = false;
        assert_eq!(controller.uniform().misc[3], 0.0, "lighting off means the flat look, not normal maps");
    }

    #[test]
    fn the_clouds_drift_with_real_time_and_switch_off_with_the_light() {
        let mut controller = LightingController::new();
        controller.time_scale = 0.0;
        controller.update(2000.0);
        let u = controller.uniform();
        assert_eq!(u.clouds[0], 2.0, "moves although the day clock is stopped");
        assert_eq!(u.clouds[1], clouds::DEFAULT_STRENGTH);
        assert!((Vec3::from_slice(&u.sun_dir[..3]).length() - 1.0).abs() < 1e-4);
        controller.cloud_speed = 2.0;
        controller.update(1000.0);
        assert_eq!(controller.uniform().clouds[0], 4.0, "faster wind adds drift without moving what was passed");
        controller.cloud_speed = 0.0;
        controller.update(1000.0);
        assert_eq!(controller.uniform().clouds[0], 4.0, "speed 0: they stand still");
        controller.clouds = false;
        assert_eq!(controller.uniform().clouds[1], 0.0);
        controller.clouds = true;
        controller.enabled = false;
        assert_eq!(controller.uniform().clouds[1], 0.0, "the flat look has no cloud shadows");
    }

    #[test]
    fn point_lights_are_copied_and_capped() {
        let state = LightingController::new().engine.state();
        let lights: Vec<_> = (0..12)
            .map(|i| PointLight::new(Vec3::new(i as f32, 0.0, 1.0), Vec3::new(1.0, 0.5, 0.25), 6.0, 2.0))
            .collect();
        let u = Lighting::new(&state, &Shading::default(), true, &lights[..2]);
        assert_eq!(u.misc[0], 2.0);
        assert_eq!(u.lights[1], [1.0, 0.0, 1.0, 6.0]);
        assert_eq!(u.light_colors[1], [1.0, 0.5, 0.25, 2.0]);
        assert_eq!(u.lights[2], [0.0; 4], "unused slots are zero");

        let capped = Lighting::new(&state, &Shading::default(), true, &lights);
        assert_eq!(capped.misc[0], MAX_POINT_LIGHTS as f32);
    }

    // ---- the controller

    #[test]
    fn the_controller_keeps_the_nearest_dynamic_lights() {
        let mut controller = LightingController::new();
        let lights: Vec<_> = (0..20).rev().map(|i| PointLight::new(Vec3::new(i as f32, 0.0, 0.0), Vec3::ONE, 5.0, 1.0)).collect();
        controller.set_dynamic_lights(lights, Vec3::new(3.0, 0.0, 0.0));
        let xs: Vec<f32> = controller.dynamic_lights().iter().map(|l| l.position.x).collect();
        assert_eq!(xs.len(), MAX_POINT_LIGHTS);
        assert_eq!(xs[0], 3.0, "nearest first");
        assert!(xs.iter().all(|&x| (x - 3.0).abs() <= 4.0), "{xs:?}");
        assert_eq!(controller.uniform().misc[0], MAX_POINT_LIGHTS as f32);

        let mut nan = LightingController::new();
        nan.set_dynamic_lights([PointLight::new(Vec3::splat(f32::NAN), Vec3::ONE, 5.0, 1.0)], Vec3::ZERO);
        assert!(nan.uniform().misc[0] <= 1.0, "bad input does not panic");
    }

    #[test]
    fn time_passes_at_the_configured_speed_and_can_be_stopped() {
        let mut controller = LightingController::new();
        let start = controller.engine.time_of_day();
        controller.update(10_000.0);
        assert!(controller.engine.time_of_day() > start);

        let mut fast = LightingController::new();
        fast.time_scale = 100.0;
        fast.update(10_000.0);
        assert!(fast.engine.time_of_day() > controller.engine.time_of_day());

        let mut stopped = LightingController::new();
        stopped.time_scale = 0.0;
        stopped.update(1.0e6);
        assert_eq!(stopped.engine.time_of_day(), start);
        stopped.time_scale = 1.0;
        stopped.update(f32::NAN);
        stopped.update(-5.0);
        assert_eq!(stopped.engine.time_of_day(), start, "nonsense time steps are ignored");
    }

    #[test]
    fn ambient_occlusion_follows_the_menu_setting() {
        let mut controller = LightingController::new();
        assert_eq!(controller.uniform().grading[1], 0.5);
        controller.ambient_occlusion = false;
        controller.apply_settings();
        assert_eq!(controller.uniform().grading[1], 0.0);
        controller.ambient_occlusion = true;
        controller.ao_strength = 7.0;
        controller.apply_settings();
        assert_eq!(controller.uniform().grading[1], 1.0, "clamped to the valid range");
    }

    #[test]
    fn a_day_goes_through_the_phases_and_the_description_says_so() {
        let mut controller = LightingController::new();
        controller.time_scale = 1.0;
        let mut seen = std::collections::BTreeSet::new();
        let day_ms = 360.0 / DEFAULT_AZIMUTH_SPEED;
        let mut t = 0.0;
        while t < day_ms {
            controller.update(1000.0);
            t += 1000.0;
            let state = controller.engine.state();
            seen.insert(time_name(state.time_of_day));
            let u = controller.uniform();
            assert!(u.sun_faces.iter().all(|v| v.is_finite()) && u.moon_faces.iter().all(|v| v.is_finite()));
        }
        assert!(seen.contains("day") && seen.contains("night"), "{seen:?}");
        let text = controller.describe();
        assert!(text.contains("light on") && text.contains("point light"), "{text}");
    }
}
