//! Structures: the ruins of the Builders (GDD §3.5).
//!
//! ## Placement (like Minecraft's "random spread")
//! The world is divided into square *regions* of `spacing` chunks. Each region
//! holds at most one structure of each kind, at a pseudo-random chunk inside
//! it (never within `separation` chunks of the region's far edge, so two
//! structures can't sit side by side). The start is kept only if the biome
//! there suits the structure. All of this is a pure function of the seed, so
//! every chunk can independently work out which structures overlap it.
//!
//! ## Building across chunk borders
//! A ruin can span several chunks. Each chunk looks for structure starts in
//! the surrounding chunks (up to [`MAX_RADIUS`] blocks away), regenerates
//! each structure's layout from its seed, and writes only the blocks that fall
//! inside itself. The layout depends only on the seed and on the *origin*
//! height (from `column_profile`), so all chunks agree. Foundations that
//! extend down to the ground use the current chunk's own terrain, which is
//! also order-independent.
//!
//! ## Ruins
//! Ruins are procedurally varied (sizes, decay, collapses) and "decay" is a
//! deterministic per-block hash, so a missing brick is missing for everyone.
//! Each ruin holds one **lore tablet**; which story fragment it shows is
//! derived from its position (see [`tablet_fragment`]).

use crate::noise::Rng;
use crate::overworld::{Overworld, SEA_LEVEL};
use ld_core::coords::ChunkPos;
use ld_core::world_size::{MAX_Y, MIN_Y};
use ld_world::{BlockId, BlockRegistry};

/// Furthest any structure block can be from its origin (horizontally, blocks).
pub const MAX_RADIUS: i32 = 12;

/// The kinds of structures, one per era of the story.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StructureKind {
    /// Golden age: broken halls on open land.
    SurfaceRuin,
    /// Expansion: half-buried halls in the desert.
    DesertHall,
    /// Expansion: collapsed towers on the sea floor.
    SunkenTower,
    /// The dark chapter: sealed vaults deep in the deepslate.
    DeepVault,
}

impl StructureKind {
    pub const ALL: [StructureKind; 4] = [
        StructureKind::SurfaceRuin,
        StructureKind::DesertHall,
        StructureKind::SunkenTower,
        StructureKind::DeepVault,
    ];

    /// (spacing, separation) in chunks, and a salt that decorrelates kinds.
    fn spread(self) -> (i32, i32, u64) {
        match self {
            StructureKind::SurfaceRuin => (20, 6, 0x5A1),
            StructureKind::DesertHall => (26, 8, 0xDE5),
            StructureKind::SunkenTower => (22, 7, 0x70E),
            StructureKind::DeepVault => (18, 5, 0xDEE),
        }
    }

    /// Story era (matches `era` in `lore.toml`).
    pub fn era(self) -> &'static str {
        match self {
            StructureKind::SurfaceRuin => "golden",
            StructureKind::DesertHall | StructureKind::SunkenTower => "expansion",
            StructureKind::DeepVault => "deep",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            StructureKind::SurfaceRuin => "surface ruin",
            StructureKind::DesertHall => "desert hall",
            StructureKind::SunkenTower => "sunken tower",
            StructureKind::DeepVault => "deep vault",
        }
    }
}

/// Where a structure begins.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StructureStart {
    pub kind: StructureKind,
    /// Origin (world block coordinates): centre of the floor.
    pub x: i32,
    pub y: i32,
    pub z: i32,
    pub seed: u64,
}

/// Block IDs used by ruins.
pub struct RuinBlocks {
    bricks: BlockId,
    cracked: BlockId,
    mossy: BlockId,
    chiseled: BlockId,
    deep_bricks: BlockId,
    deep_cracked: BlockId,
    sandstone: BlockId,
    cut_sandstone: BlockId,
    chiseled_sandstone: BlockId,
    sand: BlockId,
    tablet: BlockId,
    water: BlockId,
    stone: BlockId,
}

