use std::cell::RefCell;

use super::{splitmix64, Generator};
use crate::block::{id, Block};
use crate::grid::{lower_left, lower_right, row_offset, to_iso};
use crate::block::id::TREE;
use crate::CHUNK_SIZE_Z;

/// Terraced highlands with sheer cliffs, lakes and natural arches. Not from the Java engine.
///
/// The ground is a Perlin heightmap squeezed into steps: wide plateaus joined by one or two blocks of
/// steep slope, which shows up as cliff faces of bare stone. Arches are tunnels along the zero line
/// of a second noise field, with an elliptical cross section whose height changes slowly along the
/// way. Where the rock above a tunnel is thick enough it stays as a bridge, where it is not the
/// tunnel is a canyon.
///
/// Nothing floats, by construction rather than by luck: every column is solid from the bottom up to
/// its ground height, and a tunnel only removes blocks between a solid floor and the ground
/// height. Whatever is left above a tunnel touches the full columns beside it. The generator stays
/// a pure function of the coordinates, so no chunk needs to know about its neighbours.
pub struct TerrainGenerator {
    seed: u64,
}

/// Water fills everything below this layer.
pub const SEA_LEVEL: i32 = 4;
/// Height of one terrace step.
const STEP: f32 = 5.5;
/// Lowest ground height (the bedrock layer is part of it).
const MIN_GROUND: i32 = 1;
/// Highest ground height, kept below the top of the world so a roof still has air above it.
const MAX_GROUND: i32 = CHUNK_SIZE_Z - 3;

/// Everything about one block column that the blocks in it depend on.
#[derive(Clone, Copy)]
struct Column {
    /// Blocks `z < ground` are rock.
    ground: i32,
    /// How far the lowest of the four neighbouring grounds is below this one.
    drop: i32,
    /// Where the column holds a tunnel; `None` when it does not.
    tunnel: Option<Tunnel>,
}

#[derive(Clone, Copy)]
struct Tunnel {
    floor: i32,
    roof: i32,
}

thread_local! {
    /// The last column asked for. A chunk asks for all layers of a column in a row, so this saves
    /// the noise from being evaluated once per block.
    static LAST: RefCell<Option<(u64, i32, i32, Column)>> = const { RefCell::new(None) };
}

impl TerrainGenerator {
    pub fn new(seed: u64) -> Self {
        // Mix once so that neighbouring seeds give unrelated worlds.
        let mut state = seed;
        Self { seed: splitmix64(&mut state) }
    }

    /// The height of the ground in a column, without arches. The first block above rock is at `ground`.
    pub fn ground(&self, x: i32, y: i32) -> i32 {
        let (gx, gy) = to_iso(x, y);
        self.ground_at(gx, gy)
    }

    fn ground_at(&self, gx: f32, gy: f32) -> i32 {
        // Broad shape plus a little detail, stretched so that the middle of the range is also reached.
        let n = self.fbm(gx * 0.022, gy * 0.022, 4, 0) * 0.5 + 0.5;
        let level = ((n - 0.5) * 1.9 + 0.5).clamp(0.0, 1.0) * 5.0;
        let (floor, frac) = (level.floor(), level - level.floor());
        // Flat on both ends, steep in the middle: the step is the cliff.
        let s = smoothstep(smoothstep(smoothstep(frac)));
        let height = 1.0 + (floor + s) * STEP;
        (height.round() as i32).clamp(MIN_GROUND, MAX_GROUND)
    }

    fn column(&self, x: i32, y: i32) -> Column {
        if let Some(column) = LAST.with(|last| match *last.borrow() {
            Some((seed, cx, cy, column)) if seed == self.seed && cx == x && cy == y => Some(column),
            _ => None,
        }) {
            return column;
        }

        let (gx, gy) = to_iso(x, y);
        let ground = self.ground_at(gx, gy);
        let off = row_offset(y);
        let neighbours = [lower_left(x, y), lower_right(x, y), (x - 1 + off, y - 1), (x + off, y - 1)];
        let lowest = neighbours.iter().map(|&(nx, ny)| self.ground(nx, ny)).min().unwrap_or(ground);
        let mut tunnel = self.tunnel_at(gx, gy, ground);

        // The rock over a tunnel must touch a column next to it at the height of its underside,
        // or it would hang in the air (a cliff edge right beside a bridge does that). Such a
        // tunnel is left out here. That only adds rock, so no other column loses its support.
        // Of two tunnel columns with the same roof the one that comes first in (roof, y, x) order
        // holds the other, so two of them cannot only hold each other.
        if let Some(t) = tunnel.filter(|t| t.roof + 1 < ground) {
            let z = t.roof + 1;
            let held = neighbours.iter().any(|&(nx, ny)| {
                let (ngx, ngy) = to_iso(nx, ny);
                let n_ground = self.ground_at(ngx, ngy);
                z < n_ground
                    && self.tunnel_at(ngx, ngy, n_ground).is_none_or(|n| (n.roof, ny, nx) < (t.roof, y, x))
            });
            if !held {
                tunnel = None;
            }
        }
        let column = Column { ground, drop: ground - lowest, tunnel };

        LAST.with(|last| *last.borrow_mut() = Some((self.seed, x, y, column)));
        column
    }

