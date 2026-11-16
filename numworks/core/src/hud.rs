//! Drawing helpers for everything that isn't the 3D view: the status line,
//! the hotbar, menus and text screens.
//!
//! Screen layout while playing:
//!
//! ```text
//!   y   0..200  3D view
//!   y 200..218  status line (messages, block being mined, FPS)
//!   y 218..240  hotbar (9 slots) and clock
//! ```
//!
//! The 3D view never overlaps the bottom bar, so the bar only needs
//! redrawing when its content changes (no flicker, no wasted time).

use crate::blocks::*;
use crate::color::hex;
use crate::render::VIEW_H;
use crate::text::Text;
use crate::{Platform, SCREEN_H, SCREEN_W};

/// Width of one character of the small font, in pixels.
pub const SMALL_W: u16 = 7;
/// Height of the small font.
pub const SMALL_H: u16 = 14;
/// Width of one character of the large font.
pub const LARGE_W: u16 = 10;
/// Height of the large font.
pub const LARGE_H: u16 = 18;

/// Bottom bar background.
pub const BAR_BG: u16 = hex(0x1C1E24);
/// Normal text colour.
pub const TEXT: u16 = hex(0xE8E8E8);
/// Dimmed text colour.
pub const DIM: u16 = hex(0x9098A8);
/// Highlight colour (Loredrop's gold, like the Hollow's cracks).
pub const GOLD: u16 = hex(0xE8B64C);
/// Menu background.
pub const MENU_BG: u16 = hex(0x14161C);

/// Top of the status line.
pub const STATUS_Y: u16 = VIEW_H as u16;
/// Top of the hotbar row.
pub const HOTBAR_Y: u16 = STATUS_Y + 18;
/// Size of one hotbar slot.
pub const SLOT: u16 = 22;

/// One hotbar slot: a block and how many of it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Slot {
    /// The block (`AIR` = empty slot).
    pub block: Block,
    /// How many (up to 64).
    pub count: u8,
}

impl Slot {
    /// An empty slot.
    pub const EMPTY: Slot = Slot { block: AIR, count: 0 };
}

/// Draws `text` padded with spaces to `width` characters, so shorter text
/// erases what was there before without clearing (and flickering).
#[allow(clippy::too_many_arguments)]
pub fn text_padded(
    p: &mut dyn Platform,
    text: &str,
    x: u16,
    y: u16,
    width: usize,
    large: bool,
    fg: u16,
    bg: u16,
) {
    let mut t: Text<64> = Text::new();
    let _ = core::fmt::Write::write_str(&mut t, text);
    let n = text.chars().count();
    for _ in n..width {
        let _ = core::fmt::Write::write_str(&mut t, " ");
    }
    p.draw_string(t.as_str(), x, y, large, fg, bg);
}

/// Draws the status line text (left) and `right` text (right-aligned).
pub fn status_line(p: &mut dyn Platform, left: &str, right: &str) {
    let right_chars = right.chars().count() as u16;
    let left_width = ((SCREEN_W as u16 - 8) / SMALL_W - right_chars - 1) as usize;
    text_padded(p, left, 4, STATUS_Y + 2, left_width, false, TEXT, BAR_BG);
    p.draw_string(right, SCREEN_W as u16 - 4 - right_chars * SMALL_W, STATUS_Y + 2, false, GOLD, BAR_BG);
}

/// Draws the whole hotbar and the clock panel.
pub fn hotbar(p: &mut dyn Platform, slots: &[Slot; 9], selected: usize, panel: &str) {
    p.fill_rect(0, HOTBAR_Y, SCREEN_W as u16, SCREEN_H as u16 - HOTBAR_Y, BAR_BG);
    for (i, s) in slots.iter().enumerate() {
        let x = 2 + i as u16 * SLOT;
        let frame = if i == selected { GOLD } else { hex(0x3A3E48) };
        p.fill_rect(x, HOTBAR_Y + 1, SLOT - 2, 2, frame);
        p.fill_rect(x, HOTBAR_Y + SLOT - 3, SLOT - 2, 2, frame);
        p.fill_rect(x, HOTBAR_Y + 1, 2, SLOT - 2, frame);
        p.fill_rect(x + SLOT - 4, HOTBAR_Y + 1, 2, SLOT - 2, frame);
        if s.block != AIR && s.count > 0 {
            icon(p, s.block, x + 2, HOTBAR_Y + 3);
        }
    }
    p.draw_string(panel, 2 + 9 * SLOT + 4, HOTBAR_Y + 4, false, DIM, BAR_BG);
}

/// Draws a 16x16 picture of `block` (its side texture).
pub fn icon(p: &mut dyn Platform, block: Block, x: u16, y: u16) {
    let tex = &TEXTURES[def(block).tex[1] as usize];
    let mut px = [0u16; 256];
    for (o, &c) in px.iter_mut().zip(tex.iter()) {
        *o = if c == TRANSPARENT { BAR_BG } else { c };
    }
    p.push_rect(x, y, 16, 16, &px);
}

/// Clears the screen to the menu background and draws a centred title.
pub fn menu_frame(p: &mut dyn Platform, title: &str) {
    p.fill_rect(0, 0, SCREEN_W as u16, SCREEN_H as u16, MENU_BG);
    let w = title.chars().count() as u16 * LARGE_W;
    p.draw_string(title, (SCREEN_W as u16 - w) / 2, 10, true, GOLD, MENU_BG);
    p.fill_rect(40, 34, SCREEN_W as u16 - 80, 1, hex(0x3A3E48));
}

/// Draws a list of menu items starting at `y`, the `selected` one marked.
pub fn menu_items(p: &mut dyn Platform, items: &[&str], selected: usize, y: u16) {
    for (i, item) in items.iter().enumerate() {
        let sel = i == selected;
        let yy = y + i as u16 * 24;
        let mut t: Text<40> = Text::new();
        let _ = core::fmt::Write::write_fmt(&mut t, format_args!("{} {}", if sel { ">" } else { " " }, item));
        text_padded(p, t.as_str(), 50, yy, 24, true, if sel { GOLD } else { TEXT }, MENU_BG);
    }
}

/// Draws a centred line of small text.
pub fn centered_small(p: &mut dyn Platform, text: &str, y: u16, color: u16) {
    let w = text.chars().count() as u16 * SMALL_W;
    p.draw_string(text, (SCREEN_W as u16).saturating_sub(w) / 2, y, false, color, MENU_BG);
}
