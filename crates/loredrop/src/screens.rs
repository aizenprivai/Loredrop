//! Full-screen menus drawn over the world: the inventory (with the Creative
//! item list) and, later, crafting tables, furnaces, lore tablets and the
//! journal.
//!
//! While a screen is open the mouse is released, the player stands still,
//! and clicks go to the menu instead of the world.

use crate::hud::{SLOT, draw_slot};
use egui::{Color32, FontId, Pos2, Rect, Sense, Stroke, TextureHandle, Vec2};
use ld_game::crafting::{ShapedRecipe, ShapelessRecipe};
use ld_game::inventory::{HOTBAR, click_slot};
use ld_game::{Click, Inventory, ItemRegistry, ItemStack, RecipeBook};

/// Which menu is open.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum Screen {
    #[default]
    None,
    /// The player inventory with its 2×2 crafting grid.
    Inventory,
    /// A crafting table's 3×3 grid.
    CraftingTable,
    /// The furnace at this position.
    Furnace(ld_core::coords::BlockPos),
    /// Reading a lore tablet: fragment `index` of `era`.
    Tablet { era: String, index: usize },
    /// The journal of discovered lore.
    Journal,
}

/// State that lives while menus are open.
#[derive(Default)]
pub struct MenuState {
    /// The stack "held by the mouse".
    pub cursor: Option<ItemStack>,
    /// Creative item-list search text.
    pub search: String,
    /// Crafting grid contents (row-major; 4 used for 2×2, 9 for 3×3).
    pub grid: [Option<ItemStack>; 9],
    /// Recipe browser search text and the recipe being shown.
    pub recipe_search: String,
    pub shown_recipe: Option<ld_game::ItemId>,
}

/// Everything a menu needs to read and change.
pub struct MenuCtx<'a> {
    pub inventory: &'a mut Inventory,
    pub items: &'a ItemRegistry,
    pub icons: &'a [TextureHandle],
    pub creative: bool,
    pub state: &'a mut MenuState,
    pub recipes: &'a RecipeBook,
    /// The open furnace, when the screen is a furnace.
    pub furnace: Option<&'a mut ld_game::furnace::Furnace>,
    pub lore: &'a ld_assets::Lore,
    pub journal: &'a [(String, usize)],
}

