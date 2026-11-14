//! 16×16×16 block storage with palette compression.
//!
//! Most sections contain only a handful of block types, so instead of storing
//! a 16-bit ID for each of the 4096 blocks (8 KB), a section keeps:
//!
//! * a **palette** — the list of distinct block types it contains, and
//! * for each block, a small **index** into that palette, packed into `u64`s
//!   using only as many bits as the palette needs (4 bits for ≤ 16 types,
//!   5 for ≤ 32, …).
//!
//! A section made of a single block type (all air, all stone) stores no
//! per-block data at all: [`Section::Uniform`].
//!
//! | Contents            | Memory  |
//! |---------------------|---------|
//! | uniform (e.g. air)  | 0 bytes |
//! | ≤ 16 block types    | ~2 KB   |
//! | ≤ 256 block types   | ~4 KB   |
//!
//! Indices never straddle two `u64`s (like modern Minecraft): with 5 bits,
//! 12 entries fit per `u64` and 4 bits are left unused. This keeps lookups to a
//! shift and a mask.

use crate::block::BlockId;

/// Number of blocks in a section.
pub const VOLUME: usize = 16 * 16 * 16;

/// Index of local block `(x, y, z)` (each 0..16) inside a section.
#[inline]
pub fn index(x: usize, y: usize, z: usize) -> usize {
    debug_assert!(x < 16 && y < 16 && z < 16);
    (y << 8) | (z << 4) | x
}

/// A 16³ cube of blocks.
#[derive(Debug, Clone, PartialEq)]
pub enum Section {
    /// Every block is the same type.
    Uniform(BlockId),
    Paletted(Paletted),
}

impl Default for Section {
    fn default() -> Self {
        Section::Uniform(BlockId::AIR)
    }
}

/// Palette + bit-packed indices. See the module docs.
#[derive(Debug, Clone, PartialEq)]
pub struct Paletted {
    palette: Vec<BlockId>,
    bits: u32,
    data: Vec<u64>,
}

impl Paletted {
    fn per_word(bits: u32) -> usize {
        (64 / bits) as usize
    }

    fn new(bits: u32, palette: Vec<BlockId>) -> Self {
        let words = VOLUME.div_ceil(Self::per_word(bits));
        Self { palette, bits, data: vec![0; words] }
    }

    #[inline]
    fn get_index(&self, i: usize) -> usize {
        let per = Self::per_word(self.bits);
        let word = self.data[i / per];
        let shift = (i % per) as u32 * self.bits;
        ((word >> shift) & ((1u64 << self.bits) - 1)) as usize
    }

    #[inline]
    fn set_index(&mut self, i: usize, v: usize) {
        let per = Self::per_word(self.bits);
        let shift = (i % per) as u32 * self.bits;
        let mask = ((1u64 << self.bits) - 1) << shift;
        let w = &mut self.data[i / per];
        *w = (*w & !mask) | ((v as u64) << shift);
    }

    /// Re-packs into a wider bit width when the palette outgrows the current one.
    fn grow(&mut self) {
        let mut bigger = Paletted::new(self.bits + 1, self.palette.clone());
        for i in 0..VOLUME {
            bigger.set_index(i, self.get_index(i));
        }
        *self = bigger;
    }
}

/// Minimum bits per block once a section is paletted (as in Minecraft).
const MIN_BITS: u32 = 4;

impl Section {
    /// Block at local index `i` (see [`index`]).
    #[inline]
    pub fn get(&self, i: usize) -> BlockId {
        match self {
            Section::Uniform(b) => *b,
            Section::Paletted(p) => p.palette[p.get_index(i)],
        }
    }

    /// Sets the block at local index `i`.
    pub fn set(&mut self, i: usize, id: BlockId) {
        match self {
            Section::Uniform(b) if *b == id => {}
            Section::Uniform(b) => {
                // First differing block: switch to paletted storage.
                let mut p = Paletted::new(MIN_BITS, vec![*b, id]);
                p.set_index(i, 1);
                *self = Section::Paletted(p);
            }
            Section::Paletted(p) => {
                let slot = match p.palette.iter().position(|&b| b == id) {
                    Some(s) => s,
                    None => {
                        p.palette.push(id);
                        if p.palette.len() > 1 << p.bits {
                            p.grow();
                        }
                        p.palette.len() - 1
                    }
                };
                p.set_index(i, slot);
            }
        }
    }

    /// Builds a section from a full array of blocks (fast path for world
    /// generation: one palette pass instead of 4096 individual `set` calls).
    pub fn from_blocks(blocks: &[BlockId; VOLUME]) -> Self {
        let first = blocks[0];
        if blocks.iter().all(|&b| b == first) {
            return Section::Uniform(first);
        }
        let mut palette: Vec<BlockId> = Vec::new();
        let mut indices = vec![0u16; VOLUME];
        for (i, &b) in blocks.iter().enumerate() {
            // Palettes are tiny, so a linear search beats hashing here.
            let slot = match palette.iter().position(|&p| p == b) {
                Some(s) => s,
                None => {
                    palette.push(b);
                    palette.len() - 1
                }
            };
            indices[i] = slot as u16;
        }
        let bits = (usize::BITS - (palette.len() - 1).leading_zeros()).max(MIN_BITS);
        let mut p = Paletted::new(bits, palette);
        for (i, &v) in indices.iter().enumerate() {
            p.set_index(i, v as usize);
        }
        Section::Paletted(p)
    }