impl RuinBlocks {
    pub fn new(r: &BlockRegistry) -> Result<Self, String> {
        Ok(Self {
            bricks: r.require("stone_bricks")?,
            cracked: r.require("cracked_stone_bricks")?,
            mossy: r.require("mossy_stone_bricks")?,
            chiseled: r.require("chiseled_stone_bricks")?,
            deep_bricks: r.require("deepslate_bricks")?,
            deep_cracked: r.require("cracked_deepslate_bricks")?,
            sandstone: r.require("sandstone")?,
            cut_sandstone: r.require("cut_sandstone")?,
            chiseled_sandstone: r.require("chiseled_sandstone")?,
            sand: r.require("sand")?,
            tablet: r.require("lore_tablet")?,
            water: r.require("water")?,
            stone: r.require("stone")?,
        })
    }

    pub fn tablet(&self) -> BlockId {
        self.tablet
    }
}

/// Deterministic per-position hash in 0..1 (for decay and material mixes).
fn hash01(x: i32, y: i32, z: i32, salt: u64) -> f64 {
    let mut r = Rng::new(
        salt ^ (x as i64 as u64).wrapping_mul(0x9E37_79B9)
            ^ ((y as i64 as u64) << 21)
            ^ ((z as i64 as u64) << 42),
    );
    r.next_f64()
}

impl Overworld {
    /// The start of structure `kind` in the region containing chunk `region`
    /// coordinates, if that region has one and the biome/terrain suits it.
    fn region_start(&self, kind: StructureKind, rx: i32, rz: i32) -> Option<StructureStart> {
        let (spacing, separation, salt) = kind.spread();
        let mut rng = Rng::for_position(self.seed(), rx, rz, salt);
        let cx = rx * spacing + rng.range(0, spacing - separation - 1);
        let cz = rz * spacing + rng.range(0, spacing - separation - 1);
        let seed = rng.next_u64();
        // Anywhere inside the chosen chunk, so ruins often straddle borders.
        let (x, z) = (cx * 16 + rng.range(0, 15), cz * 16 + rng.range(0, 15));
        // Cheap biome check first; terrain height only if it passes.
        let biome = self.biomes().get(self.biome_at(x, z)).name.as_str();
        let ok_biome = match kind {
            StructureKind::SurfaceRuin => {
                matches!(biome, "plains" | "forest" | "birch_forest" | "taiga" | "snowy_plains" | "mountains")
            }
            StructureKind::DesertHall => biome == "desert",
            StructureKind::SunkenTower => matches!(biome, "ocean" | "deep_ocean"),
            StructureKind::DeepVault => !matches!(biome, "ocean" | "deep_ocean" | "river"),
        };
        if !ok_biome {
            return None;
        }
        let p = self.column_profile(x, z);
        let y = match kind {
            StructureKind::SurfaceRuin => {
                if p.water_above || p.top > 150 {
                    return None;
                }
                p.top + 1
            }
            StructureKind::DesertHall => {
                if p.water_above {
                    return None;
                }
                p.top - 2 // half buried
            }
            StructureKind::SunkenTower => {
                if !p.water_above || p.top > SEA_LEVEL - 6 {
                    return None; // needs some water above it
                }
                p.top + 1
            }
            StructureKind::DeepVault => Rng::new(seed).range(-44, -24),
        };
        Some(StructureStart { kind, x, y, z, seed })
    }

