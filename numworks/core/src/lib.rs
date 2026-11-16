//! # Loredrop — NumWorks calculator edition (core)
//!
//! The whole game, written so it runs on a NumWorks graphing calculator
//! (ARM Cortex-M, a few hundred KB of RAM, 320x240 screen, no GPU) and, for
//! testing, on a PC.
//!
//! * `#![no_std]` and no heap: every buffer has a fixed size known at compile
//!   time, so memory use can never grow during play.
//! * The game never talks to hardware directly. It goes through the
//!   [`Platform`] trait, implemented by the calculator app (`numworks/app`)
//!   and by the PC test harness (`numworks/desktop`).
//!
//! Module map:
//! * [`blocks`] / `gen_blocks` — block types and textures (generated from the
//!   PC game's data files by `numworks/tools/gen_blocks.py`).
//! * [`noise`] — deterministic hashing and value noise.
//! * [`worldgen`] — terrain, biomes, caves, ores and trees for one column.
//! * [`world`] — the loaded area around the player and the list of edits.
//! * [`player`] — movement and collisions (Minecraft numbers, 20 ticks/s).
//! * [`sky`] — time of day, sky colours, sun and moon.
//! * [`render`] — the 3D view: one ray per pixel through the voxel grid.
//! * [`hud`] — hotbar, status line, menus and text.
//! * [`game`] — ties everything together; one call per frame.
//! * [`settings`] — every player-adjustable option, with descriptions.

#![no_std]

pub mod blocks;
pub mod color;
pub mod entities;
pub mod furnace;
pub mod game;
#[rustfmt::skip]
#[allow(clippy::all)]
pub mod gen_blocks;
#[rustfmt::skip]
#[allow(clippy::all)]
pub mod gen_items;
#[rustfmt::skip]
#[allow(clippy::all)]
pub mod gen_mobs;
pub mod hud;
pub mod input;
pub mod inventory;
pub mod items;
pub mod math;
pub mod mobs;
pub mod models;
pub mod noise;
pub mod player;
pub mod probe;
pub mod ramcache;
pub mod render;
pub mod settings;
pub mod sky;
pub mod structures;
pub mod survival;
pub mod text;
pub mod ui;
pub mod world;
pub mod worldgen;

/// Screen width in pixels (the same on every NumWorks model).
pub const SCREEN_W: usize = 320;
/// Screen height in pixels.
pub const SCREEN_H: usize = 240;

/// Everything the game needs from the machine it runs on.
///
/// Colours are RGB565 (5 bits red, 6 green, 5 blue), the calculator's
/// native screen format. Coordinates are in screen pixels, origin top-left.
pub trait Platform {
    /// Copies `pixels` (row by row, `w * h` values) to a screen rectangle.
    fn push_rect(&mut self, x: u16, y: u16, w: u16, h: u16, pixels: &[u16]);
    /// Fills a screen rectangle with one colour.
    fn fill_rect(&mut self, x: u16, y: u16, w: u16, h: u16, color: u16);
    /// Draws text with the platform font. `large` selects the bigger font
    /// (about 10x18 pixels per character; the small one is about 7x14).
    fn draw_string(&mut self, text: &str, x: u16, y: u16, large: bool, fg: u16, bg: u16);
    /// Which keys are held right now, one bit per key (see [`input::Key`]).
    fn keyboard(&mut self) -> u64;
    /// Milliseconds since some fixed moment (only differences matter).
    fn millis(&mut self) -> u64;
    /// A random 32-bit number (used for the world seed and random events).
    fn random(&mut self) -> u32;
    /// Waits `ms` milliseconds (used by menus to save battery).
    fn sleep(&mut self, ms: u32);
}
