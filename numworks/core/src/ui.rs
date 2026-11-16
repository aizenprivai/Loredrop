//! Inventory screens: the player's inventory (with a 2x2 crafting grid),
//! the crafting table (3x3), the furnace, the recipe book and the creative
//! item palette.
//!
//! Everything is driven by a cursor moved with the arrow keys (the nearest
//! slot in that direction is chosen), like a game-pad on PC:
//!
//! * **OK**: pick up / put down / swap a whole stack (left click on PC).
//! * **EXE**: pick up half a stack, or put down one item (right click).
//! * On the crafting result, OK crafts once into the hand and EXE crafts as
//!   many as possible straight into the inventory (shift-click).
//! * **Toolbox**: recipe book — a list of what can be crafted now; OK
//!   crafts one, EXE crafts as many as possible.
//! * **Back** (or Var): close. Items left in the grid or in the hand go
//!   back to the inventory.

use crate::furnace::Furnace;
use crate::hud::{self, FRAME, GOLD, MENU_BG, SLOT_BG, TEXT};
use crate::input::{Input, Key};
use crate::inventory::{HOTBAR, Inventory, Recipe, SLOTS, consume_grid, craft_result};
use crate::items::*;
use crate::text::Text;
use crate::{Platform, SCREEN_H, SCREEN_W};
use core::fmt::Write;

/// Which screen is open.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Container {
    /// The player's inventory with a 2x2 crafting grid.
    Inventory,
    /// A crafting table (3x3 grid).
    Table,
    /// A furnace (index into the game's furnace list).
    Furnace(usize),
    /// Creative mode: every item.
    Creative,
}

/// A slot on screen.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SlotRef {
    /// Inventory slot (0..9 = hotbar).
    Inv(u8),
    /// Crafting grid cell.
    Grid(u8),
    /// Crafting result.
    Result,
    /// Furnace input.
    FurnaceIn,
    /// Furnace fuel.
    FurnaceFuel,
    /// Furnace output.
    FurnaceOut,
    /// Creative palette entry (index on the current page).
    Palette(u8),
    /// Armour slot (0 helmet .. 3 boots).
    Armor(u8),
}

/// Distance between slot centres.
const PITCH: u16 = 20;
/// Left edge of the 9-wide inventory rows.
const INV_X: u16 = 70;
/// Creative palette size.
const PAL_COLS: usize = 9;
const PAL_ROWS: usize = 5;
const PAL_PAGE: usize = PAL_COLS * PAL_ROWS;
/// Recipe-book lines visible at once.
const BOOK_LINES: usize = 8;

/// State of an open inventory screen.
pub struct InvScreen {
    /// Which screen.
    pub container: Container,
    /// Slot under the cursor.
    pub cursor: SlotRef,
    /// Stack held "in the hand" (picked up with OK).
    pub held: Stack,
    /// Crafting grid contents (2x2 uses the first 4 cells).
    pub grid: [Stack; 9],
    /// Recipe book open.
    pub book: bool,
    book_index: usize,
    page: usize,
    /// Needs redrawing.
    pub dirty: bool,
    /// One-line message (e.g. "Inventory full").
    message: Text<40>,
}

/// Every item listed in the creative palette, in table order.
fn palette_items() -> impl Iterator<Item = Item> {
    (1..ITEM_COUNT as Item).filter(|&i| item_def(i).creative)
}

fn palette_len() -> usize {
    palette_items().count()
}

impl InvScreen {
    /// A closed screen (placeholder until opened).
    pub const fn new() -> Self {
        Self {
            container: Container::Inventory,
            cursor: SlotRef::Inv(0),
            held: Stack::EMPTY,
            grid: [Stack::EMPTY; 9],
            book: false,
            book_index: 0,
            page: 0,
            dirty: true,
            message: Text::new(),
        }
    }

    /// Opens `container` with the cursor on hotbar slot `selected`.
    pub fn open(&mut self, container: Container, selected: usize) {
        *self = Self::new();
        self.container = container;
        self.cursor = SlotRef::Inv(selected as u8);
    }

