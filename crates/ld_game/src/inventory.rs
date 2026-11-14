//! Item stacks, the player inventory, and Minecraft's slot-click rules.

use crate::items::{ItemId, ItemRegistry};

/// A stack of identical items in one slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ItemStack {
    pub item: ItemId,
    pub count: u16,
    /// Durability used up (tools only; 0 = brand new).
    pub damage: u16,
}

impl ItemStack {
    pub fn new(item: ItemId, count: u16) -> Self {
        Self { item, count, damage: 0 }
    }

    /// Whether two stacks may merge (same item, both undamaged).
    pub fn stacks_with(&self, other: &ItemStack) -> bool {
        self.item == other.item && self.damage == 0 && other.damage == 0
    }
}

/// Mouse button used on a slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Click {
    /// Pick up / put down the whole stack, merge, or swap.
    Left,
    /// Pick up half / put down one.
    Right,
}

/// Applies one click on `slot` with `cursor` (the stack "held by the mouse"),
/// following Minecraft's rules exactly.
pub fn click_slot(
    slot: &mut Option<ItemStack>,
    cursor: &mut Option<ItemStack>,
    click: Click,
    items: &ItemRegistry,
) {
    match (slot.as_mut(), cursor.as_mut(), click) {
        (None, None, _) => {}
        // Empty hand: take everything (left) or the larger half (right).
        (Some(s), None, Click::Left) => {
            *cursor = Some(*s);
            *slot = None;
        }
        (Some(s), None, Click::Right) => {
            let take = s.count.div_ceil(2);
            *cursor = Some(ItemStack { count: take, ..*s });
            s.count -= take;
            if s.count == 0 {
                *slot = None;
            }
        }
        // Empty slot: put everything (left) or one item (right).
        (None, Some(c), Click::Left) => {
            *slot = Some(*c);
            *cursor = None;
        }
        (None, Some(c), Click::Right) => {
            *slot = Some(ItemStack { count: 1, ..*c });
            c.count -= 1;
            if c.count == 0 {
                *cursor = None;
            }
        }
        // Both full.
        (Some(s), Some(c), click) => {
            let max = items.get(s.item).max_stack;
            if s.stacks_with(c) {
                let room = max.saturating_sub(s.count);
                let moved = match click {
                    Click::Left => room.min(c.count),
                    Click::Right => room.min(1),
                };
                s.count += moved;
                c.count -= moved;
                if c.count == 0 {
                    *cursor = None;
                }
            } else {
                std::mem::swap(slot, cursor);
            }
        }
    }
}

/// Main inventory size: 9 hotbar slots (0–8) + 27 storage slots (9–35).
pub const SLOTS: usize = 36;
pub const HOTBAR: usize = 9;

/// The player's inventory.
#[derive(Debug, Clone)]
pub struct Inventory {
    pub slots: [Option<ItemStack>; SLOTS],
    /// Helmet, chestplate, leggings, boots (used from Phase 6).
    pub armor: [Option<ItemStack>; 4],
    pub offhand: Option<ItemStack>,
    /// Selected hotbar slot (0–8).
    pub selected: usize,
}

impl Default for Inventory {
    fn default() -> Self {
        Self { slots: [None; SLOTS], armor: [None; 4], offhand: None, selected: 0 }
    }
}

impl Inventory {
    /// The stack in the selected hotbar slot.
    pub fn held(&self) -> Option<ItemStack> {
        self.slots[self.selected]
    }

    pub fn held_mut(&mut self) -> &mut Option<ItemStack> {
        &mut self.slots[self.selected]
    }

    /// Removes one item from the held stack (e.g. after placing a block).
    pub fn consume_held(&mut self) {
        if let Some(s) = self.held_mut() {
            s.count -= 1;
            if s.count == 0 {
                *self.held_mut() = None;
            }
        }
    }

    /// Moves the selection by wheel notches, wrapping around (scrolling down
    /// = negative notches = moving right, as in Minecraft).
    pub fn scroll(&mut self, notches: i32) {
        self.selected = (self.selected as i32 - notches).rem_euclid(HOTBAR as i32) as usize;
    }

    /// Adds items, filling matching stacks first, then empty slots (hotbar
    /// before storage, like Minecraft). Returns what didn't fit.
    pub fn add(&mut self, mut stack: ItemStack, items: &ItemRegistry) -> Option<ItemStack> {
        let max = items.get(stack.item).max_stack;
        for slot in self.slots.iter_mut().flatten() {
            if slot.stacks_with(&stack) && slot.count < max {
                let moved = (max - slot.count).min(stack.count);
                slot.count += moved;
                stack.count -= moved;
                if stack.count == 0 {
                    return None;
                }
            }
        }
        for slot in self.slots.iter_mut() {
            if slot.is_none() {
                let moved = max.min(stack.count);
                *slot = Some(ItemStack { count: moved, ..stack });
                stack.count -= moved;
                if stack.count == 0 {
                    return None;
                }
            }
        }
        Some(stack)
    }

