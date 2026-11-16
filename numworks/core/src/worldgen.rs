//! World generation, one column (1 x HEIGHT x 1 blocks) at a time.
//!
//! A simplified, calculator-sized version of the PC game's overworld:
//! continents and oceans, hills and mountains, seven biomes, worm caves and
//! caverns with lava at the bottom, ore veins, granite/diorite/andesite
//! pockets, a deepslate layer and trees. Everything is a pure function of the
//! seed and the coordinates, so any column can be rebuilt at any time — that
//! is what lets the world be endless with only a small window kept in memory.
//!
//! Heights are squeezed to fit the calculator: the world is [`HEIGHT`] blocks
//! tall with the sea at [`SEA_LEVEL`] (the PC game uses 384 and 63).

use crate::blocks::*;
use crate::math::{ifloor, smoothstep};
use crate::noise::{fbm2, hash2, hash3, value3};

/// World height in blocks (y = 0 is the bedrock floor).
pub const HEIGHT: usize = 48;
/// Water fills everything at or below this height that the terrain leaves empty.
pub const SEA_LEVEL: i32 = 20;
/// Below this height stone turns into deepslate (with a ragged border).
const DEEPSLATE_LEVEL: i32 = 8;
/// Caves at or below this height fill with lava.
const LAVA_LEVEL: i32 = 4;

/// Biomes of the calculator edition (a subset of the PC game's).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Biome {
    /// Open water over sand or gravel.
    Ocean,
    /// Sandy shore.
    Beach,
    /// Grass with the odd oak.
    Plains,
    /// Dense oaks and birches.
    Forest,
    /// Sand, sandstone and cacti.
    Desert,
    /// Snowy grass, frozen water, spruces.
    Snowy,
    /// High stone peaks with snow caps.
    Mountains,
}

impl Biome {
    /// Name shown to the player.
    pub fn name(self) -> &'static str {
        match self {
            Biome::Ocean => "Ocean",
            Biome::Beach => "Beach",
            Biome::Plains => "Plains",
            Biome::Forest => "Forest",
            Biome::Desert => "Desert",
            Biome::Snowy => "Snowy Plains",
            Biome::Mountains => "Mountains",
        }
    }
}

/// The large-scale shape of one column.
#[derive(Clone, Copy, Debug)]
pub struct ColumnInfo {
    /// Height of the highest terrain block (before caves and trees).
    pub height: i32,
    /// Biome.
    pub biome: Biome,
}

// Seeds for the independent noise layers, mixed with the world seed.
const S_CONT: u32 = 0xC047_1E01;
const S_MOUNT: u32 = 0x3A7D_0F02;
const S_HILLS: u32 = 0x9B11_2E03;
const S_TEMP: u32 = 0x7E3A_9904;
const S_HUMID: u32 = 0x4D5C_AA05;
const S_CAVE_A: u32 = 0x1CA7_E006;
const S_CAVE_B: u32 = 0x2CA7_E007;
const S_CAVERN: u32 = 0x3CA7_E008;
const S_STONE: u32 = 0x5701_E009;
const S_ORE: u32 = 0x0BE5_000A;
const S_TREE: u32 = 0x7BEE_000B;
const S_MISC: u32 = 0x0D15_C00C;

/// Climate and height of the column at `(x, z)`.
pub fn column_info(seed: u32, x: i32, z: i32) -> ColumnInfo {
    let (fx, fz) = (x as f32, z as f32);
    let cont = fbm2(fx / 256.0, fz / 256.0, seed ^ S_CONT, 4);
    let mount = fbm2(fx / 128.0, fz / 128.0, seed ^ S_MOUNT, 3);
    let hills = fbm2(fx / 40.0, fz / 40.0, seed ^ S_HILLS, 3);
    let temp = fbm2(fx / 320.0, fz / 320.0, seed ^ S_TEMP, 2);
    let humid = fbm2(fx / 256.0, fz / 256.0, seed ^ S_HUMID, 2);

    // Continents: negative values sink below the sea.
    let base = SEA_LEVEL as f32 + 2.0 + cont * 16.0;
    // Mountains only rise on land.
    let m = smoothstep(0.08, 0.4, mount) * smoothstep(-0.05, 0.15, cont);
    let h = base + hills * 4.0 * (1.0 + 1.5 * m) + m * 20.0;
    let height = (ifloor(h)).clamp(3, HEIGHT as i32 - 4);

    let biome = if height < SEA_LEVEL - 1 {
        Biome::Ocean
    } else if height <= SEA_LEVEL + 1 && temp > -0.2 {
        Biome::Beach
    } else if m > 0.45 && height >= SEA_LEVEL + 11 {
        Biome::Mountains
    } else if temp < -0.18 {
        Biome::Snowy
    } else if temp > 0.15 && humid < 0.05 {
        Biome::Desert
    } else if humid > 0.05 {
        Biome::Forest
    } else {
        Biome::Plains
    };
    ColumnInfo { height, biome }
}

