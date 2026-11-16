//! Memory and CPU probes for the built-in speed test.
//!
//! The first real-calculator test showed each ray costing far more CPU
//! cycles than its instruction count explains, so the processor is mostly
//! waiting. These tiny loops measure, on the calculator itself, how long
//! one read takes in each place the renderer reads from:
//!
//! * `calc` — the loop alone (a pseudo-random number and an addition, no
//!   memory read); the other numbers have this subtracted,
//! * `ram1k` — random reads in 1 KB of RAM: if the CPU's data cache is on,
//!   this fits in it and should cost almost nothing,
//! * `ram32k` — random reads in 32 KB of RAM (the world store),
//! * `flash` — random reads in the textures, which live in the app's
//!   external flash memory,
//! * `hot` — reads that keep hitting the same 64 bytes of flash, like the
//!   constants the compiler places next to the code.
//!
//! All table sizes are powers of two so the index is a simple bit mask (an
//! earlier version used a division there, which polluted the RAM numbers).

use crate::Platform;
use crate::blocks::TEXTURES;
use crate::world::World;
use core::hint::black_box;

/// Next pseudo-random value (cheap linear congruential step).
#[inline(always)]
fn lcg(x: u32) -> u32 {
    x.wrapping_mul(1_664_525).wrapping_add(1_013_904_223)
}

/// Loop iterations per probe.
const N: u32 = 1 << 21;

/// Runs `f` and returns tenths of a nanosecond per iteration (the clock
/// only counts milliseconds, so many iterations are timed together).
fn time(p: &mut dyn Platform, f: impl FnOnce(u32) -> u32) -> u32 {
    let n = black_box(N);
    let t0 = p.millis();
    black_box(f(n));
    let ms = p.millis() - t0;
    (ms * 10_000_000 / N as u64) as u32
}

/// The probe loop: `n` random indices below `mask + 1`, each passed to
/// `read`, results summed. Every probe runs this same loop, so they differ
/// only by the read itself.
#[inline(always)]
fn walk(n: u32, mask: u32, seed: u32, read: impl Fn(usize) -> u32) -> u32 {
    let mut x = black_box(seed);
    let mut acc = 0u32;
    for _ in 0..n {
        x = lcg(x);
        acc = acc.wrapping_add(read(((x >> 8) & mask) as usize));
    }
    acc
}

/// Results in tenths of a nanosecond: the bare loop, then the extra cost of
/// one read in 1 KB of RAM, 32 KB of RAM, random flash and repeated flash.
pub fn run(p: &mut dyn Platform, world: &World) -> [u32; 5] {
    let (blocks, filled) = world.raw_tables();
    let (ram1k, ram32k) = (&filled[..1024], &blocks[..32 * 1024]);
    let flash_texels = TEXTURES.len() * 256;
    assert!(flash_texels >= 1 << 15);
    // SAFETY (all reads): every index is masked below the length checked here.
    let calc = time(p, |n| walk(n, 1023, 1, |i| i as u32));
    let small = time(p, |n| walk(n, 1023, 7, |i| unsafe { *ram1k.get_unchecked(i) } as u32));
    let big = time(p, |n| walk(n, 32 * 1024 - 1, 7, |i| unsafe { *ram32k.get_unchecked(i) } as u32));
    let flash = time(p, |n| {
        walk(n, (1 << 15) - 1, 3, |i| unsafe { *TEXTURES.get_unchecked(i >> 8).get_unchecked(i & 255) }
            as u32)
    });
    let hot = time(p, |n| walk(n, 31, 5, |i| unsafe { *TEXTURES.get_unchecked(1).get_unchecked(i) } as u32));
    [
        calc,
        small.saturating_sub(calc),
        big.saturating_sub(calc),
        flash.saturating_sub(calc),
        hot.saturating_sub(calc),
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
