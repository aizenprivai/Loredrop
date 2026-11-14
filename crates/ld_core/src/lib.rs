//! # ld_core — shared foundations
//!
//! The bottom of the dependency graph: every other Loredrop crate may depend on
//! this one, and this one depends on nothing internal. It holds only small,
//! stable building blocks — coordinate types and world-size constants — so that
//! changes here are rare.

pub mod coords;

/// World-size constants for the Overworld (see GDD §3.1).
///
/// These are the *defaults*; dimensions can override height later, which is why
/// code should prefer reading a dimension's height over using these directly.
pub mod world_size {
    /// Width/depth of a chunk column, and the edge length of a cubic section.
    pub const CHUNK_SIZE: i32 = 16;
    /// Lowest buildable Y level (inclusive).
    pub const MIN_Y: i32 = -64;
    /// Highest buildable Y level (exclusive): blocks exist for `MIN_Y..MAX_Y`.
    pub const MAX_Y: i32 = 320;
    /// Number of 16³ sections stacked in one chunk column: 384 / 16 = 24.
    pub const SECTIONS_PER_COLUMN: i32 = (MAX_Y - MIN_Y) / CHUNK_SIZE;
}