    /// All structure starts whose blocks may reach into chunk `pos`.
    pub fn structures_near(&self, pos: ChunkPos) -> Vec<StructureStart> {
        let mut out = Vec::new();
        let (x0, z0) = (pos.x * 16 - MAX_RADIUS, pos.z * 16 - MAX_RADIUS);
        let (x1, z1) = (pos.x * 16 + 16 + MAX_RADIUS, pos.z * 16 + 16 + MAX_RADIUS);
        for kind in StructureKind::ALL {
            let (spacing, _, _) = kind.spread();
            let r_min = |c: i32| (c.div_euclid(16)).div_euclid(spacing);
            for rx in r_min(x0)..=r_min(x1) {
                for rz in r_min(z0)..=r_min(z1) {
                    if let Some(s) = self.region_start(kind, rx, rz)
                        && s.x >= x0
                        && s.x < x1
                        && s.z >= z0
                        && s.z < z1
                    {
                        out.push(s);
                    }
                }
            }
        }
        out
    }

    /// Every structure start with its origin inside the given block area
    /// (used by the `--map` tool and tests).
    pub fn structures_in_area(&self, x0: i32, z0: i32, x1: i32, z1: i32) -> Vec<StructureStart> {
        let mut out = Vec::new();
        for kind in StructureKind::ALL {
            let (spacing, _, _) = kind.spread();
            let r = |c: i32| c.div_euclid(16).div_euclid(spacing);
            for rx in r(x0)..=r(x1) {
                for rz in r(z0)..=r(z1) {
                    if let Some(s) = self.region_start(kind, rx, rz)
                        && (x0..x1).contains(&s.x)
                        && (z0..z1).contains(&s.z)
                    {
                        out.push(s);
                    }
                }
            }
        }
        out
    }
}

/// Writes structure blocks into one chunk's flat block array.
struct Writer<'a> {
    blocks: &'a mut [BlockId],
    ox: i32,
    oz: i32,
}

#[inline]
fn idx(x: usize, y: i32, z: usize) -> usize {
    ((y - MIN_Y) as usize) * 256 + z * 16 + x
}

impl Writer<'_> {
    fn local(&self, x: i32, y: i32, z: i32) -> Option<usize> {
        let (lx, lz) = (x - self.ox, z - self.oz);
        ((0..16).contains(&lx) && (0..16).contains(&lz) && (MIN_Y..MAX_Y).contains(&y))
            .then(|| idx(lx as usize, y, lz as usize))
    }

    fn set(&mut self, x: i32, y: i32, z: i32, b: BlockId) {
        if let Some(i) = self.local(x, y, z) {
            self.blocks[i] = b;
        }
    }

    fn get(&self, x: i32, y: i32, z: i32) -> Option<BlockId> {
        self.local(x, y, z).map(|i| self.blocks[i])
    }

    /// Extends a foundation column down from `y` until it meets solid ground
    /// (uses this chunk's own terrain, so it's order-independent).
    fn foundation(&mut self, x: i32, y: i32, z: i32, b: BlockId, soft: &dyn Fn(BlockId) -> bool) {
        let mut yy = y;
        while yy > MIN_Y && yy > y - 24 {
            match self.get(x, yy, z) {
                Some(cur) if soft(cur) => self.set(x, yy, z, b),
                _ => break,
            }
            yy -= 1;
        }
    }
}

/// Places every structure overlapping chunk `pos`.
pub fn place_structures(wg: &Overworld, blocks: &mut [BlockId], pos: ChunkPos, rb: &RuinBlocks) {
    let starts = wg.structures_near(pos);
    if starts.is_empty() {
        return;
    }
    let water = rb.water;
    // "Soft" = what foundations may replace: air, water, plants, leaves, logs.
    let soft = move |b: BlockId| b == BlockId::AIR || b == water || wg.trees.is_vegetation(b);
    let mut w = Writer { blocks, ox: pos.x * 16, oz: pos.z * 16 };
    for s in starts {
        match s.kind {
            StructureKind::SurfaceRuin => surface_ruin(&mut w, &s, rb, &soft),
            StructureKind::DesertHall => desert_hall(&mut w, &s, rb, &soft),
            StructureKind::SunkenTower => sunken_tower(&mut w, &s, rb, &soft),
            StructureKind::DeepVault => deep_vault(&mut w, &s, rb),
        }
    }
}

