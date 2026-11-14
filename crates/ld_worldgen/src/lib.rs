//! # ld_worldgen — world generation
//!
//! Generators implement [`TerrainGenerator`] and must be **deterministic**: the
//! same seed and position always produce the same column, on any thread, in any
//! order. That is what makes seeds shareable and lets chunks generate in parallel.
//!
//! **Phase 3 status:** only [`TestTerrain`], a simple rolling-hills generator
//! used to exercise the chunk system. The real generator (continents, biomes,
//! caves, ores, structures) is Phase 4 and will implement the same trait.

mod noise;
mod test_terrain;

pub use test_terrain::TestTerrain;

use ld_core::coords::ChunkPos;
use ld_world::ChunkColumn;

/// Produces chunk columns. Called from worker threads, hence `Send + Sync`.
pub trait TerrainGenerator: Send + Sync {
    /// Generates the column at `pos`.
    fn generate(&self, pos: ChunkPos) -> ChunkColumn;
    /// World Y of the highest solid block at (x, z); used to place the player.
    fn surface_height(&self, x: i32, z: i32) -> i32;
}