/// Draws the open screen. Returns true if it asked to close.
pub fn draw(ctx: &egui::Context, screen: &Screen, m: &mut MenuCtx) -> bool {
    match screen {
        Screen::None => false,
        Screen::Inventory => inventory_screen(ctx, m, 2),
        Screen::CraftingTable => inventory_screen(ctx, m, 3),
        Screen::Furnace(_) => inventory_screen(ctx, m, 0),
        Screen::Tablet { era, index } => tablet_screen(ctx, m, era, *index),
        Screen::Journal => journal_screen(ctx, m),
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
fn slot_ui(ui: &mut egui::Ui, slot: &mut Option<ItemStack>, m: &mut MenuCtx, on_shift: ShiftClick) {
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

fn inventory_screen(ctx: &egui::Context, m: &mut MenuCtx, grid_size: usize) -> bool {
    let mut close = false;
    egui::Area::new(egui::Id::unique("inventory"))
        .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            panel_frame().show(ui, |ui| {
                ui.spacing_mut().item_spacing = Vec2::splat(2.0);
                if m.creative && grid_size == 2 {
                    creative_list(ui, m);
                    ui.add_space(10.0);
                }
                ui.horizontal(|ui| {
                    if grid_size == 0 {
                        furnace_panel(ui, m);
                    } else {
                        crafting_grid(ui, m, grid_size);
                    }
                    ui.add_space(16.0);
                    recipe_browser(ui, m);
                });
                ui.add_space(8.0);
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

/// The crafting grid, an arrow and the result slot.
fn crafting_grid(ui: &mut egui::Ui, m: &mut MenuCtx, size: usize) {
    ui.vertical(|ui| {
        ui.label(egui::RichText::new("Crafting").color(Color32::WHITE).size(16.0));
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                for row in 0..size {
                    ui.horizontal(|ui| {
                        for col in 0..size {
                            let i = row * 3 + col;
                            let mut s = m.state.grid[i];
                            slot_ui(ui, &mut s, m, None);
                            m.state.grid[i] = s;
                        }
                    });
                }
            });
            ui.add_space(8.0);
            // A drawn arrow (the UI font has no arrow glyph).
            let (r, _) = ui.allocate_exact_size(Vec2::new(28.0, SLOT), Sense::hover());
            let (y, x0, x1) = (r.center().y, r.left() + 2.0, r.right() - 2.0);
            let col = Color32::from_gray(200);
            ui.painter().line_segment([Pos2::new(x0, y), Pos2::new(x1 - 8.0, y)], Stroke::new(4.0, col));
            ui.painter().add(egui::Shape::convex_polygon(
                vec![Pos2::new(x1 - 10.0, y - 8.0), Pos2::new(x1, y), Pos2::new(x1 - 10.0, y + 8.0)],
                col,
                Stroke::NONE,
            ));
            ui.add_space(8.0);
            let cells = grid_cells(&m.state.grid, size);
            let result = m.recipes.craft_result(&cells, size);
            let (rect, resp) = ui.allocate_exact_size(Vec2::splat(SLOT + 6.0), Sense::click());
            draw_slot(ui.painter(), rect, result, m.icons, m.items, resp.hovered());
            if let Some(r) = result {
                let shift = ui.input(|i| i.modifiers.shift);
                if resp.clicked() && shift {
                    // QoL: craft as many as the ingredients allow, straight into the inventory.
                    let mut guard = 0;
                    while guard < 64 {
                        guard += 1;
                        let cells = grid_cells(&m.state.grid, size);
                        if m.recipes.craft_result(&cells, size) != Some(r) {
                            break;
                        }
                        if m.inventory.add(r, m.items).is_some() {
                            break; // inventory full (the overflow is lost only if it can't fit at all)
                        }
                        consume_grid(&mut m.state.grid, size);
                    }
                } else if resp.clicked() || resp.secondary_clicked() {
                    let max = m.items.get(r.item).max_stack;
                    let ok = match m.state.cursor {
                        None => true,
                        Some(c) => c.stacks_with(&r) && c.count + r.count <= max,
                    };
                    if ok {
                        match &mut m.state.cursor {
                            Some(c) => c.count += r.count,
                            None => m.state.cursor = Some(r),
                        }
                        consume_grid(&mut m.state.grid, size);
                    }
                }
                resp.on_hover_text(&m.items.get(r.item).display);
            }
        });
    });
}

/// The active part of the 3×3 storage as a `size`×`size` grid.
fn grid_cells(grid: &[Option<ItemStack>; 9], size: usize) -> Vec<Option<ItemStack>> {
    (0..size * size).map(|i| grid[(i / size) * 3 + i % size]).collect()
}

fn consume_grid(grid: &mut [Option<ItemStack>; 9], size: usize) {
    let mut cells = grid_cells(grid, size);
    RecipeBook::consume_ingredients(&mut cells);
    for (i, c) in cells.into_iter().enumerate() {
        grid[(i / size) * 3 + i % size] = c;
    }
}

/// QoL recipe browser (GDD §6): search any item and see how it's made.
fn recipe_browser(ui: &mut egui::Ui, m: &mut MenuCtx) {
    ui.vertical(|ui| {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Recipes").color(Color32::WHITE).size(16.0));
            ui.add(
                egui::TextEdit::singleline(&mut m.state.recipe_search)
                    .hint_text("How do I make…")
                    .desired_width(150.0),
            );
        });
        let q = m.state.recipe_search.to_lowercase();
        if !q.is_empty() {
            let hits: Vec<_> = m
                .items
                .iter()
                .filter(|(id, d)| {
                    d.display.to_lowercase().contains(&q) && {
                        let (a, b, c) = m.recipes.recipes_for(*id);
                        !(a.is_empty() && b.is_empty() && c.is_empty())
                    }
                })
                .map(|(id, d)| (id, d.display.clone()))
                .take(6)
                .collect();
            ui.horizontal_wrapped(|ui| {
                for (id, name) in hits {
                    if ui.small_button(name).clicked() {
                        m.state.shown_recipe = Some(id);
                    }
                }
            });
        }
        if let Some(id) = m.state.shown_recipe {
            let (shaped, shapeless, smelt) = m.recipes.recipes_for(id);
            ui.label(egui::RichText::new(&m.items.get(id).display).color(Color32::LIGHT_YELLOW));
            if let Some(r) = shaped.first() {
                show_shaped(ui, r, m);
            } else if let Some(r) = shapeless.first() {
                show_shapeless(ui, r, m);
            } else if let Some(r) = smelt.first() {
                ui.label(format!("Smelt {} in a furnace", m.items.get(r.input.0[0]).display));
            }
        }
    });
}

