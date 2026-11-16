//! Loredrop for NumWorks calculators — the calculator side.
//!
//! This file only connects the game (`ld_nw_core`) to the calculator: screen,
//! keyboard, clock and random numbers come from NumWorks' EADK interface
//! (the functions an "external app" may call). The game itself is entirely
//! in `ld_nw_core`, which is also tested on PC.
//!
//! An external app is an ELF file with three special sections (name, API
//! level, icon) and a `main` function; see NumWorks' `eadk/README.md`.

#![no_std]
#![no_main]

use core::panic::PanicInfo;
use ld_nw_core::Platform;
use ld_nw_core::game::Game;

/// App name shown on the calculator's home screen (NUL-terminated).
#[used]
#[unsafe(link_section = ".rodata.eadk_app_name")]
pub static EADK_APP_NAME: [u8; 9] = *b"Loredrop\0";

/// EADK API level this app was written for.
#[used]
#[unsafe(link_section = ".rodata.eadk_api_level")]
pub static EADK_APP_API_LEVEL: u32 = 0;

/// Home-screen icon (55x56, NWI format, made by `tools/gen_icon.py`).
#[used]
#[unsafe(link_section = ".rodata.eadk_app_icon")]
pub static EADK_APP_ICON: [u8; ICON.len()] = *ICON;
const ICON: &[u8; 3519] = include_bytes!("../icon.nwi");

/// Raw EADK functions provided by the calculator (see `eadk.h`).
mod eadk {
    /// Screen rectangle, as `eadk_rect_t`.
    #[repr(C)]
    pub struct Rect {
        pub x: u16,
        pub y: u16,
        pub width: u16,
        pub height: u16,
    }

    /// Screen point, as `eadk_point_t`.
    #[repr(C)]
    pub struct Point {
        pub x: u16,
        pub y: u16,
    }

    unsafe extern "C" {
        pub fn eadk_display_push_rect(rect: Rect, pixels: *const u16);
        pub fn eadk_display_push_rect_uniform(rect: Rect, color: u16);
        pub fn eadk_display_draw_string(
            text: *const u8,
            point: Point,
            large_font: bool,
            text_color: u16,
            background_color: u16,
        );
        pub fn eadk_keyboard_scan() -> u64;
        pub fn eadk_timing_millis() -> u64;
        pub fn eadk_timing_msleep(ms: u32);
        pub fn eadk_random() -> u32;
    }
}

/// The calculator, seen by the game.
struct Calculator;

impl Platform for Calculator {
    fn push_rect(&mut self, x: u16, y: u16, w: u16, h: u16, pixels: &[u16]) {
        debug_assert!(pixels.len() >= w as usize * h as usize);
        unsafe { eadk::eadk_display_push_rect(eadk::Rect { x, y, width: w, height: h }, pixels.as_ptr()) }
    }

    fn fill_rect(&mut self, x: u16, y: u16, w: u16, h: u16, color: u16) {
        unsafe { eadk::eadk_display_push_rect_uniform(eadk::Rect { x, y, width: w, height: h }, color) }
    }

    fn draw_string(&mut self, text: &str, x: u16, y: u16, large: bool, fg: u16, bg: u16) {
        // EADK wants a NUL-terminated string.
        let mut buf = [0u8; 65];
        let n = text.len().min(64);
        buf[..n].copy_from_slice(&text.as_bytes()[..n]);
        unsafe { eadk::eadk_display_draw_string(buf.as_ptr(), eadk::Point { x, y }, large, fg, bg) }
    }

    fn keyboard(&mut self) -> u64 {
        unsafe { eadk::eadk_keyboard_scan() }
    }

    fn millis(&mut self) -> u64 {
        unsafe { eadk::eadk_timing_millis() }
    }

    fn random(&mut self) -> u32 {
        unsafe { eadk::eadk_random() }
    }

    fn sleep(&mut self, ms: u32) {
        unsafe { eadk::eadk_timing_msleep(ms) }
    }
}

/// The whole game state lives here, in RAM reserved at install time, never
/// on the (small) stack.
static mut GAME: Game = Game::new();

/// Entry point, called by the calculator when the app is opened. Returning
/// goes back to the home screen.
#[unsafe(no_mangle)]
pub extern "C" fn main() {
    let mut calc = Calculator;
    // SAFETY: the calculator runs one thread; `main` is the only user of GAME.
    // GAME is freshly initialised at each launch by the startup code (which
    // copies initial values from flash), so there is nothing to reset here.
    // Building a new `Game` value here could overflow the stack.
    let game = unsafe { &mut *core::ptr::addr_of_mut!(GAME) };
    // Wait for the OK/EXE key used to open the app to be released, so it
    // doesn't immediately select the first menu entry.
    while calc.keyboard() != 0 {
        calc.sleep(10);
    }
    while game.frame(&mut calc) {}
}

/// On a bug, show a message instead of freezing on a garbled screen.
#[panic_handler]
fn panic(_info: &PanicInfo<'_>) -> ! {
    let mut calc = Calculator;
    calc.fill_rect(0, 0, 320, 240, 0x0000);
    calc.draw_string("Loredrop crashed (bug).", 10, 100, true, 0xFFFF, 0x0000);
    calc.draw_string("Press Reset on the back to restart.", 10, 130, false, 0xFFFF, 0x0000);
    loop {
        calc.sleep(1000);
    }
}
