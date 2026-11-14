//! Coordinate types for the voxel world.
//!
//! The world uses three coordinate spaces, and mixing them up is a classic
//! source of bugs, so each one gets its own type:
//!
//! | Type          | Unit             | Example use                         |
//! |---------------|------------------|-------------------------------------|
//! | [`BlockPos`]  | one block        | "the block at (10, 64, -3)"         |
//! | [`ChunkPos`]  | one 16×16 column | which column to load/unload         |
//! | [`SectionPos`]| one 16³ cube     | the unit we mesh and upload to GPU  |
//!
//! Entity and camera positions are *not* here: they use `f64` (`glam::DVec3`) so
//! precision stays good millions of blocks from spawn; the renderer converts to
//! camera-relative `f32` just before drawing.

use crate::world_size::CHUNK_SIZE;

/// Integer position of a single block in the world.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BlockPos {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

/// Horizontal position of a chunk column (a 16×16×384 slice of the world).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ChunkPos {
    pub x: i32,
    pub z: i32,
}

/// Position of a 16×16×16 section. `y` is a section index, so `y = -4`
/// covers block Y levels -64..-48.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SectionPos {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

impl BlockPos {
    pub const fn new(x: i32, y: i32, z: i32) -> Self {
        Self { x, y, z }
    }

    /// The chunk column containing this block.
    ///
    /// Uses `div_euclid` rather than `/` so that negative coordinates round
    /// *down* (block -1 is in chunk -1, not chunk 0).
    pub fn chunk(self) -> ChunkPos {
        ChunkPos { x: self.x.div_euclid(CHUNK_SIZE), z: self.z.div_euclid(CHUNK_SIZE) }
    }

    /// The section containing this block.
    pub fn section(self) -> SectionPos {
        SectionPos {
            x: self.x.div_euclid(CHUNK_SIZE),
            y: self.y.div_euclid(CHUNK_SIZE),
            z: self.z.div_euclid(CHUNK_SIZE),
        }
    }

    /// Position inside its section, each axis in `0..16`.
    pub fn local(self) -> (u32, u32, u32) {
        (
            self.x.rem_euclid(CHUNK_SIZE) as u32,
            self.y.rem_euclid(CHUNK_SIZE) as u32,
            self.z.rem_euclid(CHUNK_SIZE) as u32,
        )
    }
}

impl ChunkPos {
    pub const fn new(x: i32, z: i32) -> Self {
        Self { x, z }
    }

    /// Chebyshev ("square ring") distance between two columns. Render distance
    /// in Minecraft-style games is a square around the player, so this is the
    /// metric chunk loading uses.
    pub fn chebyshev_distance(self, other: ChunkPos) -> i32 {
        (self.x - other.x).abs().max((self.z - other.z).abs())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn negative_blocks_round_down_to_correct_chunk() {
        assert_eq!(BlockPos::new(-1, 0, -16).chunk(), ChunkPos::new(-1, -1));
        assert_eq!(BlockPos::new(-17, 0, 15).chunk(), ChunkPos::new(-2, 0));
        assert_eq!(BlockPos::new(0, 0, 0).chunk(), ChunkPos::new(0, 0));
    }

    #[test]
    fn local_coords_are_always_in_range() {
        assert_eq!(BlockPos::new(-1, -64, 17).local(), (15, 0, 1));
        assert_eq!(BlockPos::new(-64, -64, -64).section().y, -4);
    }

    #[test]
    fn chebyshev_distance_is_square_ring() {
        let o = ChunkPos::new(0, 0);
        assert_eq!(o.chebyshev_distance(ChunkPos::new(3, -2)), 3);
        assert_eq!(o.chebyshev_distance(ChunkPos::new(-5, 5)), 5);
    }
}
