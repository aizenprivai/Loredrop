//! Debug & performance overlay (F3) and the crosshair, drawn with egui.

use crate::frame_stats::StatsSummary;
use crate::game::{Game, GameMode};
use crate::world::WorldStats;
use egui::{Align2, Color32, FontId, Pos2, Stroke};
use ld_render::{GpuInfo, RenderStats};

/// Everything the overlay displays, gathered by the app each frame.
pub struct OverlayData<'a> {
    pub stats: StatsSummary,
    pub game: &'a Game,
    pub gpu: &'a GpuInfo,
    pub render: RenderStats,
    pub resolution: (u32, u32),
    pub vsync: bool,
    pub fps_limit: u32,
    pub mouse_captured: bool,
    pub world: WorldStats,
    pub render_distance: u32,
}

/// Compass name for a yaw angle (Minecraft convention).
fn facing(yaw: f32) -> &'static str {
    match ((yaw.rem_euclid(360.0) + 45.0) / 90.0) as u32 % 4 {
        0 => "south (towards +Z)",
        1 => "west (towards -X)",
        2 => "north (towards -Z)",
        _ => "east (towards +X)",
    }
}

pub fn draw(ctx: &egui::Context, show_debug: bool, d: &OverlayData) {
    let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::unique("hud")));
    let screen = ctx.content_rect();

    // Crosshair: small white plus with a dark outline so it's visible on any background.
    let c = screen.center();
    for (w, col) in [(4.0, Color32::from_black_alpha(160)), (2.0, Color32::WHITE)] {
        painter.line_segment([c - egui::vec2(9.0, 0.0), c + egui::vec2(9.0, 0.0)], Stroke::new(w, col));
        painter.line_segment([c - egui::vec2(0.0, 9.0), c + egui::vec2(0.0, 9.0)], Stroke::new(w, col));
    }

    if !d.mouse_captured {
        text_block(
            &painter,
            Pos2::new(c.x, screen.bottom() - 40.0),
            Align2::CENTER_BOTTOM,
            &["Click to capture the mouse  ·  Esc to release  ·  F3 debug overlay".into()],
        );
    }
    if !show_debug {
        return;
    }

    let p = &d.game.player;
    let pos = p.pos;
    let (bx, by, bz) = (pos.x.floor() as i32, pos.y.floor() as i32, pos.z.floor() as i32);
    let speed = glam::DVec2::new(p.vel.x, p.vel.z).length() * 20.0;
    let yes = |b: bool| if b { "yes" } else { "no" };
    let mode = match d.game.mode {
        GameMode::Creative => "Creative",
    };
    let left = vec![
        format!("Loredrop {} (Phase 4 · world generation)", env!("CARGO_PKG_VERSION")),
        format!("{} fps  (avg {:.2} ms, worst {:.2} ms)", d.stats.fps, d.stats.avg_ms, d.stats.worst_ms),
        format!("{} ticks/s (target 20)   tick {}", d.stats.tps, d.game.tick),
        String::new(),
        format!("XYZ: {:.3} / {:.5} / {:.3}", pos.x, pos.y, pos.z),
        format!(
            "Block: {bx} {by} {bz}   Chunk: {} {} {}",
            bx.div_euclid(16),
            by.div_euclid(16),
            bz.div_euclid(16)
        ),
        format!("Facing: {}  ({:.1} / {:.1})", facing(d.game.yaw), d.game.yaw, d.game.pitch),
        format!("Speed: {speed:.2} blocks/s   Vertical: {:.3}", p.vel.y),
        format!("Mode: {mode}   Ground: {}   Flying: {}", yes(p.on_ground), yes(p.flying)),
        format!("Sprinting: {}   Sneaking: {}", yes(p.sprinting), yes(p.sneaking)),
        format!("Standing on: {}", standing_on(d.game)),
        format!("Biome: {}", biome_here(d.game)),
        format!("Seed: {}", d.game.world.seed as i64),
        String::new(),
        format!(
            "Chunks: {}/{} ready (render distance {})   loaded {}",
            d.world.columns_ready, d.world.columns_wanted, d.render_distance, d.world.columns_loaded
        ),
        format!(
            "Jobs: {} generating, {} meshing, {} waiting to upload ({} threads)",
            d.world.generating, d.world.meshing, d.world.upload_queue, d.world.threads
        ),
        format!(
            "Sections: {} drawn, {} culled, {} on GPU",
            d.render.chunks.sections_drawn, d.render.chunks.sections_culled, d.render.chunks.sections_loaded
        ),
        format!(
            "Memory: world {:.1} MB, meshes {:.1} MB",
            d.world.world_bytes as f64 / 1e6,
            d.render.chunks.mesh_bytes as f64 / 1e6
        ),
    ];
    let right = vec![
        format!("Graphics API: {}", d.gpu.backend),
        d.gpu.adapter.clone(),
        format!("{} · {}", d.gpu.device_type, d.gpu.driver),
        format!("Display: {}x{}", d.resolution.0, d.resolution.1),
        format!(
            "VSync: {}   FPS limit: {}",
            if d.vsync { "on" } else { "off" },
            if d.fps_limit == 0 { "unlimited".into() } else { d.fps_limit.to_string() }
        ),
        format!("Draw calls: {}   Triangles: {}", d.render.draw_calls, d.render.triangles),
    ];
    text_block(&painter, screen.left_top() + egui::vec2(4.0, 4.0), Align2::LEFT_TOP, &left);
    text_block(&painter, screen.right_top() + egui::vec2(-4.0, 4.0), Align2::RIGHT_TOP, &right);
}

/// Display name of the biome at the player's position.
fn biome_here(game: &Game) -> String {
    let p = game.player.pos;
    let b = ld_core::coords::BlockPos::new(p.x.floor() as i32, 0, p.z.floor() as i32);
    let (lx, _, lz) = b.local();
    match game.world.map.get(b.chunk()) {
        Some(col) => game.world.biomes.get(col.biome(lx as usize, lz as usize)).display.clone(),
        None => "(not loaded)".into(),
    }
}

/// Display name of the block under the player's feet.
fn standing_on(game: &Game) -> String {
    let p = game.player.pos;
    let below =
        ld_core::coords::BlockPos::new(p.x.floor() as i32, (p.y - 0.01).floor() as i32, p.z.floor() as i32);
    match game.world.map.block(below) {
        Some(id) => game.world.registry.get(id).display.clone(),
        None => "(not loaded)".into(),
    }
}

/// Draws lines of text with a translucent background behind each line,
/// in the style of Minecraft's F3 screen.
fn text_block(painter: &egui::Painter, origin: Pos2, align: Align2, lines: &[String]) {
    let font = FontId::monospace(13.0);
    let line_h = 16.0;
    let bottom_up = align == Align2::CENTER_BOTTOM;
    for (i, line) in lines.iter().enumerate() {
        if line.is_empty() {
            continue;
        }
        let dy = if bottom_up { -(i as f32) * line_h } else { i as f32 * line_h };
        let pos = origin + egui::vec2(0.0, dy);
        let galley = painter.layout_no_wrap(line.clone(), font.clone(), Color32::from_gray(224));
        let rect = align.anchor_size(pos, galley.size()).expand(1.5);
        painter.rect_filled(rect, 0.0, Color32::from_black_alpha(110));
        painter.galley(rect.min + egui::vec2(1.5, 1.5), galley, Color32::from_gray(224));
    }
}
