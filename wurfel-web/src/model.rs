//! Static 3D models from glTF binaries (`.glb`), as the triangles the world pipeline already draws.
//! Pure CPU code with no graphics dependency, so it is tested natively.
//!
//! Read: positions, indices, per-vertex colours (`COLOR_0`), each material's base colour and base
//! colour texture. Ignored: normals, other textures, alpha and skins.
//!
//! # Stage
//!
//! Nodes named in `STAGE_NODES` ("Env", the ground disc Sketchfab exports add) are skipped.
//!
//! # Animation
//!
//! There is no animation support yet. A file whose first animation moves nodes (translation,
//! rotation, scale; no skins) is loaded **posed at the middle of that animation**, so an animated
//! model shows a typical frame instead of its (often collapsed) rest pose.
//!
//! The result is plain [`Vertex`]es in the pipeline of `shader.wgsl`, so they share the
//! projection and the depth buffer with blocks and sprites and sort against them exactly.
//!
//! # Colour
//!
//! A vertex's `color` is `baseColorFactor * COLOR_0`. The shader multiplies it with the texel when
//! the vertex has a texture, exactly what glTF specifies, and shows it as is when it has none.
//!
//! # Textures
//!
//! The shader reads the sprite atlas, a `texture_2d_array`. A model's images are separate, so the
//! triangles are grouped by image: [`Model::append`] returns one [`Draw`] per group, with the range
//! of vertices it wrote. A textured vertex has `layer == 0` and the glTF `uv` unchanged (glTF's
//! origin is the top left, like wgpu's). To draw a group the renderer binds a texture array of one
//! layer holding [`Model::textures`]`[texture]` as bind group 1, the layout the atlas already uses,
//! and draws the range; groups without a texture ignore group 1. The sampler's address mode decides
//! what happens outside 0..1 (glTF models usually repeat); that is chosen when the bind group is made.
//!
//! # Axes
//!
//! glTF is right-handed, Y up, in metres. The engine's ground frame is x, y, height z in blocks
//! (see `wurfel_sim::grid`). A glTF point `(x, y, z)` becomes `(x, -z, y)`: a rotation, so the
//! triangle winding is kept. One glTF metre is one block unless `scale` says otherwise.

#![cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]

use std::collections::{BTreeMap, HashMap};
use std::ops::Range;

use glam::{Mat4, Quat, Vec3, Vec4};

use crate::mesh::{Vertex, FACE_UNLIT, NO_SPRITE};

/// A decoded image, 8 bits per channel, rows from the top.
#[derive(Debug, Clone)]
pub struct Texture {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

#[derive(Debug, Clone, Copy)]
struct Corner {
    position: [f32; 3],
    color: [f32; 3],
    uv: [f32; 2],
}

/// A run of vertices that share one texture, as written by [`Model::append`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Draw {
    /// Index into [`Model::textures`], or `None` for flat coloured vertices.
    pub texture: Option<usize>,
    /// Positions in the vertex list that was appended to.
    pub vertices: Range<usize>,
}

/// A loaded model: triangles in engine axes, the origin where the glTF origin was.
#[derive(Debug, Clone, Default)]
pub struct Model {
    /// The corners of each group, three to a triangle. The key is an index into `textures`.
    groups: BTreeMap<Option<usize>, Vec<Corner>>,
    textures: Vec<Texture>,
    /// Things that were skipped (a texture that could not be decoded...). The model still loads.
    pub warnings: Vec<String>,
}

