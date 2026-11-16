//! RGB565 colour helpers (the calculator screen's 16-bit colour format).

/// Builds an RGB565 colour from 8-bit red, green and blue.
#[inline]
pub const fn rgb(r: u8, g: u8, b: u8) -> u16 {
    ((r as u16 >> 3) << 11) | ((g as u16 >> 2) << 5) | (b as u16 >> 3)
}

/// Builds an RGB565 colour from a `0xRRGGBB` value.
#[inline]
pub const fn hex(c: u32) -> u16 {
    rgb((c >> 16) as u8, (c >> 8) as u8, c as u8)
}

/// Multiplies a colour's brightness by `f / 256` (`f` = 256 leaves it
/// unchanged; larger values brighten and saturate).
#[inline]
pub fn scale(c: u16, f: u32) -> u16 {
    let r = (((c >> 11) as u32 * f) >> 8).min(31);
    let g = ((((c >> 5) & 63) as u32 * f) >> 8).min(63);
    let b = (((c & 31) as u32 * f) >> 8).min(31);
    ((r << 11) | (g << 5) | b) as u16
}

/// Mixes two colours: `t = 0` gives `a`, `t = 256` gives `b`.
#[inline]
pub fn mix(a: u16, b: u16, t: u32) -> u16 {
    let t = t.min(256);
    let u = 256 - t;
    let r = ((a >> 11) as u32 * u + (b >> 11) as u32 * t) >> 8;
    let g = (((a >> 5) & 63) as u32 * u + ((b >> 5) & 63) as u32 * t) >> 8;
    let bl = ((a & 31) as u32 * u + (b & 31) as u32 * t) >> 8;
    ((r << 11) | (g << 5) | bl) as u16
}

/// Black.
pub const BLACK: u16 = 0x0000;
/// White.
pub const WHITE: u16 = 0xFFFF;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn helpers() {
        assert_eq!(rgb(255, 255, 255), WHITE);
        assert_eq!(hex(0xFF0000), 0xF800);
        assert_eq!(scale(WHITE, 256), WHITE);
        assert_eq!(scale(WHITE, 0), BLACK);
        assert_eq!(mix(BLACK, WHITE, 0), BLACK);
        assert_eq!(mix(BLACK, WHITE, 256), WHITE);
    }
}