/// Weathered stone-brick mix: mostly plain, some cracked or mossy.
fn weathered(rb: &RuinBlocks, x: i32, y: i32, z: i32, salt: u64, mossy: f64) -> BlockId {
    let h = hash01(x, y, z, salt ^ 0xB71C);
    if h < mossy {
        rb.mossy
    } else if h < mossy + 0.2 {
        rb.cracked
    } else {
        rb.bricks
    }
}

/// Golden-age hall: a square floor, crumbling walls that get more broken
/// toward the top, chiseled corner pillars, a doorway, and a tablet on a
/// pedestal in the middle.
fn surface_ruin(w: &mut Writer, s: &StructureStart, rb: &RuinBlocks, soft: &dyn Fn(BlockId) -> bool) {
    let mut rng = Rng::new(s.seed);
    let half = rng.range(4, 6); // 9×9 to 13×13
    let wall_h = rng.range(3, 5);
    let door = rng.below(4); // which side the doorway is on
    let (x0, z0, y0) = (s.x, s.z, s.y);
    for dz in -half..=half {
        for dx in -half..=half {
            let (x, z) = (x0 + dx, z0 + dz);
            // Floor with foundation, missing tiles here and there.
            if hash01(x, y0, z, s.seed) > 0.12 {
                w.set(x, y0 - 1, z, weathered(rb, x, y0 - 1, z, s.seed, 0.25));
                w.foundation(x, y0 - 2, z, rb.stone, soft);
            }
            // Clear the inside so hills and trees don't fill the hall.
            for y in y0..y0 + wall_h + 2 {
                w.set(x, y, z, BlockId::AIR);
            }
            let edge = dx.abs() == half || dz.abs() == half;
            let corner = dx.abs() == half && dz.abs() == half;
            let in_door = match door {
                0 => dz == -half && dx.abs() <= 1,
                1 => dz == half && dx.abs() <= 1,
                2 => dx == -half && dz.abs() <= 1,
                _ => dx == half && dz.abs() <= 1,
            };
            if corner {
                for y in y0..y0 + wall_h + 1 {
                    if hash01(x, y, z, s.seed ^ 1) > 0.08 * (y - y0) as f64 {
                        w.set(x, y, z, rb.chiseled);
                    }
                }
            } else if edge && !in_door {
                for y in y0..y0 + wall_h {
                    // Higher courses are more likely to have fallen.
                    let keep = 0.95 - 0.22 * (y - y0) as f64;
                    if hash01(x, y, z, s.seed ^ 2) < keep {
                        w.set(x, y, z, weathered(rb, x, y, z, s.seed, 0.3));
                    }
                }
            }
        }
    }
    // Pedestal and tablet.
    w.set(x0, y0, z0, rb.chiseled);
    w.set(x0, y0 + 1, z0, rb.tablet);
}

/// Expansion era: a long sandstone hall half sunk into the desert, its roof
/// partly collapsed and sand drifted inside, a tablet at the far end.
fn desert_hall(w: &mut Writer, s: &StructureStart, rb: &RuinBlocks, soft: &dyn Fn(BlockId) -> bool) {
    let mut rng = Rng::new(s.seed);
    let (hx, hz) = (rng.range(4, 5), rng.range(6, 8));
    let height = 6;
    let (x0, y0, z0) = (s.x, s.y, s.z);
    for dz in -hz..=hz {
        for dx in -hx..=hx {
            let (x, z) = (x0 + dx, z0 + dz);
            w.set(x, y0 - 1, z, rb.cut_sandstone);
            w.foundation(x, y0 - 2, z, rb.sandstone, soft);
            let edge = dx.abs() == hx || dz.abs() == hz;
            for y in y0..y0 + height {
                let b = if edge {
                    if (dx.abs() == hx && dz.rem_euclid(4) == 0) || (y - y0) == height - 1 {
                        rb.chiseled_sandstone // pillars and a carved frieze
                    } else {
                        rb.cut_sandstone
                    }
                } else {
                    // Drifted sand inside: a dune sloping down from one wall.
                    let dune = 2 - (dx + hx) / 3;
                    if y - y0 < dune { rb.sand } else { BlockId::AIR }
                };
                w.set(x, y, z, b);
            }
            // Roof, with a collapsed band across the middle.
            let collapsed = dz.abs() <= 2 && hash01(x, y0 + height, z, s.seed) < 0.8;
            if !collapsed {
                w.set(x, y0 + height, z, rb.cut_sandstone);
            }
        }
    }
    // Entrance on the +z end (the downhill side of the dune is irrelevant: sand
    // drifts in through the roof).
    for dx in -1..=1 {
        for y in y0..y0 + 3 {
            w.set(x0 + dx, y, z0 + hz, BlockId::AIR);
        }
    }
    w.set(x0, y0, z0 - hz + 1, rb.chiseled_sandstone);
    w.set(x0, y0 + 1, z0 - hz + 1, rb.tablet);
}

