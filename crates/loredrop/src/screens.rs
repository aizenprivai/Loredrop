//! Full-screen menus drawn over the world: the inventory (with the Creative
//! item list) and, later, crafting tables, furnaces, lore tablets and the
//! journal.
//!
//! While a screen is open the mouse is released, the player stands still,
//! and clicks go to the menu instead of the world.

use crate::hud::{SLOT, draw_slot};
use egui::{Color32, FontId, Pos2, Rect, Sense, Stroke, TextureHandle, Vec2};
use ld_game::inventory::{HOTBAR, click_slot};
use ld_game::{Click, Inventory, ItemRegistry, ItemStack};

/// Which menu is open.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum Screen {
    #[default]
    None,
    Inventory,
}

/// State that lives while menus are open.
#[derive(Default)]
pub struct MenuState {
    /// The stack "held by the mouse".
    pub cursor: Option<ItemStack>,
    /// Creative item-list search text.
    pub search: String,
}

/// Everything a menu needs to read and change.
pub struct MenuCtx<'a> {
    pub inventory: &'a mut Inventory,
    pub items: &'a ItemRegistry,
    pub icons: &'a [TextureHandle],
    pub creative: bool,
    pub state: &'a mut MenuState,
}

/// Draws the open screen. Returns true if it asked to close.
pub fn draw(ctx: &egui::Context, screen: &Screen, m: &mut MenuCtx) -> bool {
    match screen {
        Screen::None => false,
        Screen::Inventory => inventory_screen(ctx, m),
    }
}

fn panel_frame() -> egui::Frame {
    egui::Frame::new()
        .fill(Color32::from_rgba_unmultiplied(28, 28, 34, 235))
        .stroke(Stroke::new(2.0, Color32::from_rgb(90, 90, 100)))
        .inner_margin(12.0)
}

/// Callback for shift-clicking a slot.
type ShiftClick<'a> = Option<&'a mut dyn FnMut(&mut Option<ItemStack>)>;

/// One clickable slot bound to an `Option<ItemStack>`.
fn slot_ui(
    ui: &mut egui::Ui,
    slot: &mut Option<ItemStack>,
    m: &mut MenuCtx,
    on_shift: Option<&mut dyn FnMut(&mut Option<ItemStack>)>,
) {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(SLOT - 2.0), Sense::click());
    draw_slot(ui.painter(), rect, *slot, m.icons, m.items, resp.hovered());
    let shift = ui.input(|i| i.modifiers.shift);
    if resp.clicked() {
        match on_shift {
            Some(f) if shift => f(slot),
            _ => click_slot(slot, &mut m.state.cursor, Click::Left, m.items),
        }
    } else if resp.secondary_clicked() {
        click_slot(slot, &mut m.state.cursor, Click::Right, m.items);
    }
    if resp.hovered()
        && let Some(s) = slot
    {
        resp.on_hover_text(&m.items.get(s.item).display);
    }
}