    /// Shift-click: moves a stack between the hotbar and storage.
    pub fn quick_move(&mut self, index: usize, items: &ItemRegistry) {
        let Some(stack) = self.slots[index].take() else { return };
        let target = if index < HOTBAR { HOTBAR..SLOTS } else { 0..HOTBAR };
        let mut rest = Some(stack);
        let max = items.get(stack.item).max_stack;
        for pass in 0..2 {
            for i in target.clone() {
                let Some(r) = rest.as_mut() else { break };
                match &mut self.slots[i] {
                    Some(s) if pass == 0 && s.stacks_with(r) && s.count < max => {
                        let moved = (max - s.count).min(r.count);
                        s.count += moved;
                        r.count -= moved;
                    }
                    slot @ None if pass == 1 => {
                        *slot = Some(*r);
                        r.count = 0;
                    }
                    _ => {}
                }
                if rest.is_some_and(|r| r.count == 0) {
                    rest = None;
                }
            }
        }
        self.slots[index] = rest;
    }

    /// Sorts the 27 storage slots (QoL, GDD §6): merges partial stacks and
    /// orders by item. The hotbar is left alone.
    pub fn sort_storage(&mut self, items: &ItemRegistry) {
        let mut stacks: Vec<ItemStack> = self.slots[HOTBAR..].iter_mut().filter_map(|s| s.take()).collect();
        stacks.sort_by_key(|s| (s.item, s.damage));
        let mut merged: Vec<ItemStack> = Vec::new();
        for s in stacks {
            let max = items.get(s.item).max_stack;
            match merged.last_mut() {
                Some(last) if last.stacks_with(&s) && last.count < max => {
                    let moved = (max - last.count).min(s.count);
                    last.count += moved;
                    if s.count > moved {
                        merged.push(ItemStack { count: s.count - moved, ..s });
                    }
                }
                _ => merged.push(s),
            }
        }
        for (i, s) in merged.into_iter().enumerate() {
            self.slots[HOTBAR + i] = Some(s);
        }
    }

    /// Total count of an item across all slots.
    pub fn count_of(&self, item: ItemId) -> u32 {
        self.slots.iter().flatten().filter(|s| s.item == item).map(|s| s.count as u32).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::items::tests::registries;

    #[test]
    fn click_rules_match_minecraft() {
        let (_, items) = registries();
        let dirt = items.id("dirt").unwrap();
        let stone = items.id("stone").unwrap();
        let (mut slot, mut cursor) = (Some(ItemStack::new(dirt, 33)), None);
        click_slot(&mut slot, &mut cursor, Click::Right, &items);
        assert_eq!(
            (slot.unwrap().count, cursor.unwrap().count),
            (16, 17),
            "right-click takes the larger half"
        );
        click_slot(&mut slot, &mut cursor, Click::Right, &items);
        assert_eq!((slot.unwrap().count, cursor.unwrap().count), (17, 16), "right-click puts down one");
        click_slot(&mut slot, &mut cursor, Click::Left, &items);
        assert_eq!((slot.unwrap().count, cursor), (33, None), "left-click merges");
        let mut other = Some(ItemStack::new(stone, 5));
        click_slot(&mut slot, &mut other, Click::Left, &items);
        assert_eq!((slot.unwrap().item, other.unwrap().item), (stone, dirt), "different items swap");
        let (mut full, mut c) = (Some(ItemStack::new(dirt, 60)), Some(ItemStack::new(dirt, 10)));
        click_slot(&mut full, &mut c, Click::Left, &items);
        assert_eq!((full.unwrap().count, c.unwrap().count), (64, 6), "merging stops at the stack limit");
    }

    #[test]
    fn add_fills_existing_stacks_then_hotbar_first() {
        let (_, items) = registries();
        let dirt = items.id("dirt").unwrap();
        let mut inv = Inventory::default();
        inv.slots[20] = Some(ItemStack::new(dirt, 60));
        assert_eq!(inv.add(ItemStack::new(dirt, 10), &items), None);
        assert_eq!(inv.slots[20].unwrap().count, 64);
        assert_eq!(inv.slots[0].unwrap().count, 6, "overflow goes to the first hotbar slot");
        // Fill everything: the remainder is returned.
        let mut full = Inventory::default();
        let left = full.add(ItemStack::new(dirt, 64 * 36 + 5), &items);
        assert_eq!(left.unwrap().count, 5);
        let pick = items.id("iron_pickaxe").unwrap();
        let mut inv = Inventory::default();
        inv.add(ItemStack::new(pick, 3), &items);
        assert_eq!(inv.slots.iter().flatten().count(), 3, "tools never stack");
    }

    #[test]
    fn quick_move_and_sort() {
        let (_, items) = registries();
        let (dirt, stone) = (items.id("dirt").unwrap(), items.id("stone").unwrap());
        let mut inv = Inventory::default();
        inv.slots[0] = Some(ItemStack::new(dirt, 10));
        inv.slots[15] = Some(ItemStack::new(dirt, 50));
        inv.quick_move(0, &items);
        assert_eq!(inv.slots[0], None);
        assert_eq!(inv.slots[15].unwrap().count, 60);
        inv.slots[30] = Some(ItemStack::new(stone, 3));
        inv.slots[12] = Some(ItemStack::new(dirt, 30));
        inv.sort_storage(&items);
        let storage: Vec<_> = inv.slots[HOTBAR..].iter().flatten().map(|s| (s.item, s.count)).collect();
        assert_eq!(storage.len(), 3);
        assert_eq!(inv.count_of(dirt), 90);
        assert_eq!(inv.count_of(stone), 3);
    }

    #[test]
    fn scroll_wraps() {
        let mut inv = Inventory::default();
        inv.scroll(-1);
        assert_eq!(inv.selected, 1);
        inv.scroll(2);
        assert_eq!(inv.selected, 8);
    }
}