/// Expansion era: a round brick tower standing on the sea floor, its top
/// broken off at an angle, flooded inside, with a tablet at its base.
fn sunken_tower(w: &mut Writer, s: &StructureStart, rb: &RuinBlocks, soft: &dyn Fn(BlockId) -> bool) {
    let mut rng = Rng::new(s.seed);
    let r = rng.range(3, 4) as f64;
    let height = rng.range(9, 15);
    let tilt_x = rng.range(-2, 2); // the break is a sloped plane
    let tilt_z = rng.range(-2, 2);
    let (x0, y0, z0) = (s.x, s.y, s.z);
    let ri = r.ceil() as i32;
    for dz in -ri..=ri {
        for dx in -ri..=ri {
            let d = ((dx * dx + dz * dz) as f64).sqrt();
            if d > r + 0.5 {
                continue;
            }
            let (x, z) = (x0 + dx, z0 + dz);
            w.set(x, y0 - 1, z, weathered(rb, x, y0 - 1, z, s.seed, 0.5));
            w.foundation(x, y0 - 2, z, rb.stone, soft);
            let wall = d > r - 0.7;
            let top = y0 + height + (dx * tilt_x + dz * tilt_z) / 2;
            for y in y0..top.min(SEA_LEVEL + 3) {
                if wall {
                    // Windows every few courses.
                    let window = (y - y0) % 5 == 3 && (dx == 0 || dz == 0);
                    if !window && hash01(x, y, z, s.seed) > 0.06 {
                        w.set(x, y, z, weathered(rb, x, y, z, s.seed, 0.6));
                    }
                } else if y > SEA_LEVEL {
                    w.set(x, y, z, BlockId::AIR);
                } else {
                    w.set(x, y, z, rb.water); // flooded inside
                }
            }
        }
    }
    w.set(x0, y0, z0, rb.chiseled);
    w.set(x0, y0 + 1, z0, rb.tablet);
}

/// The dark chapter: a sealed deepslate-brick vault with pillars, deep
/// underground (found through caves or by digging), tablet at its heart.
fn deep_vault(w: &mut Writer, s: &StructureStart, rb: &RuinBlocks) {
    let mut rng = Rng::new(s.seed);
    let (hx, hz) = (rng.range(5, 7), rng.range(5, 7));
    let h = 5;
    let (x0, y0, z0) = (s.x, s.y, s.z);
    for dz in -hz..=hz {
        for dx in -hx..=hx {
            let (x, z) = (x0 + dx, z0 + dz);
            let edge = dx.abs() == hx || dz.abs() == hz;
            // Pillars on a 4-block grid, keeping the middle free for the tablet.
            let pillar =
                dx.rem_euclid(4) == 0 && dz.rem_euclid(4) == 0 && !edge && (dx.abs() > 1 || dz.abs() > 1);
            for y in y0 - 1..=y0 + h {
                let shell = edge || y == y0 - 1 || y == y0 + h;
                let b = if shell || pillar {
                    if hash01(x, y, z, s.seed) < 0.25 { rb.deep_cracked } else { rb.deep_bricks }
                } else {
                    BlockId::AIR
                };
                w.set(x, y, z, b);
            }
        }
    }
    w.set(x0, y0, z0, rb.deep_bricks);
    w.set(x0, y0 + 1, z0, rb.tablet);
}