impl Model {
    /// Parse a `.glb`. A `.gltf` with external or base64 buffers is not supported: export the
    /// binary form. Every mesh of the default scene is included, with its node transforms.
    pub fn from_glb(bytes: &[u8]) -> Result<Model, String> {
        let gltf = gltf::Gltf::from_slice(bytes).map_err(|e| format!("not a valid glTF: {e}"))?;
        let blob = gltf.blob.as_deref();
        let buffers: Vec<&[u8]> = gltf
            .buffers()
            .map(|buffer| match buffer.source() {
                gltf::buffer::Source::Bin => blob.ok_or_else(|| "the file has no binary chunk".to_string()),
                gltf::buffer::Source::Uri(_) => Err("external or embedded buffers are not supported: export as .glb".to_string()),
            })
            .collect::<Result<_, _>>()?;

        let scene = gltf.default_scene().or_else(|| gltf.scenes().next()).ok_or("the file has no scene")?;
        let pose = middle_pose(&gltf, &buffers);
        let mut loader = Loader { model: Model::default(), buffers, images: vec![None; gltf.images().len()], pose };
        for node in scene.nodes() {
            loader.add_node(&node, IMPORTED_TO_ENGINE)?;
        }
        let model = loader.model;
        if model.groups.values().all(|corners| corners.is_empty()) {
            return Err("the scene has no triangles".to_string());
        }
        Ok(model)
    }

    pub fn triangle_count(&self) -> usize {
        self.groups.values().map(|corners| corners.len() / 3).sum()
    }

    /// The images the [`Draw::texture`] indices refer to.
    pub fn textures(&self) -> &[Texture] {
        &self.textures
    }

    /// The bounding box `(min, max)` in engine axes, or `None` for an empty model.
    pub fn bounds(&self) -> Option<(Vec3, Vec3)> {
        let mut points = self.groups.values().flatten().map(|c| Vec3::from(c.position));
        let first = points.next()?;
        Some(points.fold((first, first), |(lo, hi), p| (lo.min(p), hi.max(p))))
    }

    /// Append the model standing at `position` (ground x, ground y, height), turned by `yaw`
    /// radians around the vertical axis and scaled by `scale`. Returns the vertex ranges to draw,
    /// one per texture (see the module docs); a model without textures gives a single draw.
    pub fn append(&self, out: &mut Vec<Vertex>, position: Vec3, yaw: f32, scale: f32) -> Vec<Draw> {
        let transform = Mat4::from_translation(position) * Mat4::from_rotation_z(yaw) * Mat4::from_scale(Vec3::splat(scale));
        let mut draws = Vec::new();
        for (&texture, corners) in &self.groups {
            let start = out.len();
            let layer = if texture.is_some() { 0.0 } else { NO_SPRITE };
            out.extend(corners.iter().map(|c| {
                let world = transform.transform_point3(Vec3::from(c.position));
                Vertex { position: world.to_array(), color: c.color, shade: [FACE_UNLIT, 0.0], point: [0.0; 3], uv: c.uv, layer }
            }));
            if out.len() > start {
                draws.push(Draw { texture, vertices: start..out.len() });
            }
        }
        draws
    }
}

struct Loader<'a> {
    model: Model,
    buffers: Vec<&'a [u8]>,
    /// For each glTF image: its index in `model.textures` once decoded; `Some(None)` when decoding failed.
    images: Vec<Option<Option<usize>>>,
    /// Node transforms taken from the animation, by node index.
    pose: HashMap<usize, NodePose>,
}

/// The parts of a node's transform an animation sets; the others keep the node's own value.
#[derive(Debug, Clone, Copy, Default)]
struct NodePose {
    translation: Option<Vec3>,
    rotation: Option<Quat>,
    scale: Option<Vec3>,
}

/// The value of the keyframes at `time`: linear between the neighbours, held outside the range.
fn sample<T: Copy>(times: &[f32], values: &[T], time: f32, step: bool, lerp: impl Fn(T, T, f32) -> T) -> Option<T> {
    let first = *values.first()?;
    if times.len() != values.len() || time <= *times.first()? {
        return Some(first);
    }
    let last = values.len() - 1;
    if time >= times[last] {
        return Some(values[last]);
    }
    let next = times.iter().position(|&t| t > time)?;
    let (a, b) = (next - 1, next);
    if step {
        return Some(values[a]);
    }
    let span = times[b] - times[a];
    Some(lerp(values[a], values[b], if span > 0.0 { (time - times[a]) / span } else { 0.0 }))
}