/// Fills `out` (index = y) with the blocks of the column at `(x, z)` and
/// returns its biome. World edits are applied separately by `world`.
pub fn generate_column(seed: u32, x: i32, z: i32, out: &mut [Block; HEIGHT]) -> Biome {
    let info = column_info(seed, x, z);
    let h = info.height;
    let (fx, fz) = (x as f32, z as f32);

    // 1. Rock, water and air.
    let deep_edge = DEEPSLATE_LEVEL + (hash2(x, z, seed ^ S_MISC) % 3) as i32 - 1;
    for (y, cell) in out.iter_mut().enumerate() {
        let y = y as i32;
        *cell = if y == 0 || (y == 1 && hash3(x, y, z, seed ^ S_MISC) & 1 == 0) {
            BEDROCK
        } else if y <= h {
            if y < deep_edge { DEEPSLATE } else { STONE }
        } else if y <= SEA_LEVEL {
            if y == SEA_LEVEL && info.biome == Biome::Snowy { ICE } else { WATER }
        } else {
            AIR
        };
    }

    // 2. Stone pockets (granite, diorite, andesite), one kind per region.
    let pocket = match hash2(ifloor(fx / 24.0), ifloor(fz / 24.0), seed ^ S_STONE) % 3 {
        0 => GRANITE,
        1 => DIORITE,
        _ => ANDESITE,
    };
    for y in deep_edge.max(2)..(h - 3) {
        if value3(fx / 10.0, y as f32 / 7.0, fz / 10.0, seed ^ S_STONE) > 0.76 {
            out[y as usize] = pocket;
        }
    }

    // 3. Surface layers.
    surface(&info, x, z, seed, out);

    // 4. Caves: worm tunnels where two noise fields are both near their middle,
    //    plus rarer big caverns deep down. The top few blocks under water stay
    //    sealed so the sea doesn't pour into caves.
    let sealed_top = if h < SEA_LEVEL { h - 3 } else { h };
    for y in 2..=sealed_top.min(HEIGHT as i32 - 1) {
        let fy = y as f32;
        let a = value3(fx / 18.0, fy / 9.0, fz / 18.0, seed ^ S_CAVE_A) - 0.5;
        let b = value3(fx / 18.0, fy / 9.0, fz / 18.0, seed ^ S_CAVE_B) - 0.5;
        let worm = a * a + b * b < 0.0035;
        let cavern = y < SEA_LEVEL - 4 && value3(fx / 28.0, fy / 12.0, fz / 28.0, seed ^ S_CAVERN) > 0.8;
        if worm || cavern {
            out[y as usize] = if y <= LAVA_LEVEL { LAVA } else { AIR };
        }
    }

    // 5. Ores in small veins (2x2x2 cells, 3 in 4 blocks of a chosen cell).
    for y in 1..=h.min(HEIGHT as i32 - 1) {
        let b = out[y as usize];
        if b != STONE && b != DEEPSLATE {
            continue;
        }
        let ore = ore_at(seed, x, y, z);
        if ore != AIR && hash3(x, y, z, seed ^ S_ORE) & 3 != 0 {
            out[y as usize] = if b == DEEPSLATE { deepslate_variant(ore) } else { ore };
        }
    }

    // 6. Trees and cacti (they may reach in from neighbouring columns).
    trees(seed, x, z, out);

    // 6b. Plants on the surface: grass and flowers, ferns in the cold,
    //     dead bushes in the desert.
    plants(seed, x, z, &info, out);

    // 7. Ruins of the Builders, with lore tablets.
    crate::structures::apply(seed, x, z, out);
    info.biome
}

