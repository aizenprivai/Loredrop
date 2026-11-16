//! The world: a window of [`SIZE`] x [`SIZE`] columns around the player, plus
//! a list of the changes the player made.
//!
//! The world is endless, but only the window is kept in memory (48 KB). It
//! works like a ring: column `(x, z)` is stored at `(x mod SIZE, z mod SIZE)`,
//! so when the player walks, the columns left behind are simply regenerated
//! as the new columns ahead. Because generation is a pure function of the
//! seed, a column can be rebuilt identically at any time; the player's
//! changes are kept in a separate list ([`MAX_EDITS`] entries) and re-applied
//! on top.

use crate::blocks::*;
use crate::worldgen::{HEIGHT, generate_column};

/// Width of the loaded window in blocks (a power of two). The view distance
/// can't exceed half of this.
pub const SIZE: usize = 32;
const MASK: i32 = SIZE as i32 - 1;
const SHIFT: u32 = SIZE.trailing_zeros();
/// Number of 4x4-column groups in the window.
const GROUPS: usize = (SIZE / 4) * (SIZE / 4);
/// How many changed blocks the world remembers. Past this, new changes still
/// show but are forgotten once the player walks away and comes back.
pub const MAX_EDITS: usize = 1024;
/// Marks a window slot that holds no column yet.
const EMPTY: (i16, i16) = (i16::MIN, i16::MIN);
/// The world ends this far from the origin (in blocks) on each axis.
pub const BORDER: i32 = 1_000_000;

/// One block changed by the player.
#[derive(Clone, Copy)]
struct Edit {
    x: i32,
    z: i32,
    y: u8,
    block: Block,
}

/// The loaded part of the world and the player's changes.
pub struct World {
    blocks: [Block; SIZE * SIZE * HEIGHT],
    /// One bit per block, set when it isn't air (same order as `blocks`).
    /// Rays mostly cross air: testing these 6 KB instead of the 48 KB of
    /// blocks keeps their memory reads in the CPU's small fast cache.
    filled: [u8; SIZE * SIZE * HEIGHT / 8],
    /// Which column each slot holds, as `(x >> SHIFT, z >> SHIFT)`.
    owner: [(i16, i16); SIZE * SIZE],
    /// Height of the highest opaque block of each slot (light from the sky
    /// reaches everything above it).
    top: [u8; SIZE * SIZE],
    /// Height of the highest non-air block of each slot.
    high: [u8; SIZE * SIZE],
    /// Highest block in each 4x4 group of slots (for skipping sky).
    group_high: [u8; GROUPS],
    /// Highest non-air block in the whole window: a ray above this height
    /// going up can only meet the sky (a big saving for the renderer).
    ceiling: i32,
    edits: [Edit; MAX_EDITS],
    edit_count: usize,
    /// World seed.
    pub seed: u32,
}

impl World {
    /// An empty world (nothing loaded). `const` so it can live in a static
    /// without ever being built on the small calculator stack.
    pub const fn new() -> Self {
        Self {
            blocks: [AIR; SIZE * SIZE * HEIGHT],
            filled: [0; SIZE * SIZE * HEIGHT / 8],
            owner: [EMPTY; SIZE * SIZE],
            top: [0; SIZE * SIZE],
            high: [0; SIZE * SIZE],
            group_high: [0; GROUPS],
            ceiling: HEIGHT as i32,
            edits: [Edit { x: 0, z: 0, y: 0, block: AIR }; MAX_EDITS],
            edit_count: 0,
            seed: 0,
        }
    }

    /// Forgets everything and starts a new world with `seed`.
    pub fn reset(&mut self, seed: u32) {
        self.owner = [EMPTY; SIZE * SIZE];
        self.high = [0; SIZE * SIZE];
        self.group_high = [0; GROUPS];
        self.ceiling = 0;
        self.edit_count = 0;
        self.seed = seed;
    }

    #[inline]
    fn slot(x: i32, z: i32) -> usize {
        ((z & MASK) as usize) * SIZE + (x & MASK) as usize
    }

    #[inline]
    fn tag(x: i32, z: i32) -> (i16, i16) {
        ((x >> SHIFT) as i16, (z >> SHIFT) as i16)
    }