    fn grid_size(&self) -> usize {
        if self.container == Container::Table { 3 } else { 2 }
    }

    /// Screen position (top-left of the 16x16 icon) of a slot.
    fn pos(&self, s: SlotRef) -> (u16, u16) {
        match s {
            SlotRef::Inv(i) if (i as usize) < HOTBAR => (INV_X + i as u16 * PITCH, 186),
            SlotRef::Inv(i) => {
                let k = i as u16 - HOTBAR as u16;
                (INV_X + (k % 9) * PITCH, 120 + (k / 9) * PITCH)
            }
            SlotRef::Grid(i) => {
                let n = self.grid_size() as u16;
                let (x0, y0) = if n == 3 { (80, 32) } else { (100, 42) };
                (x0 + (i as u16 % n) * PITCH, y0 + (i as u16 / n) * PITCH)
            }
            SlotRef::Result => (176, 52),
            SlotRef::FurnaceIn => (100, 32),
            SlotRef::FurnaceFuel => (100, 76),
            SlotRef::FurnaceOut => (176, 54),
            SlotRef::Armor(i) => (40, 112 + i as u16 * PITCH),
            SlotRef::Palette(i) => {
                (INV_X + (i as u16 % PAL_COLS as u16) * PITCH, 30 + (i as u16 / PAL_COLS as u16) * PITCH)
            }
        }
    }

    /// Every slot shown on this screen (at most 64), in a fixed order.
    fn slots(&self, out: &mut [SlotRef; 64]) -> usize {
        let mut n = 0;
        let mut push = |s: SlotRef| {
            out[n] = s;
            n += 1;
        };
        match self.container {
            Container::Creative => {
                let first = self.page * PAL_PAGE;
                let count = palette_len().saturating_sub(first).min(PAL_PAGE);
                for i in 0..count {
                    push(SlotRef::Palette(i as u8));
                }
                for i in 0..HOTBAR {
                    push(SlotRef::Inv(i as u8));
                }
                return n;
            }
            Container::Furnace(_) => {
                push(SlotRef::FurnaceIn);
                push(SlotRef::FurnaceFuel);
                push(SlotRef::FurnaceOut);
            }
            _ => {
                let g = self.grid_size();
                for i in 0..g * g {
                    push(SlotRef::Grid(i as u8));
                }
                push(SlotRef::Result);
            }
        }
        for i in 0..SLOTS {
            push(SlotRef::Inv(i as u8));
        }
        if self.container == Container::Inventory {
            for i in 0..4 {
                push(SlotRef::Armor(i));
            }
        }
        n
    }

    /// Moves the cursor to the nearest slot in direction `(dx, dy)`.
    fn move_cursor(&mut self, dx: i32, dy: i32) {
        let mut list = [SlotRef::Result; 64];
        let n = self.slots(&mut list);
        let (cx, cy) = self.pos(self.cursor);
        let mut best: Option<(i32, SlotRef)> = None;
        for &s in &list[..n] {
            let (x, y) = self.pos(s);
            let (ex, ey) = (x as i32 - cx as i32, y as i32 - cy as i32);
            // Must lie in the pressed direction.
            let along = ex * dx + ey * dy;
            if along <= 0 {
                continue;
            }
            let across = (ex * dy - ey * dx).abs();
            let score = along + across * 3;
            if best.is_none_or(|(b, _)| score < b) {
                best = Some((score, s));
            }
        }
        if let Some((_, s)) = best {
            self.cursor = s;
            self.dirty = true;
        }
    }

    /// Reads a slot.
    fn get(&self, s: SlotRef, inv: &Inventory, furnace: Option<&Furnace>) -> Stack {
        match s {
            SlotRef::Inv(i) => inv.slots[i as usize],
            SlotRef::Armor(i) => inv.armor[i as usize],
            SlotRef::Grid(i) => self.grid[i as usize],
            SlotRef::Result => {
                craft_result(&self.grid[..self.grid_size() * self.grid_size()], self.grid_size())
                    .unwrap_or(Stack::EMPTY)
            }
            SlotRef::FurnaceIn => furnace.map_or(Stack::EMPTY, |f| f.input),
            SlotRef::FurnaceFuel => furnace.map_or(Stack::EMPTY, |f| f.fuel),
            SlotRef::FurnaceOut => furnace.map_or(Stack::EMPTY, |f| f.output),
            SlotRef::Palette(i) => palette_items()
                .nth(self.page * PAL_PAGE + i as usize)
                // Shown as a single item; taking it gives a full stack.
                .map_or(Stack::EMPTY, |it| Stack::new(it, 1)),
        }
    }

