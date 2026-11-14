//! A section plus a one-block border, copied out of the world.

use crate::table::BARRIER;
use ld_core::world_size::{MAX_Y, MIN_Y};
use ld_world::{BiomeId, BlockId, ChunkColumn};

/// Edge length of the padded cube.
pub const PADDED: usize = 18;

/// The 16³ section being meshed, surrounded by a 1-block border: 18³ blocks.
/// Coordinates run from −1 to 16 on each axis.
pub struct Neighborhood {
    blocks: Box<[BlockId; PADDED * PADDED * PADDED]>,
    /// Biome of each column in the padded 18×18 area (for colour blending).
    biomes: [BiomeId; PADDED * PADDED],
}

impl Neighborhood {
    #[inline]
    fn idx(x: i32, y: i32, z: i32) -> usize {
        (((y + 1) as usize * PADDED) + (z + 1) as usize) * PADDED + (x + 1) as usize
    }

    /// Block at padded local coordinates (each −1..=16).
    #[inline]
    pub fn get(&self, x: i32, y: i32, z: i32) -> BlockId {
        self.blocks[Self::idx(x, y, z)]
    }

    /// Biome of padded column (x, z), each −1..=16.
    #[inline]
    pub fn biome(&self, x: i32, z: i32) -> BiomeId {
        self.biomes[(z + 1) as usize * PADDED + (x + 1) as usize]
    }

    /// Builds the neighbourhood of section `section_index` (0..24) of the
    /// centre column. `columns[dz+1][dx+1]` is the column at offset (dx, dz);
    /// `columns[1][1]` must be the centre. Missing neighbours count as air.
    pub fn from_columns(columns: [[Option<&ChunkColumn>; 3]; 3], section_index: usize) -> Self {
        let mut blocks = Box::new([BlockId::AIR; PADDED * PADDED * PADDED]);
        let mut biomes = [BiomeId(0); PADDED * PADDED];
        let base_y = MIN_Y + section_index as i32 * 16;
        for (zi, row) in columns.iter().enumerate() {
            for (xi, col) in row.iter().enumerate() {
                let Some(col) = col else { continue };
                // Which local x/z range of this neighbour column falls in the border.
                let xr = match xi {
                    0 => 15..=15,
                    1 => 0..=15,
                    _ => 0..=0,
                };
                let zr = match zi {
                    0 => 15..=15,
                    1 => 0..=15,
                    _ => 0..=0,
                };
                for lz in zr.clone() {
                    for lx in xr.clone() {
                        let px = lx as i32 + (xi as i32 - 1) * 16;
                        let pz = lz as i32 + (zi as i32 - 1) * 16;
                        biomes[(pz + 1) as usize * PADDED + (px + 1) as usize] = col.biome(lx, lz);
                    }
                }
                for ly in -1..=16 {
                    let wy = base_y + ly;
                    for lz in zr.clone() {
                        for lx in xr.clone() {
                            let b = if wy < MIN_Y {
                                BARRIER
                            } else if wy >= MAX_Y {
                                BlockId::AIR
                            } else {
                                col.get(lx, wy, lz)
                            };
                            let px = lx as i32 + (xi as i32 - 1) * 16;
                            let pz = lz as i32 + (zi as i32 - 1) * 16;
                            blocks[Self::idx(px, ly, pz)] = b;
                        }
                    }
                }
            }
        }
        Self { blocks, biomes }
    }

    /// Builds a neighbourhood from a function (tests and benchmarks).
    pub fn from_fn(f: impl Fn(i32, i32, i32) -> BlockId) -> Self {
        let mut blocks = Box::new([BlockId::AIR; PADDED * PADDED * PADDED]);
        for y in -1..=16 {
            for z in -1..=16 {
                for x in -1..=16 {
                    blocks[Self::idx(x, y, z)] = f(x, y, z);
                }
            }
        }
        Self { blocks, biomes: [BiomeId(0); PADDED * PADDED] }
    }

    /// True if the inner 16³ is entirely air (nothing to mesh).
    pub fn inner_is_empty(&self) -> bool {
        (0..16).all(|y| (0..16).all(|z| (0..16).all(|x| self.get(x, y, z) == BlockId::AIR)))
    }
}