    /// Whether column `(x, z)` is currently in memory.
    #[inline]
    pub fn is_loaded(&self, x: i32, z: i32) -> bool {
        self.owner[Self::slot(x, z)] == Self::tag(x, z)
    }

    /// The block at `(x, y, z)`. Below the world is bedrock, above it air,
    /// and columns that aren't loaded read as air.
    #[inline]
    pub fn get(&self, x: i32, y: i32, z: i32) -> Block {
        if y < 0 {
            return BEDROCK;
        }
        if y >= HEIGHT as i32 || !self.is_loaded(x, z) {
            return AIR;
        }
        self.blocks[Self::index(Self::slot(x, z), y as usize)]
    }

    /// Where block `y` of window slot `slot` is stored. The world is stored
    /// layer by layer (all of y = 0, then y = 1...), so the index is built
    /// with shifts only — the renderer computes it for every block a ray
    /// crosses.
    #[inline(always)]
    fn index(slot: usize, y: usize) -> usize {
        (y << (2 * SHIFT)) | slot
    }

    /// For walking a ray through storage: the index of `cell` (computed
    /// with wrapping arithmetic, so it stays right even while y is outside
    /// the world) and, per axis, what to add and which bits to keep when
    /// stepping by `step`. Use as
    /// `idx = (idx & !mask[a]) | (idx.wrapping_add(add[a]) & mask[a])`.
    #[inline]
    pub fn index_stepper(cell: [i32; 3], step: [i32; 3]) -> (u32, [u32; 3], [u32; 3]) {
        let s = SHIFT;
        let idx = ((cell[1] as u32) << (2 * s)) | (((cell[2] & MASK) as u32) << s) | (cell[0] & MASK) as u32;
        let add = [step[0] as u32, (step[1] as u32) << (2 * s), (step[2] as u32) << s];
        let mask = [MASK as u32, u32::MAX << (2 * s), (MASK as u32) << s];
        (idx, add, mask)
    }

    /// The height (y) of storage index `idx` from `index_stepper`. Negative
    /// when a ray stepped below the world (the index wrapped around).
    #[inline(always)]
    pub fn index_y(idx: u32) -> i32 {
        (idx as i32) >> (2 * SHIFT)
    }

    /// The x and z of storage index `idx`, wrapped into the window
    /// (`0..SIZE`); their low bits are those of the real coordinates.
    #[inline(always)]
    pub fn index_xz(idx: u32) -> (u32, u32) {
        (idx & MASK as u32, (idx >> SHIFT) & MASK as u32)
    }

    /// `group_high` of the column holding storage index `idx` (x and z
    /// come straight from the index's low bits).
    #[inline(always)]
    pub fn group_high_at(&self, idx: u32) -> i32 {
        let (x, z) = ((idx >> 2) & (MASK as u32 >> 2), (idx >> (SHIFT + 2)) & (MASK as u32 >> 2));
        // SAFETY: both parts are below SIZE / 4, so the index is below GROUPS.
        unsafe { *self.group_high.get_unchecked((z as usize) * (SIZE / 4) + x as usize) as i32 }
    }

    /// The raw block table and fill map (for the speed test's memory probes).
    pub fn raw_tables(&self) -> (&[u8], &[u8]) {
        (&self.blocks, &self.filled)
    }

    /// Reads storage index `i` from `index_stepper`. The caller must have
    /// checked that the cell's y is inside `0..HEIGHT`.
    #[inline(always)]
    pub fn get_index(&self, i: u32) -> Block {
        debug_assert!((i as usize) < self.blocks.len());
        // SAFETY: with y in 0..HEIGHT the index is below SIZE*SIZE*HEIGHT.
        unsafe { *self.blocks.get_unchecked(i as usize) }
    }

