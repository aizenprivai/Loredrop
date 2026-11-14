//! The in-game HUD: hotbar with item icons, counts and durability bars.
//!
//! Drawn with egui's painter in a pixel-art style. Sizes follow the GUI scale.

use egui::{Align2, Color32, FontId, Pos2, Rect, Stroke, TextureHandle, Vec2};
use ld_game::{Inventory, ItemKind, ItemRegistry, ItemStack};

/// Side length of one slot, in egui points (before GUI scale).
pub const SLOT: f32 = 40.0;

/// Draws one inventory slot (background, item, count, durability).
pub fn draw_slot(
    painter: &egui::Painter,
    rect: Rect,
    stack: Option<ItemStack>,
    icons: &[TextureHandle],
    items: &ItemRegistry,
    highlight: bool,
) {
    painter.rect_filled(rect, 0.0, Color32::from_rgba_unmultiplied(20, 20, 24, 170));
    painter.rect_stroke(rect, 0.0, Stroke::new(2.0, Color32::from_rgb(70, 70, 78)), egui::StrokeKind::Inside);
    if highlight {
        painter.rect_stroke(
            rect.expand(2.0),
            0.0,
            Stroke::new(3.0, Color32::WHITE),
            egui::StrokeKind::Inside,
        );
    }
    let Some(s) = stack else { return };
    let icon_rect = rect.shrink(rect.width() * 0.12);
    if let Some(tex) = icons.get(s.item.0 as usize) {
        painter.image(
            tex.id(),
            icon_rect,
            Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
            Color32::WHITE,
        );
    }
    if s.count > 1 {
        let pos = rect.right_bottom() - Vec2::new(3.0, 1.0);
        let font = FontId::monospace(rect.height() * 0.32);
        painter.text(
            pos + Vec2::new(1.5, 1.5),
            Align2::RIGHT_BOTTOM,
            s.count.to_string(),
            font.clone(),
            Color32::from_black_alpha(220),
        );
        painter.text(pos, Align2::RIGHT_BOTTOM, s.count.to_string(), font, Color32::WHITE);
    }
    if let ItemKind::Tool(t) = &items.get(s.item).kind
        && s.damage > 0
    {
        let left = 1.0 - s.damage as f32 / t.durability as f32;
        let bar = Rect::from_min_size(
            rect.left_bottom() + Vec2::new(5.0, -6.0),
            Vec2::new(rect.width() - 10.0, 3.0),
        );
        painter.rect_filled(bar, 0.0, Color32::BLACK);
        // Green → yellow → red as the tool wears out (like Minecraft).
        let color = Color32::from_rgb(
            ((1.0 - left) * 2.0).min(1.0).mul_add(255.0, 0.0) as u8,
            (left * 2.0).min(1.0).mul_add(255.0, 0.0) as u8,
            0,
        );
        painter.rect_filled(Rect::from_min_size(bar.min, Vec2::new(bar.width() * left, 3.0)), 0.0, color);
    }
}

/// The hotbar along the bottom of the screen, plus the held item's name.
pub fn draw_hotbar(ctx: &egui::Context, inv: &Inventory, icons: &[TextureHandle], items: &ItemRegistry) {
    let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::unique("hotbar")));
    let screen = ctx.content_rect();
    let total = SLOT * 9.0;
    let origin = Pos2::new(screen.center().x - total / 2.0, screen.bottom() - SLOT - 8.0);
    for i in 0..9 {
        let rect = Rect::from_min_size(origin + Vec2::new(i as f32 * SLOT, 0.0), Vec2::splat(SLOT - 2.0));
        draw_slot(&painter, rect, inv.slots[i], icons, items, i == inv.selected);
    }
    if let Some(s) = inv.held() {
        let name = &items.get(s.item).display;
        let pos = Pos2::new(screen.center().x, origin.y - 10.0);
        let font = FontId::proportional(15.0);
        painter.text(
            pos + Vec2::new(1.0, 1.0),
            Align2::CENTER_BOTTOM,
            name,
            font.clone(),
            Color32::from_black_alpha(200),
        );
        painter.text(pos, Align2::CENTER_BOTTOM, name, font, Color32::WHITE);
    }
}
