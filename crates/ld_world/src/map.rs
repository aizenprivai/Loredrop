//! All loaded chunk columns of one dimension.

use crate::block::BlockId;
use crate::column::ChunkColumn;
use ahash::AHashMap;
use ld_core::coords::{BlockPos, ChunkPos, SectionPos};
use std::sync::Arc;

/// Loaded columns, keyed by position. Columns are `Arc`-shared so worker
/// threads can read them without locks (see crate docs).
#[derive(Debug, Default)]
pub struct ChunkMap {
    columns: AHashMap<ChunkPos, Arc<ChunkColumn>>,
}

impl ChunkMap {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, column: ChunkColumn) {
        self.columns.insert(column.pos, Arc::new(column));
    }

    pub fn remove(&mut self, pos: ChunkPos) -> Option<Arc<ChunkColumn>> {
        self.columns.remove(&pos)
    }

    pub fn get(&self, pos: ChunkPos) -> Option<&Arc<ChunkColumn>> {
        self.columns.get(&pos)
    }

    pub fn contains(&self, pos: ChunkPos) -> bool {
        self.columns.contains_key(&pos)
    }

    pub fn len(&self) -> usize {
        self.columns.len()
    }

    pub fn is_empty(&self) -> bool {
        self.columns.is_empty()
    }

    pub fn positions(&self) -> impl Iterator<Item = ChunkPos> + '_ {
        self.columns.keys().copied()
    }

    /// The block at `pos`, or `None` if its column isn't loaded.
    pub fn block(&self, pos: BlockPos) -> Option<BlockId> {
        let (lx, _, lz) = pos.local();
        self.columns.get(&pos.chunk()).map(|c| c.get(lx as usize, pos.y, lz as usize))
    }

    /// Changes a block. Returns the sections whose meshes must be rebuilt
    /// (empty if the column isn't loaded, the position is outside the build
    /// height, or the block didn't change).
    pub fn set_block(&mut self, pos: BlockPos, id: BlockId) -> Vec<SectionPos> {
        let (lx, _, lz) = pos.local();
        let Some(col) = self.columns.get_mut(&pos.chunk()) else { return Vec::new() };
        if col.get(lx as usize, pos.y, lz as usize) == id {
            return Vec::new();
        }
        // Copy-on-write: clones the column only if a worker still holds it.
        if !Arc::make_mut(col).set(lx as usize, pos.y, lz as usize, id) {
            return Vec::new();
        }
        affected_sections(pos)
    }

    /// Approximate memory used by all loaded block data.
    pub fn heap_bytes(&self) -> usize {
        self.columns.values().map(|c| c.heap_bytes()).sum()
    }
}

/// Sections whose mesh can change when the block at `pos` changes: its own
/// section, plus any neighbouring section it touches. Because meshes include
/// ambient occlusion (which looks at diagonal neighbours), a block on an edge
/// or corner affects up to 8 sections.
pub fn affected_sections(pos: BlockPos) -> Vec<SectionPos> {
    let s = pos.section();
    let (lx, ly, lz) = pos.local();
    let offsets = |l: u32| -> &'static [i32] {
        match l {
            0 => &[0, -1],
            15 => &[0, 1],
            _ => &[0],
        }
    };
    let mut out = Vec::with_capacity(8);
    for &dx in offsets(lx) {
        for &dy in offsets(ly) {
            for &dz in offsets(lz) {
                out.push(SectionPos { x: s.x + dx, y: s.y + dy, z: s.z + dz });
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interior_block_affects_one_section_corner_affects_eight() {
        assert_eq!(affected_sections(BlockPos::new(5, 5, 5)).len(), 1);
        assert_eq!(affected_sections(BlockPos::new(0, 5, 5)).len(), 2);
        let corner = affected_sections(BlockPos::new(-1, 16, 15));
        assert_eq!(corner.len(), 8);
        assert!(corner.contains(&SectionPos { x: 0, y: 0, z: 1 }));
    }

    #[test]
    fn set_block_copy_on_write_keeps_old_snapshot() {
        let mut m = ChunkMap::new();
        m.insert(ChunkColumn::new(ChunkPos::new(0, 0)));
        let snapshot = m.get(ChunkPos::new(0, 0)).unwrap().clone(); // a "worker" holds it
        let changed = m.set_block(BlockPos::new(1, 70, 1), BlockId(3));
        assert_eq!(changed.len(), 1);
        assert_eq!(m.block(BlockPos::new(1, 70, 1)), Some(BlockId(3)));
        assert_eq!(snapshot.get(1, 70, 1), BlockId::AIR, "worker's copy unchanged");
        assert!(m.set_block(BlockPos::new(1, 70, 1), BlockId(3)).is_empty(), "no-op change");
        assert!(m.set_block(BlockPos::new(99, 70, 1), BlockId(3)).is_empty(), "unloaded");
        assert_eq!(m.block(BlockPos::new(99, 70, 1)), None);
    }
}
