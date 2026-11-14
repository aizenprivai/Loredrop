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

/// HUD icons (hearts, hunger, armour, air), loaded once as egui textures.
pub struct StatIcons {
    pub heart: [TextureHandle; 3],
    pub food: [TextureHandle; 3],
    pub armor: [TextureHandle; 3],
    pub bubble: TextureHandle,
}

impl StatIcons {
    /// Loads `textures/gui/*.png`; missing files become blank icons.
    pub fn load(ctx: &egui::Context, assets: &std::path::Path) -> Self {
        let load = |name: &str| {
            let path = assets.join("textures").join("gui").join(format!("{name}.png"));
            let img =
                image::open(&path).map(|i| i.to_rgba8()).unwrap_or_else(|_| image::RgbaImage::new(9, 9));
            let ci = egui::ColorImage::from_rgba_unmultiplied(
                [img.width() as usize, img.height() as usize],
                img.as_raw(),
            );
            ctx.load_texture(name, ci, egui::TextureOptions::NEAREST)
        };
        let set = |base: &str| {
            [load(&format!("{base}_full")), load(&format!("{base}_half")), load(&format!("{base}_empty"))]
        };
        Self { heart: set("heart"), food: set("food"), armor: set("armor"), bubble: load("air_bubble") }
    }
}

/// What the survival bars show this frame.
pub struct Bars {
    pub health: f32,
    pub food: i32,
    pub armor: i32,
    /// Breath 0–300, or `None` when not under water.
    pub air: Option<i32>,
    /// 0 right after being hurt (for the red flash).
    pub hurt_flash: u32,
    /// Eating progress 0..1 (0 = not eating).
    pub eating: f32,
}

/// Hearts (left) and hunger (right) above the hotbar, armour above the
/// hearts, air bubbles above the hunger bar — the Minecraft layout.
pub fn draw_bars(ctx: &egui::Context, icons: &StatIcons, bars: &Bars) {
    let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::unique("bars")));
    let screen = ctx.content_rect();
    let size = 18.0;
    let total = SLOT * 9.0;
    let left = screen.center().x - total / 2.0;
    let right = screen.center().x + total / 2.0;
    let row1 = screen.bottom() - SLOT - 8.0 - size - 6.0;
    let row2 = row1 - size - 2.0;
    let uv = Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
    // Value 0..20 drawn as 10 icons of 2 points each.
    let row = |value: f32, set: &[TextureHandle; 3], y: f32, from_right: bool| {
        for i in 0..10 {
            let v = value - i as f32 * 2.0;
            let tex = if v >= 2.0 {
                &set[0]
            } else if v >= 1.0 {
                &set[1]
            } else {
                &set[2]
            };
            let x = if from_right { right - (i + 1) as f32 * size } else { left + i as f32 * size };
            painter.image(
                tex.id(),
                Rect::from_min_size(Pos2::new(x, y), Vec2::splat(size)),
                uv,
                Color32::WHITE,
            );
        }
    };
    // Hearts blink while hurt.
    if bars.hurt_flash > 6 || bars.hurt_flash % 4 < 2 {
        row(bars.health.ceil(), &icons.heart, row1, false);
    }
    row(bars.food as f32, &icons.food, row1, true);
    if bars.armor > 0 {
        row(bars.armor as f32, &icons.armor, row2, false);
    }
    if let Some(air) = bars.air {
        // 10 bubbles; each pops as 30 ticks of breath are used.
        let n = ((air.max(0) as f32 / 300.0) * 10.0).ceil() as i32;
        for i in 0..n {
            let x = right - (i + 1) as f32 * size;
            painter.image(
                icons.bubble.id(),
                Rect::from_min_size(Pos2::new(x, row2), Vec2::splat(size)),
                uv,
                Color32::WHITE,
            );
        }
    }
    // Eating progress under the crosshair.
    if bars.eating > 0.0 {
        let c = screen.center() + Vec2::new(0.0, 22.0);
        let r = Rect::from_center_size(c, Vec2::new(40.0, 4.0));
        painter.rect_filled(r, 0.0, Color32::from_black_alpha(160));
        painter.rect_filled(
            Rect::from_min_size(r.min, Vec2::new(r.width() * bars.eating, 4.0)),
            0.0,
            Color32::from_rgb(230, 180, 90),
        );
    }
    // Red flash over the screen just after taking damage.
    if bars.hurt_flash < 8 {
        let a = (1.0 - bars.hurt_flash as f32 / 8.0) * 70.0;
        painter.rect_filled(screen, 0.0, Color32::from_rgba_unmultiplied(160, 0, 0, a as u8));
    }
}
