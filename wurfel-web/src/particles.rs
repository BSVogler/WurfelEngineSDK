//! Turns the simulation's particles ([`wurfel_sim::particle`]) into vertices for the existing
//! block pipeline: one screen-aligned, rotated square per particle (the Java particles were
//! camera-facing sprites), flagged `FACE_UNLIT` so the light engine leaves the colour alone.
//!
//! A fading particle gets see-through (`mesh::set_alpha` style: the vertex's `shade[1]` is its
//! transparency), and the depth peeling blends the layers like it does for sprites. Particles at
//! 10% opacity or less are not drawn (`peel::MIN_ALPHA`).

use glam::Vec3;
use wurfel_sim::light::PointLight;
use wurfel_sim::particle::{Particle, ParticleEmitter, Particles};

use crate::mesh::{Vertex, FACE_BILLBOARD};
use crate::sprites::{self, Sprites};

/// Screen width of one block in the projection, and the height of one block on screen: a square
/// on screen needs `WIDTH / HEIGHT` times its width as vertical extent in `z`.
const BLOCK_WIDTH_PX: f32 = 200.0;

/// Ejira's jetpack: two emitters attached behind the player, left and right of the back
/// (`Ejira.update`). They burn while the jetpack is on, spray against the direction the player
/// moves vertically, and the second one also lights its surroundings.
pub struct Jetpack {
    pub flames: [ParticleEmitter; 2],
    last_z: Option<f32>,
}

/// Sideways distance of each nozzle from the player's middle, in blocks (Java: 25 units).
const NOZZLE_SIDE: f32 = 0.25;
/// How far behind the player's back they sit (Java: 20 units).
const NOZZLE_BEHIND: f32 = 0.2;
/// Height above the feet (Java: half an edge length).
const NOZZLE_HEIGHT: f32 = 0.5;
/// The brightness of the second nozzle's light (Java: `setBrightness(10.1f)`).
const NOZZLE_LIGHT: f32 = 10.1;

/// The speed the flame leaves with when the player moves up or down at `vertical` blocks per
/// second: against the movement and faster than it, and just sinking while the player falls
/// (`Ejira.update`, "not physically correct"). Hovering leaves the flame standing still.
pub fn exhaust_speed(vertical: f32) -> f32 {
    let speed = -vertical * 1.5;
    if speed > 0.0 {
        -0.1
    } else {
        speed
    }
}

impl Jetpack {
    pub fn new() -> Self {
        let mut flames = [ParticleEmitter::jetpack(), ParticleEmitter::jetpack()];
        flames[1].set_brightness(NOZZLE_LIGHT);
        Jetpack { flames, last_z: None }
    }

    /// Where the nozzles are for a player standing at `feet` and facing `facing` (the sprite
    /// facing, a unit vector in the screen-aligned game space of `sprites::facing_of`).
    pub fn nozzles(feet: Vec3, facing: [f32; 2]) -> [Vec3; 2] {
        // Back to the ground frame: screen right is (+x, -y), screen down is (+x, +y).
        let ahead = glam::Vec2::new(facing[0] + facing[1], facing[1] - facing[0]).normalize_or_zero();
        let ahead = if ahead == glam::Vec2::ZERO { glam::Vec2::new(1.0, 1.0).normalize() } else { ahead };
        let side = glam::Vec2::new(ahead.y, -ahead.x);
        let back = feet + Vec3::new(-ahead.x, -ahead.y, 0.0) * NOZZLE_BEHIND + Vec3::Z * NOZZLE_HEIGHT;
        [back + side.extend(0.0) * NOZZLE_SIDE, back - side.extend(0.0) * NOZZLE_SIDE]
    }

    /// Advance by `dt` seconds. `burning` is the player's feet and facing while the jetpack burns,
    /// `None` otherwise.
    pub fn update(&mut self, dt: f32, particles: &mut Particles, burning: Option<(Vec3, [f32; 2])>) {
        let z = burning.map(|(feet, _)| feet.z);
        let vertical = match (z, self.last_z) {
            (Some(z), Some(last)) if dt > 0.0 => (z - last) / dt,
            _ => 0.0,
        };
        self.last_z = z;
        let exhaust = exhaust_speed(vertical);
        let positions = burning.map(|(feet, facing)| Self::nozzles(feet, facing));
        for (i, flame) in self.flames.iter_mut().enumerate() {
            flame.active = positions.is_some();
            if let Some(positions) = positions {
                flame.position = positions[i];
                flame.velocity.z = exhaust;
            }
            flame.update(dt, particles);
        }
    }

