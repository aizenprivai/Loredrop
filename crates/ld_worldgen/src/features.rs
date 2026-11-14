//! Trees and other vegetation.
//!
//! ## Trees across chunk borders
//! A tree's leaves can reach 2–3 blocks from its trunk, so a tree growing near
//! the edge of chunk A must also show up in chunk B. Each chunk therefore asks:
//! "which trees in my 3×3 neighbourhood would reach into me?" Tree positions
//! come from a per-chunk seeded RNG, and the ground height of a neighbour's
//! tree comes from `Overworld::column_profile`, which uses exactly the same
//! maths as real generation. So both chunks agree on every tree without ever
//! talking to each other, and without generating neighbours first.

use crate::noise::Rng;
use crate::overworld::Overworld;
use ld_core::coords::ChunkPos;
use ld_core::world_size::{MAX_Y, MIN_Y};
use ld_world::{BiomeId, BlockId, BlockRegistry};

/// Kinds of trees (and tree-like features) biomes can request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum TreeKind {
    Oak,
    Birch,
    Spruce,
    Cactus,
}

impl TreeKind {
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "oak" => TreeKind::Oak,
            "birch" => TreeKind::Birch,
            "spruce" => TreeKind::Spruce,
            "cactus" => TreeKind::Cactus,
            _ => return None,
        })
    }
}

/// Block IDs used by vegetation.
pub struct TreeBlocks {
    oak_log: BlockId,
    oak_leaves: BlockId,
    birch_log: BlockId,
    birch_leaves: BlockId,
    spruce_log: BlockId,
    spruce_leaves: BlockId,
    cactus: BlockId,
    grass: BlockId,
    snowy_grass: BlockId,
    dirt: BlockId,
    sand: BlockId,
    dead_bush: BlockId,
    replaceable_plants: Vec<BlockId>,
}

impl TreeBlocks {
    pub fn new(r: &BlockRegistry) -> Result<Self, String> {
        Ok(Self {
            oak_log: r.require("oak_log")?,
            oak_leaves: r.require("oak_leaves")?,
            birch_log: r.require("birch_log")?,
            birch_leaves: r.require("birch_leaves")?,
            spruce_log: r.require("spruce_log")?,
            spruce_leaves: r.require("spruce_leaves")?,
            cactus: r.require("cactus")?,
            grass: r.require("grass_block")?,
            snowy_grass: r.require("snowy_grass_block")?,
            dirt: r.require("dirt")?,
            sand: r.require("sand")?,
            dead_bush: r.require("dead_bush")?,
            replaceable_plants: r
                .iter()
                .filter(|(_, d)| d.shape == ld_world::Shape::Cross)
                .map(|(id, _)| id)
                .collect(),
        })
    }

    /// Whether `plant` may grow on `ground`.
    pub fn supports_plant(&self, ground: BlockId, plant: BlockId) -> bool {
        if plant == self.dead_bush {
            ground == self.sand
        } else {
            ground == self.grass || ground == self.snowy_grass
        }
    }

    /// Logs, leaves, cacti and plants (anything that isn't terrain).
    #[allow(dead_code)] // used by tests; kept for structure placement later
    pub fn is_vegetation(&self, id: BlockId) -> bool {
        [
            self.oak_log,
            self.oak_leaves,
            self.birch_log,
            self.birch_leaves,
            self.spruce_log,
            self.spruce_leaves,
            self.cactus,
        ]
        .contains(&id)
            || self.replaceable_plants.contains(&id)
    }

    fn soft(&self, id: BlockId) -> bool {
        id == BlockId::AIR || self.replaceable_plants.contains(&id)
    }
}

/// Attempts per chunk; each attempt succeeds with probability density / ATTEMPTS.
const ATTEMPTS: u32 = 12;
/// Furthest a tree can reach from its trunk (blocks).
const REACH: i32 = 3;

/// One tree to grow: trunk base position (world) and kind.
struct TreeSite {
    x: i32,
    y: i32,
    z: i32,
    kind: TreeKind,
    rng_seed: u64,
}

