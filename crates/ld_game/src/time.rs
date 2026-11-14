//! The day/night cycle: world time, the sun's path, sky colours and how
//! bright daylight is.
//!
//! Timing follows Minecraft exactly: a day is 24 000 ticks (20 minutes at
//! 20 ticks/s). Tick 0 is sunrise (06:00), 6 000 is noon, 12 000 sunset,
//! 18 000 midnight. The sun's angle uses Minecraft's "celestial angle"
//! curve, which lingers slightly around noon and midnight. The colours are
//! our own.
//!
//! Everything here is pure maths so it can be unit-tested and shared by the
//! game (spawning, F3) and the renderer (sky, light levels).

use std::f32::consts::{PI, TAU};

/// Ticks in one full day.
pub const DAY_TICKS: u64 = 24_000;

/// Moon phases (full → waning → new → waxing), one per day like Minecraft.
pub const MOON_PHASES: u64 = 8;

/// The world clock.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct WorldTime {
    /// Ticks since the world began (never wraps in practice).
    pub ticks: u64,
}

/// Everything the renderer needs to draw the sky and light the world.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SkyState {
    /// Unit vector toward the sun (the moon is opposite). Rises in the east (+X).
    pub sun_dir: [f32; 3],
    /// Sky colour straight up and at the horizon (sRGB 0..1). The horizon
    /// colour is also the fog colour.
    pub zenith: [f32; 3],
    pub horizon: [f32; 3],
    /// Sunrise/sunset glow colour (sRGB) and strength 0..1.
    pub glow: [f32; 3],
    pub glow_strength: f32,
    /// How visible the stars are, 0..1.
    pub stars: f32,
    /// Moon phase 0..7 (0 = full, 4 = new).
    pub moon_phase: u32,
    /// Brightness multiplier for sunlit (sky-lit) surfaces: 1 at day,
    /// ≈0.2 at night — moonlight, so the world is dark but not black.
    pub daylight: f32,
}

impl WorldTime {
    /// A clock set to `time_of_day` (0..24 000) on day 0.
    pub fn at(time_of_day: u64) -> Self {
        Self { ticks: time_of_day % DAY_TICKS }
    }

    /// Ticks into the current day (0 = sunrise).
    pub fn time_of_day(self) -> u64 {
        self.ticks % DAY_TICKS
    }

    /// Whole days since the world began.
    pub fn day(self) -> u64 {
        self.ticks / DAY_TICKS
    }

    /// Clock time as (hours, minutes), with tick 0 = 06:00 like Minecraft.
    pub fn clock(self) -> (u32, u32) {
        let minutes = ((self.time_of_day() + 6_000) % DAY_TICKS) * 1440 / DAY_TICKS;
        ((minutes / 60) as u32, (minutes % 60) as u32)
    }

    /// Minecraft's celestial angle: 0 = noon, 0.25 = sunset, 0.5 = midnight,
    /// 0.75 = sunrise. Slightly eased so noon and midnight last longer.
    pub fn celestial_angle(self) -> f32 {
        let f = ((self.time_of_day() as f32 / DAY_TICKS as f32) - 0.25).rem_euclid(1.0);
        let eased = 1.0 - ((f * PI).cos() + 1.0) / 2.0;
        f + (eased - f) / 3.0
    }

    /// 1 in full daylight, 0 in full night, smooth in between.
    pub fn sun_height_factor(self) -> f32 {
        ((self.celestial_angle() * TAU).cos() * 2.0 + 0.5).clamp(0.0, 1.0)
    }

    /// True between dusk and dawn — when hostile creatures may roam the
    /// surface (Minecraft: sky light low enough, roughly 13 000–23 000).
    pub fn is_night(self) -> bool {
        self.sun_height_factor() < 0.25
    }

    /// How many light levels the night takes off sky light (0 at day, 11 at
    /// night — Minecraft's "sky darkening"). Spawning uses sky light minus this.
    pub fn sky_darkening(self) -> u8 {
        ((1.0 - self.sun_height_factor()) * 11.0).round() as u8
    }

    pub fn moon_phase(self) -> u32 {
        (self.day() % MOON_PHASES) as u32
    }