    /// The glow of the nozzles that have one, while the jetpack burns.
    pub fn lights(&self) -> impl Iterator<Item = PointLight> + '_ {
        self.flames.iter().filter_map(|f| f.light())
    }
}

/// Depth units that put a billboard's wall (see `sprites::billboard_point`) back to the depth of its
/// anchor at a zero offset: a particle sits in the world where it is, not in front of it.
const DEPTH_BIAS: f32 = -(1.0 + sprites::DEPTH_Z * sprites::SCREEN_Y / sprites::SCREEN_Z);

/// The sprite every Java particle wears (`new Particle((byte) 22)`: entity 22, a soft blob), tinted by
/// the particle's colour.
const PARTICLE_SPRITE: u8 = 22;

/// The six vertices (two triangles) of one particle: a [`FACE_BILLBOARD`] square. Its vertices are
/// the particle's position with the corner's screen offset in pixels, and the shader makes the
/// square face the camera, so particles stay flat on the screen when the free camera turns.
/// With the atlas the square shows the particle sprite, scaled like the Java sprite (its picture
/// is smaller than its box); without it the square is flat colour.
pub fn quad(p: &Particle, sprites: Option<&Sprites>) -> [Vertex; 6] {
    let alpha = p.color()[3].clamp(0.0, 1.0);
    let art = sprites.and_then(|s| s.entity(PARTICLE_SPRITE, 0).map(|region| (s, region)));
    // The picture fills only part of the 200 pixel box of the Java sprite.
    let fill = art.map_or(1.0, |(_, r)| r.w.max(r.h) as f32 / r.orig_w as f32);
    let half = 0.5 * p.size() * fill;
    let (sin, cos) = p.rotation().to_radians().sin_cos();
    let c = p.position;
    // Corner in screen units (1 = a block's width), rotated, as pixels from the centre (y down).
    let corner = |u: f32, v: f32| {
        let (ru, rv) = (u * cos - v * sin, u * sin + v * cos);
        let mut vertex = Vertex::flat(
            c.to_array(),
            [p.color()[0], p.color()[1], p.color()[2]],
            [FACE_BILLBOARD, 1.0 - alpha],
            [ru * half * BLOCK_WIDTH_PX, -rv * half * BLOCK_WIDTH_PX, DEPTH_BIAS],
        );
        if let Some((sprites, region)) = art {
            let page = &sprites.atlas.pages[region.page];
            let (u0, u1) = (region.x as f32 / page.width as f32, (region.x + region.w) as f32 / page.width as f32);
            let (v0, v1) = (region.y as f32 / page.height as f32, (region.y + region.h) as f32 / page.height as f32);
            vertex.uv = [if u < 0.0 { u0 } else { u1 }, if v < 0.0 { v1 } else { v0 }];
            vertex.layer = region.page as f32;
        }
        vertex
    };
    let (a, b, cc, d) = (corner(-1.0, -1.0), corner(1.0, -1.0), corner(1.0, 1.0), corner(-1.0, 1.0));
    [a, b, cc, a, cc, d]
}

/// Append all particles to `out`.
pub fn append(particles: &Particles, sprites: Option<&Sprites>, out: &mut Vec<Vertex>) {
    out.reserve(particles.len() * 6);
    for p in particles.iter() {
        out.extend_from_slice(&quad(p, sprites));
    }
}

/// All particles as vertices, ready to upload and draw with the block pipeline.
#[cfg(test)]
fn vertices(particles: &Particles) -> Vec<Vertex> {
    let mut out = Vec::new();
    append(particles, None, &mut out);
    out
}

#[cfg(test)]
mod tests {
    use glam::Vec3;
    use wurfel_sim::particle::{ParticleSpec, ParticleType};

    use super::*;
    use crate::mesh::FACE_BILLBOARD;

    fn spawn(spec: ParticleSpec, at: Vec3) -> Particles {
        let mut particles = Particles::new(8, 1);
        assert!(particles.spawn(&spec, at, Vec3::ZERO, Vec3::ZERO));
        particles
    }

    fn screen(v: &Vertex) -> (f32, f32) {
        let [x, y, z] = sprites::billboard_corner(v);
        ((x - y) * 100.0, (x + y) * 50.0 - z * 122.0)
    }

