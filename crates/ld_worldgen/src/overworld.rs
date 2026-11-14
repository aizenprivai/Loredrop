//! The Overworld generator: terrain, biomes, caves, water, surface, ores and
//! vegetation, built with the same *kinds* of techniques as modern Minecraft
//! (1.18+), using our own noises, curves and numbers (worlds are therefore
//! not seed-compatible with Minecraft).
//!
//! ## Pipeline for one chunk column
//! 1. **Climate** — five smooth noises (temperature, humidity,
//!    continentalness, erosion, weirdness) sampled on a coarse 4-block grid.
//! 2. **Terrain shape** — continentalness, erosion and "peaks & valleys"
//!    (folded weirdness) are turned into a target surface height and a
//!    roughness factor by hand-tuned curves: oceans, coasts, plains, hills,
//!    mountain ranges and river valleys.
//! 3. **Density** — every point gets a value: positive = solid, negative =
//!    air. Density = (target height − y) + 3D noise × roughness, so cliffs and
//!    overhangs appear where roughness is high. Three cave noises
//!    ("cheese" caverns, "spaghetti" tunnels, thin "noodle" passages) are
//!    subtracted. Density is computed on a 4×8×4 lattice and smoothly
//!    interpolated per block — about 80× fewer noise samples, like Minecraft.
//! 4. **Fill** — solid → stone (deepslate below Y 0, bedrock floor); air above
//!    the terrain and below sea level → water; air in caves below Y −54 → lava.
//! 5. **Biomes** — chosen per block column from the climate (see
//!    `ld_world::biome`), so biomes always fit the terrain they sit on.
//! 6. **Surface** — biome-specific top/filler layers (grass + dirt, sand +
//!    sandstone, snow, gravel sea floors, bare stone on steep mountains).
//! 7. **Ores** — veins by depth (`ores.toml`).
//! 8. **Vegetation** — trees (which may hang over chunk borders) and plants.
//!
//! Everything is a pure function of (seed, position), so chunks can be
//! generated in any order on any thread with identical results.

use crate::TerrainGenerator;
use crate::features::{self, TreeKind};
use crate::noise::{DoubleNoise, OctaveNoise, Rng};
use crate::ores::{self, OreRule};
use ld_core::coords::ChunkPos;
use ld_core::world_size::{MAX_Y, MIN_Y};
use ld_world::section::{Section, VOLUME};
use ld_world::{BiomeId, BiomeRegistry, BlockId, BlockRegistry, ChunkColumn};
use std::sync::Arc;

/// Sea level: water fills open air at or below this Y.
pub const SEA_LEVEL: i32 = 63;
/// Lava fills caves at or below this Y (like Minecraft's deep lava lakes).
const LAVA_LEVEL: i32 = -54;

/// Lattice cell size (blocks).
const CELL_XZ: i32 = 4;
const CELL_Y: i32 = 8;
const LAT_XZ: usize = 16 / CELL_XZ as usize + 1; // 5
const LAT_Y: usize = ((MAX_Y - MIN_Y) / CELL_Y) as usize + 1; // 49
const HEIGHT: usize = (MAX_Y - MIN_Y) as usize; // 384

/// Climate values and derived terrain shape at one (x, z).
#[derive(Clone, Copy, Debug, Default)]
pub struct Climate {
    pub temperature: f32,
    pub humidity: f32,
    pub continentalness: f32,
    pub erosion: f32,
    pub weirdness: f32,
    /// Target surface height (world Y).
    pub height: f32,
    /// How strongly 3D noise roughens the terrain (blocks).
    pub roughness: f32,
}

impl Climate {
    fn lerp(a: &Climate, b: &Climate, t: f32) -> Climate {
        let l = |x: f32, y: f32| x + (y - x) * t;
        Climate {
            temperature: l(a.temperature, b.temperature),
            humidity: l(a.humidity, b.humidity),
            continentalness: l(a.continentalness, b.continentalness),
            erosion: l(a.erosion, b.erosion),
            weirdness: l(a.weirdness, b.weirdness),
            height: l(a.height, b.height),
            roughness: l(a.roughness, b.roughness),
        }
    }

    /// The five parameters in the order `BiomeRegistry::select` expects.
    pub fn point(&self) -> [f32; 5] {
        [self.temperature, self.humidity, self.continentalness, self.erosion, self.weirdness]
    }
}

/// Block IDs the generator uses directly.
struct Blocks {
    air: BlockId,
    stone: BlockId,
    deepslate: BlockId,
    bedrock: BlockId,
    water: BlockId,
    lava: BlockId,
    ice: BlockId,
    gravel: BlockId,
    snow_block: BlockId,
    dirt: BlockId,
}

/// Per-biome block IDs, resolved once from the biome file's block names.
struct BiomeBlocks {
    surface: BlockId,
    filler: BlockId,
    filler_depth: u32,
    deep_filler: Option<(BlockId, u32)>,
    underwater: BlockId,
}

pub struct Overworld {
    seed: u64,
    temperature: DoubleNoise,
    humidity: DoubleNoise,
    continentalness: DoubleNoise,
    erosion: DoubleNoise,
    weirdness: DoubleNoise,
    detail: OctaveNoise,
    cheese: DoubleNoise,
    spaghetti: [DoubleNoise; 2],
    spaghetti_radius: OctaveNoise,
    noodle: [DoubleNoise; 2],
    noodle_toggle: OctaveNoise,
    surface_noise: OctaveNoise,
    blocks: Blocks,
    biomes: Arc<BiomeRegistry>,
    biome_blocks: Vec<BiomeBlocks>,
    ores: Vec<OreRule>,
    pub(crate) trees: features::TreeBlocks,
    /// For each biome: (tree kinds with weights, total weight).
    biome_trees: Vec<(Vec<(TreeKind, u32)>, u32)>,
    /// For each biome: plants (block, attempts per chunk).
    biome_plants: Vec<Vec<(BlockId, u32)>>,
    ocean_like: Vec<bool>,
}