/// Every animated node's transform halfway through the first animation.
fn middle_pose(gltf: &gltf::Gltf, buffers: &[&[u8]]) -> HashMap<usize, NodePose> {
    use gltf::animation::util::ReadOutputs;
    use gltf::animation::Interpolation;

    let mut pose: HashMap<usize, NodePose> = HashMap::new();
    let Some(animation) = gltf.animations().next() else { return pose };
    let read = |channel: &gltf::animation::Channel| {
        let reader = channel.reader(|buffer| buffers.get(buffer.index()).copied());
        (reader.read_inputs().map(|i| i.collect::<Vec<f32>>()), reader.read_outputs())
    };
    let times: Vec<(Vec<f32>, ReadOutputs)> = animation.channels().filter_map(|c| read(&c).0.zip(read(&c).1)).collect();
    let (start, end) = times.iter().flat_map(|(t, _)| t.iter().copied()).fold((f32::MAX, f32::MIN), |(lo, hi), t| (lo.min(t), hi.max(t)));
    if start > end {
        return pose;
    }
    let time = (start + end) / 2.0;
    for channel in animation.channels() {
        let (Some(inputs), Some(outputs)) = read(&channel) else { continue };
        let interpolation = channel.sampler().interpolation();
        let step = interpolation == Interpolation::Step;
        // A cubic spline stores (in tangent, value, out tangent) per key: the value is the middle one.
        fn values_of<T: Copy>(all: Vec<T>, cubic: bool) -> Vec<T> {
            if cubic { all.chunks_exact(3).map(|k| k[1]).collect() } else { all }
        }
        let cubic = interpolation == Interpolation::CubicSpline;
        let entry = pose.entry(channel.target().node().index()).or_default();
        match outputs {
            ReadOutputs::Translations(values) => {
                let values = values_of(values.map(Vec3::from).collect(), cubic);
                entry.translation = sample(&inputs, &values, time, step, |a, b, t| a.lerp(b, t));
            }
            ReadOutputs::Scales(values) => {
                let values = values_of(values.map(Vec3::from).collect(), cubic);
                entry.scale = sample(&inputs, &values, time, step, |a, b, t| a.lerp(b, t));
            }
            ReadOutputs::Rotations(values) => {
                let values = values_of(values.into_f32().map(Quat::from_array).collect(), cubic);
                entry.rotation = sample(&inputs, &values, time, step, |a, b, t| a.slerp(b, t));
            }
            ReadOutputs::MorphTargetWeights(_) => {}
        }
    }
    pose
}