    #[test]
    fn a_burning_jetpack_puts_visible_flame_quads_behind_the_player() {
        let mut particles = Particles::new(256, 1);
        let mut jetpack = Jetpack::new();
        let feet = Vec3::new(3.0, 4.0, 5.0);
        jetpack.update(0.2, &mut particles, None);
        assert!(vertices(&particles).is_empty(), "nothing while it is off");
        jetpack.update(0.2, &mut particles, Some((feet, [0.0, 1.0])));
        particles.update(&wurfel_sim::World::new(wurfel_sim::generator::AirGenerator), 1.0 / 60.0);
        let out = vertices(&particles);
        assert!(!out.is_empty());
        for q in out.chunks(6) {
            let (a, c) = (screen(&q[0]), screen(&q[2]));
            assert!((a.0 - c.0).abs() + (a.1 - c.1).abs() > 1.0, "quad has area");
            assert!(q[0].color.iter().any(|&v| v > 0.0), "not black");
        }
    }

    #[test]
    fn the_jetpack_has_two_nozzles_left_and_right_of_the_back() {
        let feet = Vec3::new(3.0, 4.0, 5.0);
        let screen_of = |p: Vec3| ((p.x - p.y) * 100.0, (p.x + p.y) * 50.0 - p.z * 122.0);
        // Facing south (down the screen): the back is up the screen, the nozzles side by side.
        let [a, b] = Jetpack::nozzles(feet, [0.0, 1.0]);
        let (sa, sb) = (screen_of(a), screen_of(b));
        assert!((sa.0 - sb.0).abs() > 40.0, "apart horizontally on screen: {sa:?} {sb:?}");
        assert!((sa.1 - sb.1).abs() < 0.01, "at the same height on screen: {sa:?} {sb:?}");
        assert!(((a.z + b.z) / 2.0 - (feet.z + 0.5)).abs() < 1e-5, "half a block above the feet");
        let middle = screen_of(feet);
        assert!(((sa.0 + sb.0) / 2.0 - middle.0).abs() < 1.0, "centred behind the player");
        assert!((sa.1 + sb.1) / 2.0 < middle.1, "behind a player who faces the viewer is further up the screen");
        // Facing away they swap sides of the back, which now lies below the feet on screen.
        let [c, d] = Jetpack::nozzles(feet, [0.0, -1.0]);
        assert!((screen_of(c).1 + screen_of(d).1) / 2.0 > (sa.1 + sb.1) / 2.0, "the back is now down the screen");
        assert!(((screen_of(c).0 + screen_of(d).0) / 2.0 - middle.0).abs() < 1.0);
    }

    #[test]
    fn the_flame_goes_against_the_vertical_movement_and_only_sinks_when_not_rising() {
        assert_eq!(exhaust_speed(3.0), -4.5);
        assert_eq!(exhaust_speed(0.0), 0.0, "hovering: the flame stays where it is born");
        assert_eq!(exhaust_speed(-2.0), -0.1, "falling: the flame just sinks");
        let mut particles = Particles::new(256, 1);
        let mut jetpack = Jetpack::new();
        jetpack.update(0.1, &mut particles, Some((Vec3::new(0.0, 0.0, 1.0), [0.0, 1.0])));
        jetpack.update(0.1, &mut particles, Some((Vec3::new(0.0, 0.0, 1.3), [0.0, 1.0])));
        assert!(jetpack.flames.iter().all(|f| (f.velocity.z + 4.5).abs() < 1e-3), "rising at 3 blocks/s");
    }

    #[test]
    fn only_the_second_nozzle_glows_and_only_while_burning() {
        let mut particles = Particles::new(64, 1);
        let mut jetpack = Jetpack::new();
        assert_eq!(jetpack.lights().count(), 0);
        jetpack.update(0.05, &mut particles, Some((Vec3::ZERO, [0.0, 1.0])));
        assert_eq!(jetpack.lights().count(), 1);
        jetpack.update(0.05, &mut particles, None);
        assert_eq!(jetpack.lights().count(), 0);
    }

    #[test]
    fn six_vertices_per_particle() {
        let mut particles = Particles::new(8, 1);
        particles.burst(&ParticleSpec::regular(), Vec3::ZERO, 5, Vec3::ZERO, Vec3::ZERO);
        assert_eq!(vertices(&particles).len(), 30);
        assert!(vertices(&Particles::new(8, 1)).is_empty());
    }

