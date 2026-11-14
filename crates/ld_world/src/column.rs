//! A chunk column: 24 stacked sections covering Y −64..320.

use crate::block::BlockId;
use crate::section::{self, Section};
use ld_core::coords::ChunkPos;
use ld_core::world_size::{MAX_Y, MIN_Y, SECTIONS_PER_COLUMN};

/// One 16 × 384 × 16 column of the world.
#[derive(Debug, Clone)]
pub struct ChunkColumn {
    pub pos: ChunkPos,
    sections: Vec<Section>,
}

impl ChunkColumn {
    /// An all-air column.
    pub fn new(pos: ChunkPos) -> Self {
        Self { pos, sections: vec![Section::default(); SECTIONS_PER_COLUMN as usize] }
    }

    /// Converts a world Y to (section index 0..24, local y 0..16), or `None`
    /// outside the buildable height.
    #[inline]
    fn split_y(y: i32) -> Option<(usize, usize)> {
        if !(MIN_Y..MAX_Y).contains(&y) {
            return None;
        }
        let rel = (y - MIN_Y) as usize;
        Some((rel >> 4, rel & 15))
    }

    /// Block at local x/z (0..16) and world y. Outside the height range is air.
    #[inline]
    pub fn get(&self, x: usize, y: i32, z: usize) -> BlockId {
        match Self::split_y(y) {
            Some((s, ly)) => self.sections[s].get(section::index(x, ly, z)),
            None => BlockId::AIR,
        }
    }

    /// Sets a block. Returns false (and does nothing) outside the height range.
    pub fn set(&mut self, x: usize, y: i32, z: usize, id: BlockId) -> bool {
        match Self::split_y(y) {
            Some((s, ly)) => {
                self.sections[s].set(section::index(x, ly, z), id);
                true
            }
            None => false,
        }
    }

    /// Section `i` (0 = the bottom section, Y −64..−48).
    pub fn section(&self, i: usize) -> &Section {
        &self.sections[i]
    }

    pub fn section_mut(&mut self, i: usize) -> &mut Section {
        &mut self.sections[i]
    }

    pub fn sections(&self) -> &[Section] {
        &self.sections
    }

    /// Compacts every section (call after generation).
    pub fn compact(&mut self) {
        for s in &mut self.sections {
            s.compact();
        }
    }

    /// Approximate memory used by this column, in bytes.
    pub fn heap_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.sections.len() * std::mem::size_of::<Section>()
            + self.sections.iter().map(Section::heap_bytes).sum::<usize>()
    }
}

/// Section index (0..24) → section Y coordinate in [`ld_core::coords::SectionPos`] terms.
pub fn section_y(index: usize) -> i32 {
    index as i32 + MIN_Y / 16
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_set_across_height_range() {
        let mut c = ChunkColumn::new(ChunkPos::new(0, 0));
        assert!(c.set(3, -64, 4, BlockId(1)));
        assert!(c.set(3, 319, 4, BlockId(2)));
        assert!(!c.set(3, 320, 4, BlockId(3)), "above build limit");
        assert!(!c.set(3, -65, 4, BlockId(3)), "below build limit");
        assert_eq!(c.get(3, -64, 4), BlockId(1));
        assert_eq!(c.get(3, 319, 4), BlockId(2));
        assert_eq!(c.get(3, 0, 4), BlockId::AIR);
        assert_eq!(c.get(3, 500, 4), BlockId::AIR);
        assert!(!c.section(0).is_empty() && !c.section(23).is_empty() && c.section(5).is_empty());
        assert_eq!(section_y(0), -4);
        assert_eq!(section_y(23), 19);
    }
}
