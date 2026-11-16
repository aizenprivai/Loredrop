//! Deterministic hashing and value noise, in `f32` and integer maths only, so
//! the same seed builds the same world on the calculator and on a PC.

/// Mixes integer coordinates and a seed into a well-spread 32-bit value.
#[inline]
pub fn hash3(x: i32, y: i32, z: i32, seed: u32) -> u32 {
    let mut h = seed
        ^ (x as u32).wrapping_mul(0x8DA6_B343)
        ^ (y as u32).wrapping_mul(0xD816_3841)
        ^ (z as u32).wrapping_mul(0xCB1A_B31F);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    h = h.wrapping_mul(0x297A_2D39);
    h ^= h >> 15;
    h
}

/// 2D hash.
#[inline]
pub fn hash2(x: i32, z: i32, seed: u32) -> u32 {
    hash3(x, 0x5bd1, z, seed)
}

/// Hash as a float in `0..1`.
#[inline]
fn unit(h: u32) -> f32 {
    (h >> 8) as f32 * (1.0 / 16_777_216.0)
}

#[inline]
fn fade(t: f32) -> f32 {
    t * t * (3.0 - 2.0 * t)
}

#[inline]
fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// Smooth 2D value noise in `0..1`.
pub fn value2(x: f32, z: f32, seed: u32) -> f32 {
    let (x0, z0) = (crate::math::ifloor(x), crate::math::ifloor(z));
    let (sx, sz) = (fade(x - x0 as f32), fade(z - z0 as f32));
    let a = unit(hash2(x0, z0, seed));
    let b = unit(hash2(x0 + 1, z0, seed));
    let c = unit(hash2(x0, z0 + 1, seed));
    let d = unit(hash2(x0 + 1, z0 + 1, seed));
    lerp(lerp(a, b, sx), lerp(c, d, sx), sz)
}

/// Fractal 2D value noise in roughly `-1..1` (`octaves` layers, each twice
/// as detailed and half as strong as the previous one).
pub fn fbm2(x: f32, z: f32, seed: u32, octaves: u32) -> f32 {
    let (mut sum, mut amp, mut freq, mut norm) = (0.0, 1.0, 1.0, 0.0);
    for o in 0..octaves {
        sum += (value2(x * freq, z * freq, seed.wrapping_add(o * 7919)) * 2.0 - 1.0) * amp;
        norm += amp;
        amp *= 0.5;
        freq *= 2.0;
    }
    sum / norm
}

/// Smooth 3D value noise in `0..1`.
pub fn value3(x: f32, y: f32, z: f32, seed: u32) -> f32 {
    let (x0, y0, z0) = (crate::math::ifloor(x), crate::math::ifloor(y), crate::math::ifloor(z));
    let (sx, sy, sz) = (fade(x - x0 as f32), fade(y - y0 as f32), fade(z - z0 as f32));
    let c = |dx, dy, dz| unit(hash3(x0 + dx, y0 + dy, z0 + dz, seed));
    let bottom = lerp(lerp(c(0, 0, 0), c(1, 0, 0), sx), lerp(c(0, 0, 1), c(1, 0, 1), sx), sz);
    let top = lerp(lerp(c(0, 1, 0), c(1, 1, 0), sx), lerp(c(0, 1, 1), c(1, 1, 1), sx), sz);
    lerp(bottom, top, sy)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic_and_in_range() {
        for i in 0..2000 {
            let (x, z) = (i as f32 * 0.37 - 300.0, i as f32 * 0.11 + 5.0);
            let v = value2(x, z, 42);
            assert!((0.0..=1.0).contains(&v));
            assert_eq!(v, value2(x, z, 42));
            assert!(fbm2(x, z, 1, 4).abs() <= 1.0);
            let w = value3(x, z * 0.5, x * 0.3, 9);
            assert!((0.0..=1.0).contains(&w));
        }
        assert_ne!(value2(0.5, 0.5, 1), value2(0.5, 0.5, 2), "seed matters");
    }

    #[test]
    fn continuous() {
        // Neighbouring samples must be close: noise is smooth, not random.
        for i in 0..500 {
            let x = i as f32 * 0.01;
            assert!((value2(x, 3.3, 7) - value2(x + 0.01, 3.3, 7)).abs() < 0.05);
        }
    }
}