    /// Mutable access to a storage slot (not the result or palette).
    fn slot_mut<'a>(
        &'a mut self,
        s: SlotRef,
        inv: &'a mut Inventory,
        furnace: Option<&'a mut Furnace>,
    ) -> Option<&'a mut Stack> {
        match s {
            SlotRef::Inv(i) => Some(&mut inv.slots[i as usize]),
            SlotRef::Armor(i) => Some(&mut inv.armor[i as usize]),
            SlotRef::Grid(i) => Some(&mut self.grid[i as usize]),
            SlotRef::FurnaceIn => furnace.map(|f| &mut f.input),
            SlotRef::FurnaceFuel => furnace.map(|f| &mut f.fuel),
            _ => None,
        }
    }

    fn say(&mut self, text: &str) {
        self.message.clear();
        let _ = self.message.write_str(text);
    }

    /// Handles one frame of input. Returns false when the screen closes
    /// (the caller then calls [`InvScreen::close`]).
    pub fn update(&mut self, input: &Input, inv: &mut Inventory, mut furnace: Option<&mut Furnace>) -> bool {
        if input.pressed(Key::Back) || input.pressed(Key::Var) {
            if self.book {
                self.book = false;
                self.dirty = true;
                return true;
            }
            return false;
        }
        if self.container == Container::Creative
            && (input.pressed(Key::LeftParen) || input.pressed(Key::RightParen))
        {
            let pages = palette_len().div_ceil(PAL_PAGE);
            self.page = if input.pressed(Key::RightParen) {
                (self.page + 1) % pages
            } else {
                (self.page + pages - 1) % pages
            };
            if matches!(self.cursor, SlotRef::Palette(_)) {
                self.cursor = SlotRef::Palette(0);
            }
            self.dirty = true;
        }
        if input.pressed(Key::Toolbox) && matches!(self.container, Container::Inventory | Container::Table) {
            self.book = !self.book;
            self.book_index = 0;
            self.dirty = true;
            return true;
        }
        if self.book {
            self.update_book(input, inv);
            return true;
        }
        for (k, dx, dy) in [(Key::Left, -1, 0), (Key::Right, 1, 0), (Key::Up, 0, -1), (Key::Down, 0, 1)] {
            if input.pressed(k) {
                self.move_cursor(dx, dy);
            }
        }
        let ok = input.pressed(Key::Ok);
        let one = input.pressed(Key::Exe);
        if !ok && !one {
            return true;
        }
        self.message.clear();
        self.dirty = true;
        let cur = self.cursor;
        match cur {
            SlotRef::Result => self.take_result(inv, one),
            SlotRef::FurnaceOut => {
                if let Some(f) = furnace.as_deref_mut() {
                    let out = f.output;
                    if !out.is_empty() {
                        if self.held.is_empty() {
                            self.held = out;
                            f.output = Stack::EMPTY;
                        } else if self.held.item == out.item {
                            let room = item_def(out.item).max_stack - self.held.count;
                            let n = room.min(out.count);
                            self.held.count += n;
                            f.output.take(n);
                        }
                    }
                }
            }
            SlotRef::Palette(_) => {
                let s = self.get(cur, inv, None);
                if !self.held.is_empty() {
                    self.held = Stack::EMPTY; // putting an item back in the palette deletes it
                } else if !s.is_empty() {
                    self.held = Stack::new(s.item, if one { 1 } else { item_def(s.item).max_stack });
                }
            }
            _ => {
                let is_fuel_slot = cur == SlotRef::FurnaceFuel;
                let held = self.held;
                if is_fuel_slot && !held.is_empty() && fuel_ticks(held.item) == 0 {
                    self.say("That doesn't burn.");
                    return true;
                }
                if let SlotRef::Armor(k) = cur
                    && !held.is_empty()
                    && !matches!(item_def(held.item).kind, ItemKind::Armor { slot, .. } if slot == k)
                {
                    self.say("That doesn't go there.");
                    return true;
                }
                let Some(slot) = self.slot_mut(cur, inv, furnace) else { return true };
                let max = item_def(held.item.max(slot.item)).max_stack.max(1);
                let mut new_held = held;
                if ok {
                    if held.is_empty() {
                        new_held = *slot;
                        *slot = Stack::EMPTY;
                    } else if slot.is_empty() {
                        *slot = held;
                        new_held = Stack::EMPTY;
                    } else if slot.item == held.item && max > 1 {
                        let n = held.count.min(max - slot.count.min(max));
                        slot.count += n;
                        new_held.take(n);
                    } else {
                        new_held = *slot;
                        *slot = held;
                    }
                } else if held.is_empty() {
                    if !slot.is_empty() {
                        let half = slot.count.div_ceil(2);
                        new_held = Stack { count: half, ..*slot };
                        slot.take(half);
                    }
                } else if slot.is_empty() {
                    *slot = Stack { count: 1, ..held };
                    new_held.take(1);
                } else if slot.item == held.item && slot.count < max {
                    slot.count += 1;
                    new_held.take(1);
                }
                self.held = new_held;
            }
        }
        true
    }

    /// Takes the crafting result: once into the hand, or (`all`) as many
    /// times as possible into the inventory.
    fn take_result(&mut self, inv: &mut Inventory, all: bool) {
        let n = self.grid_size();
        for _ in 0..if all { 64 } else { 1 } {
            let Some(result) = craft_result(&self.grid[..n * n], n) else { return };
            if all {
                if !inv.has_room_for(result) {
                    self.say("Inventory full.");
                    return;
                }
                inv.add(result);
            } else if self.held.is_empty() {
                self.held = result;
            } else if self.held.item == result.item
                && self.held.count + result.count <= item_def(result.item).max_stack
            {
                self.held.count += result.count;
            } else {
                return;
            }
            consume_grid(&mut self.grid[..n * n]);
        }
    }

    /// Recipes the book lists: craftable now and fitting the grid.
    fn book_recipes(&self, inv: &Inventory, out: &mut [Recipe; 64]) -> usize {
        let n = self.grid_size();
        let mut len = 0;
        for r in Recipe::all() {
            if len < out.len() && r.fits(n) && r.can_craft(inv) {
                out[len] = r;
                len += 1;
            }
        }
        len
    }

    fn update_book(&mut self, input: &Input, inv: &mut Inventory) {
        let mut list = [Recipe::Shaped(0); 64];
        let len = self.book_recipes(inv, &mut list);
        if input.pressed(Key::Up) && self.book_index > 0 {
            self.book_index -= 1;
            self.dirty = true;
        }
        if input.pressed(Key::Down) && self.book_index + 1 < len {
            self.book_index += 1;
            self.dirty = true;
        }
        let ok = input.pressed(Key::Ok);
        let all = input.pressed(Key::Exe);
        if (ok || all) && self.book_index < len {
            let r = list[self.book_index];
            let mut made = 0;
            for _ in 0..if all { 64 } else { 1 } {
                if !r.craft(inv) {
                    break;
                }
                made += 1;
            }
            self.message.clear();
            if made == 0 {
                let _ = self.message.write_str("Inventory full.");
            } else {
                let _ = write!(
                    self.message,
                    "Crafted {} x{}",
                    item_def(r.result().item).name,
                    made as u32 * r.result().count as u32
                );
            }
            let new_len = self.book_recipes(inv, &mut list);
            self.book_index = self.book_index.min(new_len.saturating_sub(1));
            self.dirty = true;
        }
    }

    /// Closes the screen: grid and hand go back to the inventory; what
    /// doesn't fit is passed to `overflow` (the game drops it on the
    /// ground). Returns how many items didn't fit.
    pub fn close(&mut self, inv: &mut Inventory, mut overflow: impl FnMut(Stack)) -> u32 {
        let mut lost = 0;
        for s in self.grid.iter_mut().chain(core::iter::once(&mut self.held)) {
            if !s.is_empty() {
                let left = inv.add(*s);
                if left > 0 {
                    overflow(Stack { count: left, ..*s });
                }
                lost += left as u32;
                *s = Stack::EMPTY;
            }
        }
        lost
    }

    // ------------------------------------------------------------- drawing

    fn draw_slot(&self, p: &mut dyn Platform, s: SlotRef, stack: Stack) {
        let (x, y) = self.pos(s);
        let frame = if s == self.cursor { GOLD } else { FRAME };
        p.fill_rect(x - 2, y - 2, 20, 2, frame);
        p.fill_rect(x - 2, y + 16, 20, 2, frame);
        p.fill_rect(x - 2, y, 2, 16, frame);
        p.fill_rect(x + 16, y, 2, 16, frame);
        hud::draw_stack(p, stack, x, y, SLOT_BG);
    }

    /// Draws the screen if anything changed.
    pub fn draw(&mut self, p: &mut dyn Platform, inv: &Inventory, furnace: Option<&Furnace>) {
        if !self.dirty {
            return;
        }
        self.dirty = false;
        p.fill_rect(0, 0, SCREEN_W as u16, SCREEN_H as u16, MENU_BG);
        let title = match self.container {
            Container::Inventory => "Inventory",
            Container::Table => "Crafting Table",
            Container::Furnace(_) => "Furnace",
            Container::Creative => "Creative Items",
        };
        p.draw_string(title, 8, 4, false, GOLD, MENU_BG);
        if self.book {
            self.draw_book(p, inv);
        } else {
            let mut list = [SlotRef::Result; 64];
            let n = self.slots(&mut list);
            for &s in &list[..n] {
                self.draw_slot(p, s, self.get(s, inv, furnace));
            }
            match self.container {
                Container::Inventory | Container::Table => {
                    let (x, y) = self.pos(SlotRef::Result);
                    p.draw_string("->", x - 26, y, false, TEXT, MENU_BG);
                }
                Container::Furnace(_) => {
                    if let Some(f) = furnace {
                        // Flame gauge (fuel left) and arrow (progress).
                        let flame = if f.burn_total > 0 {
                            (f.burn_left as u32 * 12 / f.burn_total as u32) as u16
                        } else {
                            0
                        };
                        p.fill_rect(102, 54, 12, 12, hud::SLOT_BG);
                        if flame > 0 {
                            p.fill_rect(102, 66 - flame, 12, flame, crate::color::hex(0xF08020));
                        }
                        let prog = (f.cook as u32 * 40 / f.cook_total.max(1) as u32) as u16;
                        p.fill_rect(126, 60, 40, 4, hud::SLOT_BG);
                        p.fill_rect(126, 60, prog, 4, TEXT);
                    }
                }
                Container::Creative => {
                    let pages = palette_len().div_ceil(PAL_PAGE);
                    let mut t: Text<24> = Text::new();
                    let _ = write!(t, "Page {}/{}  ( )", self.page + 1, pages);
                    p.draw_string(t.as_str(), 200, 4, false, hud::DIM, MENU_BG);
                }
            }
        }
        // Info line: what's under the cursor or in the hand.
        let mut info: Text<48> = Text::new();
        if !self.message.as_str().is_empty() {
            let _ = info.write_str(self.message.as_str());
        } else if !self.held.is_empty() {
            let _ = write!(info, "Holding {} x{}", item_def(self.held.item).name, self.held.count);
        } else if !self.book {
            let s = self.get(self.cursor, inv, furnace);
            if !s.is_empty() {
                let _ = write!(info, "{}", item_def(s.item).name);
                let max = durability(s.item);
                if max > 0 {
                    let _ = write!(info, " ({}/{})", max - s.wear.min(max), max);
                } else if s.count > 1 {
                    let _ = write!(info, " x{}", s.count);
                }
            }
        }
        if !self.held.is_empty() {
            hud::draw_stack(p, self.held, 8, 206, MENU_BG);
        }
        hud::text_padded(p, info.as_str(), 30, 207, 41, false, TEXT, MENU_BG);
        let help = if self.book {
            "OK craft 1  EXE craft all  Back: grid"
        } else {
            match self.container {
                Container::Creative => "OK take  EXE one  ( ) page  Back close",
                Container::Furnace(_) => "OK move  EXE half/one  Back close",
                _ => "OK move  EXE half/one  Toolbox recipes",
            }
        };
        p.draw_string(help, 8, 224, false, hud::DIM, MENU_BG);
    }

    fn draw_book(&self, p: &mut dyn Platform, inv: &Inventory) {
        let mut list = [Recipe::Shaped(0); 64];
        let len = self.book_recipes(inv, &mut list);
        p.draw_string("Recipe book: what you can make now", 8, 22, false, hud::DIM, MENU_BG);
        if len == 0 {
            p.draw_string("Nothing yet. Gather wood (hold OK on a log).", 8, 50, false, TEXT, MENU_BG);
            return;
        }
        let first = self.book_index.saturating_sub(BOOK_LINES - 1);
        for (row, &r) in list[first..len].iter().take(BOOK_LINES).enumerate() {
            let y = 42 + row as u16 * 20;
            let sel = first + row == self.book_index;
            let res = r.result();
            hud::draw_stack(p, res, 20, y, MENU_BG);
            let mut t: Text<40> = Text::new();
            let _ = write!(t, "{}{} x{}", if sel { "> " } else { "  " }, item_def(res.item).name, res.count);
            hud::text_padded(p, t.as_str(), 42, y + 1, 34, false, if sel { GOLD } else { TEXT }, MENU_BG);
        }
    }
}