/// Calibrates a noise so its values have the given standard deviation.
/// (The raw spread of summed noise depends on octave count; calibrating keeps
/// biome thresholds meaningful no matter how the noise is configured.)
fn calibrated(rng: &mut Rng, octaves: usize, wavelength: f64, target_sd: f64) -> DoubleNoise {
    let raw = DoubleNoise::new(rng, octaves, wavelength, 1.0);
    let n = 1024;
    let mut sum2 = 0.0;
    for i in 0..n {
        let (x, z) = ((i % 32) as f64 * wavelength * 0.37, (i / 32) as f64 * wavelength * 0.41);
        sum2 += raw.sample2(x, z).powi(2);
    }
    let sd = (sum2 / n as f64).sqrt().max(1e-6);
    let mut tuned = raw;
    tuned.set_gain(target_sd / sd);
    tuned
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Piecewise-linear curve through `(x, y)` points (sorted by x).
fn curve(x: f32, pts: &[(f32, f32)]) -> f32 {
    if x <= pts[0].0 {
        return pts[0].1;
    }
    for w in pts.windows(2) {
        let ((x0, y0), (x1, y1)) = (w[0], w[1]);
        if x <= x1 {
            return y0 + (y1 - y0) * (x - x0) / (x1 - x0);
        }
    }
    pts[pts.len() - 1].1
}

/// Base height (blocks relative to sea level) from continentalness: deep
/// ocean floors, the continental shelf, beaches, then gently rising inland.
const CONTINENT_CURVE: &[(f32, f32)] = &[
    (-1.2, -45.0),
    (-0.7, -38.0),
    (-0.455, -26.0),
    (-0.3, -15.0),
    (-0.19, -6.0),
    (-0.13, -1.0),
    (-0.11, 1.0),
    (-0.05, 3.0),
    (0.1, 7.0),
    (0.4, 14.0),
    (1.2, 24.0),
];

impl Overworld {
    pub fn new(
        seed: u64,
        blocks: &BlockRegistry,
        biomes: Arc<BiomeRegistry>,
        ores_toml: &str,
    ) -> Result<Self, String> {
        let mut rng = Rng::new(seed);
        // Climate noises: big, smooth features (sizes in blocks).
        let continentalness = calibrated(&mut rng, 5, 1400.0, 0.42);
        let erosion = calibrated(&mut rng, 4, 800.0, 0.42);
        let weirdness = calibrated(&mut rng, 3, 480.0, 0.42);
        let temperature = calibrated(&mut rng, 3, 2200.0, 0.42);
        let humidity = calibrated(&mut rng, 3, 1500.0, 0.42);
        let detail = OctaveNoise::new(&mut rng, 4, 110.0);
        let cheese = calibrated(&mut rng, 3, 130.0, 0.32);
        let spaghetti = [calibrated(&mut rng, 2, 70.0, 0.3), calibrated(&mut rng, 2, 70.0, 0.3)];
        let spaghetti_radius = OctaveNoise::new(&mut rng, 1, 90.0);
        let noodle = [calibrated(&mut rng, 1, 30.0, 0.3), calibrated(&mut rng, 1, 30.0, 0.3)];
        let noodle_toggle = OctaveNoise::new(&mut rng, 1, 120.0);
        let surface_noise = OctaveNoise::new(&mut rng, 2, 24.0);

        let b = |n: &str| blocks.require(n);
        let gen_blocks = Blocks {
            air: BlockId::AIR,
            stone: b("stone")?,
            deepslate: b("deepslate")?,
            bedrock: b("bedrock")?,
            water: b("water")?,
            lava: b("lava")?,
            ice: b("ice")?,
            gravel: b("gravel")?,
            snow_block: b("snow_block")?,
            dirt: b("dirt")?,
        };
        let mut biome_blocks = Vec::new();
        let mut biome_trees = Vec::new();
        let mut biome_plants = Vec::new();
        let mut ocean_like = Vec::new();
        for (_, d) in biomes.iter() {
            biome_blocks.push(BiomeBlocks {
                surface: b(&d.surface)?,
                filler: b(&d.filler)?,
                filler_depth: d.filler_depth,
                deep_filler: d
                    .deep_filler
                    .as_deref()
                    .map(|n| b(n).map(|id| (id, d.deep_filler_depth)))
                    .transpose()?,
                underwater: b(&d.underwater)?,
            });
            let mut kinds = Vec::new();
            for (name, &w) in &d.trees {
                let kind = TreeKind::parse(name)
                    .ok_or_else(|| format!("biome '{}': unknown tree kind '{name}'", d.name))?;
                kinds.push((kind, w));
            }
            kinds.sort_by_key(|k| k.0 as u8); // HashMap order is random: sort for determinism
            let total = kinds.iter().map(|k| k.1).sum();
            biome_trees.push((kinds, total));
            let mut plants: Vec<(BlockId, u32)> =
                d.plants.iter().map(|(n, &a)| b(n).map(|id| (id, a))).collect::<Result<_, _>>()?;
            plants.sort_by_key(|p| p.0);
            biome_plants.push(plants);
            ocean_like.push(d.continentalness[1] < -0.15 || d.name == "river");
        }
        let ores = ores::parse(ores_toml, blocks, &biomes)?;
        Ok(Self {
            seed,
            temperature,
            humidity,
            continentalness,
            erosion,
            weirdness,
            detail,
            cheese,
            spaghetti,
            spaghetti_radius,
            noodle,
            noodle_toggle,
            surface_noise,
            blocks: gen_blocks,
            trees: features::TreeBlocks::new(blocks)?,
            biomes,
            biome_blocks,
            ores,
            biome_trees,
            biome_plants,
            ocean_like,
        })
    }

    // ------------------------------------------------------------------
    // Climate & terrain shape
    // ------------------------------------------------------------------

    /// Climate and terrain shape at block column (x, z).
    pub fn climate(&self, x: i32, z: i32) -> Climate {
        let (fx, fz) = (x as f64, z as f64);
        let c = self.continentalness.sample2(fx, fz) as f32;
        let e = self.erosion.sample2(fx, fz) as f32;
        let w = self.weirdness.sample2(fx, fz) as f32;
        let t = self.temperature.sample2(fx, fz) as f32;
        let h = self.humidity.sample2(fx, fz) as f32;

        // "Peaks and valleys": weirdness folded so |w| ≈ 2/3 → peaks (+1),
        // w ≈ 0 → valleys (−1). Rivers follow the valleys.
        // Clamped: noise tails can exceed |w| = 1, which would push pv below −1
        // (and powf of a negative number is NaN — this once made 320-high walls).
        let pv = (1.0 - (3.0 * w.abs() - 2.0).abs()).clamp(-1.0, 1.0);
        let pv01 = (pv + 1.0) * 0.5;
        let inland = smoothstep(-0.11, 0.25, c);
        let mountain = ((-0.2 - e) / 0.6).clamp(0.0, 1.0); // low erosion → mountains
        let flat = ((e - 0.3) / 0.5).clamp(0.0, 1.0); // high erosion → flat plains

        let base = curve(c, CONTINENT_CURVE);
        let peaks = inland * mountain.powf(1.2) * (25.0 + 125.0 * pv01.powf(1.5));
        let hills = inland * (1.0 - mountain) * (1.0 - flat) * (4.0 + 12.0 * pv);
        let mut height = SEA_LEVEL as f32 + base + peaks + hills;

        // River valleys: where weirdness crosses zero inland, cut down to just
        // below sea level so water fills the channel.
        let river = (1.0 - smoothstep(0.0, 0.07, w.abs())) * smoothstep(-0.11, 0.0, c) * (1.0 - mountain);
        if height > 59.0 {
            height += (59.0 - height) * river;
        }
        let roughness = 2.5 + peaks * 0.2 + (1.0 - flat) * 2.0;
        Climate {
            temperature: t,
            humidity: h,
            continentalness: c,
            erosion: e,
            weirdness: w,
            height,
            roughness,
        }
    }

    /// Biome for a climate point.
    pub fn biome_for(&self, c: &Climate) -> BiomeId {
        self.biomes.select(c.point())
    }

    /// Returns (terrain density, final density with caves) at a lattice point.
    /// Positive = solid. Values are roughly "blocks to the surface".
    fn density(&self, x: i32, y: i32, z: i32, c: &Climate) -> (f32, f32) {
        let (fx, fy, fz) = (x as f64, y as f64, z as f64);
        let detail = self.detail.sample(fx, fy * 1.4, fz) as f32;
        let terrain = (c.height - y as f32) + c.roughness * 2.0 * detail;
        if terrain <= 0.0 || y <= MIN_Y + 4 {
            return (terrain, terrain); // open air, or the solid world floor
        }
        let depth = c.height - y as f32;
        // Keep sea and river floors sealed so caves don't open into water.
        let seal = if c.height < (SEA_LEVEL - 1) as f32 { (12.0 - depth).max(0.0) * 8.0 } else { 0.0 };
        // Fewer caves right at the bottom of the world.
        let floor = ((MIN_Y + 10 - y) as f32).max(0.0) * 6.0;

        // Cheese caves: big caverns where a 3D noise is high. Harder to
        // trigger near the surface so the ground isn't riddled with holes.
        let cheese_n = self.cheese.sample(fx, fy * 1.6, fz) as f32;
        let cheese_t = 0.44 + 0.5 * (1.0 - (depth / 16.0).clamp(0.0, 1.0));
        let cheese = (cheese_t - cheese_n) * 30.0;

        // Spaghetti caves: where two noises are BOTH near zero → long winding
        // tunnels. Allowed to reach the surface, creating cave entrances.
        let a = self.spaghetti[0].sample(fx, fy * 1.3, fz) as f32;
        let b = self.spaghetti[1].sample(fx, fy * 1.3, fz) as f32;
        let r = 0.085 + 0.03 * self.spaghetti_radius.sample(fx, fy, fz) as f32;
        let spaghetti = ((a * a + b * b).sqrt() - r) * 160.0;

        // Noodle caves: thin, twisty passages, only underground and only in
        // some regions.
        let noodle = if y < 30 && self.noodle_toggle.sample(fx, fy, fz) > 0.0 {
            let a = self.noodle[0].sample(fx, fy, fz) as f32;
            let b = self.noodle[1].sample(fx, fy, fz) as f32;
            ((a * a + b * b).sqrt() - 0.045) * 220.0
        } else {
            f32::MAX
        };

        let caves = cheese.min(spaghetti).min(noodle) + seal + floor;
        (terrain, terrain.min(caves))
    }

    // ------------------------------------------------------------------
    // Lattice
    // ------------------------------------------------------------------

    /// Climates at the 5×5 lattice columns of a chunk (index `lz * 5 + lx`).
    fn lattice_climate(&self, bx: i32, bz: i32) -> [Climate; LAT_XZ * LAT_XZ] {
        std::array::from_fn(|i| {
            let (lx, lz) = ((i % LAT_XZ) as i32, (i / LAT_XZ) as i32);
            self.climate(bx + lx * CELL_XZ, bz + lz * CELL_XZ)
        })
    }

    /// Density values (terrain, final) along one lattice column.
    fn lattice_column(&self, x: i32, z: i32, c: &Climate) -> [(f32, f32); LAT_Y] {
        std::array::from_fn(|ly| self.density(x, MIN_Y + ly as i32 * CELL_Y, z, c))
    }

    /// Climate at block column (x, z) of a chunk, interpolated from the lattice.
    fn column_climate(lat: &[Climate; LAT_XZ * LAT_XZ], x: usize, z: usize) -> Climate {
        let (cx, cz) = (x / CELL_XZ as usize, z / CELL_XZ as usize);
        let (tx, tz) =
            ((x % CELL_XZ as usize) as f32 / CELL_XZ as f32, (z % CELL_XZ as usize) as f32 / CELL_XZ as f32);
        let at = |ix: usize, iz: usize| &lat[iz * LAT_XZ + ix];
        let top = Climate::lerp(at(cx, cz), at(cx + 1, cz), tx);
        let bottom = Climate::lerp(at(cx, cz + 1), at(cx + 1, cz + 1), tx);
        Climate::lerp(&top, &bottom, tz)
    }

    /// Interpolates the 4 corner lattice columns at a block (fractions within the cell).
    #[inline]
    fn interp(corners: [&[(f32, f32); LAT_Y]; 4], ly: usize, tx: f32, ty: f32, tz: f32) -> (f32, f32) {
        let l = |a: f32, b: f32, t: f32| a + (b - a) * t;
        let pick = |c: &[(f32, f32); LAT_Y], i: usize| c[i.min(LAT_Y - 1)];
        let mut out = [0f32; 2];
        for (k, o) in out.iter_mut().enumerate() {
            let v = |c: &[(f32, f32); LAT_Y], i: usize| if k == 0 { pick(c, i).0 } else { pick(c, i).1 };
            let lo = l(
                l(v(corners[0], ly), v(corners[1], ly), tx),
                l(v(corners[2], ly), v(corners[3], ly), tx),
                tz,
            );
            let hi = l(
                l(v(corners[0], ly + 1), v(corners[1], ly + 1), tx),
                l(v(corners[2], ly + 1), v(corners[3], ly + 1), tx),
                tz,
            );
            *o = l(lo, hi, ty);
        }
        (out[0], out[1])
    }

    /// What block the density field puts at (y) given interpolated densities.
    #[inline]
    fn base_block(&self, y: i32, terrain: f32, fin: f32, cold: bool) -> BlockId {
        let b = &self.blocks;
        if fin > 0.0 {
            b.stone
        } else if terrain <= 0.0 {
            if y > SEA_LEVEL {
                b.air
            } else if y == SEA_LEVEL && cold {
                b.ice
            } else {
                b.water
            }
        } else if y <= LAVA_LEVEL {
            b.lava
        } else {
            b.air
        }
    }

    /// Profile of a single block column anywhere in the world, computed with
    /// exactly the same maths as chunk generation. Used to place features that
    /// cross chunk borders and to find a spawn point.
    pub fn column_profile(&self, x: i32, z: i32) -> ColumnProfile {
        let (x0, z0) = (x.div_euclid(CELL_XZ) * CELL_XZ, z.div_euclid(CELL_XZ) * CELL_XZ);
        let corner_xz = [(x0, z0), (x0 + CELL_XZ, z0), (x0, z0 + CELL_XZ), (x0 + CELL_XZ, z0 + CELL_XZ)];
        let climates: Vec<Climate> = corner_xz.iter().map(|&(cx, cz)| self.climate(cx, cz)).collect();
        let (tx, tz) = ((x - x0) as f32 / CELL_XZ as f32, (z - z0) as f32 / CELL_XZ as f32);
        let climate = Climate::lerp(
            &Climate::lerp(&climates[0], &climates[1], tx),
            &Climate::lerp(&climates[2], &climates[3], tx),
            tz,
        );
        let biome = self.biome_for(&climate);
        let cols: Vec<[(f32, f32); LAT_Y]> =
            corner_xz.iter().zip(&climates).map(|(&(cx, cz), c)| self.lattice_column(cx, cz, c)).collect();
        let corners = [&cols[0], &cols[1], &cols[2], &cols[3]];
        let mut top = MIN_Y;
        for y in (MIN_Y..MAX_Y).rev() {
            let rel = y - MIN_Y;
            let (ly, ty) = ((rel / CELL_Y) as usize, (rel % CELL_Y) as f32 / CELL_Y as f32);
            if Self::interp(corners, ly, tx, ty, tz).1 > 0.0 {
                top = y;
                break;
            }
        }
        // The highest solid block has open (non-cave) air above it, so it is
        // under water exactly when it lies below sea level.
        let water_above = top < SEA_LEVEL;
        ColumnProfile { top, biome, water_above, climate }
    }

    /// Biome at a block column (cheap: climate only, no density).
    pub fn biome_at(&self, x: i32, z: i32) -> BiomeId {
        let (x0, z0) = (x.div_euclid(CELL_XZ) * CELL_XZ, z.div_euclid(CELL_XZ) * CELL_XZ);
        let c = [
            self.climate(x0, z0),
            self.climate(x0 + CELL_XZ, z0),
            self.climate(x0, z0 + CELL_XZ),
            self.climate(x0 + CELL_XZ, z0 + CELL_XZ),
        ];
        let (tx, tz) = ((x - x0) as f32 / CELL_XZ as f32, (z - z0) as f32 / CELL_XZ as f32);
        self.biome_for(&Climate::lerp(&Climate::lerp(&c[0], &c[1], tx), &Climate::lerp(&c[2], &c[3], tx), tz))
    }

    pub fn biomes(&self) -> &BiomeRegistry {
        &self.biomes
    }
}

/// Summary of one block column, see [`Overworld::column_profile`].
#[derive(Clone, Copy, Debug)]
pub struct ColumnProfile {
    /// Y of the highest solid block.
    pub top: i32,
    pub biome: BiomeId,
    /// True if the top solid block is under water.
    pub water_above: bool,
    pub climate: Climate,
}

/// Index into a full-column block array (matches section layout).
#[inline]
fn idx(x: usize, y: i32, z: usize) -> usize {
    ((y - MIN_Y) as usize) * 256 + z * 16 + x
}

impl TerrainGenerator for Overworld {
    fn surface_height(&self, x: i32, z: i32) -> i32 {
        self.column_profile(x, z).top
    }

    fn find_spawn(&self) -> (i32, i32, i32) {
        // Spiral outward from the origin until we find dry land in a normal biome.
        for ring in 0..64i32 {
            for i in -ring..=ring {
                for (x, z) in [(i, -ring), (i, ring), (-ring, i), (ring, i)] {
                    let (wx, wz) = (x * 16 + 8, z * 16 + 8);
                    let p = self.column_profile(wx, wz);
                    if !self.ocean_like[p.biome.0 as usize] && !p.water_above && p.top >= SEA_LEVEL {
                        return (wx, p.top + 1, wz);
                    }
                }
            }
        }
        (8, self.column_profile(8, 8).top + 1, 8)
    }

    fn generate(&self, pos: ChunkPos) -> ChunkColumn {
        let (bx, bz) = (pos.x * 16, pos.z * 16);
        let b = &self.blocks;

        // 1–3. Climate and density on the lattice.
        let lat_climate = self.lattice_climate(bx, bz);
        let lat: Vec<[(f32, f32); LAT_Y]> = (0..LAT_XZ * LAT_XZ)
            .map(|i| {
                let (lx, lz) = ((i % LAT_XZ) as i32, (i / LAT_XZ) as i32);
                self.lattice_column(bx + lx * CELL_XZ, bz + lz * CELL_XZ, &lat_climate[i])
            })
            .collect();

        // Per-column climate & biome.
        let mut climates = [Climate::default(); 256];
        let mut biomes = [BiomeId(0); 256];
        for z in 0..16 {
            for x in 0..16 {
                let c = Self::column_climate(&lat_climate, x, z);
                biomes[z * 16 + x] = self.biome_for(&c);
                climates[z * 16 + x] = c;
            }
        }

        // 4. Fill blocks from the interpolated density.
        let mut blocks = vec![b.air; HEIGHT * 256];
        let mut terrain_top = [MIN_Y; 256]; // highest solid block per column
        for z in 0..16 {
            for x in 0..16 {
                let (cx, cz) = (x / CELL_XZ as usize, z / CELL_XZ as usize);
                let (tx, tz) = ((x % 4) as f32 / 4.0, (z % 4) as f32 / 4.0);
                let corners = [
                    &lat[cz * LAT_XZ + cx],
                    &lat[cz * LAT_XZ + cx + 1],
                    &lat[(cz + 1) * LAT_XZ + cx],
                    &lat[(cz + 1) * LAT_XZ + cx + 1],
                ];
                let cold = self.biomes.get(biomes[z * 16 + x]).snowy;
                let mut top_found = false;
                for y in (MIN_Y..MAX_Y).rev() {
                    let rel = y - MIN_Y;
                    let (ly, ty) = ((rel / CELL_Y) as usize, (rel % CELL_Y) as f32 / CELL_Y as f32);
                    let (t, f) = Self::interp(corners, ly, tx, ty, tz);
                    let block = self.base_block(y, t, f, cold);
                    if block == b.stone && !top_found {
                        top_found = true;
                        terrain_top[z * 16 + x] = y;
                    }
                    blocks[idx(x, y, z)] = block;
                }
            }
        }

        // Deepslate, bedrock.
        let mut rng = Rng::for_position(self.seed, pos.x, pos.z, 1);
        for z in 0..16 {
            for x in 0..16 {
                for y in MIN_Y..8 {
                    let i = idx(x, y, z);
                    if blocks[i] == b.stone {
                        // Fully deepslate below Y 0; mixed between 0 and 8.
                        if y < 0 || rng.below(8) as i32 >= y {
                            blocks[i] = b.deepslate;
                        }
                    }
                }
                blocks[idx(x, MIN_Y, z)] = b.bedrock;
                for y in MIN_Y + 1..MIN_Y + 5 {
                    if rng.below(5) as i32 >= y - MIN_Y {
                        blocks[idx(x, y, z)] = b.bedrock;
                    }
                }
            }
        }

        // 6. Surface layers.
        for z in 0..16 {
            for x in 0..16 {
                self.apply_surface(&mut blocks, &terrain_top, &climates, &biomes, x, z, bx, bz);
            }
        }

        // 7. Ores.
        self.place_ores(&mut blocks, &biomes, pos);

        // 8. Vegetation (trees may come from neighbouring chunks).
        features::place_trees(self, &mut blocks, pos, &biomes, &terrain_top);
        self.place_plants(&mut blocks, &biomes, &terrain_top, pos);

        // Pack into sections.
        let mut col = ChunkColumn::new(pos);
        for s in 0..HEIGHT / 16 {
            let slice: &[BlockId; VOLUME] =
                blocks[s * VOLUME..(s + 1) * VOLUME].try_into().expect("section slice");
            *col.section_mut(s) = Section::from_blocks(slice);
        }
        for z in 0..16 {
            for x in 0..16 {
                col.set_biome(x, z, biomes[z * 16 + x]);
            }
        }
        col
    }
}

impl Overworld {
    #[allow(clippy::too_many_arguments)]
    fn apply_surface(
        &self,
        blocks: &mut [BlockId],
        tops: &[i32; 256],
        climates: &[Climate; 256],
        biomes: &[BiomeId; 256],
        x: usize,
        z: usize,
        bx: i32,
        bz: i32,
    ) {
        let b = &self.blocks;
        let top = tops[z * 16 + x];
        if top <= MIN_Y {
            return;
        }
        let biome_id = biomes[z * 16 + x];
        let biome = self.biomes.get(biome_id);
        let bb = &self.biome_blocks[biome_id.0 as usize];
        let (wx, wz) = (bx + x as i32, bz + z as i32);
        let noise = self.surface_noise.sample(wx as f64, 0.0, wz as f64) as f32;
        let above = blocks.get(idx(x, top + 1, z)).copied().unwrap_or(b.air);
        let underwater = above == b.water || above == b.ice;

        // Steepness from neighbouring tops (inside the chunk; edges use the climate height).
        let height_at = |dx: i32, dz: i32| -> f32 {
            let (nx, nz) = (x as i32 + dx, z as i32 + dz);
            if (0..16).contains(&nx) && (0..16).contains(&nz) {
                tops[nz as usize * 16 + nx as usize] as f32
            } else {
                climates[z * 16 + x].height
            }
        };
        let slope = [(1, 0), (-1, 0), (0, 1), (0, -1)]
            .iter()
            .map(|&(dx, dz)| (height_at(dx, dz) - top as f32).abs())
            .fold(0.0f32, f32::max);
        let is_mountain = biome.name == "mountains";

        let (mut surface, mut filler) =
            if underwater { (bb.underwater, bb.underwater) } else { (bb.surface, bb.filler) };
        // Mountains: bare rock on steep slopes and high up, snow on the peaks.
        if !underwater && is_mountain {
            let rock_line = 140.0 + noise * 10.0;
            if top as f32 > 172.0 + noise * 8.0 {
                surface = b.snow_block;
                filler = b.stone;
            } else if slope > 2.5 || top as f32 > rock_line {
                surface = b.stone;
                filler = b.stone;
            }
        } else if !underwater
            && slope > 4.0
            && bb.surface != b.stone
            && surface != bb.underwater
            && filler == b.dirt
        {
            // Very steep grassy cliffs elsewhere show rock too.
            surface = b.stone;
            filler = b.stone;
        }
        if underwater && top < SEA_LEVEL - 12 && bb.underwater != b.gravel && biome.continentalness[1] < -0.15
        {
            surface = b.gravel; // deeper sea floors are gravelly
            filler = b.gravel;
        }

        let depth = bb.filler_depth as i32 + (noise * 1.5 + 1.0) as i32;
        let mut y = top;
        let mut placed = 0;
        while y > MIN_Y && placed <= depth + bb.deep_filler.map(|d| d.1 as i32).unwrap_or(0) {
            let i = idx(x, y, z);
            if blocks[i] != b.stone {
                break; // reached a cave or already-replaced rock
            }
            blocks[i] = if placed == 0 {
                surface
            } else if placed <= depth {
                filler
            } else if let Some((deep, _)) = bb.deep_filler {
                deep
            } else {
                break;
            };
            placed += 1;
            y -= 1;
        }
    }

    fn place_ores(&self, blocks: &mut [BlockId], biomes: &[BiomeId; 256], pos: ChunkPos) {
        let b = &self.blocks;
        for (n, rule) in self.ores.iter().enumerate() {
            let mut rng = Rng::for_position(self.seed, pos.x, pos.z, 100 + n as u64);
            for _ in 0..rule.attempts(&mut rng) {
                let (x, z) = (rng.range(0, 15), rng.range(0, 15));
                let y = rule.pick_y(&mut rng);
                if !rule.biomes.is_empty() && !rule.biomes.contains(&biomes[z as usize * 16 + x as usize]) {
                    continue;
                }
                for o in ores::vein_offsets(rule.size, &mut rng) {
                    let (vx, vy, vz) = (x + o[0], y + o[1], z + o[2]);
                    if !(0..16).contains(&vx) || !(0..16).contains(&vz) || !(MIN_Y..MAX_Y).contains(&vy) {
                        continue; // veins stay inside their own chunk
                    }
                    let i = idx(vx as usize, vy, vz as usize);
                    let host = blocks[i];
                    if host == b.stone {
                        blocks[i] = rule.block;
                    } else if host == b.deepslate {
                        blocks[i] = rule.deepslate.unwrap_or(rule.block);
                    }
                }
            }
        }
    }

    fn place_plants(
        &self,
        blocks: &mut [BlockId],
        biomes: &[BiomeId; 256],
        tops: &[i32; 256],
        pos: ChunkPos,
    ) {
        let mut rng = Rng::for_position(self.seed, pos.x, pos.z, 7);
        // Decide per column which biome's plant list applies, by sampling positions.
        for _ in 0..48 {
            let (x, z) = (rng.range(0, 15) as usize, rng.range(0, 15) as usize);
            let biome = biomes[z * 16 + x];
            let plants = &self.biome_plants[biome.0 as usize];
            let total: u32 = plants.iter().map(|p| p.1).sum();
            if total == 0 {
                continue;
            }
            // Each sample places with probability total/48, choosing a plant by weight.
            if rng.below(48) >= total.min(48) {
                continue;
            }
            let mut pick = rng.below(total);
            let plant = plants.iter().find(|p| {
                if pick < p.1 {
                    true
                } else {
                    pick -= p.1;
                    false
                }
            });
            let Some(&(plant, _)) = plant else { continue };
            let top = tops[z * 16 + x];
            if top + 1 >= MAX_Y || top <= MIN_Y {
                continue;
            }
            let ground = blocks[idx(x, top, z)];
            let above = idx(x, top + 1, z);
            if blocks[above] == BlockId::AIR && self.trees.supports_plant(ground, plant) {
                blocks[above] = plant;
            }
        }
    }

    /// Picks a tree kind for a biome (None if the biome has no trees).
    pub(crate) fn tree_for(&self, biome: BiomeId, rng: &mut Rng) -> Option<TreeKind> {
        let (kinds, total) = &self.biome_trees[biome.0 as usize];
        if *total == 0 {
            return None;
        }
        let mut pick = rng.below(*total);
        for &(k, w) in kinds {
            if pick < w {
                return Some(k);
            }
            pick -= w;
        }
        None
    }

    pub(crate) fn seed(&self) -> u64 {
        self.seed
    }

    pub(crate) fn surface_block(&self, biome: BiomeId) -> BlockId {
        self.biome_blocks[biome.0 as usize].surface
    }

    /// Whether a tree may grow on top of column height `top` in `biome`.
    /// Uses only information both a chunk and its neighbours can compute
    /// identically (not slopes), so cross-border trees always agree.
    pub(crate) fn tree_allowed(&self, biome: BiomeId, top: i32) -> bool {
        top >= SEA_LEVEL && !(self.biomes.get(biome).name == "mountains" && top > 135)
    }

    #[cfg(test)]
    pub(crate) fn water(&self) -> BlockId {
        self.blocks.water
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn generator(seed: u64) -> Overworld {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/data/");
        let read = |f: &str| std::fs::read_to_string(format!("{dir}{f}")).unwrap();
        let blocks = BlockRegistry::from_toml(&read("blocks.toml")).unwrap();
        let biomes = Arc::new(BiomeRegistry::from_toml(&read("biomes.toml")).unwrap());
        Overworld::new(seed, &blocks, biomes, &read("ores.toml")).unwrap()
    }

    #[test]
    fn curve_interpolates_and_clamps() {
        let pts = [(0.0, 0.0), (1.0, 10.0), (2.0, 0.0)];
        assert_eq!(curve(-5.0, &pts), 0.0);
        assert_eq!(curve(0.5, &pts), 5.0);
        assert_eq!(curve(1.5, &pts), 5.0);
        assert_eq!(curve(9.0, &pts), 0.0);
    }

    #[test]
    fn deterministic_generation() {
        let g = generator(42);
        let a = g.generate(ChunkPos::new(5, -7));
        let b = generator(42).generate(ChunkPos::new(5, -7));
        for (sa, sb) in a.sections().iter().zip(b.sections()) {
            assert_eq!(sa, sb);
        }
        let c = generator(43).generate(ChunkPos::new(5, -7));
        assert!(
            a.sections().iter().zip(c.sections()).any(|(x, y)| x != y),
            "different seed, different world"
        );
    }

    #[test]
    fn column_profile_matches_generated_chunk() {
        // The cross-chunk feature logic relies on this exact agreement.
        let g = generator(7);
        for (cx, cz) in [(0, 0), (3, -2), (-9, 14)] {
            let col = g.generate(ChunkPos::new(cx, cz));
            for (x, z) in [(0usize, 0usize), (5, 11), (15, 15), (8, 3)] {
                let p = g.column_profile(cx * 16 + x as i32, cz * 16 + z as i32);
                assert_eq!(col.biome(x, z), p.biome);
                // The generated top may be a tree or plant above p.top; find the topmost terrain block.
                let solid_top = (MIN_Y..MAX_Y).rev().find(|&y| {
                    let id = col.get(x, y, z);
                    id != BlockId::AIR
                        && id != g.water()
                        && id != g.blocks.ice
                        && !g.trees.is_vegetation(id)
                        && id != g.blocks.lava
                });
                assert_eq!(solid_top, Some(p.top), "chunk ({cx},{cz}) column ({x},{z})");
            }
        }
    }

    #[test]
    fn world_has_oceans_land_mountains_and_varied_biomes() {
        let g = generator(12345);
        let mut seen = std::collections::HashSet::new();
        let (mut ocean, mut total, mut max_h, mut min_h) = (0, 0, i32::MIN, i32::MAX);
        for i in 0..60 {
            for j in 0..60 {
                let p = g.column_profile(i * 160 - 4800, j * 160 - 4800);
                seen.insert(p.biome);
                total += 1;
                ocean += p.water_above as i32;
                max_h = max_h.max(p.top);
                min_h = min_h.min(p.top);
            }
        }
        let share = ocean as f32 / total as f32;
        assert!((0.15..0.6).contains(&share), "ocean share {share}");
        assert!(max_h > 150, "mountains reach {max_h}");
        assert!(min_h < 30, "deep oceans reach {min_h}");
        assert!(seen.len() >= 9, "only {} biomes in a 9600-block square", seen.len());
    }

    #[test]
    fn layers_bedrock_deepslate_and_ores_exist() {
        let g = generator(99);
        let (mut deepslate, mut air_underground, mut ores) = (0, 0, 0);
        for cx in 0..4 {
            let col = g.generate(ChunkPos::new(cx, 0));
            assert_eq!(col.get(3, MIN_Y, 3), g.blocks.bedrock);
            for y in (MIN_Y + 6)..40 {
                for x in 0..16 {
                    let id = col.get(x, y, 7);
                    if id == g.blocks.deepslate {
                        deepslate += 1;
                    } else if id == BlockId::AIR || id == g.blocks.lava {
                        air_underground += 1;
                    } else if id != g.blocks.stone && id != g.water() {
                        ores += 1;
                    }
                }
            }
        }
        assert!(deepslate > 1000, "deepslate {deepslate}");
        let _ = air_underground; // cave volume is checked broadly in cave_volume_share
        assert!(ores > 20, "ores/rock variants {ores}");
    }

    #[test]
    fn spawn_is_on_dry_land() {
        let g = generator(12345);
        let (x, y, z) = g.find_spawn();
        let p = g.column_profile(x, z);
        assert_eq!(p.top + 1, y);
        assert!(y > SEA_LEVEL && !p.water_above);
    }
}
#[cfg(test)]
mod cave_tests {
    use super::*;
    /// Prints the share of underground volume that is cave (run with --nocapture).
    #[test]
    fn cave_volume_share() {
        let g = tests::generator(99);
        let (mut under, mut cave) = (0u64, 0u64);
        for i in 0..12 {
            let col = g.generate(ChunkPos::new(i * 37 - 200, i * 23 - 100));
            for x in 0..16 {
                for z in 0..16 {
                    let top = (MIN_Y..MAX_Y)
                        .rev()
                        .find(|&y| {
                            col.get(x, y, z) == g.blocks.stone || col.get(x, y, z) == g.blocks.deepslate
                        })
                        .unwrap_or(MIN_Y);
                    for y in (MIN_Y + 5)..(top - 8) {
                        under += 1;
                        let id = col.get(x, y, z);
                        if id == BlockId::AIR || id == g.blocks.lava {
                            cave += 1;
                        }
                    }
                }
            }
        }
        let share = cave as f64 / under as f64;
        eprintln!("cave share of underground volume: {:.2}%", share * 100.0);
        assert!((0.02..0.25).contains(&share), "cave share {share}");
    }
}
#[cfg(test)]
mod robustness_tests {
    use super::*;

    /// Climate and density must be finite everywhere, including noise tails
    /// (a NaN here once produced walls up to the build limit).
    #[test]
    fn climate_and_density_are_always_finite() {
        let g = tests::generator(12345);
        for i in 0..40_000 {
            let (x, z) = ((i % 200) * 151 - 15_000, (i / 200) * 149 - 15_000);
            let c = g.climate(x, z);
            assert!(c.height.is_finite() && c.roughness.is_finite(), "climate NaN at {x},{z}: {c:?}");
            assert!((-80.0..400.0).contains(&c.height), "height {} at {x},{z}", c.height);
            if i % 50 == 0 {
                for y in (MIN_Y..MAX_Y).step_by(24) {
                    let (t, f) = g.density(x, y, z, &c);
                    assert!(t.is_finite() && f.is_finite(), "density NaN at {x},{y},{z}");
                }
            }
        }
    }

    #[test]
    fn nothing_reaches_the_build_limit() {
        let g = tests::generator(12345);
        for i in 0..4000 {
            let p = g.column_profile((i % 64) * 97 - 3000, (i / 64) * 89 - 3000);
            assert!(p.top < 300, "terrain at {} is suspiciously high", p.top);
        }
    }
}
#[cfg(test)]
mod find_cave {
    use super::*;
    #[test]
    #[ignore] // developer helper: cargo test -p ld_worldgen find_big_cave -- --ignored --nocapture
    fn find_big_cave() {
        let g = tests::generator(12345);
        for cx in -6..6 {
            for cz in -6..6 {
                let col = g.generate(ChunkPos::new(cx, cz));
                for y in (-40..30).step_by(2) {
                    let air = (2..14).all(|d| col.get(d, y, 8) == BlockId::AIR)
                        && (2..14).all(|d| col.get(8, y, d) == BlockId::AIR);
                    if air {
                        eprintln!("big cave at {} {} {}", cx * 16 + 8, y, cz * 16 + 8);
                        return;
                    }
                }
            }
        }
    }
}
