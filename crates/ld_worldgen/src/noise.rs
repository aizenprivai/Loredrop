//! Minimal deterministic 2D value noise (Phase 3 placeholder).
//!
//! Phase 4 will choose a proper noise library or implementation (e.g. OpenSimplex /
//! Perlin with SIMD) after benchmarking; this exists only so test terrain has hills.

/// Hashes integer coordinates + seed to a float in 0..1 (SplitMix64-style).
#[inline]
pub fn hash01(x: i32, z: i32, seed: u64) -> f64 {
    let mut h = seed
        ^ (x as i64 as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
        ^ (z as i64 as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F);
    h = (h ^ (h >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    h = (h ^ (h >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    h ^= h >> 31;
    (h >> 11) as f64 / (1u64 << 53) as f64
}

/// Smoothly interpolated value noise in 0..1.
pub fn value2(x: f64, z: f64, seed: u64) -> f64 {
    let (x0, z0) = (x.floor(), z.floor());
    let (fx, fz) = (x - x0, z - z0);
    let (sx, sz) = (fx * fx * (3.0 - 2.0 * fx), fz * fz * (3.0 - 2.0 * fz));
    let (ix, iz) = (x0 as i32, z0 as i32);
    let a = hash01(ix, iz, seed);
    let b = hash01(ix + 1, iz, seed);
    let c = hash01(ix, iz + 1, seed);
    let d = hash01(ix + 1, iz + 1, seed);
    let top = a + (b - a) * sx;
    let bottom = c + (d - c) * sx;
    top + (bottom - top) * sz
}

/// Fractal (multi-octave) value noise in roughly −1..1.
pub fn fbm2(x: f64, z: f64, seed: u64, octaves: u32) -> f64 {
    let (mut sum, mut amp, mut freq, mut norm) = (0.0, 1.0, 1.0, 0.0);
    for o in 0..octaves {
        sum += (value2(x * freq, z * freq, seed.wrapping_add(o as u64 * 7919)) * 2.0 - 1.0) * amp;
        norm += amp;
        amp *= 0.5;
        freq *= 2.0;
    }
    sum / norm
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic_and_in_range() {
        for i in 0..1000 {
            let (x, z) = (i as f64 * 0.37 - 100.0, i as f64 * 0.11 + 5.0);
            let v = value2(x, z, 42);
            assert!((0.0..=1.0).contains(&v));
            assert_eq!(v, value2(x, z, 42));
            assert!(fbm2(x, z, 1, 4).abs() <= 1.0);
        }
        assert_ne!(value2(0.5, 0.5, 1), value2(0.5, 0.5, 2), "seed matters");
    }
}
