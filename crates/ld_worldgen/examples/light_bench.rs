//! Developer tool: times light computation for real terrain sections.
//! `cargo run --release -p ld_worldgen --example light_bench`
use ld_core::coords::ChunkPos;
use ld_world::{ChunkColumn, LightRegion, LightTable};
use ld_worldgen::TerrainGenerator;
use std::sync::Arc;
use std::time::Instant;

fn main() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/data/");
    let reg =
        ld_world::BlockRegistry::from_toml(&std::fs::read_to_string(format!("{dir}blocks.toml")).unwrap())
            .unwrap();
    let biomes = Arc::new(
        ld_world::BiomeRegistry::from_toml(&std::fs::read_to_string(format!("{dir}biomes.toml")).unwrap())
            .unwrap(),
    );
    let ores = std::fs::read_to_string(format!("{dir}ores.toml")).unwrap();
    let g = ld_worldgen::Overworld::new(12345, &reg, biomes, &ores).unwrap();
    let table = LightTable::new(&reg);
    let cols: Vec<ChunkColumn> = (-1..=1)
        .flat_map(|z| (-1..=1).map(move |x| (x, z)))
        .map(|(x, z)| g.generate(ChunkPos::new(x, z)))
        .collect();
    let refs: [[Option<&ChunkColumn>; 3]; 3] =
        std::array::from_fn(|z| std::array::from_fn(|x| Some(&cols[z * 3 + x])));
    let mut total = 0.0;
    let mut n = 0;
    for i in 0..24 {
        if cols[4].section(i).is_empty() {
            continue;
        }
        let t = Instant::now();
        let reps = 20;
        for _ in 0..reps {
            std::hint::black_box(LightRegion::around_section(refs, i, &table));
        }
        let ms = t.elapsed().as_secs_f64() * 1000.0 / reps as f64;
        println!("section {i:2} (y {:4}): {ms:.2} ms", -64 + i as i32 * 16);
        total += ms;
        n += 1;
    }
    println!("average {:.2} ms over {n} non-empty sections", total / n as f64);
}
