//! Time of day, daylight, sky colours, the (square) sun and moon, and stars.
//!
//! One day lasts 24 000 ticks (20 minutes), as on PC: tick 0 is sunrise,
//! 6 000 noon, 12 000 sunset and 18 000 midnight.

use crate::color::{hex, mix};
use crate::math::{PI, clamp, cos, ifloor, sin, smoothstep};
use crate::noise::hash3;

/// Ticks in one full day.
pub const DAY_TICKS: u32 = 24_000;

const DAY_ZENITH: u16 = hex(0x78A7FF);
const DAY_HORIZON: u16 = hex(0xC6DBFF);
const NIGHT_ZENITH: u16 = hex(0x03050C);
const NIGHT_HORIZON: u16 = hex(0x111827);
const SUNSET: u16 = hex(0xF0905A);
const SUN: u16 = hex(0xFFF3B0);
const MOON: u16 = hex(0xD8DEE8);

/// Everything the renderer needs to know about the sky for one frame.
pub struct Sky {
    /// Unit vector towards the sun.
    pub sun: [f32; 3],
    /// Brightness of daylit blocks, 0..1.
    pub daylight: f32,
    /// How much it is night, 0..1 (for stars).
    pub night: f32,
    /// Strength of the orange glow around the sun at sunrise/sunset, 0..1.
    sunset: f32,
    /// Fog / horizon colour.
    pub fog: u16,
    /// Sky colour by height of the view direction (index = (dir.y + 1) * 16).
    lut: [u16; 33],
}

impl Sky {
    /// The sky at time `ticks` (any value; only the time of day matters).
    pub fn at(ticks: u32) -> Sky {
        let a = (ticks % DAY_TICKS) as f32 / DAY_TICKS as f32 * 2.0 * PI;
        let sun = [cos(a), sin(a), 0.0];
        let day = smoothstep(-0.2, 0.25, sun[1]);
        let sunset = clamp(1.0 - abs(sun[1]) * 4.0, 0.0, 1.0) * 0.7;
        let t = (day * 256.0) as u32;
        let zenith = mix(NIGHT_ZENITH, DAY_ZENITH, t);
        let horizon = mix(NIGHT_HORIZON, DAY_HORIZON, t);
        let mut lut = [0u16; 33];
        for (i, c) in lut.iter_mut().enumerate() {
            let dy = i as f32 / 16.0 - 1.0;
            *c = if dy <= 0.0 {
                horizon
            } else {
                mix(horizon, zenith, (clamp(dy * 1.8, 0.0, 1.0) * 256.0) as u32)
            };
        }
        Sky { sun, daylight: day, night: 1.0 - day, sunset, fog: horizon, lut }
    }

    /// Colour of the sky seen along unit direction `d`.
    pub fn color(&self, d: [f32; 3]) -> u16 {
        let i = ((d[1] + 1.0) * 16.0) as usize;
        let mut c = self.lut[i.min(32)];
        let s = self.sun;
        let along = d[0] * s[0] + d[1] * s[1];
        // Axes across the sun's face: Z, and the sun's "up".
        let across1 = d[2];
        let across2 = -d[0] * s[1] + d[1] * s[0];
        if along > 0.0 && abs(across1) < 0.09 * along && abs(across2) < 0.09 * along {
            return SUN;
        }
        if along < 0.0 && abs(across1) < -0.07 * along && abs(across2) < -0.07 * along {
            return mix(c, MOON, 230);
        }
        // Sunrise / sunset glow: low in the sky, on the sun's side.
        if self.sunset > 0.0 && along > 0.0 && d[1] < 0.5 {
            let k = self.sunset * along * (1.0 - d[1].max(0.0) * 2.0);
            c = mix(c, SUNSET, (k * 256.0) as u32);
        }
        if self.night > 0.3 && d[1] > 0.0 {
            let h = hash3(ifloor(d[0] * 90.0), ifloor(d[1] * 90.0), ifloor(d[2] * 90.0), 0x57A5);
            if h.is_multiple_of(500) {
                c = mix(c, 0xFFFF, (self.night * 200.0) as u32);
            }
        }
        c
    }
}

#[inline]
fn abs(x: f32) -> f32 {
    crate::math::abs(x)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn day_and_night() {
        let noon = Sky::at(6000);
        let midnight = Sky::at(18000);
        assert!(noon.daylight > 0.99 && midnight.daylight < 0.01);
        assert!(noon.sun[1] > 0.99);
        // Looking straight at the noon sun shows the sun.
        assert_eq!(noon.color([0.0, 1.0, 0.0]), SUN);
        assert_ne!(noon.color([0.0, 0.0, 1.0]), SUN);
    }
}