/// Top layers of the column: grass and dirt, sand, snow...
fn surface(info: &ColumnInfo, x: i32, z: i32, seed: u32, out: &mut [Block; HEIGHT]) {
    let h = info.height;
    let underwater = h < SEA_LEVEL;
    let (top, filler, depth, deep, deep_depth) = match info.biome {
        Biome::Ocean => {
            let floor = if h < SEA_LEVEL - 6 { GRAVEL } else { SAND };
            (floor, floor, 3, AIR, 0)
        }
        Biome::Beach => (SAND, SAND, 3, SANDSTONE, 2),
        Biome::Desert => (SAND, SAND, 4, SANDSTONE, 3),
        Biome::Snowy => (SNOWY_GRASS_BLOCK, DIRT, 3, AIR, 0),
        Biome::Mountains => {
            if h > SEA_LEVEL + 21 + (hash2(x, z, seed ^ S_MISC) % 3) as i32 {
                (SNOW_BLOCK, STONE, 1, AIR, 0)
            } else if h > SEA_LEVEL + 16 {
                (STONE, STONE, 1, AIR, 0)
            } else {
                (GRASS_BLOCK, DIRT, 3, AIR, 0)
            }
        }
        Biome::Plains | Biome::Forest => (GRASS_BLOCK, DIRT, 3, AIR, 0),
    };
    // Grass doesn't grow under water: use sand near the shore, gravel deeper.
    let top = if underwater && matches!(top, GRASS_BLOCK | SNOWY_GRASS_BLOCK) {
        if h >= SEA_LEVEL - 4 { SAND } else { GRAVEL }
    } else {
        top
    };
    for d in 0..(depth + deep_depth) {
        let y = h - d;
        if y <= 1 {
            break;
        }
        out[y as usize] = if d == 0 {
            top
        } else if d < depth {
            filler
        } else {
            deep
        };
    }
}

/// Puts a plant on top of the column if its biome grows one there.
fn plants(seed: u32, x: i32, z: i32, info: &ColumnInfo, out: &mut [Block; HEIGHT]) {
    let y = info.height + 1;
    if !(1..HEIGHT as i32 - 1).contains(&y) || out[y as usize] != AIR {
        return;
    }
    let ground = out[(y - 1) as usize];
    let r = hash2(x, z, seed ^ S_MISC ^ 0x9A17) % 1000;
    let plant = match (info.biome, ground) {
        (Biome::Plains, GRASS_BLOCK) => match r {
            0..=219 => SHORT_GRASS,
            220..=239 => DANDELION,
            240..=259 => POPPY,
            260..=269 => CORNFLOWER,
            _ => AIR,
        },
        (Biome::Forest, GRASS_BLOCK) | (Biome::Mountains, GRASS_BLOCK) => match r {
            0..=139 => SHORT_GRASS,
            140..=169 => FERN,
            170..=179 => POPPY,
            _ => AIR,
        },
        (Biome::Snowy, SNOWY_GRASS_BLOCK) => {
            if r < 60 {
                FERN
            } else {
                AIR
            }
        }
        (Biome::Desert, SAND) => {
            if r < 12 {
                DEAD_BUSH
            } else {
                AIR
            }
        }
        _ => AIR,
    };
    out[y as usize] = plant;
}

/// Which ore (if any) a vein cell holds at this position.
fn ore_at(seed: u32, x: i32, y: i32, z: i32) -> Block {
    let r = hash3(x >> 1, y >> 1, z >> 1, seed ^ S_ORE ^ 0x55) % 1000;
    // (ore, lowest y, highest y, share of cells out of 1000)
    const TABLE: [(Block, i32, i32, u32); 6] = [
        (COAL_ORE, 4, 46, 16),
        (IRON_ORE, 1, 38, 10),
        (COPPER_ORE, 10, 34, 8),
        (GOLD_ORE, 1, 15, 3),
        (LAPIS_ORE, 1, 15, 2),
        (DIAMOND_ORE, 1, 9, 2),
    ];
    let mut acc = 0;
    for (ore, lo, hi, share) in TABLE {
        if r < acc + share {
            return if (lo..=hi).contains(&y) { ore } else { AIR };
        }
        acc += share;
    }
    AIR
}