/// Ghost (read-only) slots showing a shaped recipe; tags show their first item.
fn show_shaped(ui: &mut egui::Ui, r: &ShapedRecipe, m: &MenuCtx) {
    for y in 0..r.height {
        ui.horizontal(|ui| {
            for x in 0..r.width {
                let item = r.cells[y * r.width + x].as_ref().map(|ing| ItemStack::new(ing.0[0], 1));
                let (rect, _) = ui.allocate_exact_size(Vec2::splat(SLOT * 0.7), Sense::hover());
                draw_slot(ui.painter(), rect, item, m.icons, m.items, false);
            }
        });
    }
    if r.result.count > 1 {
        ui.label(format!("Makes {}", r.result.count));
    }
}

fn show_shapeless(ui: &mut egui::Ui, r: &ShapelessRecipe, m: &MenuCtx) {
    ui.horizontal(|ui| {
        for ing in &r.ingredients {
            let (rect, _) = ui.allocate_exact_size(Vec2::splat(SLOT * 0.7), Sense::hover());
            draw_slot(ui.painter(), rect, Some(ItemStack::new(ing.0[0], 1)), m.icons, m.items, false);
        }
    });
    ui.label(format!("Any arrangement · makes {}", r.result.count));
}

/// Returns crafting-grid items to the inventory when a menu closes.
pub fn return_grid(state: &mut MenuState, inventory: &mut Inventory, items: &ItemRegistry) {
    for cell in state.grid.iter_mut() {
        if let Some(s) = cell.take() {
            let _ = inventory.add(s, items);
        }
    }
}

/// Furnace: input on top, fuel below with a flame meter, arrow showing
/// smelting progress, output on the right.
fn furnace_panel(ui: &mut egui::Ui, m: &mut MenuCtx) {
    let Some(f) = m.furnace.take() else { return };
    ui.vertical(|ui| {
        ui.label(egui::RichText::new("Furnace").color(Color32::WHITE).size(16.0));
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                let mut input = f.input;
                slot_ui(ui, &mut input, m, None);
                f.input = input;
                // Flame meter: how much of the current fuel item is left.
                let (r, _) = ui.allocate_exact_size(Vec2::new(SLOT - 2.0, 10.0), Sense::hover());
                ui.painter().rect_filled(r, 0.0, Color32::from_gray(40));
                if f.burn_total > 0 {
                    let w = r.width() * f.burn_left as f32 / f.burn_total as f32;
                    ui.painter().rect_filled(
                        Rect::from_min_size(r.min, Vec2::new(w, r.height())),
                        0.0,
                        Color32::from_rgb(255, 140, 30),
                    );
                }
                let mut fuel = f.fuel;
                slot_ui(ui, &mut fuel, m, None);
                f.fuel = fuel;
            });
            ui.add_space(10.0);
            // Progress arrow.
            let (r, _) = ui.allocate_exact_size(Vec2::new(44.0, SLOT), Sense::hover());
            let y = r.center().y;
            ui.painter().rect_filled(
                Rect::from_min_max(Pos2::new(r.left(), y - 4.0), Pos2::new(r.right() - 10.0, y + 4.0)),
                0.0,
                Color32::from_gray(60),
            );
            if f.cook_total > 0 {
                let w = (r.width() - 10.0) * f.cook as f32 / f.cook_total as f32;
                ui.painter().rect_filled(
                    Rect::from_min_max(Pos2::new(r.left(), y - 4.0), Pos2::new(r.left() + w, y + 4.0)),
                    0.0,
                    Color32::WHITE,
                );
            }
            ui.painter().add(egui::Shape::convex_polygon(
                vec![
                    Pos2::new(r.right() - 12.0, y - 9.0),
                    Pos2::new(r.right(), y),
                    Pos2::new(r.right() - 12.0, y + 9.0),
                ],
                Color32::from_gray(160),
                Stroke::NONE,
            ));
            ui.add_space(10.0);
            // Output: take only (left = all, right = half, shift = into inventory).
            let (rect, resp) = ui.allocate_exact_size(Vec2::splat(SLOT + 6.0), Sense::click());
            draw_slot(ui.painter(), rect, f.output, m.icons, m.items, resp.hovered());
            if let Some(out) = f.output {
                if resp.clicked() && ui.input(|i| i.modifiers.shift) {
                    f.output = m.inventory.add(out, m.items);
                } else if resp.clicked() || resp.secondary_clicked() {
                    let max = m.items.get(out.item).max_stack;
                    match &mut m.state.cursor {
                        None => {
                            m.state.cursor = Some(out);
                            f.output = None;
                        }
                        Some(c) if c.stacks_with(&out) => {
                            let moved = (max - c.count).min(out.count);
                            c.count += moved;
                            f.output =
                                (out.count > moved).then_some(ItemStack { count: out.count - moved, ..out });
                        }
                        _ => {}
                    }
                }
            }
        });
    });
    m.furnace = Some(f);
}