impl Loader<'_> {
    fn add_node(&mut self, node: &gltf::Node, parent: Mat4) -> Result<(), String> {
        let local = match self.pose.get(&node.index()) {
            Some(pose) => {
                let (translation, rotation, scale) = node.transform().decomposed();
                Mat4::from_scale_rotation_translation(
                    pose.scale.unwrap_or(Vec3::from(scale)),
                    pose.rotation.unwrap_or(Quat::from_array(rotation)),
                    pose.translation.unwrap_or(Vec3::from(translation)),
                )
            }
            None => Mat4::from_cols_array_2d(&node.transform().matrix()),
        };
        if node.name().is_some_and(|name| STAGE_NODES.contains(&name)) {
            return Ok(());
        }
        let here = parent * local;
        if let Some(mesh) = node.mesh() {
            for primitive in mesh.primitives() {
                if primitive.mode() == gltf::mesh::Mode::Triangles {
                    self.add_primitive(&primitive, here)?;
                }
            }
        }
        for child in node.children() {
            self.add_node(&child, here)?;
        }
        Ok(())
    }

    fn add_primitive(&mut self, primitive: &gltf::Primitive, transform: Mat4) -> Result<(), String> {
        let reader = primitive.reader(|buffer| self.buffers.get(buffer.index()).copied());
        let positions: Vec<[f32; 3]> = reader.read_positions().ok_or("a primitive has no positions")?.collect();
        let indices: Vec<u32> = match reader.read_indices() {
            Some(indices) => indices.into_u32().collect(),
            None => (0..positions.len() as u32).collect(),
        };
        let vertex_colors: Option<Vec<[f32; 3]>> = reader.read_colors(0).map(|c| c.into_rgb_f32().collect());

        let pbr = primitive.material().pbr_metallic_roughness();
        let factor = pbr.base_color_factor();
        let (texture, uvs) = match pbr.base_color_texture() {
            Some(info) => {
                let uvs: Option<Vec<[f32; 2]>> = reader.read_tex_coords(info.tex_coord()).map(|t| t.into_f32().collect());
                match (uvs, self.texture(&info.texture().source())) {
                    (Some(uvs), Some(texture)) => (Some(texture), Some(uvs)),
                    (None, _) => {
                        self.model.warnings.push(format!("a primitive has a texture but no TEXCOORD_{}", info.tex_coord()));
                        (None, None)
                    }
                    (_, None) => (None, None),
                }
            }
            None => (None, None),
        };

        let corners = self.model.groups.entry(texture).or_default();
        for triangle in indices.chunks_exact(3) {
            for &i in triangle {
                let i = i as usize;
                let p = positions.get(i).ok_or("an index points past the positions")?;
                let tint = vertex_colors.as_ref().and_then(|c| c.get(i)).copied().unwrap_or([1.0; 3]);
                corners.push(Corner {
                    position: transform.transform_point3(Vec3::from(*p)).to_array(),
                    color: [factor[0] * tint[0], factor[1] * tint[1], factor[2] * tint[2]],
                    uv: uvs.as_ref().and_then(|t| t.get(i)).copied().unwrap_or([0.0; 2]),
                });
            }
        }
        Ok(())
    }

    /// The index in `model.textures` of a glTF image, decoding it the first time. `None` (with a
    /// warning) when it cannot be read, so the primitive keeps its flat colour.
    fn texture(&mut self, image: &gltf::Image) -> Option<usize> {
        if let Some(known) = self.images[image.index()] {
            return known;
        }
        let decoded = self.decode(image);
        let index = match decoded {
            Ok(texture) => {
                self.model.textures.push(texture);
                Some(self.model.textures.len() - 1)
            }
            Err(message) => {
                self.model.warnings.push(format!("image {}: {message}", image.index()));
                None
            }
        };
        self.images[image.index()] = Some(index);
        index
    }

    fn decode(&self, image: &gltf::Image) -> Result<Texture, String> {
        let gltf::image::Source::View { view, .. } = image.source() else {
            return Err("external images are not supported: export as .glb".to_string());
        };
        let buffer = self.buffers.get(view.buffer().index()).ok_or("the image's buffer is missing")?;
        let bytes = buffer.get(view.offset()..view.offset() + view.length()).ok_or("the image lies outside its buffer")?;
        let decoded = image::load_from_memory(bytes).map_err(|e| format!("cannot decode: {e}"))?.to_rgba8();
        Ok(Texture { width: decoded.width(), height: decoded.height(), rgba: decoded.into_raw() })
    }
}

/// Nodes (with everything below them) that are not part of the model: the ground disc and sky
/// that Sketchfab exports put around a showcased asset.
const STAGE_NODES: [&str; 1] = ["Env"];

/// glTF (x right, y up, z toward the viewer) to the engine's (x, y, height): `(x, -z, y)`.
const IMPORTED_TO_ENGINE: Mat4 = Mat4::from_cols(
    Vec4::new(1.0, 0.0, 0.0, 0.0),
    Vec4::new(0.0, 0.0, 1.0, 0.0),
    Vec4::new(0.0, -1.0, 0.0, 0.0),
    Vec4::new(0.0, 0.0, 0.0, 1.0),
);

#[cfg(test)]
mod tests {
    use super::*;