fn inventory_screen(ctx: &egui::Context, m: &mut MenuCtx) -> bool {
    let mut close = false;
    egui::Area::new(egui::Id::unique("inventory"))
        .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            panel_frame().show(ui, |ui| {
                ui.spacing_mut().item_spacing = Vec2::splat(2.0);
                if m.creative {
                    creative_list(ui, m);
                    ui.add_space(10.0);
                }
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Inventory").color(Color32::WHITE).size(16.0));
                    ui.add_space(12.0);
                    if ui
                        .button("Sort")
                        .on_hover_text("Sort and merge the storage slots (hotbar untouched)")
                        .clicked()
                    {
                        m.inventory.sort_storage(m.items);
                    }
                    if ui.button("Close").clicked() {
                        close = true;
                    }
                });
                // Armour + off-hand (functional in Phase 6).
                ui.horizontal(|ui| {
                    for i in 0..4 {
                        let mut s = m.inventory.armor[i];
                        slot_ui(ui, &mut s, m, None);
                        m.inventory.armor[i] = s;
                    }
                    ui.add_space(SLOT / 2.0);
                    let mut off = m.inventory.offhand;
                    slot_ui(ui, &mut off, m, None);
                    m.inventory.offhand = off;
                });
                ui.add_space(6.0);
                // Storage (3 rows), then the hotbar separated by a gap.
                for row in 0..4 {
                    if row == 3 {
                        ui.add_space(8.0);
                    }
                    ui.horizontal(|ui| {
                        for col in 0..9 {
                            let index = if row == 3 { col } else { HOTBAR + row * 9 + col };
                            let mut s = m.inventory.slots[index];
                            let mut moved = false;
                            slot_ui(ui, &mut s, m, Some(&mut |_| moved = true));
                            m.inventory.slots[index] = s;
                            if moved {
                                m.inventory.quick_move(index, m.items);
                            }
                        }
                    });
                }
            });
        });
    draw_cursor(ctx, m);
    close
}

/// Creative mode: every item in a scrollable grid, filtered by a search box.
/// Clicking an item puts a full stack on the cursor; clicking the list while
/// holding something deletes it (as in Minecraft).
fn creative_list(ui: &mut egui::Ui, m: &mut MenuCtx) {
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("All items").color(Color32::WHITE).size(16.0));
        ui.add(egui::TextEdit::singleline(&mut m.state.search).hint_text("Search…").desired_width(200.0));
    });
    let query = m.state.search.to_lowercase();
    let matches: Vec<_> = m
        .items
        .iter()
        .filter(|(_, d)| {
            query.is_empty() || d.display.to_lowercase().contains(&query) || d.name.contains(&query)
        })
        .map(|(id, d)| (id, d.max_stack))
        .collect();
    egui::ScrollArea::vertical().max_height(SLOT * 4.0 + 8.0).show(ui, |ui| {
        for chunk in matches.chunks(9) {
            ui.horizontal(|ui| {
                for &(id, max) in chunk {
                    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(SLOT - 2.0), Sense::click());
                    draw_slot(
                        ui.painter(),
                        rect,
                        Some(ItemStack::new(id, 1)),
                        m.icons,
                        m.items,
                        resp.hovered(),
                    );
                    if resp.clicked() || resp.secondary_clicked() {
                        m.state.cursor = if m.state.cursor.is_some() {
                            None // holding something: the list acts as a bin
                        } else {
                            let n = if resp.clicked() { max } else { 1 };
                            Some(ItemStack::new(id, n))
                        };
                    }
                    resp.on_hover_text(&m.items.get(id).display);
                }
            });
        }
        if matches.is_empty() {
            ui.label("No items match.");
        }
    });
}

/// Draws the stack held by the mouse at the pointer.
fn draw_cursor(ctx: &egui::Context, m: &MenuCtx) {
    let (Some(stack), Some(pos)) = (m.state.cursor, ctx.pointer_hover_pos()) else { return };
    let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Tooltip, egui::Id::unique("cursor")));
    let rect = Rect::from_center_size(pos, Vec2::splat(SLOT - 6.0));
    if let Some(tex) = m.icons.get(stack.item.0 as usize) {
        painter.image(tex.id(), rect, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
    }
    if stack.count > 1 {
        painter.text(
            rect.right_bottom(),
            egui::Align2::RIGHT_BOTTOM,
            stack.count.to_string(),
            FontId::monospace(13.0),
            Color32::WHITE,
        );
    }
}

/// Puts the cursor stack back into the inventory when a menu closes (what
/// doesn't fit is lost for now; Phase 5 item entities will drop it instead).
pub fn return_cursor(state: &mut MenuState, inventory: &mut Inventory, items: &ItemRegistry, creative: bool) {
    if let Some(s) = state.cursor.take()
        && !creative
    {
        let _ = inventory.add(s, items);
    }
}
