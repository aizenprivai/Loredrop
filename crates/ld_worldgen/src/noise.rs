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

// ===========================================================================
// Gradient noise used by the real generator (Phase 4)
// ===========================================================================

/// Small, fast, seedable random number generator (xoroshiro128++).
/// Deterministic on every platform, so the same seed builds the same world.
#[derive(Clone)]
pub struct Rng {
    s0: u64,
    s1: u64,
}

impl Rng {
    pub fn new(seed: u64) -> Self {
        // SplitMix64 spreads a single seed into two well-mixed state words.
        let mut z = seed;
        let mut next = || {
            z = z.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let mut x = z;
            x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            x ^ (x >> 31)
        };
        let (s0, s1) = (next(), next());
        Self { s0, s1: if s0 == 0 && s1 == 0 { 1 } else { s1 } }
    }

    /// An RNG for one purpose at one place, e.g. "trees in chunk (3, -2)".
    pub fn for_position(seed: u64, x: i32, z: i32, salt: u64) -> Self {
        Self::new(
            seed ^ (x as i64 as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
                ^ (z as i64 as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F)
                ^ salt.wrapping_mul(0x1656_67B1_9E37_79F9),
        )
    }

    pub fn next_u64(&mut self) -> u64 {
        let (s0, mut s1) = (self.s0, self.s1);
        let result = s0.wrapping_add(s1).rotate_left(17).wrapping_add(s0);
        s1 ^= s0;
        self.s0 = s0.rotate_left(49) ^ s1 ^ (s1 << 21);
        self.s1 = s1.rotate_left(28);
        result
    }

    /// Uniform float in 0..1.
    pub fn next_f64(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }

    /// Uniform integer in `0..n` (n > 0).
    pub fn below(&mut self, n: u32) -> u32 {
        (((self.next_u64() >> 32) * n as u64) >> 32) as u32
    }

    /// Uniform integer in `lo..=hi`.
    pub fn range(&mut self, lo: i32, hi: i32) -> i32 {
        lo + self.below((hi - lo + 1).max(1) as u32) as i32
    }
}

/// Classic "improved" gradient noise (Ken Perlin, 2002), with a seeded
/// permutation table and a random offset so different noises don't line up.
#[derive(Clone)]
pub struct Perlin {
    perm: [u8; 512],
    offset: [f64; 3],
}

#[inline]
fn fade(t: f64) -> f64 {
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}

#[inline]
fn grad(hash: u8, x: f64, y: f64, z: f64) -> f64 {
    // 12 edge directions of a cube (plus 4 repeats), as in Perlin's reference.
    match hash & 15 {
        0 => x + y,
        1 => -x + y,
        2 => x - y,
        3 => -x - y,
        4 => x + z,
        5 => -x + z,
        6 => x - z,
        7 => -x - z,
        8 => y + z,
        9 => -y + z,
        10 => y - z,
        11 => -y - z,
        12 => x + y,
        13 => -y + z,
        14 => -x + y,
        _ => -y - z,
    }
}

#[inline]
fn lerp(t: f64, a: f64, b: f64) -> f64 {
    a + t * (b - a)
}

impl Perlin {
    pub fn new(rng: &mut Rng) -> Self {
        let offset = [rng.next_f64() * 256.0, rng.next_f64() * 256.0, rng.next_f64() * 256.0];
        let mut p: [u8; 256] = std::array::from_fn(|i| i as u8);
        for i in (1..256).rev() {
            let j = rng.below(i as u32 + 1) as usize;
            p.swap(i, j);
        }
        let mut perm = [0u8; 512];
        for i in 0..512 {
            perm[i] = p[i & 255];
        }
        Self { perm, offset }
    }