/// Which lore fragment (index among fragments of the structure's era) a
/// tablet at this position shows. Deterministic, so it never changes.
pub fn tablet_fragment(x: i32, y: i32, z: i32, era_fragment_count: usize) -> usize {
    if era_fragment_count == 0 {
        return 0;
    }
    (hash01(x, y, z, 0x7AB1E7) * era_fragment_count as f64) as usize % era_fragment_count
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::TerrainGenerator;
    use crate::overworld::tests::generator;

    #[test]
    fn structures_are_spread_out_and_in_fitting_biomes() {
        let g = generator(12345);
        let all = g.structures_in_area(-8000, -8000, 8000, 8000);
        let count = |k| all.iter().filter(|s| s.kind == k).count();
        for k in StructureKind::ALL {
            assert!(count(k) > 0, "no {} in a 16 km square", k.name());
        }
        // Same kind never closer than the separation allows.
        for a in &all {
            for b in &all {
                if a != b && a.kind == b.kind {
                    let (_, sep, _) = a.kind.spread();
                    let d = (a.x - b.x).abs().max((a.z - b.z).abs());
                    assert!(d >= sep * 16, "{} too close: {d} blocks", a.kind.name());
                }
            }
        }
        for s in &all {
            let biome = g.biomes().get(g.biome_at(s.x, s.z)).name.clone();
            match s.kind {
                StructureKind::DesertHall => assert_eq!(biome, "desert"),
                StructureKind::SunkenTower => assert!(biome.contains("ocean")),
                _ => {}
            }
        }
    }

    #[test]
    fn a_ruin_spanning_chunks_has_exactly_one_tablet() {
        let g = generator(12345);
        let text =
            std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/data/blocks.toml"))
                .unwrap();
        let rb = RuinBlocks::new(&BlockRegistry::from_toml(&text).unwrap()).unwrap();
        // Pick a ruin whose origin is near a chunk edge, so it must cross the border.
        let s = g
            .structures_in_area(-6000, -6000, 6000, 6000)
            .into_iter()
            .find(|s| {
                s.kind == StructureKind::SurfaceRuin && (s.x.rem_euclid(16) <= 2 || s.x.rem_euclid(16) >= 13)
            })
            .expect("a surface ruin near a chunk edge");
        let (cx, cz) = (s.x.div_euclid(16), s.z.div_euclid(16));
        let mut tablets = 0;
        let mut bricks_in_chunks = 0;
        for dz in -1..=1 {
            for dx in -1..=1 {
                let col = g.generate(ChunkPos::new(cx + dx, cz + dz));
                let mut any = false;
                for y in s.y - 2..s.y + 8 {
                    for z in 0..16 {
                        for x in 0..16 {
                            let id = col.get(x, y, z);
                            tablets += (id == rb.tablet) as i32;
                            any |= id == rb.bricks || id == rb.chiseled || id == rb.cracked || id == rb.mossy;
                        }
                    }
                }
                bricks_in_chunks += any as i32;
            }
        }
        assert_eq!(tablets, 1);
        assert!(bricks_in_chunks >= 2, "ruin near an edge must appear in both chunks: {bricks_in_chunks}");
    }

    #[test]
    fn tablet_fragments_cover_all_choices() {
        let mut seen = std::collections::HashSet::new();
        for i in 0..200 {
            seen.insert(tablet_fragment(i * 31, 64, i * 17, 4));
        }
        assert_eq!(seen.len(), 4);
    }
}
