//! Rolling hills with trees: a stand-in generator for testing chunks (Phase 3).

use crate::TerrainGenerator;
use crate::noise::{fbm2, hash01};
use ld_core::coords::ChunkPos;
use ld_core::world_size::MIN_Y;
use ld_world::{BlockId, BlockRegistry, ChunkColumn};

/// Grass-covered hills around Y 64 over dirt and stone, a bedrock floor, sand
/// in low spots, and simple oak trees (kept inside their own column so this
/// placeholder doesn't need cross-chunk structure logic — that's Phase 4).
pub struct TestTerrain {
    seed: u64,
    ids: Ids,
}

struct Ids {
    stone: BlockId,
    dirt: BlockId,
    grass: BlockId,
    sand: BlockId,
    bedrock: BlockId,
    log: BlockId,
    leaves: BlockId,
}

impl TestTerrain {
    pub fn new(seed: u64, reg: &BlockRegistry) -> Result<Self, String> {
        Ok(Self {
            seed,
            ids: Ids {
                stone: reg.require("stone")?,
                dirt: reg.require("dirt")?,
                grass: reg.require("grass_block")?,
                sand: reg.require("sand")?,
                bedrock: reg.require("bedrock")?,
                log: reg.require("oak_log")?,
                leaves: reg.require("oak_leaves")?,
            },
        })
    }

    fn height(&self, x: i32, z: i32) -> i32 {
        let (fx, fz) = (x as f64, z as f64);
        let hills = fbm2(fx / 96.0, fz / 96.0, self.seed, 4) * 22.0;
        let bumps = fbm2(fx / 24.0, fz / 24.0, self.seed ^ 0xABCD, 2) * 4.0;
        64 + (hills + bumps) as i32
    }
}

impl TerrainGenerator for TestTerrain {
    fn surface_height(&self, x: i32, z: i32) -> i32 {
        self.height(x, z)
    }

    // Index loops are clearer here: x/z are also used as coordinates.
    #[allow(clippy::needless_range_loop)]
    fn generate(&self, pos: ChunkPos) -> ChunkColumn {
        let mut col = ChunkColumn::new(pos);
        let ids = &self.ids;
        let (bx, bz) = (pos.x * 16, pos.z * 16);
        let mut heights = [[0i32; 16]; 16];
        for (lz, row) in heights.iter_mut().enumerate() {
            for (lx, h) in row.iter_mut().enumerate() {
                *h = self.height(bx + lx as i32, bz + lz as i32);
            }
        }
        let min_h = heights.iter().flatten().copied().min().unwrap_or(64);

        // Fill whole sections of stone cheaply where they're entirely underground.
        let full_stone_sections = ((min_h - 4 - MIN_Y) / 16).max(0) as usize;
        for s in 1..full_stone_sections {
            *col.section_mut(s) = ld_world::Section::Uniform(ids.stone);
        }
        let first_y = MIN_Y + full_stone_sections.max(1) as i32 * 16;

        for lz in 0..16 {
            for lx in 0..16 {
                let h = heights[lz][lx];
                col.set(lx, MIN_Y, lz, ids.bedrock);
                for y in (MIN_Y + 1)..first_y {
                    col.set(lx, y, lz, ids.stone);
                }
                let low = h <= 61;
                for y in first_y..=h {
                    let b = if y == h {
                        if low { ids.sand } else { ids.grass }
                    } else if y > h - 4 {
                        if low { ids.sand } else { ids.dirt }
                    } else {
                        ids.stone
                    };
                    col.set(lx, y, lz, b);
                }
            }
        }

        // Trees: about 1 per 120 surface blocks, 2 blocks away from the column edge.
        for lz in 2..14 {
            for lx in 2..14 {
                let (wx, wz) = (bx + lx as i32, bz + lz as i32);
                let h = heights[lz][lx];
                if h <= 61 || hash01(wx, wz, self.seed ^ 0x7EE5) > 1.0 / 120.0 {
                    continue;
                }
                let trunk = 4 + (hash01(wx, wz, self.seed ^ 0x1234) * 2.0) as i32;
                for dy in -2i32..=1 {
                    let radius: i32 = if dy >= 0 { 1 } else { 2 };
                    let y = h + trunk + dy;
                    for dz in -radius..=radius {
                        for dx in -radius..=radius {
                            if radius == 2 && dx.abs() == 2 && dz.abs() == 2 {
                                continue; // round off the corners
                            }
                            let (x, z) = ((lx as i32 + dx) as usize, (lz as i32 + dz) as usize);
                            if col.get(x, y, z) == BlockId::AIR {
                                col.set(x, y, z, ids.leaves);
                            }
                        }
                    }
                }
                col.set(lx, h + trunk + 2, lz, ids.leaves);
                col.set(lx, h, lz, ids.dirt);
                for y in h + 1..=h + trunk {
                    col.set(lx, y, lz, ids.log);
                }
            }
        }
        col.compact();
        col
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn registry() -> BlockRegistry {
        let text =
            std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/data/blocks.toml"))
                .unwrap();
        BlockRegistry::from_toml(&text).unwrap()
    }

    #[test]
    fn deterministic_with_surface_and_bedrock() {
        let reg = registry();
        let g = TestTerrain::new(7, &reg).unwrap();
        let a = g.generate(ChunkPos::new(3, -2));
        let b = g.generate(ChunkPos::new(3, -2));
        for (sa, sb) in a.sections().iter().zip(b.sections()) {
            assert_eq!(sa, sb);
        }
        let h = g.surface_height(3 * 16 + 5, -2 * 16 + 5);
        let top = a.get(5, h, 5);
        assert!(
            top == reg.id("grass_block").unwrap()
                || top == reg.id("sand").unwrap()
                || top == reg.id("dirt").unwrap()
        );
        let above = a.get(5, h + 1, 5);
        assert!(above == BlockId::AIR || above == reg.id("oak_log").unwrap());
        assert_eq!(a.get(0, MIN_Y, 0), reg.id("bedrock").unwrap());
        assert_eq!(a.get(0, 0, 0), reg.id("stone").unwrap());
    }

    #[test]
    fn neighbouring_columns_line_up() {
        let reg = registry();
        let g = TestTerrain::new(7, &reg).unwrap();
        // Heights are continuous across the column border.
        for z in 0..16 {
            assert!((g.surface_height(15, z) - g.surface_height(16, z)).abs() <= 3);
        }
    }
}
