//! The player's inventory (36 slots, the first 9 are the hotbar) and
//! crafting: matching a crafting grid against the recipes, and the recipe
//! book (craft straight from the inventory, like the PC game's recipe book).

use crate::items::*;

/// Number of inventory slots (hotbar included).
pub const SLOTS: usize = 36;
/// Number of hotbar slots (inventory slots `0..HOTBAR`).
pub const HOTBAR: usize = 9;

/// The player's items.
#[derive(Clone, Copy)]
pub struct Inventory {
    /// All slots; `0..HOTBAR` is the hotbar.
    pub slots: [Stack; SLOTS],
}

impl Default for Inventory {
    fn default() -> Self {
        Self::new()
    }
}

impl Inventory {
    /// An empty inventory.
    pub const fn new() -> Self {
        Self { slots: [Stack::EMPTY; SLOTS] }
    }

    /// Empties every slot.
    pub fn clear(&mut self) {
        self.slots = [Stack::EMPTY; SLOTS];
    }

    /// Adds a stack, filling matching stacks first, then empty slots
    /// (hotbar first, as on PC). Returns how many didn't fit.
    pub fn add(&mut self, stack: Stack) -> u8 {
        if stack.is_empty() {
            return 0;
        }
        let max = item_def(stack.item).max_stack.max(1);
        let mut left = stack.count;
        if max > 1 {
            for s in self.slots.iter_mut() {
                if left == 0 {
                    break;
                }
                if s.item == stack.item && s.count < max {
                    let n = left.min(max - s.count);
                    s.count += n;
                    left -= n;
                }
            }
        }
        for s in self.slots.iter_mut() {
            if left == 0 {
                break;
            }
            if s.is_empty() {
                let n = left.min(max);
                *s = Stack { item: stack.item, count: n, wear: stack.wear };
                left -= n;
            }
        }
        left
    }

    /// How many items accepted by ingredient `ing` the inventory holds.
    pub fn count(&self, ing: u16) -> u32 {
        self.slots.iter().filter(|s| !s.is_empty() && matches(ing, s.item)).map(|s| s.count as u32).sum()
    }

    /// Removes `n` items accepted by ingredient `ing` (caller checked `count`).
    pub fn remove(&mut self, ing: u16, mut n: u32) {
        for s in self.slots.iter_mut().rev() {
            if n == 0 {
                break;
            }
            if !s.is_empty() && matches(ing, s.item) {
                let k = n.min(s.count as u32) as u8;
                s.take(k);
                n -= k as u32;
            }
        }
    }

    /// Whether `stack` would fit completely.
    pub fn has_room_for(&self, stack: Stack) -> bool {
        let mut copy = *self;
        copy.add(stack) == 0
    }
}

/// What the crafting grid (`n` x `n`, row by row) makes, if anything.
pub fn craft_result(grid: &[Stack], n: usize) -> Option<Stack> {
    // Bounding box of the filled cells.
    let (mut x0, mut y0, mut x1, mut y1) = (n, n, 0, 0);
    let mut filled = 0;
    for y in 0..n {
        for x in 0..n {
            if !grid[y * n + x].is_empty() {
                x0 = x0.min(x);
                y0 = y0.min(y);
                x1 = x1.max(x);
                y1 = y1.max(y);
                filled += 1;
            }
        }
    }
    if filled == 0 {
        return None;
    }
    let (w, h) = (x1 - x0 + 1, y1 - y0 + 1);
    let cell = |x: usize, y: usize| grid[(y0 + y) * n + x0 + x];
    for r in SHAPED.iter() {
        if r.w as usize != w || r.h as usize != h {
            continue;
        }
        for mirror in [false, true] {
            let ok = (0..h).all(|y| {
                (0..w).all(|x| {
                    let ing = r.cells[y * w + if mirror { w - 1 - x } else { x }];
                    let s = cell(x, y);
                    if ing == 0 { s.is_empty() } else { !s.is_empty() && matches(ing, s.item) }
                })
            });
            if ok {
                return Some(Stack::new(r.result, r.count));
            }
        }
    }
    for r in SHAPELESS.iter() {
        if r.ingredients.len() != filled {
            continue;
        }
        let mut used = [false; 9];
        let ok = r.ingredients.iter().all(|&ing| {
            let found = (0..n * n).find(|&i| !used[i] && !grid[i].is_empty() && matches(ing, grid[i].item));
            if let Some(i) = found {
                used[i] = true;
            }
            found.is_some()
        });
        if ok {
            return Some(Stack::new(r.result, r.count));
        }
    }
    None
}

/// Uses up one item from every filled cell of the grid (after crafting).
pub fn consume_grid(grid: &mut [Stack]) {
    for s in grid.iter_mut() {
        if !s.is_empty() {
            s.take(1);
        }
    }
}

/// A recipe in the recipe book.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Recipe {
    /// Index into [`SHAPED`].
    Shaped(usize),
    /// Index into [`SHAPELESS`].
    Shapeless(usize),
}

impl Recipe {
    /// Every recipe, shaped first.
    pub fn all() -> impl Iterator<Item = Recipe> {
        (0..SHAPED.len()).map(Recipe::Shaped).chain((0..SHAPELESS.len()).map(Recipe::Shapeless))
    }