    /// Noise value, roughly in −1..1.
    pub fn sample(&self, x: f64, y: f64, z: f64) -> f64 {
        let (x, y, z) = (x + self.offset[0], y + self.offset[1], z + self.offset[2]);
        let (fx, fy, fz) = (x.floor(), y.floor(), z.floor());
        let (xi, yi, zi) =
            ((fx as i64 & 255) as usize, (fy as i64 & 255) as usize, (fz as i64 & 255) as usize);
        let (x, y, z) = (x - fx, y - fy, z - fz);
        let (u, v, w) = (fade(x), fade(y), fade(z));
        let p = &self.perm;
        let a = p[xi] as usize + yi;
        let (aa, ab) = (p[a] as usize + zi, p[a + 1] as usize + zi);
        let b = p[xi + 1] as usize + yi;
        let (ba, bb) = (p[b] as usize + zi, p[b + 1] as usize + zi);
        lerp(
            w,
            lerp(
                v,
                lerp(u, grad(p[aa], x, y, z), grad(p[ba], x - 1.0, y, z)),
                lerp(u, grad(p[ab], x, y - 1.0, z), grad(p[bb], x - 1.0, y - 1.0, z)),
            ),
            lerp(
                v,
                lerp(u, grad(p[aa + 1], x, y, z - 1.0), grad(p[ba + 1], x - 1.0, y, z - 1.0)),
                lerp(u, grad(p[ab + 1], x, y - 1.0, z - 1.0), grad(p[bb + 1], x - 1.0, y - 1.0, z - 1.0)),
            ),
        )
    }
}

/// Several octaves of [`Perlin`] noise: each octave doubles the frequency and
/// halves the amplitude, adding finer detail. Output is normalised so its
/// typical spread is similar regardless of octave count.
#[derive(Clone)]
pub struct OctaveNoise {
    octaves: Vec<Perlin>,
    /// Frequency of the first (largest) octave, in cycles per block.
    base_freq: f64,
    norm: f64,
}

impl OctaveNoise {
    /// `wavelength`: size in blocks of the largest features.
    pub fn new(rng: &mut Rng, octaves: usize, wavelength: f64) -> Self {
        let octaves: Vec<Perlin> = (0..octaves).map(|_| Perlin::new(rng)).collect();
        let norm = (0..octaves.len()).map(|i| 0.5f64.powi(i as i32)).sum::<f64>();
        Self { octaves, base_freq: 1.0 / wavelength, norm }
    }

    pub fn sample(&self, x: f64, y: f64, z: f64) -> f64 {
        let mut sum = 0.0;
        let (mut f, mut a) = (self.base_freq, 1.0);
        for o in &self.octaves {
            sum += o.sample(x * f, y * f, z * f) * a;
            f *= 2.0;
            a *= 0.5;
        }
        sum / self.norm
    }

    pub fn sample2(&self, x: f64, z: f64) -> f64 {
        self.sample(x, 0.0, z)
    }
}

/// Two octave noises sampled at slightly different scales and added, then
/// stretched so values spread across roughly −1..1. Combining two offset
/// noises hides the grid-aligned patterns single Perlin noise can show — the
/// same idea modern Minecraft uses for its climate noises.
#[derive(Clone)]
pub struct DoubleNoise {
    a: OctaveNoise,
    b: OctaveNoise,
    gain: f64,
}

impl DoubleNoise {
    pub fn new(rng: &mut Rng, octaves: usize, wavelength: f64, gain: f64) -> Self {
        Self {
            a: OctaveNoise::new(rng, octaves, wavelength),
            b: OctaveNoise::new(rng, octaves, wavelength),
            gain,
        }
    }

    pub fn sample(&self, x: f64, y: f64, z: f64) -> f64 {
        const K: f64 = 1.018_1;
        (self.a.sample(x, y, z) + self.b.sample(x * K, y * K, z * K)) * self.gain
    }

    pub fn sample2(&self, x: f64, z: f64) -> f64 {
        self.sample(x, 0.0, z)
    }

    pub fn set_gain(&mut self, gain: f64) {
        self.gain = gain;
    }
}

#[cfg(test)]
mod gradient_tests {
    use super::*;

    #[test]
    fn rng_is_deterministic_and_uniform() {
        let (mut a, mut b) = (Rng::new(5), Rng::new(5));
        let mut buckets = [0u32; 10];
        for _ in 0..10_000 {
            let v = a.next_f64();
            assert_eq!(v, b.next_f64());
            buckets[(v * 10.0) as usize] += 1;
        }
        assert!(buckets.iter().all(|&n| (800..1200).contains(&n)), "{buckets:?}");
        assert!((0..1000).all(|_| (3..=7).contains(&a.range(3, 7))));
    }

    #[test]
    fn perlin_is_smooth_bounded_and_zero_at_lattice() {
        let p = Perlin::new(&mut Rng::new(1));
        let mut max: f64 = 0.0;
        for i in 0..20_000 {
            let (x, y, z) = (i as f64 * 0.137, i as f64 * 0.071, i as f64 * 0.0123);
            let v = p.sample(x, y, z);
            max = max.max(v.abs());
            // Smooth: tiny step → tiny change.
            assert!((p.sample(x + 1e-4, y, z) - v).abs() < 1e-2);
        }
        assert!(max <= 1.1 && max > 0.5, "max {max}");
    }

    #[test]
    fn double_noise_spreads_across_range() {
        let n = DoubleNoise::new(&mut Rng::new(9), 4, 300.0, 1.0);
        let vals: Vec<f64> =
            (0..4000).map(|i| n.sample2((i % 63) as f64 * 97.0, (i / 63) as f64 * 89.0)).collect();
        let mean = vals.iter().sum::<f64>() / vals.len() as f64;
        let sd = (vals.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / vals.len() as f64).sqrt();
        assert!(mean.abs() < 0.1, "mean {mean}");
        assert!(sd > 0.15, "sd {sd}");
    }
}