    fn tunnel_at(&self, gx: f32, gy: f32, ground: i32) -> Option<Tunnel> {
        // Tunnels only exist in some regions, so the highlands also have unbroken stretches.
        let region = self.fbm(gx * 0.012, gy * 0.012, 2, 3);
        if region < -0.15 {
            return None;
        }
        // The tunnel runs along the line where this noise is zero.
        let line = self.fbm(gx * 0.035, gy * 0.035, 2, 1);
        let half_width = 0.045 + 0.03 * (region + 0.15).min(0.5);
        let dist = line.abs();
        if dist >= half_width {
            return None;
        }
        // Centre height and radius change slowly along the tunnel.
        let center = 7.0 + 5.0 * (self.fbm(gx * 0.02, gy * 0.02, 1, 2) * 0.5 + 0.5) * 2.0;
        let radius = 3.0 + 1.5 * (self.fbm(gx * 0.03, gy * 0.03, 1, 4) * 0.5 + 0.5);
        // Ellipse: full height in the middle of the line, closing to the sides.
        let across = dist / half_width;
        let reach = radius * (1.0 - across * across).sqrt();
        let floor = (center - radius).round() as i32;
        let roof = (center + reach).round() as i32;
        // A floor needs rock under it, a tunnel needs something to cut through.
        if floor < SEA_LEVEL + 1 || floor >= ground || roof <= floor {
            return None;
        }
        Some(Tunnel { floor, roof })
    }

    /// Fractal Perlin noise in -1..1.
    fn fbm(&self, x: f32, y: f32, octaves: u32, stream: u32) -> f32 {
        let (mut sum, mut amplitude, mut norm, mut frequency) = (0.0, 1.0, 0.0, 1.0);
        for octave in 0..octaves {
            sum += amplitude * self.perlin(x * frequency, y * frequency, stream * 8 + octave);
            norm += amplitude;
            amplitude *= 0.5;
            frequency *= 2.0;
        }
        sum / norm
    }

    /// Classic 2D gradient noise in about -1..1.
    fn perlin(&self, x: f32, y: f32, stream: u32) -> f32 {
        let (x0, y0) = (x.floor(), y.floor());
        let (fx, fy) = (x - x0, y - y0);
        let (ix, iy) = (x0 as i32, y0 as i32);
        let corner = |dx: i32, dy: i32| {
            let angle = self.hash(ix + dx, iy + dy, stream) as f32 / u32::MAX as f32 * std::f32::consts::TAU;
            angle.cos() * (fx - dx as f32) + angle.sin() * (fy - dy as f32)
        };
        let (u, v) = (fade(fx), fade(fy));
        let top = lerp(corner(0, 0), corner(1, 0), u);
        let bottom = lerp(corner(0, 1), corner(1, 1), u);
        // A unit gradient field peaks at sqrt(2)/2.
        (lerp(top, bottom, v) * std::f32::consts::SQRT_2).clamp(-1.0, 1.0)
    }

