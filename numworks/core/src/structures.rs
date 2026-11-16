//! Ruins of the Builders, each holding a lore tablet — the game's namesake.
//!
//! Three kinds, one per era of the story (as on PC):
//! * **surface ruins** (golden age): broken stone-brick walls on a floor,
//!   in grassy and snowy biomes;
//! * **desert halls** (expansion): half-buried sandstone halls in deserts;
//! * **deep vaults** (what they became): sealed deepslate-brick rooms near
//!   the bottom of the world.
//!
//! Like the rest of the world, a ruin is a pure function of the seed: the
//! world is split into 48x48 cells, each with at most one surface ruin and
//! one deep vault, and every column just checks whether it lies inside one.

use crate::blocks::*;
use crate::mobs::{Era, LORE};
use crate::noise::{hash2, hash3};
use crate::worldgen::{Biome, HEIGHT, SEA_LEVEL, column_info};

/// Size of the cells that hold at most one ruin of each kind.
const CELL: i32 = 48;
const S_RUIN: u32 = 0x5EED_0A11;
const S_VAULT: u32 = 0x5EED_0B22;
const S_DECAY: u32 = 0x5EED_0C33;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    Surface,
    DesertHall,
    Vault,
}

/// A ruin placed in the world.
#[derive(Clone, Copy, Debug)]
struct Site {
    kind: Kind,
    /// Smallest x and z of the footprint.
    x0: i32,
    z0: i32,
    /// Floor height.
    y0: i32,
    /// Footprint width (square).
    size: i32,
}

/// The surface ruin (or desert hall) of cell `(cx, cz)`, if any.
fn surface_site(seed: u32, cx: i32, cz: i32) -> Option<Site> {
    let h = hash2(cx, cz, seed ^ S_RUIN);
    if h % 100 >= 40 {
        return None;
    }
    let x0 = cx * CELL + 6 + ((h >> 8) % (CELL as u32 - 18)) as i32;
    let z0 = cz * CELL + 6 + ((h >> 16) % (CELL as u32 - 18)) as i32;
    let info = column_info(seed, x0 + 4, z0 + 4);
    if info.height <= SEA_LEVEL || info.height > HEIGHT as i32 - 9 {
        return None;
    }
    match info.biome {
        Biome::Desert => Some(Site { kind: Kind::DesertHall, x0, z0, y0: info.height - 3, size: 9 }),
        Biome::Plains | Biome::Forest | Biome::Snowy | Biome::Mountains => {
            Some(Site { kind: Kind::Surface, x0, z0, y0: info.height, size: 7 })
        }
        _ => None,
    }
}

/// The deep vault of cell `(cx, cz)`, if any.
fn vault_site(seed: u32, cx: i32, cz: i32) -> Option<Site> {
    let h = hash2(cx, cz, seed ^ S_VAULT);
    if h % 100 >= 35 {
        return None;
    }
    let x0 = cx * CELL + 4 + ((h >> 8) % (CELL as u32 - 12)) as i32;
    let z0 = cz * CELL + 4 + ((h >> 16) % (CELL as u32 - 12)) as i32;
    Some(Site { kind: Kind::Vault, x0, z0, y0: 2 + ((h >> 24) % 3) as i32, size: 7 })
}

/// Adds the parts of any ruin that cover column `(x, z)`.
pub fn apply(seed: u32, x: i32, z: i32, out: &mut [Block; HEIGHT]) {
    let (cx, cz) = (x.div_euclid(CELL), z.div_euclid(CELL));
    for site in [surface_site(seed, cx, cz), vault_site(seed, cx, cz)].into_iter().flatten() {
        let (lx, lz) = (x - site.x0, z - site.z0);
        if (0..site.size).contains(&lx) && (0..site.size).contains(&lz) {
            build(seed, &site, x, z, lx, lz, out);
        }
    }
}

/// Weathered stone bricks: plain, mossy or cracked.
fn brick(seed: u32, x: i32, y: i32, z: i32) -> Block {
    match hash3(x, y, z, seed ^ S_DECAY) % 10 {
        0..=3 => MOSSY_STONE_BRICKS,
        4..=5 => CRACKED_STONE_BRICKS,
        _ => STONE_BRICKS,
    }
}