/// The deepslate version of an ore.
fn deepslate_variant(ore: Block) -> Block {
    match ore {
        COAL_ORE => DEEPSLATE_COAL_ORE,
        IRON_ORE => DEEPSLATE_IRON_ORE,
        COPPER_ORE => DEEPSLATE_COPPER_ORE,
        GOLD_ORE => DEEPSLATE_GOLD_ORE,
        LAPIS_ORE => DEEPSLATE_LAPIS_ORE,
        DIAMOND_ORE => DEEPSLATE_DIAMOND_ORE,
        other => other,
    }
}

/// Size of the grid cells that hold at most one tree each.
const TREE_CELL: i32 = 6;

/// Adds the parts of nearby trees that fall inside column `(x, z)`.
fn trees(seed: u32, x: i32, z: i32, out: &mut [Block; HEIGHT]) {
    let (cx, cz) = (x.div_euclid(TREE_CELL), z.div_euclid(TREE_CELL));
    for tcx in cx - 1..=cx + 1 {
        for tcz in cz - 1..=cz + 1 {
            let r = hash2(tcx, tcz, seed ^ S_TREE);
            let ox = tcx * TREE_CELL + 1 + (r % 4) as i32;
            let oz = tcz * TREE_CELL + 1 + ((r >> 4) % 4) as i32;
            let (dx, dz) = (x - ox, z - oz);
            if dx.abs() > 2 || dz.abs() > 2 {
                continue; // cheap reject before computing the climate
            }
            let info = column_info(seed, ox, oz);
            let (density, kind) = match info.biome {
                Biome::Forest => {
                    (85, if (r >> 16).is_multiple_of(3) { TreeKind::Birch } else { TreeKind::Oak })
                }
                Biome::Plains => (10, TreeKind::Oak),
                Biome::Snowy => (45, TreeKind::Spruce),
                Biome::Mountains => (25, TreeKind::Spruce),
                Biome::Desert => (20, TreeKind::Cactus),
                Biome::Beach | Biome::Ocean => (0, TreeKind::Oak),
            };
            if (r >> 8) % 100 >= density || info.height < SEA_LEVEL || info.height > HEIGHT as i32 - 10 {
                continue;
            }
            // Mountain stone and snow tops are too high for trees.
            if info.biome == Biome::Mountains && info.height > SEA_LEVEL + 16 {
                continue;
            }
            place_tree(kind, info.height + 1, dx, dz, r, seed, x, z, out);
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum TreeKind {
    Oak,
    Birch,
    Spruce,
    Cactus,
}

/// Writes the slice of one tree at offset `(dx, dz)` from its trunk.
/// `ground` is the first air block above the terrain at the trunk.
#[allow(clippy::too_many_arguments)]
fn place_tree(
    kind: TreeKind,
    ground: i32,
    dx: i32,
    dz: i32,
    r: u32,
    seed: u32,
    x: i32,
    z: i32,
    out: &mut [Block; HEIGHT],
) {
    let rand = (r >> 20) % 3;
    let (log, leaves, trunk) = match kind {
        TreeKind::Oak => (OAK_LOG, OAK_LEAVES, 4 + rand as i32),
        TreeKind::Birch => (BIRCH_LOG, BIRCH_LEAVES, 5 + rand as i32),
        TreeKind::Spruce => (SPRUCE_LOG, SPRUCE_LEAVES, 6 + rand as i32),
        TreeKind::Cactus => (CACTUS, AIR, 1 + rand as i32),
    };
    let set = |out: &mut [Block; HEIGHT], y: i32, b: Block, force: bool| {
        if (0..HEIGHT as i32).contains(&y) && (force || out[y as usize] == AIR) && out[y as usize] != BEDROCK
        {
            out[y as usize] = b;
        }
    };
    if kind == TreeKind::Cactus {
        if dx == 0 && dz == 0 && out[(ground - 1) as usize] == SAND {
            for y in ground..ground + trunk {
                set(out, y, log, false);
            }
        }
        return;
    }
    // Trees only grow on soil.
    let soil = out.get((ground - 1) as usize).copied().unwrap_or(AIR);
    let on_soil = matches!(soil, GRASS_BLOCK | SNOWY_GRASS_BLOCK | DIRT);
    if dx == 0 && dz == 0 && !on_soil {
        return;
    }
    let top = ground + trunk; // first block above the trunk
    match kind {
        TreeKind::Spruce => {
            for y in ground + 2..=top {
                let from_top = top - y;
                let radius =
                    if from_top == 0 { 0 } else { (from_top % 2) + if from_top > 3 { 1 } else { 0 } };
                let inside = dx.abs() <= radius
                    && dz.abs() <= radius
                    && !(radius == 2 && dx.abs() == 2 && dz.abs() == 2);
                if inside {
                    set(out, y, leaves, false);
                }
            }
        }
        _ => {
            for y in top - 3..=top {
                let radius = if y >= top - 1 { 1 } else { 2 };
                if dx.abs() > radius || dz.abs() > radius {
                    continue;
                }
                // Corners are randomly trimmed (always on the top layer), like on PC.
                let trim = dx.abs() == radius && dz.abs() == radius;
                if trim && (y == top || hash3(x, y, z, seed ^ S_TREE) & 1 == 0) {
                    continue;
                }
                set(out, y, leaves, false);
            }
        }
    }
    if dx == 0 && dz == 0 {
        for y in ground..top {
            set(out, y, log, true);
        }
    }
}

/// Finds a dry, grassy place near the origin for the player to start.
/// Returns the block position of the ground column `(x, z)` and its height.
pub fn find_spawn(seed: u32) -> (i32, i32, i32) {
    let mut col = [AIR; HEIGHT];
    for ring in 0..64 {
        for i in -ring..=ring {
            for (x, z) in [(i * 4, -ring * 4), (i * 4, ring * 4), (-ring * 4, i * 4), (ring * 4, i * 4)] {
                let info = column_info(seed, x, z);
                if matches!(info.biome, Biome::Plains | Biome::Forest) && info.height > SEA_LEVEL {
                    generate_column(seed, x, z, &mut col);
                    // Stand on the highest solid block (trees included).
                    let top = (0..HEIGHT).rev().find(|&y| def(col[y]).solid).unwrap_or(0) as i32;
                    return (x, top, z);
                }
            }
        }
    }
    (0, HEIGHT as i32 - 1, 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic() {
        let (mut a, mut b) = ([AIR; HEIGHT], [AIR; HEIGHT]);
        for i in 0..200 {
            generate_column(7, i * 13 - 900, i * 5, &mut a);
            generate_column(7, i * 13 - 900, i * 5, &mut b);
            assert_eq!(a, b);
            assert_eq!(a[0], BEDROCK);
        }
    }

    #[test]
    fn varied_world() {
        // Over a big area we should meet every biome and plenty of ores.
        let mut seen = [false; 7];
        let mut col = [AIR; HEIGHT];
        let mut ores = 0;
        let mut caves = 0;
        for i in -60..60 {
            for j in -60..60 {
                let (x, z) = (i * 23, j * 23);
                let biome = generate_column(12345, x, z, &mut col);
                seen[biome as usize] = true;
                let h = column_info(12345, x, z).height as usize;
                ores +=
                    col.iter().filter(|&&b| matches!(b, COAL_ORE | IRON_ORE | DEEPSLATE_IRON_ORE)).count();
                caves += col[2..h.saturating_sub(4)].iter().filter(|&&b| b == AIR || b == LAVA).count();
            }
        }
        assert!(seen.iter().all(|&s| s), "biomes seen: {seen:?}");
        assert!(ores > 1000, "ores: {ores}");
        assert!(caves > 1000, "cave blocks: {caves}");
    }

    #[test]
    fn spawn_is_on_land() {
        for seed in [1, 2, 3, 99, 4242] {
            let (x, y, z) = find_spawn(seed);
            assert!(y >= SEA_LEVEL, "seed {seed}: spawn at {x} {y} {z}");
        }
    }
}