/// Display name of a story era.
fn era_title(era: &str) -> &'static str {
    match era {
        "golden" => "The Age of Building",
        "expansion" => "The Gateways",
        _ => "What They Became",
    }
}

/// A parchment-coloured panel for lore text.
fn parchment() -> egui::Frame {
    egui::Frame::new()
        .fill(Color32::from_rgb(222, 206, 168))
        .stroke(Stroke::new(3.0, Color32::from_rgb(120, 92, 56)))
        .inner_margin(20.0)
}

const INK: Color32 = Color32::from_rgb(54, 40, 26);

fn tablet_screen(ctx: &egui::Context, m: &MenuCtx, era: &str, index: usize) -> bool {
    let mut close = false;
    egui::Area::new(egui::Id::unique("tablet"))
        .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            parchment().show(ui, |ui| {
                ui.set_max_width(460.0);
                ui.label(
                    egui::RichText::new(era_title(era))
                        .color(Color32::from_rgb(120, 70, 30))
                        .size(14.0)
                        .italics(),
                );
                ui.add_space(8.0);
                let text = m.lore.era(era).get(index).copied().unwrap_or("The carving is worn away.");
                ui.label(egui::RichText::new(text).color(INK).size(18.0));
                ui.add_space(14.0);
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(format!(
                            "Added to your journal ({} of {} found) · press J to read it",
                            m.journal.len(),
                            m.lore.fragments.len()
                        ))
                        .color(INK)
                        .size(12.0),
                    );
                    if ui.button("Close").clicked() {
                        close = true;
                    }
                });
            });
        });
    close
}

fn journal_screen(ctx: &egui::Context, m: &MenuCtx) -> bool {
    let mut close = false;
    egui::Area::new(egui::Id::unique("journal"))
        .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            parchment().show(ui, |ui| {
                ui.set_width(520.0);
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Journal").color(INK).size(22.0).strong());
                    ui.add_space(12.0);
                    ui.label(
                        egui::RichText::new(format!(
                            "{} of {} fragments found",
                            m.journal.len(),
                            m.lore.fragments.len()
                        ))
                        .color(INK)
                        .size(13.0),
                    );
                    if ui.button("Close").clicked() {
                        close = true;
                    }
                });
                ui.add_space(8.0);
                egui::ScrollArea::vertical().max_height(420.0).show(ui, |ui| {
                    if m.journal.is_empty() {
                        ui.label(
                            egui::RichText::new("You haven't found any lore tablets yet. Look for ruins.")
                                .color(INK)
                                .italics(),
                        );
                    }
                    for era in ["golden", "expansion", "deep"] {
                        let found: Vec<usize> =
                            m.journal.iter().filter(|(e, _)| e == era).map(|(_, i)| *i).collect();
                        if found.is_empty() {
                            continue;
                        }
                        ui.add_space(6.0);
                        ui.label(
                            egui::RichText::new(era_title(era))
                                .color(Color32::from_rgb(120, 70, 30))
                                .size(15.0)
                                .strong(),
                        );
                        let texts = m.lore.era(era);
                        for i in found {
                            if let Some(t) = texts.get(i) {
                                ui.label(egui::RichText::new(format!("“{t}”")).color(INK).size(14.0));
                                ui.add_space(4.0);
                            }
                        }
                    }
                });
            });
        });
    close
}