#[allow(clippy::too_many_arguments)]
fn build(seed: u32, s: &Site, x: i32, z: i32, lx: i32, lz: i32, out: &mut [Block; HEIGHT]) {
    let last = s.size - 1;
    let edge = lx == 0 || lz == 0 || lx == last || lz == last;
    let corner = (lx == 0 || lx == last) && (lz == 0 || lz == last);
    let centre = lx == s.size / 2 && lz == s.size / 2;
    let door = lz == 0 && lx == s.size / 2;
    let decay = hash3(x, 0, z, seed ^ S_DECAY);
    let set = |out: &mut [Block; HEIGHT], y: i32, b: Block| {
        if (1..HEIGHT as i32).contains(&y) {
            out[y as usize] = b;
        }
    };
    let y0 = s.y0;
    match s.kind {
        Kind::Surface => {
            // Foundation under the floor, then the floor (with holes).
            for y in (y0 - 4).max(1)..y0 {
                if !def(out[y as usize]).solid {
                    set(out, y, COBBLESTONE);
                }
            }
            if !decay.is_multiple_of(7) || centre {
                set(out, y0, brick(seed, x, y0, z));
            }
            for y in y0 + 1..y0 + 6 {
                set(out, y, AIR);
            }
            if edge && !door {
                // Broken walls: 0–3 blocks high; corners stand taller.
                let h = if corner { 3 + (decay % 2) as i32 } else { (decay % 4) as i32 };
                for y in y0 + 1..=y0 + h {
                    set(
                        out,
                        y,
                        if corner && y == y0 + h { CHISELED_STONE_BRICKS } else { brick(seed, x, y, z) },
                    );
                }
            }
            if centre {
                set(out, y0 + 1, LORE_TABLET);
            }
        }
        Kind::DesertHall => {
            for y in (y0 - 3).max(1)..y0 {
                set(out, y, SANDSTONE);
            }
            set(out, y0, CUT_SANDSTONE);
            let roof = y0 + 5;
            for y in y0 + 1..roof {
                set(out, y, if edge && !(door && y <= y0 + 2) { SANDSTONE } else { AIR });
            }
            // Half the roof has fallen in.
            if edge || !decay.is_multiple_of(3) {
                set(out, roof, if edge { CHISELED_SANDSTONE } else { CUT_SANDSTONE });
            }
            if centre {
                set(out, y0 + 1, LORE_TABLET);
            }
        }
        Kind::Vault => {
            set(out, y0, DEEPSLATE_BRICKS);
            let roof = y0 + 5;
            for y in y0 + 1..roof {
                let b = if edge {
                    if decay.is_multiple_of(5) { CRACKED_DEEPSLATE_BRICKS } else { DEEPSLATE_BRICKS }
                } else {
                    AIR
                };
                set(out, y, b);
            }
            set(out, roof, DEEPSLATE_BRICKS);
            if centre {
                set(out, y0 + 1, LORE_TABLET);
            }
        }
    }
}

/// Position of the lore tablet of the nearest ruin to `(x, z)` (searching
/// the 5x5 cells around), surface ruins only if `surface` is true, deep
/// vaults only otherwise. Used by the PC test harness.
pub fn nearest_tablet(seed: u32, x: i32, z: i32, surface: bool) -> Option<[i32; 3]> {
    let (cx, cz) = (x.div_euclid(CELL), z.div_euclid(CELL));
    let mut best: Option<(i64, [i32; 3])> = None;
    for dx in -2..=2 {
        for dz in -2..=2 {
            let site = if surface {
                surface_site(seed, cx + dx, cz + dz)
            } else {
                vault_site(seed, cx + dx, cz + dz)
            };
            if let Some(s) = site {
                let p = [s.x0 + s.size / 2, s.y0 + 1, s.z0 + s.size / 2];
                let d = (p[0] - x) as i64 * (p[0] - x) as i64 + (p[2] - z) as i64 * (p[2] - z) as i64;
                if best.is_none_or(|(bd, _)| d < bd) {
                    best = Some((d, p));
                }
            }
        }
    }
    best.map(|(_, p)| p)
}

/// Which lore fragment (index into `LORE`) the tablet at `(x, y, z)` holds:
/// deep tablets tell of the Hollow, desert ones of the gateways, surface
/// ones of the golden age.
pub fn fragment_at(seed: u32, x: i32, y: i32, z: i32) -> usize {
    let era = if y < 10 {
        Era::Deep
    } else if column_info(seed, x, z).biome == Biome::Desert {
        Era::Expansion
    } else {
        Era::Golden
    };
    let count = LORE.iter().filter(|(e, _)| *e == era).count().max(1);
    let pick = hash3(x, y, z, seed) as usize % count;
    LORE.iter().enumerate().filter(|(_, (e, _))| *e == era).nth(pick).map_or(0, |(i, _)| i)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worldgen::generate_column;

    #[test]
    fn ruins_exist_and_hold_tablets() {
        let mut col = [AIR; HEIGHT];
        // Every ruin's centre column holds a tablet of the right era.
        let mut found = 0;
        for cx in -10..10 {
            for cz in -10..10 {
                for site in [surface_site(77, cx, cz), vault_site(77, cx, cz)].into_iter().flatten() {
                    let (x, z) = (site.x0 + site.size / 2, site.z0 + site.size / 2);
                    generate_column(77, x, z, &mut col);
                    assert_eq!(col[site.y0 as usize + 1], LORE_TABLET, "{site:?}");
                    let f = fragment_at(77, x, site.y0 + 1, z);
                    let era = LORE[f].0;
                    assert_eq!(era == Era::Deep, site.kind == Kind::Vault);
                    found += 1;
                }
            }
        }
        assert!(found > 50, "found {found} ruins");
    }
}
