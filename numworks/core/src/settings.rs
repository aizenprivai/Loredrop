//! Player-adjustable options. Each one has a name and a description shown in
//! the pause menu's Settings screen.
//!
//! The calculator gives apps no storage, so settings go back to their
//! defaults each time the game starts.

use crate::text::Text;
use core::fmt::Write;

/// All options.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Settings {
    /// How far you can see, in blocks. Lower is faster.
    pub view_distance: u8,
    /// Size of one rendered pixel on screen: 2 is sharp, 4 is blocky but
    /// about four times faster.
    pub pixel_size: u8,
    /// Vertical field of view in degrees (the PC game's default is 70).
    pub fov: u8,
    /// How fast the arrow keys turn the view, in degrees per second.
    pub turn_speed: u16,
    /// Jump automatically when walking into a one-block step.
    pub auto_jump: bool,
    /// Show frames per second in the status line.
    pub show_fps: bool,
    /// Darkness of caves and night, from 0 (pitch black) to 4 (bright).
    pub brightness: u8,
}

impl Settings {
    /// Default values.
    pub const DEFAULT: Settings = Settings {
        view_distance: 12,
        pixel_size: 2,
        fov: 70,
        turn_speed: 150,
        auto_jump: true,
        show_fps: true,
        brightness: 2,
    };

    /// Number of entries in the settings menu.
    pub const COUNT: usize = 7;

    /// Name of entry `i`.
    pub fn name(i: usize) -> &'static str {
        ["View distance", "Pixel size", "Field of view", "Turn speed", "Auto-jump", "Show FPS", "Brightness"]
            [i]
    }

    /// Description of entry `i` (one short line).
    pub fn description(i: usize) -> &'static str {
        [
            "How far you see. Lower = faster.",
            "2 = sharp, 4 = blocky but faster.",
            "Width of the view, in degrees.",
            "Arrow-key turning speed (deg/s).",
            "Jump up one-block steps by itself.",
            "Show frames per second.",
            "How bright caves and nights are.",
        ][i]
    }

    /// Current value of entry `i` as text.
    pub fn value(&self, i: usize) -> Text<16> {
        let mut t = Text::new();
        let _ = match i {
            0 => write!(t, "{} blocks", self.view_distance),
            1 => write!(t, "{}", self.pixel_size),
            2 => write!(t, "{} deg", self.fov),
            3 => write!(t, "{}", self.turn_speed),
            4 => write!(t, "{}", if self.auto_jump { "On" } else { "Off" }),
            5 => write!(t, "{}", if self.show_fps { "On" } else { "Off" }),
            _ => write!(t, "{}", self.brightness),
        };
        t
    }

    /// Changes entry `i` one step up (`dir > 0`) or down, within its range.
    pub fn change(&mut self, i: usize, dir: i32) {
        let step = |v: i32, s: i32, lo: i32, hi: i32| (v + s * dir).clamp(lo, hi);
        match i {
            0 => self.view_distance = step(self.view_distance as i32, 2, 6, 14) as u8,
            1 => self.pixel_size = if dir > 0 { 4 } else { 2 },
            2 => self.fov = step(self.fov as i32, 5, 50, 100) as u8,
            3 => self.turn_speed = step(self.turn_speed as i32, 30, 60, 360) as u16,
            4 => self.auto_jump = !self.auto_jump,
            5 => self.show_fps = !self.show_fps,
            _ => self.brightness = step(self.brightness as i32, 1, 0, 4) as u8,
        }
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self::DEFAULT
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranges_hold() {
        let mut s = Settings::DEFAULT;
        for _ in 0..20 {
            for i in 0..Settings::COUNT {
                s.change(i, 1);
            }
        }
        assert_eq!(s.view_distance, 14);
        assert_eq!(s.pixel_size, 4);
        for _ in 0..20 {
            s.change(0, -1);
        }
        assert_eq!(s.view_distance, 6);
        for i in 0..Settings::COUNT {
            assert!(!Settings::name(i).is_empty() && !Settings::description(i).is_empty());
            assert!(!s.value(i).as_str().is_empty());
        }
    }
}
