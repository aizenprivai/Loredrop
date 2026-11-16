//! Drawing helpers for everything that isn't the 3D view: the status line,
//! the hotbar, hearts and hunger, item icons, menus and text screens.
//!
//! Screen layout while playing:
//!
//! ```text
//!   y   0..200  3D view
//!   y 200..218  status line (messages, block being mined, FPS)
//!   y 218..240  hotbar (9 slots) | hearts (top) and food or air (bottom)
//! ```
//!
//! The 3D view never overlaps the bottom bar, so the bar only needs
//! redrawing when its content changes (no flicker, no wasted time).

use crate::blocks::{TEXTURES, TRANSPARENT};
use crate::color::hex;
use crate::items::{Stack, durability, item_def};
use crate::render::VIEW_H;
use crate::survival::{MAX_AIR, Vitals};
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
/// Slot background in the inventory screens.
pub const SLOT_BG: u16 = hex(0x2A2D36);
/// Slot frame.
pub const FRAME: u16 = hex(0x3A3E48);

/// Top of the status line.
pub const STATUS_Y: u16 = VIEW_H as u16;
/// Top of the hotbar row.
pub const HOTBAR_Y: u16 = STATUS_Y + 18;
/// Size of one hotbar slot.
pub const SLOT: u16 = 22;
/// Left edge of the hearts / food panel.
pub const PANEL_X: u16 = 2 + 9 * SLOT + 2;

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

/// 3x5 digits for stack counts (bit 2 = left column).
const DIGITS: [[u8; 5]; 10] = [
    [7, 5, 5, 5, 7],
    [2, 6, 2, 2, 7],
    [7, 1, 7, 4, 7],
    [7, 1, 3, 1, 7],
    [5, 5, 7, 1, 1],
    [7, 4, 7, 1, 7],
    [7, 4, 7, 5, 7],
    [7, 1, 2, 2, 2],
    [7, 5, 7, 5, 7],
    [7, 5, 7, 1, 7],
];

/// Builds the 16x16 picture of a stack: its icon, its count (if more than
/// one) and a wear bar for used tools.
pub fn stack_pixels(s: Stack, bg: u16) -> [u16; 256] {
    let mut px = [bg; 256];
    if s.is_empty() {
        return px;
    }
    let tex = &TEXTURES[item_def(s.item).icon as usize];
    for (o, &c) in px.iter_mut().zip(tex.iter()) {
        if c != TRANSPARENT {
            *o = c;
        }
    }
    if s.count > 1 {
        // Right-aligned digits with a dark outline, bottom-right corner.
        let (tens, ones) = (s.count / 10, s.count % 10);
        let mut put = |d: u8, x0: usize| {
            for (row, bits) in DIGITS[d as usize].iter().enumerate() {
                for col in 0..3 {
                    if bits >> (2 - col) & 1 == 1 {
                        let (x, y) = (x0 + col, 10 + row);
                        for (dx, dy) in [(1usize, 0usize), (0, 1), (1, 1)] {
                            if x + dx < 16 && y + dy < 16 && px[(y + dy) * 16 + x + dx] != 0xFFFF {
                                px[(y + dy) * 16 + x + dx] = 0x0000;
                            }
                        }
                        px[y * 16 + x] = 0xFFFF;
                    }
                }
            }
        };
        put(ones, 12);
        if tens > 0 {
            put(tens, 8);
        }
    }
    let max = durability(s.item);
    if max > 0 && s.wear > 0 {
        let left = 1.0 - s.wear as f32 / max as f32;
        let n = (left * 13.0) as usize + 1;
        let color = if left > 0.5 {
            hex(0x40D040)
        } else if left > 0.2 {
            hex(0xE0C030)
        } else {
            hex(0xE04040)
        };
        for x in 1..15 {
            px[15 * 16 + x] = if x <= n { color } else { 0x0000 };
        }
    }
    px
}

/// Draws a stack's icon at `(x, y)`.
pub fn draw_stack(p: &mut dyn Platform, s: Stack, x: u16, y: u16, bg: u16) {
    p.push_rect(x, y, 16, 16, &stack_pixels(s, bg));
}