    #[test]
    fn quad_is_a_square_on_screen_centred_on_the_particle() {
        let particles = spawn(ParticleSpec::regular(), Vec3::new(2.0, 1.0, 4.0));
        let q = quad(particles.iter().next().unwrap(), None);
        let pts: Vec<(f32, f32)> = q.iter().map(screen).collect();
        let (cx, cy) = ((2.0 - 1.0) * 100.0, (2.0 + 1.0) * 50.0 - 4.0 * 122.0);
        let (minx, maxx) = pts.iter().fold((f32::MAX, f32::MIN), |(a, b), p| (a.min(p.0), b.max(p.0)));
        let (miny, maxy) = pts.iter().fold((f32::MAX, f32::MIN), |(a, b), p| (a.min(p.1), b.max(p.1)));
        // Edge 0.3 blocks wide = 60 px; rotated by some angle the bounding box is at least that.
        let side = 0.3 * 200.0;
        let d01 = ((pts[1].0 - pts[0].0).powi(2) + (pts[1].1 - pts[0].1).powi(2)).sqrt();
        let d12 = ((pts[2].0 - pts[1].0).powi(2) + (pts[2].1 - pts[1].1).powi(2)).sqrt();
        assert!((d01 - side).abs() < 0.01 && (d12 - side).abs() < 0.01, "sides {d01} {d12}");
        assert!(((minx + maxx) / 2.0 - cx).abs() < 0.01 && ((miny + maxy) / 2.0 - cy).abs() < 0.01);
    }

    #[test]
    fn vertices_are_unlit_and_carry_the_particle_colour() {
        let mut spec = ParticleSpec::regular();
        spec.color = [0.1, 0.2, 0.3, 1.0];
        let particles = spawn(spec, Vec3::ZERO);
        for v in quad(particles.iter().next().unwrap(), None) {
            assert_eq!(v.color, [0.1, 0.2, 0.3]);
            assert_eq!(v.shade, [FACE_BILLBOARD, 0.0]);
        }
    }

    #[test]
    fn a_shaded_variant_darkens_the_vertex_colour() {
        let mut particles = Particles::new(8, 1);
        let spec = ParticleSpec { cycle_distance: 0.06, collides: false, gravity: 0.0, ..ParticleSpec::regular() };
        assert!(particles.spawn(&spec, Vec3::new(0.0, 0.0, 9.0), Vec3::new(3.0, 0.0, 0.0), Vec3::ZERO));
        let flat = wurfel_sim::World::new(wurfel_sim::generator::AirGenerator);
        let mut shades = Vec::new();
        for _ in 0..120 {
            particles.update(&flat, 1.0 / 60.0);
            let p = particles.iter().next().unwrap();
            let expected = 0.5 * wurfel_sim::particle::SHADES[p.variant() as usize];
            assert_eq!(quad(p, None)[0].color, [expected; 3]);
            shades.push(p.variant());
        }
        assert!(shades.iter().any(|&v| v != 0), "the shade should have changed while moving");
    }

    #[test]
    fn fading_particle_is_see_through_not_smaller() {
        let mut spec = ParticleSpec::regular();
        spec.kind = ParticleType::Regular;
        spec.color[3] = 0.25;
        let faded = spawn(spec, Vec3::ZERO);
        spec.color[3] = 1.0;
        let full = spawn(spec, Vec3::ZERO);
        let first = |p: &Particles| quad(p.iter().next().unwrap(), None);
        assert_eq!(first(&faded)[0].point, first(&full)[0].point, "same size");
        assert_eq!(first(&full)[0].shade[1], 0.0, "opaque");
        assert!((first(&faded)[0].shade[1] - 0.75).abs() < 1e-3, "a quarter opaque");
    }

    #[test]
    fn with_the_atlas_a_particle_wears_the_blob_sprite_inside_its_region() {
        let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/sprites/sprites.atlas")).unwrap();
        let sprites = Sprites::new(crate::atlas::Atlas::parse(&text).unwrap());
        let region = sprites.entity(PARTICLE_SPRITE, 0).expect("the atlas has the particle sprite");
        let page = &sprites.atlas.pages[region.page];
        let particles = spawn(ParticleSpec::regular(), Vec3::ZERO);
        let q = quad(particles.iter().next().unwrap(), Some(&sprites));
        let (u0, u1) = (region.x as f32 / page.width as f32, (region.x + region.w) as f32 / page.width as f32);
        for v in &q {
            assert_eq!(v.layer, region.page as f32);
            assert!(v.uv[0] >= u0 - 1e-6 && v.uv[0] <= u1 + 1e-6, "inside the region, not its neighbours");
        }
        let flat = quad(particles.iter().next().unwrap(), None);
        assert!(flat[0].layer < 0.0 && (q[0].point[0].abs() < flat[0].point[0].abs()), "the picture fills part of its box");
    }
}
