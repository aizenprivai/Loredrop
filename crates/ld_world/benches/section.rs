//! Section storage benchmarks: `cargo bench -p ld_world`.

use criterion::{Criterion, criterion_group, criterion_main};
use ld_world::BlockId;
use ld_world::section::{Section, VOLUME};
use std::hint::black_box;

fn bench(c: &mut Criterion) {
    // A realistic terrain section: 4 block types.
    let mut s = Section::Uniform(BlockId(1));
    for i in 0..VOLUME {
        s.set(i, BlockId((i * 7 % 4) as u16 + 1));
    }
    c.bench_function("section_get_all_4096", |b| {
        b.iter(|| {
            let mut sum = 0u32;
            for i in 0..VOLUME {
                sum += s.get(black_box(i)).0 as u32;
            }
            sum
        })
    });
    c.bench_function("section_fill_4096_from_uniform", |b| {
        b.iter(|| {
            let mut s = Section::Uniform(BlockId(1));
            for i in 0..VOLUME {
                s.set(i, BlockId((i % 6) as u16 + 1));
            }
            s
        })
    });
}

criterion_group!(benches, bench);
criterion_main!(benches);