    fn hash(&self, x: i32, y: i32, stream: u32) -> u32 {
        let mut h = self.seed ^ (x as u32 as u64) << 32 ^ (y as u32 as u64) ^ (stream as u64) << 52;
        h = h.wrapping_add(0x9E37_79B9_7F4A_7C15);
        h = (h ^ (h >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        h = (h ^ (h >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        ((h ^ (h >> 31)) >> 32) as u32
    }

    /// Does a tree stand on top of this column? Only on grass that is dry, level and not carved
    /// away, in groves: a slow noise decides how dense the trees are, a hash picks the columns.
    fn has_tree(&self, x: i32, y: i32, column: &Column) -> bool {
        if column.ground <= SEA_LEVEL + 1
            || column.ground >= MAX_GROUND - 3
            || column.drop > 1
            || column.tunnel.is_some_and(|t| t.roof + 1 >= column.ground)
        {
            return false;
        }
        let (gx, gy) = to_iso(x, y);
        let grove = self.fbm(gx * 0.05, gy * 0.05, 2, 5) * 0.5 + 0.5;
        // From nothing in the clearings to about one column in five in the thick of a grove.
        let density = ((grove - 0.45) * 0.9).clamp(0.0, 0.2);
        (self.hash(x, y, 6) as f32 / u32::MAX as f32) < density
    }

    /// Is this a dry, flat, grassy column to start on?
    fn good_spawn(&self, x: i32, y: i32) -> bool {
        let column = self.column(x, y);
        column.ground > SEA_LEVEL + 1 && column.drop <= 1 && column.tunnel.is_none() && !self.has_tree(x, y, &column)
    }
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

fn fade(t: f32) -> f32 {
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}

fn smoothstep(t: f32) -> f32 {
    t * t * (3.0 - 2.0 * t)
}

impl Generator for TerrainGenerator {
    fn generate(&self, x: i32, y: i32, z: i32) -> Block {
        if z < 0 || z >= CHUNK_SIZE_Z {
            return Block::AIR;
        }
        let column = self.column(x, y);

        if z == column.ground && self.has_tree(x, y, &column) {
            return Block::new(TREE, 0);
        }
        if z >= column.ground || column.tunnel.is_some_and(|t| z >= t.floor && z <= t.roof) {
            return if z <= SEA_LEVEL { Block::new(id::WATER, 0) } else { Block::AIR };
        }
        if z == 0 {
            return Block::new(id::STONE, 0);
        }

        let top = z == column.ground - 1;
        let beach = column.ground <= SEA_LEVEL + 1;
        // Where the ground falls away steeply the layers show as a cliff face.
        let cliff = column.drop >= 3 && z >= column.ground - column.drop;
        let peak = column.ground >= MAX_GROUND - 3;
        if beach {
            Block::new(id::SAND, 0)
        } else if cliff || peak || z < column.ground - 3 {
            Block::new(id::STONE, 0)
        } else if top {
            Block::new(id::GRASS, 0)
        } else {
            Block::new(id::DIRT, 0)
        }
    }

    fn spawn_point(&self) -> (i32, i32) {
        // The nearest dry flat column to the origin, searching in growing squares.
        for radius in 0..200i32 {
            for dx in -radius..=radius {
                for dy in -radius..=radius {
                    if dx.abs().max(dy.abs()) == radius && self.good_spawn(dx, dy) {
                        return (dx, dy);
                    }
                }
            }
        }
        (0, 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generator::create_generator;
    use std::collections::{HashSet, VecDeque};

    fn solid(g: &TerrainGenerator, x: i32, y: i32, z: i32) -> bool {
        !matches!(g.generate(x, y, z).id(), id::AIR | id::WATER)
    }

    /// The six blocks that touch a block: above, below and the four around it on the staggered grid.
    fn touching(x: i32, y: i32, z: i32) -> [(i32, i32, i32); 6] {
        let off = row_offset(y);
        [
            (x, y, z + 1),
            (x, y, z - 1),
            (x - 1 + off, y + 1, z),
            (x + off, y + 1, z),
            (x - 1 + off, y - 1, z),
            (x + off, y - 1, z),
        ]
    }

    #[test]
    fn nothing_floats_over_a_large_area() {
        for seed in [1, 77] {
            let g = TerrainGenerator::new(seed);
            let (w, h, margin) = (160, 320, 12);
            // Everything solid, reached from the bedrock through touching solid blocks.
            let mut reached = HashSet::new();
            let mut queue = VecDeque::new();
            for x in 0..w {
                for y in 0..h {
                    if solid(&g, x, y, 0) {
                        reached.insert((x, y, 0));
                        queue.push_back((x, y, 0));
                    }
                }
            }
            while let Some((x, y, z)) = queue.pop_front() {
                for (nx, ny, nz) in touching(x, y, z) {
                    if (0..w).contains(&nx) && (0..h).contains(&ny) && (0..CHUNK_SIZE_Z).contains(&nz)
                        && solid(&g, nx, ny, nz) && reached.insert((nx, ny, nz))
                    {
                        queue.push_back((nx, ny, nz));
                    }
                }
            }
            let mut floating = Vec::new();
            for x in margin..w - margin {
                for y in margin..h - margin {
                    for z in 0..CHUNK_SIZE_Z {
                        if solid(&g, x, y, z) && !reached.contains(&(x, y, z)) {
                            floating.push((x, y, z));
                        }
                    }
                }
            }
            assert!(floating.is_empty(), "seed {seed}: {} floating blocks, e.g. {:?}", floating.len(), &floating[..floating.len().min(5)]);
        }
    }

    #[test]
    fn it_makes_cliffs_lakes_and_arches() {
        let g = TerrainGenerator::new(5);
        let (mut cliffs, mut lakes, mut arches, mut tall) = (0, 0, 0, 0);
        for x in 0..300 {
            for y in 0..600 {
                let c = g.column(x, y);
                cliffs += (c.drop >= 4) as u32;
                lakes += (c.ground <= SEA_LEVEL) as u32;
                tall += (c.ground >= 20) as u32;
                // A bridge: a tunnel with rock left over it.
                arches += c.tunnel.is_some_and(|t| t.roof + 1 < c.ground) as u32;
            }
        }
        assert!(cliffs > 500, "cliff columns: {cliffs}");
        assert!(lakes > 2000, "lake columns: {lakes}");
        assert!(tall > 2000, "high columns: {tall}");
        assert!(arches > 200, "arch columns: {arches}");
    }

    #[test]
    fn blocks_stay_inside_the_world_and_are_sensible() {
        let g = TerrainGenerator::new(9);
        for x in -50..50 {
            for y in -50..50 {
                assert_eq!(g.generate(x, y, 0).id(), id::STONE, "bedrock");
                assert!(g.generate(x, y, CHUNK_SIZE_Z - 1).is_air() || g.generate(x, y, CHUNK_SIZE_Z - 1).id() == id::WATER);
                assert!(g.generate(x, y, CHUNK_SIZE_Z).is_air());
                assert!(g.generate(x, y, -1).is_air());
            }
        }
    }

    #[test]
    fn is_a_pure_function_of_the_seed() {
        let (a, b, c) = (TerrainGenerator::new(3), TerrainGenerator::new(3), TerrainGenerator::new(4));
        let mut differs = false;
        for x in -40..40 {
            for y in -40..40 {
                for z in 0..CHUNK_SIZE_Z {
                    assert_eq!(a.generate(x, y, z), b.generate(x, y, z));
                    differs |= a.generate(x, y, z) != c.generate(x, y, z);
                }
            }
        }
        assert!(differs);
        // The column cache must not make the answer depend on the order of the questions.
        assert_eq!(a.generate(5, 6, 7), a.generate(5, 6, 7));
        let first = a.generate(1, 1, 9);
        a.generate(30, 30, 9);
        assert_eq!(a.generate(1, 1, 9), first);
    }

    #[test]
    fn players_start_on_dry_flat_ground() {
        for seed in 0..8 {
            let g = TerrainGenerator::new(seed);
            let (x, y) = g.spawn_point();
            let ground = g.ground(x, y);
            assert!(ground > SEA_LEVEL + 1, "seed {seed}: spawn in water at {x},{y}");
            assert!(solid(&g, x, y, ground - 1) && !solid(&g, x, y, ground));
        }
    }

    #[test]
    fn trees_grow_in_groves_on_grass() {
        let g = TerrainGenerator::new(5);
        let mut trees = 0;
        for x in 0..300 {
            for y in 0..600 {
                let ground = g.ground(x, y);
                if g.generate(x, y, ground).id() == TREE {
                    trees += 1;
                    assert_eq!(g.generate(x, y, ground - 1).id(), id::GRASS, "tree at {x},{y} not on grass");
                    assert!(g.generate(x, y, ground + 1).is_air());
                }
            }
        }
        assert!(trees > 300, "trees: {trees}");
        assert!(trees < 300 * 600 / 5, "too many trees: {trees}");
    }

    #[test]
    fn the_registry_knows_it() {
        let g = create_generator("terrain", 1).unwrap();
        assert!(g.generate(0, 0, 0).id() == id::STONE);
    }
}
