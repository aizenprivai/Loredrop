//! Copies of the renderer's hot read-only data in RAM.
//!
//! On the calculator, the app's constant data (block table, textures) sits
//! in external flash memory, and the speed test measured a random read
//! there at about 730 ns — roughly 25 times slower than RAM (about 30 ns)
//! and around 400 CPU cycles. The renderer reads the block table and a
//! texture for every pixel, so those reads alone could explain most of the
//! frame time.
//!
//! * The block table (about 2 KB) is copied whole: it is part of the game
//!   state, which the startup code copies from flash to RAM.
//! * Textures (174 x 512 bytes, too big for RAM) go through a small cache:
//!   the 32 most recently needed textures live in RAM; a texture missing
//!   from the cache is copied in when first needed (once per texture, not
//!   per pixel), replacing the oldest one.

use crate::blocks::{BLOCK_COUNT, BLOCKS, Block, BlockDef, TEXTURE_COUNT, TEXTURES};
use core::cell::{Cell, UnsafeCell};

/// Textures kept in RAM at once (16 KB).
pub const SLOTS: usize = 32;
const EMPTY: u8 = u8::MAX;
const _: () = assert!(TEXTURE_COUNT < EMPTY as usize);

/// RAM copies of the block table and a texture cache.
///
/// Uses `Cell`/`UnsafeCell` so the renderer, which only holds shared
/// references, can fill the cache as it goes. The game is single-threaded,
/// and texels are returned by value (never as references into the cache),
/// so a slot being reused can never leave a dangling reference.
pub struct RamTables {
    defs: [BlockDef; BLOCK_COUNT],
    slot_of: [Cell<u8>; TEXTURE_COUNT],
    owner: [Cell<u8>; SLOTS],
    next: Cell<u8>,
    data: UnsafeCell<[[u16; 256]; SLOTS]>,
    misses: Cell<u32>,
}

impl Default for RamTables {
    fn default() -> Self {
        Self::new()
    }
}

impl RamTables {
    /// Fresh tables (block table filled in, texture cache empty). `const`
    /// so the copy is made at compile time into the RAM-resident game state.
    pub const fn new() -> Self {
        Self {
            defs: BLOCKS,
            slot_of: [const { Cell::new(EMPTY) }; TEXTURE_COUNT],
            owner: [const { Cell::new(EMPTY) }; SLOTS],
            next: Cell::new(0),
            data: UnsafeCell::new([[0; 256]; SLOTS]),
            misses: Cell::new(0),
        }
    }

    /// Properties of block `b` (from the RAM copy).
    #[inline(always)]
    pub fn def(&self, b: Block) -> &BlockDef {
        &self.defs[b as usize]
    }

    /// Texel `i` (0..256, row by row) of texture `t`.
    #[inline(always)]
    pub fn texel(&self, t: u8, i: usize) -> u16 {
        let mut s = self.slot_of[t as usize].get();
        if s == EMPTY {
            s = self.load(t);
        }
        // SAFETY: single-threaded; no reference into `data` outlives this
        // call, and `s` < SLOTS, `i & 255` < 256.
        unsafe { (*self.data.get())[s as usize][i & 255] }
    }

    /// Copies texture `t` into the oldest slot and returns the slot.
    #[cold]
    #[inline(never)]
    fn load(&self, t: u8) -> u8 {
        let s = self.next.get();
        self.next.set(((s as usize + 1) % SLOTS) as u8);
        let old = self.owner[s as usize].get();
        if old != EMPTY {
            self.slot_of[old as usize].set(EMPTY);
        }
        self.owner[s as usize].set(t);
        self.slot_of[t as usize].set(s);
        // SAFETY: as in `texel`; nothing else borrows `data` right now.
        unsafe { (*self.data.get())[s as usize] = TEXTURES[t as usize] };
        self.misses.set(self.misses.get().wrapping_add(1));
        s
    }

    /// How many textures have been copied in so far (for the speed test).
    pub fn misses(&self) -> u32 {
        self.misses.get()
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use std::boxed::Box;

    #[test]
    fn cache_returns_the_right_texels() {
        let r = Box::new(RamTables::new());
        // Touch far more textures than there are slots, twice, in a mixed order.
        for round in 0..2 {
            for k in 0..TEXTURE_COUNT {
                let t = ((k * 37 + round * 11) % TEXTURE_COUNT) as u8;
                for i in [0usize, 17, 255] {
                    assert_eq!(r.texel(t, i), TEXTURES[t as usize][i]);
                }
            }
        }
        assert_eq!(r.def(crate::blocks::STONE).hardness, BLOCKS[crate::blocks::STONE as usize].hardness);
    }
}