impl Default for InvScreen {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blocks::*;
    use crate::input::keys;

    fn press(inp: &mut Input, k: Key) {
        inp.update(0);
        inp.update(keys(&[k]));
    }

    #[test]
    fn craft_planks_through_the_grid() {
        let mut inv = Inventory::new();
        inv.add(Stack::new(OAK_LOG as Item, 3));
        let mut ui = InvScreen::new();
        ui.open(Container::Inventory, 0);
        let mut inp = Input::new();
        // Pick up the logs (cursor starts on hotbar slot 0).
        press(&mut inp, Key::Ok);
        ui.update(&inp, &mut inv, None);
        assert_eq!(ui.held, Stack::new(OAK_LOG as Item, 3));
        // Move up into the grid and drop one log with EXE.
        for _ in 0..6 {
            press(&mut inp, Key::Up);
            ui.update(&inp, &mut inv, None);
        }
        assert!(matches!(ui.cursor, SlotRef::Grid(_)), "cursor at {:?}", ui.cursor);
        press(&mut inp, Key::Exe);
        ui.update(&inp, &mut inv, None);
        assert_eq!(ui.held.count, 2);
        // Result shows planks; EXE on it crafts into the inventory.
        ui.cursor = SlotRef::Result;
        assert_eq!(ui.get(SlotRef::Result, &inv, None), Stack::new(OAK_PLANKS as Item, 4));
        press(&mut inp, Key::Exe);
        ui.update(&inp, &mut inv, None);
        assert_eq!(inv.count(OAK_PLANKS as Item), 4);
        // Closing returns the held logs.
        assert_eq!(ui.close(&mut inv, |_| {}), 0);
        assert_eq!(inv.count(OAK_LOG as Item), 2);
    }

    #[test]
    fn recipe_book_crafts() {
        let mut inv = Inventory::new();
        inv.add(Stack::new(OAK_LOG as Item, 1));
        let mut ui = InvScreen::new();
        ui.open(Container::Inventory, 0);
        let mut inp = Input::new();
        press(&mut inp, Key::Toolbox);
        ui.update(&inp, &mut inv, None);
        assert!(ui.book);
        press(&mut inp, Key::Ok);
        ui.update(&inp, &mut inv, None);
        assert_eq!(inv.count(OAK_PLANKS as Item), 4);
        assert_eq!(inv.count(OAK_LOG as Item), 0);
    }
}