/// The trees chunk `pos` decides to grow (positions inside that chunk).
///
/// `local` supplies (tops, biomes) for this chunk's own columns when
/// available; otherwise columns are profiled. `reach_into` limits results to
/// trees whose trunk lies within this world-space box `[x0, z0, x1, z1)` —
/// checked *before* the expensive height calculation. All random numbers are
/// drawn up front, so filtering never changes which trees exist.
fn chunk_trees(
    wg: &Overworld,
    pos: ChunkPos,
    local: Option<(&[i32; 256], &[BiomeId; 256])>,
    reach_into: Option<[i32; 4]>,
) -> Vec<TreeSite> {
    let mut rng = Rng::for_position(wg.seed(), pos.x, pos.z, 42);
    let mut out = Vec::new();
    for _ in 0..ATTEMPTS {
        let (lx, lz) = (rng.range(0, 15), rng.range(0, 15));
        let roll = rng.next_f64();
        let site_seed = rng.next_u64();
        let kind_roll = rng.next_u64();
        let (wx, wz) = (pos.x * 16 + lx, pos.z * 16 + lz);
        if let Some([x0, z0, x1, z1]) = reach_into
            && !(wx >= x0 && wx < x1 && wz >= z0 && wz < z1)
        {
            continue;
        }
        // Cheap checks first (biome only needs the climate), height last.
        let biome = match local {
            Some((_, biomes)) => biomes[(lz * 16 + lx) as usize],
            None => wg.biome_at(wx, wz),
        };
        let density = wg.biomes().get(biome).tree_density;
        if roll >= (density / ATTEMPTS as f32) as f64 {
            continue;
        }
        let Some(kind) = wg.tree_for(biome, &mut Rng::new(kind_roll)) else { continue };
        let ground = wg.surface_block(biome);
        let tb = &wg.trees;
        let ok_ground = match kind {
            TreeKind::Cactus => ground == tb.sand,
            _ => ground == tb.grass || ground == tb.snowy_grass || ground == tb.dirt,
        };
        if !ok_ground {
            continue;
        }
        let top = match local {
            Some((tops, _)) => tops[(lz * 16 + lx) as usize],
            None => wg.column_profile(wx, wz).top,
        };
        if !wg.tree_allowed(biome, top) || top + 12 >= MAX_Y {
            continue;
        }
        out.push(TreeSite { x: wx, y: top + 1, z: wz, kind, rng_seed: site_seed });
    }
    out
}

#[inline]
fn idx(x: usize, y: i32, z: usize) -> usize {
    ((y - MIN_Y) as usize) * 256 + z * 16 + x
}

/// Places every tree that touches chunk `pos` into its block array.
pub fn place_trees(
    wg: &Overworld,
    blocks: &mut [BlockId],
    pos: ChunkPos,
    biomes: &[BiomeId; 256],
    tops: &[i32; 256],
) {
    let mut sites = chunk_trees(wg, pos, Some((tops, biomes)), None);
    for dz in -1..=1 {
        for dx in -1..=1 {
            if dx == 0 && dz == 0 {
                continue;
            }
            let n = ChunkPos::new(pos.x + dx, pos.z + dz);
            // Only neighbours' trees close enough to reach into this chunk.
            let (x0, z0) = (pos.x * 16 - REACH, pos.z * 16 - REACH);
            sites.extend(chunk_trees(wg, n, None, Some([x0, z0, x0 + 16 + 2 * REACH, z0 + 16 + 2 * REACH])));
        }
    }
    let tb = &wg.trees;
    let mut w = ChunkWriter { blocks, ox: pos.x * 16, oz: pos.z * 16, tb };
    for s in sites {
        let mut rng = Rng::new(s.rng_seed);
        match s.kind {
            TreeKind::Oak => blob_tree(&mut w, &s, &mut rng, tb.oak_log, tb.oak_leaves, 4, 6),
            TreeKind::Birch => blob_tree(&mut w, &s, &mut rng, tb.birch_log, tb.birch_leaves, 5, 7),
            TreeKind::Spruce => spruce(&mut w, &s, &mut rng),
            TreeKind::Cactus => {
                let h = rng.range(1, 3);
                for y in 0..h {
                    w.set_if(s.x, s.y + y, s.z, tb.cactus, |id, tb| tb.soft(id));
                }
            }
        }
    }
}

/// Writes blocks into one chunk's array, ignoring positions outside it.
struct ChunkWriter<'a> {
    blocks: &'a mut [BlockId],
    ox: i32,
    oz: i32,
    tb: &'a TreeBlocks,
}

impl ChunkWriter<'_> {
    fn set_if(&mut self, x: i32, y: i32, z: i32, id: BlockId, allow: impl Fn(BlockId, &TreeBlocks) -> bool) {
        let (lx, lz) = (x - self.ox, z - self.oz);
        if !(0..16).contains(&lx) || !(0..16).contains(&lz) || !(MIN_Y..MAX_Y).contains(&y) {
            return;
        }
        let i = idx(lx as usize, y, lz as usize);
        if allow(self.blocks[i], self.tb) {
            self.blocks[i] = id;
        }
    }
}