    /// True if the whole section is air (nothing to draw or collide with).
    pub fn is_empty(&self) -> bool {
        matches!(self, Section::Uniform(b) if *b == BlockId::AIR)
    }

    /// Rebuilds the palette to drop unused entries, and turns a section that
    /// became single-typed back into [`Section::Uniform`]. Call after bulk
    /// changes (world generation); individual edits don't need it.
    pub fn compact(&mut self) {
        let Section::Paletted(p) = self else { return };
        let mut counts = vec![0u32; p.palette.len()];
        for i in 0..VOLUME {
            counts[p.get_index(i)] += 1;
        }
        let used: Vec<usize> = (0..counts.len()).filter(|&s| counts[s] > 0).collect();
        if used.len() == 1 {
            *self = Section::Uniform(p.palette[used[0]]);
            return;
        }
        if used.len() == p.palette.len() {
            return;
        }
        let bits = (usize::BITS - (used.len() - 1).leading_zeros()).max(MIN_BITS);
        let mut remap = vec![0usize; p.palette.len()];
        for (new, &old) in used.iter().enumerate() {
            remap[old] = new;
        }
        let mut out = Paletted::new(bits, used.iter().map(|&s| p.palette[s]).collect());
        for i in 0..VOLUME {
            out.set_index(i, remap[p.get_index(i)]);
        }
        *p = out;
    }

    /// Approximate heap memory used, for the debug overlay.
    pub fn heap_bytes(&self) -> usize {
        match self {
            Section::Uniform(_) => 0,
            Section::Paletted(p) => p.data.len() * 8 + p.palette.len() * 2,
        }
    }

    /// Bits per block (0 for uniform sections). For tests and diagnostics.
    pub fn bits_per_block(&self) -> u32 {
        match self {
            Section::Uniform(_) => 0,
            Section::Paletted(p) => p.bits,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uniform_until_first_change() {
        let mut s = Section::default();
        assert!(s.is_empty());
        s.set(index(1, 2, 3), BlockId::AIR);
        assert!(matches!(s, Section::Uniform(_)), "setting the same block stays uniform");
        s.set(index(1, 2, 3), BlockId(5));
        assert_eq!(s.get(index(1, 2, 3)), BlockId(5));
        assert_eq!(s.get(index(0, 0, 0)), BlockId::AIR);
        assert_eq!(s.bits_per_block(), 4);
    }

    #[test]
    fn grows_and_keeps_every_value() {
        let mut s = Section::default();
        // 300 distinct block types forces growth 4 → 5 → … → 9 bits.
        for i in 0..VOLUME {
            s.set(i, BlockId((i % 300) as u16 + 1));
        }
        assert_eq!(s.bits_per_block(), 9);
        for i in 0..VOLUME {
            assert_eq!(s.get(i), BlockId((i % 300) as u16 + 1), "index {i}");
        }
    }

    #[test]
    fn compact_shrinks_and_reverts_to_uniform() {
        let mut s = Section::Uniform(BlockId(1));
        for i in 0..40 {
            s.set(i, BlockId(i as u16 + 10)); // palette now 41 entries, 6 bits
        }
        for i in 0..40 {
            s.set(i, BlockId(1)); // everything back to block 1
        }
        s.compact();
        assert_eq!(s, Section::Uniform(BlockId(1)));

        let mut s = Section::Uniform(BlockId(1));
        for i in 0..40 {
            s.set(i, BlockId(i as u16 + 10));
        }
        for i in 2..40 {
            s.set(i, BlockId(1)); // only 3 types remain
        }
        s.compact();
        assert_eq!(s.bits_per_block(), 4);
        assert_eq!(s.get(0), BlockId(10));
        assert_eq!(s.get(1), BlockId(11));
        assert_eq!(s.get(100), BlockId(1));
    }

    #[test]
    fn from_blocks_matches_individual_sets() {
        let mut arr = [BlockId::AIR; VOLUME];
        for (i, b) in arr.iter_mut().enumerate() {
            *b = BlockId((i * 31 % 40) as u16);
        }
        let s = Section::from_blocks(&arr);
        for (i, b) in arr.iter().enumerate() {
            assert_eq!(s.get(i), *b);
        }
        assert_eq!(s.bits_per_block(), 6);
        assert_eq!(Section::from_blocks(&[BlockId(3); VOLUME]), Section::Uniform(BlockId(3)));
    }

    #[test]
    fn memory_is_small() {
        let mut s = Section::Uniform(BlockId(1));
        s.set(0, BlockId(2));
        assert!(s.heap_bytes() <= 2100, "{} bytes", s.heap_bytes()); // vs 8192 uncompressed
    }
}