    /// Pack a JSON chunk and a binary chunk into a `.glb`.
    fn glb(json: String, bin: &[u8]) -> Vec<u8> {
        let mut json = json.into_bytes();
        while json.len() % 4 != 0 {
            json.push(b' ');
        }
        let mut bin = bin.to_vec();
        while bin.len() % 4 != 0 {
            bin.push(0);
        }
        let total = 12 + 8 + json.len() + 8 + bin.len();
        let mut out = Vec::new();
        out.extend_from_slice(b"glTF");
        out.extend_from_slice(&2u32.to_le_bytes());
        out.extend_from_slice(&(total as u32).to_le_bytes());
        out.extend_from_slice(&(json.len() as u32).to_le_bytes());
        out.extend_from_slice(b"JSON");
        out.extend_from_slice(&json);
        out.extend_from_slice(&(bin.len() as u32).to_le_bytes());
        out.extend_from_slice(b"BIN\0");
        out.extend_from_slice(&bin);
        out
    }

    fn floats(values: &[f32]) -> Vec<u8> {
        values.iter().flat_map(|v| v.to_le_bytes()).collect()
    }

    /// A 2x1 PNG: a red pixel and a blue pixel.
    fn png_2x1() -> Vec<u8> {
        let mut out = Vec::new();
        let mut encoder = png::Encoder::new(&mut out, 2, 1);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().unwrap();
        writer.write_image_data(&[255, 0, 0, 255, 0, 0, 255, 255]).unwrap();
        writer.finish().unwrap();
        out
    }

