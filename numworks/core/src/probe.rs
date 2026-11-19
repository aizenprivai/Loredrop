//! Memory and CPU probes for the built-in speed test.
//!
//! The first real-calculator test showed each ray costing far more CPU
//! cycles than its instruction count explains, so the processor is mostly
//! waiting. These tiny loops measure, on the calculator itself, how long
//! one operation takes in each place the renderer reads from:
//!
//! * plain arithmetic (no memory) — the CPU's own speed,
//! * random reads in a small RAM table (the 6 KB fill map),
//! * random reads in the big RAM table (the 48 KB world),
//! * random reads in the textures, which live in the app's flash memory,
//! * reads that keep hitting the same 64 bytes of flash (does the CPU's
//!   cache keep recently read flash, e.g. constants stored next to code?).
//!
//! Comparing them tells which memory is slow, and so what to optimise next.

use crate::Platform;
use crate::blocks::TEXTURES;
use crate::world::World;
use core::hint::black_box;

/// Next pseudo-random value (cheap linear congruential step).
#[inline(always)]
fn lcg(x: u32) -> u32 {
    x.wrapping_mul(1_664_525).wrapping_add(1_013_904_223)
}

/// Runs `f` and returns tenths of a nanosecond per operation for `ops`
/// operations (the clock only counts milliseconds, so many operations are
/// timed together).
fn time(p: &mut dyn Platform, ops: u32, f: impl FnOnce() -> u32) -> u32 {
    let t0 = p.millis();
    black_box(f());
    let ms = p.millis() - t0;
    (ms * 10_000_000 / ops as u64) as u32
}

/// Results in tenths of a nanosecond per operation: arithmetic, small-RAM
/// read, big-RAM read, random flash read, repeated flash read.
pub fn run(p: &mut dyn Platform, world: &World) -> [u32; 5] {
    const N: u32 = 2_000_000;
    let (blocks, filled) = world.raw_tables();
    let alu = time(p, N, || {
        let mut x = black_box(1u32);
        for _ in 0..N {
            x = lcg(x);
        }
        x
    });
    let read = |table: &[u8]| {
        let len = table.len() as u32;
        let mut x = black_box(7u32);
        let mut acc = 0u32;
        for _ in 0..N {
            x = lcg(x);
            // SAFETY: the index is reduced modulo the table length.
            acc = acc.wrapping_add(unsafe { *table.get_unchecked(((x >> 8) % len) as usize) } as u32);
        }
        acc
    };
    let small = time(p, N, || read(filled));
    let big = time(p, N, || read(blocks));
    let flash = time(p, N, || {
        let len = (TEXTURES.len() * 256) as u32;
        let mut x = black_box(3u32);
        let mut acc = 0u32;
        for _ in 0..N {
            x = lcg(x);
            let i = ((x >> 8) % len) as usize;
            acc = acc.wrapping_add(TEXTURES[i / 256][i % 256] as u32);
        }
        acc
    });
    let flash_hot = time(p, N, || {
        let mut x = black_box(5u32);
        let mut acc = 0u32;
        for _ in 0..N {
            x = lcg(x);
            // Always within the first 32 texels (64 bytes) of one texture.
            acc = acc.wrapping_add(TEXTURES[black_box(1)][((x >> 8) & 31) as usize] as u32);
        }
        acc
    });
    // The read loops also do the arithmetic: report the extra cost of the read.
    [
        alu,
        small.saturating_sub(alu),
        big.saturating_sub(alu),
        flash.saturating_sub(alu),
        flash_hot.saturating_sub(alu),
    ]
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use std::boxed::Box;

    /// Real clock for the probes.
    struct Clock(std::time::Instant);
    impl Platform for Clock {
        fn push_rect(&mut self, _: u16, _: u16, _: u16, _: u16, _: &[u16]) {}
        fn fill_rect(&mut self, _: u16, _: u16, _: u16, _: u16, _: u16) {}
        fn draw_string(&mut self, _: &str, _: u16, _: u16, _: bool, _: u16, _: u16) {}
        fn keyboard(&mut self) -> u64 {
            0
        }
        fn millis(&mut self) -> u64 {
            self.0.elapsed().as_millis() as u64
        }
        fn random(&mut self) -> u32 {
            1
        }
        fn sleep(&mut self, _: u32) {}
    }

    #[test]
    fn probes_run() {
        let mut w = Box::new(World::new());
        w.reset(1);
        w.update(0, 0, usize::MAX);
        let r = run(&mut Clock(std::time::Instant::now()), &w);
        std::println!(
            "probes (0.1 ns/op): calc {} small {} big {} flash {} hot {}",
            r[0],
            r[1],
            r[2],
            r[3],
            r[4]
        );
        assert!(r.iter().all(|&v| v < 1_000_000));
    }
}