    /// What it makes.
    pub fn result(self) -> Stack {
        match self {
            Recipe::Shaped(i) => Stack::new(SHAPED[i].result, SHAPED[i].count),
            Recipe::Shapeless(i) => Stack::new(SHAPELESS[i].result, SHAPELESS[i].count),
        }
    }

    /// Whether it fits in an `n` x `n` grid (2 = inventory, 3 = crafting table).
    pub fn fits(self, n: usize) -> bool {
        match self {
            Recipe::Shaped(i) => SHAPED[i].w as usize <= n && SHAPED[i].h as usize <= n,
            Recipe::Shapeless(i) => SHAPELESS[i].ingredients.len() <= n * n,
        }
    }

    /// Ingredients with counts (at most 9 different ones).
    fn needs(self) -> ([(u16, u32); 9], usize) {
        let mut out = [(0u16, 0u32); 9];
        let mut len = 0;
        let list: &[u16] = match self {
            Recipe::Shaped(i) => &SHAPED[i].cells,
            Recipe::Shapeless(i) => SHAPELESS[i].ingredients,
        };
        for &ing in list.iter().filter(|&&c| c != 0) {
            if let Some(e) = out[..len].iter_mut().find(|e| e.0 == ing) {
                e.1 += 1;
            } else {
                out[len] = (ing, 1);
                len += 1;
            }
        }
        (out, len)
    }

    /// Whether the inventory holds everything needed.
    pub fn can_craft(self, inv: &Inventory) -> bool {
        let (needs, len) = self.needs();
        needs[..len].iter().all(|&(ing, n)| inv.count(ing) >= n)
    }

    /// Crafts once from the inventory into the inventory. Returns false
    /// (changing nothing) if ingredients or room are missing.
    pub fn craft(self, inv: &mut Inventory) -> bool {
        if !self.can_craft(inv) {
            return false;
        }
        let mut after = *inv;
        let (needs, len) = self.needs();
        for &(ing, n) in &needs[..len] {
            after.remove(ing, n);
        }
        if after.add(self.result()) != 0 {
            return false;
        }
        *inv = after;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blocks::*;

    fn grid(cells: &[Item]) -> [Stack; 9] {
        let mut g = [Stack::EMPTY; 9];
        for (i, &c) in cells.iter().enumerate() {
            if c != NONE {
                g[i] = Stack::new(c, 1);
            }
        }
        g
    }

    #[test]
    fn shaped_and_shapeless() {
        let p = OAK_PLANKS as Item;
        let b = BIRCH_PLANKS as Item;
        // Sticks: two planks stacked, anywhere in the grid, any plank kind.
        assert_eq!(
            craft_result(&grid(&[NONE, NONE, NONE, NONE, p, NONE, NONE, b, NONE]), 3),
            Some(Stack::new(STICK, 4))
        );
        // Pickaxe needs the exact pattern.
        let pick = grid(&[p, p, p, NONE, STICK, NONE, NONE, STICK, NONE]);
        assert_eq!(craft_result(&pick, 3), Some(Stack::new(WOODEN_PICKAXE, 1)));
        // Axe matches mirrored too.
        let axe = grid(&[NONE, p, p, NONE, STICK, p, NONE, STICK, NONE]);
        assert_eq!(craft_result(&axe, 3), Some(Stack::new(WOODEN_AXE, 1)));
        // Planks from a log (shapeless), in the 2x2 grid.
        let mut g = [Stack::EMPTY; 4];
        g[3] = Stack::new(OAK_LOG as Item, 5);
        assert_eq!(craft_result(&g, 2), Some(Stack::new(OAK_PLANKS as Item, 4)));
        consume_grid(&mut g);
        assert_eq!(g[3].count, 4);
        // Junk makes nothing.
        assert_eq!(craft_result(&grid(&[DIRT as Item, STICK]), 3), None);
    }

    #[test]
    fn inventory_add_and_recipe_book() {
        let mut inv = Inventory::new();
        assert_eq!(inv.add(Stack::new(OAK_LOG as Item, 70)), 0);
        assert_eq!(inv.slots[0].count, 64);
        assert_eq!(inv.slots[1].count, 6);
        let planks = Recipe::all().find(|r| r.result().item == OAK_PLANKS as Item).unwrap();
        assert!(planks.fits(2));
        for _ in 0..3 {
            assert!(planks.craft(&mut inv));
        }
        assert_eq!(inv.count(OAK_PLANKS as Item), 12);
        let table = Recipe::all().find(|r| r.result().item == CRAFTING_TABLE as Item).unwrap();
        assert!(table.craft(&mut inv));
        assert_eq!(inv.count(OAK_PLANKS as Item), 8);
        assert_eq!(inv.count(CRAFTING_TABLE as Item), 1);
        let pick = Recipe::all().find(|r| r.result().item == WOODEN_PICKAXE).unwrap();
        assert!(!pick.fits(2) && pick.fits(3));
        assert!(!pick.can_craft(&inv), "no sticks yet");
        // Tools don't stack.
        inv.add(Stack::new(WOODEN_PICKAXE, 1));
        inv.add(Stack::new(WOODEN_PICKAXE, 1));
        assert_eq!(inv.count(WOODEN_PICKAXE), 2);
        assert!(inv.slots.iter().filter(|s| s.item == WOODEN_PICKAXE).all(|s| s.count == 1));
    }
}
