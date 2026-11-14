//! # ld_worldgen — world generation
//!
//! Generators implement [`TerrainGenerator`] and must be **deterministic**: the
//! same seed and position always produce the same column, on any thread, in any
//! order. That is what makes seeds shareable and lets chunks generate in parallel.
//!
//! * [`Overworld`] — the real generator (Phase 4): climate-driven terrain,
//!   biomes, caves, water, ores and vegetation. See its module docs.
//! * [`TestTerrain`] — the simple Phase 3 hills generator, kept for fast tests.

mod features;
pub mod noise;
mod ores;
pub mod overworld;
mod test_terrain;

pub use overworld::{Climate, ColumnProfile, Overworld, SEA_LEVEL};
pub use test_terrain::TestTerrain;

use ld_core::coords::ChunkPos;
use ld_world::ChunkColumn;

/// Produces chunk columns. Called from worker threads, hence `Send + Sync`.
pub trait TerrainGenerator: Send + Sync {
    /// Generates the column at `pos`.
    fn generate(&self, pos: ChunkPos) -> ChunkColumn;
    /// World Y of the highest solid block at (x, z); used to place the player.
    fn surface_height(&self, x: i32, z: i32) -> i32;

    /// A good block position to spawn the player (feet), near the origin.
    fn find_spawn(&self) -> (i32, i32, i32) {
        (0, self.surface_height(0, 0) + 1, 0)
    }
}