    /// Sky colours, glow, stars and daylight for this moment.
    pub fn sky(self) -> SkyState {
        let a = self.celestial_angle();
        let sun_dir = [-(a * TAU).sin(), (a * TAU).cos(), 0.0];
        let day = self.sun_height_factor();
        // Blend between a deep night blue and the day sky.
        let mix3 = |n: [f32; 3], d: [f32; 3], t: f32| [0, 1, 2].map(|i| n[i] + (d[i] - n[i]) * t);
        let zenith = mix3(NIGHT_ZENITH, DAY_ZENITH, day);
        let horizon = mix3(NIGHT_HORIZON, DAY_HORIZON, day);
        // Glow while the sun is close to the horizon (cos of its angle near 0).
        let c = (a * TAU).cos();
        let glow_strength = if c.abs() < 0.4 {
            let t = 1.0 - c.abs() / 0.4; // 1 exactly at the horizon
            t * t * (3.0 - 2.0 * t)
        } else {
            0.0
        };
        // Warmer and redder as the sun sinks lower.
        let g = (c / 0.4 * 0.5 + 0.5).clamp(0.0, 1.0);
        let glow = [1.0, 0.45 + g * 0.3, 0.25 + g * 0.15];
        // Stars come out only once dusk is well under way.
        let stars = ((0.6 - day) / 0.6).clamp(0.0, 1.0).powi(2) * 0.9;
        let daylight = MOONLIGHT + (1.0 - MOONLIGHT) * day;
        SkyState {
            sun_dir,
            zenith,
            horizon,
            glow,
            glow_strength,
            stars,
            moon_phase: self.moon_phase(),
            daylight,
        }
    }
}

/// Day sky colours (sRGB). The horizon matches the pre-Phase-8 sky colour.
const DAY_ZENITH: [f32; 3] = [0.36, 0.55, 1.0];
const DAY_HORIZON: [f32; 3] = [0.62, 0.76, 1.0];
/// Night sky colours (sRGB): very dark blue, a touch lighter at the horizon.
const NIGHT_ZENITH: [f32; 3] = [0.005, 0.008, 0.03];
const NIGHT_HORIZON: [f32; 3] = [0.03, 0.04, 0.09];
/// Brightness of sky-lit surfaces on a moonlit night.
const MOONLIGHT: f32 = 0.2;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clock_matches_minecraft() {
        assert_eq!(WorldTime::at(0).clock(), (6, 0));
        assert_eq!(WorldTime::at(6_000).clock(), (12, 0));
        assert_eq!(WorldTime::at(18_000).clock(), (0, 0));
        assert_eq!(WorldTime { ticks: 24_000 * 3 + 1_000 }.day(), 3);
    }

    #[test]
    fn celestial_angle_landmarks() {
        assert!(WorldTime::at(6_000).celestial_angle().abs() < 1e-6, "noon");
        assert!((WorldTime::at(18_000).celestial_angle() - 0.5).abs() < 1e-6, "midnight");
        // Minecraft's eased curve: at 12 000 the sun is still a little above
        // the horizon (angle < 0.25); it actually sets around 12 700.
        let t12000 = WorldTime::at(12_000).celestial_angle();
        assert!(t12000 > 0.2 && t12000 < 0.25, "{t12000}");
        let t12700 = WorldTime::at(12_700).celestial_angle();
        assert!((t12700 - 0.25).abs() < 0.01, "{t12700}");
    }

    #[test]
    fn sun_rises_in_the_east_and_is_overhead_at_noon() {
        let noon = WorldTime::at(6_000).sky().sun_dir;
        assert!(noon[1] > 0.999);
        let morning = WorldTime::at(1_000).sky().sun_dir;
        assert!(morning[0] > 0.3 && morning[1] > 0.0, "east and up: {morning:?}");
        let evening = WorldTime::at(11_000).sky().sun_dir;
        assert!(evening[0] < -0.3, "west: {evening:?}");
        let midnight = WorldTime::at(18_000).sky().sun_dir;
        assert!(midnight[1] < -0.999, "under the world");
    }

    #[test]
    fn days_are_bright_and_nights_dark() {
        let noon = WorldTime::at(6_000).sky();
        let night = WorldTime::at(18_000).sky();
        assert_eq!(noon.daylight, 1.0);
        assert!((night.daylight - MOONLIGHT).abs() < 1e-6);
        assert_eq!(noon.stars, 0.0);
        assert!(night.stars > 0.8);
        assert!(!WorldTime::at(6_000).is_night());
        assert!(WorldTime::at(18_000).is_night());
        // Minecraft: monsters can start spawning around 13 000 and stop around 23 000.
        assert!(!WorldTime::at(12_000).is_night());
        assert!(WorldTime::at(14_000).is_night());
        assert!(!WorldTime::at(23_500).is_night());
    }

    #[test]
    fn sky_darkening_matches_minecraft_extremes() {
        assert_eq!(WorldTime::at(6_000).sky_darkening(), 0);
        assert_eq!(WorldTime::at(18_000).sky_darkening(), 11);
    }

    #[test]
    fn glow_only_near_sunrise_and_sunset() {
        assert_eq!(WorldTime::at(6_000).sky().glow_strength, 0.0);
        assert_eq!(WorldTime::at(18_000).sky().glow_strength, 0.0);
        assert!(WorldTime::at(12_500).sky().glow_strength > 0.5);
        assert!(WorldTime::at(23_500).sky().glow_strength > 0.5);
    }

    #[test]
    fn moon_cycles_through_eight_phases() {
        let phases: Vec<u32> = (0..9).map(|d| WorldTime { ticks: d * DAY_TICKS }.moon_phase()).collect();
        assert_eq!(phases, vec![0, 1, 2, 3, 4, 5, 6, 7, 0]);
    }
}