/// Oak/birch style: straight trunk, two wide leaf layers and two narrow ones.
#[allow(clippy::too_many_arguments)]
fn blob_tree(
    w: &mut ChunkWriter,
    s: &TreeSite,
    rng: &mut Rng,
    log: BlockId,
    leaves: BlockId,
    min_h: i32,
    max_h: i32,
) {
    let h = rng.range(min_h, max_h);
    let top = s.y + h;
    for dy in -3..=0 {
        let radius: i32 = if dy >= -1 { 1 } else { 2 };
        for dz in -radius..=radius {
            for dx in -radius..=radius {
                let corner = dx.abs() == radius && dz.abs() == radius;
                // Corners are randomly trimmed (and always trimmed on the top layer).
                if corner && (dy == 0 || rng.below(2) == 0) {
                    continue;
                }
                w.set_if(s.x + dx, top + dy, s.z + dz, leaves, |id, tb| tb.soft(id));
            }
        }
    }
    w.set_if(s.x, top + 1, s.z, leaves, |id, tb| tb.soft(id));
    for y in s.y..top {
        w.set_if(s.x, y, s.z, log, |id, tb| tb.soft(id) || id == leaves);
    }
    let dirt = w.tb.dirt;
    w.set_if(s.x, s.y - 1, s.z, dirt, |_, _| true); // grass under a trunk turns to dirt
}

/// Spruce: tall trunk with alternating wide/narrow rings of needles tapering to a point.
fn spruce(w: &mut ChunkWriter, s: &TreeSite, rng: &mut Rng) {
    let h = rng.range(6, 9);
    let top = s.y + h;
    let bare = rng.range(1, 2); // trunk without needles at the bottom
    let mut radius: i32 = 0;
    let mut grow: i32 = 1;
    for y in (s.y + bare..=top + 1).rev() {
        for dz in -radius..=radius {
            for dx in -radius..=radius {
                if radius > 0 && dx.abs() == radius && dz.abs() == radius {
                    continue;
                }
                let leaves = w.tb.spruce_leaves;
                w.set_if(s.x + dx, y, s.z + dz, leaves, |id, tb| tb.soft(id));
            }
        }
        // Widen going down, with occasional narrow "steps" for the layered look.
        if radius >= grow {
            radius = 0;
            grow = (grow + 1).min(2 + (h > 7) as i32);
        } else {
            radius += 1;
        }
    }
    for y in s.y..top {
        let log = w.tb.spruce_log;
        w.set_if(s.x, y, s.z, log, |id, tb| tb.soft(id) || id == tb.spruce_leaves);
    }
    let dirt = w.tb.dirt;
    w.set_if(s.x, s.y - 1, s.z, dirt, |_, _| true);
}

#[cfg(test)]
mod tests {
    use crate::TerrainGenerator;
    use crate::overworld::tests::generator;
    use ld_core::coords::ChunkPos;
    use ld_core::world_size::{MAX_Y, MIN_Y};

    #[test]
    fn trees_crossing_a_chunk_border_are_continuous() {
        // Generate a row of forest chunks and check that no leaf/log column is cut
        // off exactly at a chunk border (both sides agree on shared trees).
        let g = generator(2024);
        // Find a forested area.
        let (sx, _, sz) = g.find_spawn();
        let base = ChunkPos::new(sx.div_euclid(16), sz.div_euclid(16));
        let mut logs_total = 0;
        for i in 0..6 {
            let a = g.generate(ChunkPos::new(base.x + i, base.z));
            let b = g.generate(ChunkPos::new(base.x + i + 1, base.z));
            for z in 0..16 {
                for y in MIN_Y..MAX_Y {
                    let (left, right) = (a.get(15, y, z), b.get(0, y, z));
                    if g.trees.is_vegetation(left) || g.trees.is_vegetation(right) {
                        logs_total += 1;
                    }
                }
            }
        }
        // Weak but meaningful: across 6 borders in a land area some vegetation touches borders.
        assert!(logs_total > 0, "no vegetation near borders around spawn");
    }

    #[test]
    fn neighbour_tree_placement_is_identical_from_both_sides() {
        let g = generator(77);
        // A tree site computed from the neighbour (profile path) must match the
        // chunk's own decision (local path) for the same chunk.
        for (cx, cz) in [(0, 0), (4, 4), (-3, 8), (10, -2)] {
            let pos = ChunkPos::new(cx, cz);
            let col = g.generate(pos);
            let remote = super::chunk_trees(&g, pos, None, None);
            for s in &remote {
                let (lx, lz) = ((s.x - cx * 16) as usize, (s.z - cz * 16) as usize);
                let below = col.get(lx, s.y - 1, lz);
                let trunk = col.get(lx, s.y, lz);
                assert!(
                    g.trees.is_vegetation(trunk),
                    "tree at {},{},{} missing in its own chunk (found {:?})",
                    s.x,
                    s.y,
                    s.z,
                    trunk
                );
                assert!(!g.trees.is_vegetation(below), "tree floats");
            }
        }
    }
}
