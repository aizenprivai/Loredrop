//! # ld_world — world data
//!
//! Pure data structures, with no rendering and no operating-system code, so
//! everything here is unit-testable and can run on worker threads.
//!
//! * [`block`] — block types loaded from a data file (`assets/data/blocks.toml`)
//!   into a [`BlockRegistry`]. Each block type gets a small numeric [`BlockId`].
//! * [`section`] — a 16×16×16 cube of blocks, stored compactly with a palette.
//! * [`column`](mod@column) — a 16×384×16 chunk column made of 24 sections.
//! * [`map`] — every loaded column of one dimension.
//!
//! ## Sharing between threads
//! Columns are stored as `Arc<ChunkColumn>`. A worker thread that needs to
//! read a column (e.g. to build its mesh) just clones the `Arc` — a pointer
//! copy, no locking. When the game edits a block, `Arc::make_mut` copies the
//! column only if a worker is still reading the old version ("copy-on-write"),
//! so readers never see a half-changed column.

pub mod block;
pub mod column;
pub mod map;
pub mod section;

pub use block::{BlockDef, BlockId, BlockRegistry, Face, RenderLayer, Tint};
pub use column::ChunkColumn;
pub use map::ChunkMap;
pub use section::Section;