    /// Fast read for the renderer: no check that the column is loaded, and
    /// `y` must be inside `0..HEIGHT`. Rays never leave the window, so this
    /// is safe in practice and saves time in the hottest loop of the game.
    #[inline(always)]
    pub fn get_fast(&self, x: i32, y: i32, z: i32) -> Block {
        debug_assert!((0..HEIGHT as i32).contains(&y));
        let i = ((y as usize) << (2 * SHIFT)) | (((z & MASK) as usize) << SHIFT) | (x & MASK) as usize;
        // SAFETY: the masks keep the slot below SIZE*SIZE and callers keep y
        // in 0..HEIGHT, so `i` < SIZE*SIZE*HEIGHT = blocks.len().
        unsafe { *self.blocks.get_unchecked(i) }
    }

    /// Writes one block of a loaded column.
    #[inline]
    fn write(&mut self, x: i32, y: usize, z: i32, block: Block) {
        let i = Self::index(Self::slot(x, z), y);
        self.blocks[i] = block;
        let bit = 1u8 << (i & 7);
        if block == AIR {
            self.filled[i >> 3] &= !bit;
        } else {
            self.filled[i >> 3] |= bit;
        }
    }

    /// Whether storage index `i` (from `index_stepper`, y in range) holds
    /// anything but air — read from the 1-bit-per-block map.
    #[inline(always)]
    pub fn is_filled(&self, i: u32) -> bool {
        // SAFETY: as for `get_index`, i < blocks.len(), so i / 8 < filled.len().
        unsafe { (*self.filled.get_unchecked((i >> 3) as usize) >> (i & 7)) & 1 == 1 }
    }

    /// Whether the sky is visible straight up from `(x, y, z)` (no opaque
    /// block above it). Used for daylight and dark caves.
    #[inline(always)]
    pub fn sees_sky(&self, x: i32, y: i32, z: i32) -> bool {
        y > self.top[Self::slot(x, z)] as i32
    }

    /// Changes a block and remembers the change. Returns `false` if the
    /// change can't be remembered (edit list full or column not loaded);
    /// the block is still changed when its column is loaded.
    pub fn set(&mut self, x: i32, y: i32, z: i32, block: Block) -> bool {
        if !(0..HEIGHT as i32).contains(&y) || !self.is_loaded(x, z) {
            return false;
        }
        self.write(x, y as usize, z, block);
        self.update_top(Self::slot(x, z));
        self.remember(x, y as u8, z, block)
    }

    fn remember(&mut self, x: i32, y: u8, z: i32, block: Block) -> bool {
        if let Some(e) = self.edits[..self.edit_count].iter_mut().find(|e| e.x == x && e.z == z && e.y == y) {
            e.block = block;
            return true;
        }
        if self.edit_count == MAX_EDITS {
            return false;
        }
        self.edits[self.edit_count] = Edit { x, z, y, block };
        self.edit_count += 1;
        true
    }

    /// Calls `f` with the position of every torch the player placed (they
    /// are the only light sources besides lava, and every placed block is
    /// in the change list).
    pub fn for_each_torch(&self, mut f: impl FnMut([i32; 3])) {
        for e in &self.edits[..self.edit_count] {
            if e.block == TORCH {
                f([e.x, e.y as i32, e.z]);
            }
        }
    }

    /// Number of remembered changes.
    pub fn edit_count(&self) -> usize {
        self.edit_count
    }

    fn update_top(&mut self, slot: usize) {
        let at = |y: usize| self.blocks[Self::index(slot, y)];
        self.top[slot] = (0..HEIGHT).rev().find(|&y| is_opaque(at(y))).unwrap_or(0) as u8;
        let high = (0..HEIGHT).rev().find(|&y| at(y) != AIR).unwrap_or(0) as u8;
        self.high[slot] = high;
        // Refresh the 4x4 group's highest block.
        let (gx, gz) = ((slot % SIZE) & !3, (slot / SIZE) & !3);
        let mut gh = 0;
        for z in gz..gz + 4 {
            for x in gx..gx + 4 {
                gh = gh.max(self.high[z * SIZE + x]);
            }
        }
        self.group_high[(gz / 4) * (SIZE / 4) + gx / 4] = gh;
        self.ceiling = self.ceiling.max(high as i32);
    }