    /// The triangle `(0,0,0) (1,0,0) (0,1,0)` (glTF axes), with the given material JSON, optional
    /// `COLOR_0` and optional embedded image. The node is moved by `translation`.
    fn triangle(translation: [f32; 3], material: &str, colors: Option<[[f32; 3]; 3]>, image: Option<Vec<u8>>) -> Vec<u8> {
        let mut bin = floats(&[0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0]);
        let mut views = vec![format!(r#"{{"buffer":0,"byteOffset":0,"byteLength":{}}}"#, bin.len())];
        let mut accessors = vec![r#"{"bufferView":0,"componentType":5126,"count":3,"type":"VEC3","min":[0,0,0],"max":[1,1,0]}"#.to_string()];
        let mut attributes = r#""POSITION":0"#.to_string();
        let mut push_view = |bin: &mut Vec<u8>, data: &[u8]| {
            while bin.len() % 4 != 0 {
                bin.push(0);
            }
            views.push(format!(r#"{{"buffer":0,"byteOffset":{},"byteLength":{}}}"#, bin.len(), data.len()));
            bin.extend_from_slice(data);
            views.len() - 1
        };
        if let Some(colors) = colors {
            let view = push_view(&mut bin, &floats(&colors.concat()));
            accessors.push(format!(r#"{{"bufferView":{view},"componentType":5126,"count":3,"type":"VEC3"}}"#));
            attributes += &format!(r#","COLOR_0":{}"#, accessors.len() - 1);
        }
        let mut extra = String::new();
        if let Some(png) = image {
            // uv of the corners: left half of the picture (red) to right half (blue).
            let view = push_view(&mut bin, &floats(&[0.0, 0.0, 1.0, 0.0, 0.0, 1.0]));
            accessors.push(format!(r#"{{"bufferView":{view},"componentType":5126,"count":3,"type":"VEC2"}}"#));
            attributes += &format!(r#","TEXCOORD_0":{}"#, accessors.len() - 1);
            let image_view = push_view(&mut bin, &png);
            extra = format!(
                r#","images":[{{"bufferView":{image_view},"mimeType":"image/png"}}],"textures":[{{"source":0}}]"#
            );
        }
        let json = format!(
            r#"{{"asset":{{"version":"2.0"}},"scene":0,"scenes":[{{"nodes":[0]}}],
            "nodes":[{{"mesh":0,"translation":[{},{},{}]}}],
            "meshes":[{{"primitives":[{{"attributes":{{{attributes}}},"material":0}}]}}],
            "materials":[{material}],
            "accessors":[{}],"bufferViews":[{}],"buffers":[{{"byteLength":{}}}]{extra}}}"#,
            translation[0],
            translation[1],
            translation[2],
            accessors.join(","),
            views.join(","),
            bin.len().div_ceil(4) * 4,
        );
        glb(json, &bin)
    }

    const RED: &str = r#"{"pbrMetallicRoughness":{"baseColorFactor":[1.0,0.0,0.0,1.0]}}"#;
    const TEXTURED: &str = r#"{"pbrMetallicRoughness":{"baseColorTexture":{"index":0}}}"#;

    #[test]
    fn loads_a_triangle_with_its_colour() {
        let model = Model::from_glb(&triangle([0.0; 3], RED, None, None)).unwrap();
        assert_eq!(model.triangle_count(), 1);
        let mut out = Vec::new();
        let draws = model.append(&mut out, Vec3::ZERO, 0.0, 1.0);
        assert_eq!(draws, vec![Draw { texture: None, vertices: 0..3 }]);
        assert!(out.iter().all(|v| v.color == [1.0, 0.0, 0.0] && v.shade[0] == FACE_UNLIT && v.layer == NO_SPRITE));
    }

    #[test]
    fn vertex_colours_multiply_the_material() {
        let white = r#"{"pbrMetallicRoughness":{"baseColorFactor":[0.5,1.0,1.0,1.0]}}"#;
        let colors = [[1.0, 1.0, 1.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
        let model = Model::from_glb(&triangle([0.0; 3], white, Some(colors), None)).unwrap();
        let mut out = Vec::new();
        model.append(&mut out, Vec3::ZERO, 0.0, 1.0);
        let got: Vec<[f32; 3]> = out.iter().map(|v| v.color).collect();
        assert_eq!(got, vec![[0.5, 1.0, 1.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]);
    }

    #[test]
    fn loads_the_embedded_texture() {
        let model = Model::from_glb(&triangle([0.0; 3], TEXTURED, None, Some(png_2x1()))).unwrap();
        assert!(model.warnings.is_empty(), "{:?}", model.warnings);
        assert_eq!(model.textures().len(), 1);
        let texture = &model.textures()[0];
        assert_eq!((texture.width, texture.height), (2, 1));
        assert_eq!(texture.rgba, vec![255, 0, 0, 255, 0, 0, 255, 255]);

        let mut out = Vec::new();
        let draws = model.append(&mut out, Vec3::ZERO, 0.0, 1.0);
        assert_eq!(draws, vec![Draw { texture: Some(0), vertices: 0..3 }]);
        assert!(out.iter().all(|v| v.layer == 0.0));
        let uvs: Vec<[f32; 2]> = out.iter().map(|v| v.uv).collect();
        assert_eq!(uvs, vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]]);
        // Untinted: the texel shows as it is.
        assert!(out.iter().all(|v| v.color == [1.0; 3]));
    }

    #[test]
    fn a_broken_image_keeps_the_flat_colour() {
        let model = Model::from_glb(&triangle([0.0; 3], TEXTURED, None, Some(b"not a png".to_vec()))).unwrap();
        assert_eq!(model.warnings.len(), 1, "{:?}", model.warnings);
        assert!(model.textures().is_empty());
        let mut out = Vec::new();
        let draws = model.append(&mut out, Vec3::ZERO, 0.0, 1.0);
        assert_eq!(draws, vec![Draw { texture: None, vertices: 0..3 }]);
        assert_eq!(out[0].layer, NO_SPRITE);
    }

    #[test]
    fn gltf_up_becomes_height() {
        // glTF (0, 1, 0) is up: it must end up as height z = 1, not as ground y.
        let model = Model::from_glb(&triangle([0.0; 3], RED, None, None)).unwrap();
        let (lo, hi) = model.bounds().unwrap();
        assert_eq!((lo, hi), (Vec3::ZERO, Vec3::new(1.0, 0.0, 1.0)));
    }

    #[test]
    fn node_transforms_are_applied() {
        // Moved by 2 along glTF -z (forward) = +2 along the engine's ground y.
        let model = Model::from_glb(&triangle([0.0, 0.0, -2.0], RED, None, None)).unwrap();
        let (lo, _) = model.bounds().unwrap();
        assert_eq!(lo, Vec3::new(0.0, 2.0, 0.0));
    }

    #[test]
    fn placement_moves_turns_and_scales() {
        let model = Model::from_glb(&triangle([0.0; 3], RED, None, None)).unwrap();
        let mut out = Vec::new();
        // A quarter turn about the vertical axis takes ground x to ground y.
        model.append(&mut out, Vec3::new(10.0, 20.0, 5.0), std::f32::consts::FRAC_PI_2, 2.0);
        let corner = Vec3::from(out[1].position);
        assert!((corner - Vec3::new(10.0, 22.0, 5.0)).length() < 1e-5, "{corner:?}");
        let top = Vec3::from(out[2].position);
        assert!((top - Vec3::new(10.0, 20.0, 7.0)).length() < 1e-5, "{top:?}");
    }

    /// Two triangle nodes sharing one mesh: "Drone" (animated along glTF z from 0 to -4 over two
    /// seconds, with the given interpolation) and "Env" (the stage, never part of the model).
    fn animated(interpolation: &str, translations: &[[f32; 3]]) -> Vec<u8> {
        let mut bin = floats(&[0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0]);
        let times = bin.len();
        bin.extend(floats(&[0.0, 2.0]));
        let values = bin.len();
        bin.extend(floats(&translations.concat()));
        let json = format!(
            r#"{{"asset":{{"version":"2.0"}},"scene":0,"scenes":[{{"nodes":[0,1]}}],
            "nodes":[{{"name":"Drone","mesh":0}},{{"name":"Env","mesh":0,"translation":[50,0,0]}}],
            "meshes":[{{"primitives":[{{"attributes":{{"POSITION":0}}}}]}}],
            "animations":[{{"samplers":[{{"input":1,"output":2,"interpolation":"{interpolation}"}}],
                "channels":[{{"sampler":0,"target":{{"node":0,"path":"translation"}}}}]}}],
            "accessors":[{{"bufferView":0,"componentType":5126,"count":3,"type":"VEC3","min":[0,0,0],"max":[1,1,0]}},
                {{"bufferView":1,"componentType":5126,"count":2,"type":"SCALAR","min":[0],"max":[2]}},
                {{"bufferView":2,"componentType":5126,"count":{},"type":"VEC3"}}],
            "bufferViews":[{{"buffer":0,"byteOffset":0,"byteLength":36}},
                {{"buffer":0,"byteOffset":{times},"byteLength":8}},
                {{"buffer":0,"byteOffset":{values},"byteLength":{}}}],
            "buffers":[{{"byteLength":{}}}]}}"#,
            translations.len(),
            translations.len() * 12,
            bin.len(),
        );
        glb(json, &bin)
    }

    #[test]
    fn an_animated_node_is_posed_at_the_middle_of_the_animation() {
        let model = Model::from_glb(&animated("LINEAR", &[[0.0; 3], [0.0, 0.0, -4.0]])).unwrap();
        // Halfway: moved by -2 along glTF z = +2 along the engine's ground y. The "Env" node
        // (moved by 50 along x) is not part of the model.
        let (lo, hi) = model.bounds().unwrap();
        assert_eq!((lo, hi), (Vec3::new(0.0, 2.0, 0.0), Vec3::new(1.0, 2.0, 1.0)));
        assert_eq!(model.triangle_count(), 1);
    }

    #[test]
    fn a_stepped_animation_holds_the_earlier_key() {
        let model = Model::from_glb(&animated("STEP", &[[0.0; 3], [0.0, 0.0, -4.0]])).unwrap();
        assert_eq!(model.bounds().unwrap().0, Vec3::ZERO);
    }

    #[test]
    fn a_cubic_spline_animation_uses_the_values_not_the_tangents() {
        // Keys are (in tangent, value, out tangent): the values are 0 and -4 again.
        let keys = [[9.0; 3], [0.0; 3], [9.0; 3], [9.0; 3], [0.0, 0.0, -4.0], [9.0; 3]];
        let model = Model::from_glb(&animated("CUBICSPLINE", &keys)).unwrap();
        assert_eq!(model.bounds().unwrap().0, Vec3::new(0.0, 2.0, 0.0));
    }

    #[test]
    fn rejects_garbage() {
        assert!(Model::from_glb(b"not a model").is_err());
    }
}
