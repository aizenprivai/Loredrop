//! Generation benchmarks: `cargo bench -p ld_worldgen`.

use criterion::{Criterion, criterion_group, criterion_main};
use ld_core::coords::ChunkPos;
use ld_world::{BiomeRegistry, BlockRegistry};
use ld_worldgen::{Overworld, TerrainGenerator};
use std::sync::Arc;

fn bench(c: &mut Criterion) {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/data/");
    let read = |f: &str| std::fs::read_to_string(format!("{dir}{f}")).unwrap();
    let blocks = BlockRegistry::from_toml(&read("blocks.toml")).unwrap();
    let biomes = Arc::new(BiomeRegistry::from_toml(&read("biomes.toml")).unwrap());
    let g = Overworld::new(12345, &blocks, biomes, &read("ores.toml")).unwrap();
    let mut i = 0;
    c.bench_function("overworld_generate_column", |b| {
        b.iter(|| {
            i += 1;
            g.generate(ChunkPos::new(i % 97 - 48, i / 97 - 20))
        })
    });
    c.bench_function("overworld_column_profile", |b| {
        b.iter(|| {
            i += 1;
            g.column_profile(i * 7, i * 13)
        })
    });
}

criterion_group!(benches, bench);
criterion_main!(benches);