    /// Highest block of the 4x4 group of window slots holding column
    /// `(x, z)`: a ray going up from above it can't hit anything there.
    #[inline(always)]
    pub fn group_high(&self, x: i32, z: i32) -> i32 {
        self.group_high[(((z & MASK) >> 2) as usize) * (SIZE / 4) + ((x & MASK) >> 2) as usize] as i32
    }

    /// Height of the highest non-air block in the loaded window.
    #[inline]
    pub fn ceiling(&self) -> i32 {
        self.ceiling
    }

    fn recompute_ceiling(&mut self) {
        self.ceiling = self.high.iter().copied().max().unwrap_or(0) as i32;
    }

    /// Generates column `(x, z)` into its slot and re-applies the player's
    /// changes to it.
    fn load_column(&mut self, x: i32, z: i32) {
        let slot = Self::slot(x, z);
        let mut col = [AIR; HEIGHT];
        generate_column(self.seed, x, z, &mut col);
        for e in &self.edits[..self.edit_count] {
            if e.x == x && e.z == z {
                col[e.y as usize] = e.block;
            }
        }
        for (y, &b) in col.iter().enumerate() {
            self.write(x, y, z, b);
        }
        self.owner[slot] = Self::tag(x, z);
        self.update_top(slot);
    }

    /// Loads missing columns of the window centred on `(cx, cz)`, nearest
    /// first, at most `budget` of them. Returns how many are still missing
    /// (0 = the whole window is ready).
    pub fn update(&mut self, cx: i32, cz: i32, budget: usize) -> usize {
        let half = (SIZE / 2) as i32;
        let mut budget = budget;
        let mut missing = 0;
        let mut loaded_any = false;
        for r in 0..=half {
            for dx in -r..=r {
                for dz in -r..=r {
                    if dx.abs() != r && dz.abs() != r {
                        continue; // only the ring's edge
                    }
                    if dx == half || dz == half {
                        continue; // window spans -half..half-1
                    }
                    let (x, z) = (cx + dx, cz + dz);
                    if self.is_loaded(x, z) {
                        continue;
                    }
                    if budget > 0 {
                        self.load_column(x, z);
                        budget -= 1;
                        loaded_any = true;
                    } else {
                        missing += 1;
                    }
                }
            }
        }
        if loaded_any {
            // Columns that left the window may have held the tallest blocks.
            self.recompute_ceiling();
        }
        missing
    }
}

impl Default for World {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use std::boxed::Box;

    fn world() -> Box<World> {
        let mut w = Box::new(World::new());
        w.reset(77);
        w
    }

    #[test]
    fn loads_window_and_matches_generator() {
        let mut w = world();
        assert!(w.update(0, 0, 100) > 0);
        assert_eq!(w.update(0, 0, usize::MAX), 0);
        let mut col = [AIR; HEIGHT];
        generate_column(77, 5, -7, &mut col);
        for y in 0..HEIGHT as i32 {
            assert_eq!(w.get(5, y, -7), col[y as usize]);
        }
        assert!(!w.is_loaded(40, 0));
        assert_eq!(w.get(40, 10, 0), AIR);
    }

    #[test]
    fn edits_survive_unloading() {
        let mut w = world();
        w.update(0, 0, usize::MAX);
        assert!(w.set(3, 30, 4, GLASS));
        assert_eq!(w.get(3, 30, 4), GLASS);
        // Walk far away (the slot gets reused), then come back.
        w.update(200, 0, usize::MAX);
        assert!(!w.is_loaded(3, 4));
        w.update(0, 0, usize::MAX);
        assert_eq!(w.get(3, 30, 4), GLASS);
        assert_eq!(w.edit_count(), 1);
        // Changing the same block again doesn't use a new entry.
        w.set(3, 30, 4, AIR);
        assert_eq!(w.edit_count(), 1);
    }

    #[test]
    fn sky_tracking() {
        let mut w = world();
        w.update(0, 0, usize::MAX);
        w.set(1, HEIGHT as i32 - 2, 1, STONE);
        assert!(!w.sees_sky(1, HEIGHT as i32 - 3, 1));
        w.set(1, HEIGHT as i32 - 2, 1, AIR);
        assert!(w.sees_sky(1, HEIGHT as i32 - 3, 1));
    }
}