/// Draws the whole hotbar.
pub fn hotbar(p: &mut dyn Platform, slots: &[Stack], selected: usize) {
    p.fill_rect(0, HOTBAR_Y, PANEL_X, SCREEN_H as u16 - HOTBAR_Y, BAR_BG);
    for (i, s) in slots.iter().take(9).enumerate() {
        let x = 2 + i as u16 * SLOT;
        let frame = if i == selected { GOLD } else { FRAME };
        p.fill_rect(x, HOTBAR_Y + 1, SLOT - 2, 2, frame);
        p.fill_rect(x, HOTBAR_Y + SLOT - 3, SLOT - 2, 2, frame);
        p.fill_rect(x, HOTBAR_Y + 1, 2, SLOT - 2, frame);
        p.fill_rect(x + SLOT - 4, HOTBAR_Y + 1, 2, SLOT - 2, frame);
        draw_stack(p, *s, x + 2, HOTBAR_Y + 3, BAR_BG);
    }
}

// 9x9 pictures for the hearts / food / air panel (bit 8 = left column).
const HEART: [u16; 9] = [0x0C6, 0x1EF, 0x1FF, 0x1FF, 0x0FE, 0x07C, 0x038, 0x010, 0x000];
const FOOD: [u16; 9] = [0x006, 0x00F, 0x01E, 0x038, 0x070, 0x0F0, 0x1E0, 0x180, 0x100];
const BUBBLE: [u16; 9] = [0x000, 0x038, 0x044, 0x08A, 0x082, 0x082, 0x044, 0x038, 0x000];

/// Draws one 9x9 icon; `fill` is how much of it (from the left, in 1/2
/// steps: 0 empty, 1 half, 2 full) is in `color`, the rest in `empty`.
fn panel_icon(p: &mut dyn Platform, shape: &[u16; 9], x: u16, y: u16, fill: u8, color: u16, empty: u16) {
    let mut px = [BAR_BG; 81];
    for (row, bits) in shape.iter().enumerate() {
        for col in 0..9 {
            if bits >> (8 - col) & 1 == 1 {
                let full = fill == 2 || (fill == 1 && col < 5);
                px[row * 9 + col] = if full { color } else { empty };
            }
        }
    }
    p.push_rect(x, y, 9, 9, &px);
}

/// Draws hearts (top row) and food, or air bubbles while under water
/// (bottom row), right of the hotbar.
pub fn vitals_panel(p: &mut dyn Platform, v: &Vitals, flash: bool) {
    let heart_color = if flash { hex(0xFF9090) } else { hex(0xE02828) };
    let hp = libm::ceilf(v.health) as i32;
    for i in 0..10 {
        let fill = (hp - i * 2).clamp(0, 2) as u8;
        panel_icon(p, &HEART, PANEL_X + i as u16 * 11, HOTBAR_Y + 1, fill, heart_color, hex(0x40202A));
    }
    let y = HOTBAR_Y + 12;
    if v.air < MAX_AIR {
        let bubbles = ((v.air.max(0) as i32 * 10 + MAX_AIR as i32 - 1) / MAX_AIR as i32) as u16;
        for i in 0..10u16 {
            let x = PANEL_X + (9 - i) * 11;
            panel_icon(p, &BUBBLE, x, y, if i < bubbles { 2 } else { 0 }, hex(0x80C8FF), BAR_BG);
        }
    } else {
        for i in 0..10 {
            // Food fills from the right, as on PC.
            let fill = (v.food as i32 - i * 2).clamp(0, 2) as u8;
            let x = PANEL_X + (9 - i as u16) * 11;
            panel_icon(p, &FOOD, x, y, fill, hex(0xC88440), hex(0x3A3028));
        }
    }
}

/// Clears the screen to the menu background and draws a centred title.
pub fn menu_frame(p: &mut dyn Platform, title: &str) {
    p.fill_rect(0, 0, SCREEN_W as u16, SCREEN_H as u16, MENU_BG);
    let w = title.chars().count() as u16 * LARGE_W;
    p.draw_string(title, (SCREEN_W as u16 - w) / 2, 10, true, GOLD, MENU_BG);
    p.fill_rect(40, 34, SCREEN_W as u16 - 80, 1, FRAME);
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
