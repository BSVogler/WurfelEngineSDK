//! Turns the simulation's particles ([`wurfel_sim::particle`]) into vertices for the existing
//! block pipeline: one screen-aligned, rotated square per particle (the Java particles were
//! camera-facing sprites), flagged `FACE_UNLIT` so the light engine leaves the colour alone.
//!
//! The pipeline draws opaque, so opacity cannot be shown as transparency yet: a fading particle
//! shrinks with its alpha instead (see [`SHRINK_WITH_ALPHA`]). True blending needs an alpha
//! channel in the vertex and a blend state in a second pipeline.

use glam::Vec3;
use wurfel_sim::particle::{Particle, ParticleEmitter, Particles};

use crate::mesh::{Vertex, FACE_UNLIT};

/// Draw fading particles smaller instead of transparent (the pipeline has no blending).
pub const SHRINK_WITH_ALPHA: bool = true;

/// Screen width of one block in the projection, and the height of one block on screen: a square
/// on screen needs `WIDTH / HEIGHT` times its width as vertical extent in `z`.
const BLOCK_WIDTH_PX: f32 = 200.0;
const BLOCK_HEIGHT_PX: f32 = 122.0;

/// Switch the jetpack exhaust on at the player's feet, or off when the jetpack does not burn.
pub fn light_jetpack(flame: &mut ParticleEmitter, feet: Option<Vec3>) {
    flame.active = feet.is_some();
    if let Some(feet) = feet {
        flame.position = feet;
    }
}

/// The six vertices (two triangles) of one particle.
pub fn quad(p: &Particle) -> [Vertex; 6] {
    let alpha = p.color()[3].clamp(0.0, 1.0);
    let half = 0.5 * p.size() * if SHRINK_WITH_ALPHA { alpha.sqrt() } else { 1.0 };
    let (sin, cos) = p.rotation().to_radians().sin_cos();
    let c = p.position;
    // Corner in screen units (1 = a block's width), rotated; screen right is (+x, -y) in the
    // ground frame and screen up is +z.
    let corner = |u: f32, v: f32| {
        let (ru, rv) = (u * cos - v * sin, u * sin + v * cos);
        let k = ru * half;
        Vertex::flat(
            [c.x + k, c.y - k, c.z + rv * half * BLOCK_WIDTH_PX / BLOCK_HEIGHT_PX],
            [p.color()[0], p.color()[1], p.color()[2]],
            [FACE_UNLIT, 0.0],
            [0.0; 3],
        )
    };
    let (a, b, cc, d) = (corner(-1.0, -1.0), corner(1.0, -1.0), corner(1.0, 1.0), corner(-1.0, 1.0));
    [a, b, cc, a, cc, d]
}

/// Append all particles to `out`.
pub fn append(particles: &Particles, out: &mut Vec<Vertex>) {
    out.reserve(particles.len() * 6);
    for p in particles.iter() {
        out.extend_from_slice(&quad(p));
    }
}

/// All particles as vertices, ready to upload and draw with the block pipeline.
#[cfg(test)]
fn vertices(particles: &Particles) -> Vec<Vertex> {
    let mut out = Vec::new();
    append(particles, &mut out);
    out
}

#[cfg(test)]
mod tests {
    use glam::Vec3;
    use wurfel_sim::particle::{ParticleSpec, ParticleType};

    use super::*;

    fn spawn(spec: ParticleSpec, at: Vec3) -> Particles {
        let mut particles = Particles::new(8, 1);
        assert!(particles.spawn(&spec, at, Vec3::ZERO, Vec3::ZERO));
        particles
    }

    fn screen(v: &Vertex) -> (f32, f32) {
        let [x, y, z] = v.position;
        ((x - y) * 100.0, (x + y) * 50.0 - z * 122.0)
    }

    #[test]
    fn a_burning_jetpack_puts_visible_flame_quads_under_the_player() {
        let mut particles = Particles::new(64, 1);
        let mut flame = ParticleEmitter::jetpack();
        let feet = Vec3::new(3.0, 4.0, 5.0);
        light_jetpack(&mut flame, None);
        assert_eq!(flame.update(0.2, &mut particles), 0);
        light_jetpack(&mut flame, Some(feet));
        assert!(flame.update(0.2, &mut particles) > 0);
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
    fn six_vertices_per_particle() {
        let mut particles = Particles::new(8, 1);
        particles.burst(&ParticleSpec::regular(), Vec3::ZERO, 5, Vec3::ZERO, Vec3::ZERO);
        assert_eq!(vertices(&particles).len(), 30);
        assert!(vertices(&Particles::new(8, 1)).is_empty());
    }

    #[test]
    fn quad_is_a_square_on_screen_centred_on_the_particle() {
        let particles = spawn(ParticleSpec::regular(), Vec3::new(2.0, 1.0, 4.0));
        let q = quad(particles.iter().next().unwrap());
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
        for v in quad(particles.iter().next().unwrap()) {
            assert_eq!(v.color, [0.1, 0.2, 0.3]);
            assert_eq!(v.shade, [FACE_UNLIT, 0.0]);
            assert_eq!(v.point, [0.0; 3]);
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
            assert_eq!(quad(p)[0].color, [expected; 3]);
            shades.push(p.variant());
        }
        assert!(shades.iter().any(|&v| v != 0), "the shade should have changed while moving");
    }

    #[test]
    fn fading_particle_shrinks() {
        let mut spec = ParticleSpec::regular();
        spec.kind = ParticleType::Regular;
        spec.color[3] = 0.25;
        let faded = spawn(spec, Vec3::ZERO);
        spec.color[3] = 1.0;
        let full = spawn(spec, Vec3::ZERO);
        let width = |p: &Particles| {
            let q = quad(p.iter().next().unwrap());
            ((q[1].position[0] - q[0].position[0]).powi(2) * 2.0 + (q[1].position[2] - q[0].position[2]).powi(2)).sqrt()
        };
        assert!(width(&faded) < width(&full) * 0.6);
    }
}
