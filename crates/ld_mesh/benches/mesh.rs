//! Meshing benchmarks: `cargo bench -p ld_mesh`.

use criterion::{Criterion, criterion_group, criterion_main};
use ld_mesh::{MeshTable, Neighborhood, mesh_section};
use ld_world::{BlockId, BlockRegistry};

const BLOCKS: &str = r#"
[[block]]
name = "stone"
layer = "opaque"
textures = { all = "stone" }
[[block]]
name = "dirt"
layer = "opaque"
textures = { all = "dirt" }
[[block]]
name = "grass_block"
layer = "opaque"
textures = { top = "grass_block_top", bottom = "dirt", side = "grass_block_side" }
"#;

fn bench(c: &mut Criterion) {
    let r = BlockRegistry::from_toml(BLOCKS).unwrap();
    let table = MeshTable::new(&r, |_| 0);
    let (stone, dirt, grass) = (r.id("stone").unwrap(), r.id("dirt").unwrap(), r.id("grass_block").unwrap());
    // Rolling hills crossing the section: the typical surface case.
    let terrain = Neighborhood::from_fn(|x, y, z| {
        let h = 8 + ((x as f32 * 0.4).sin() * 3.0 + (z as f32 * 0.3).cos() * 3.0) as i32;
        if y > h {
            BlockId::AIR
        } else if y == h {
            grass
        } else if y > h - 3 {
            dirt
        } else {
            stone
        }
    });
    let checker =
        Neighborhood::from_fn(|x, y, z| if (x + y + z).rem_euclid(2) == 0 { stone } else { BlockId::AIR });
    c.bench_function("mesh_surface_section", |b| b.iter(|| mesh_section(&terrain, &table)));
    c.bench_function("mesh_checkerboard_worst_case", |b| b.iter(|| mesh_section(&checker, &table)));
}

criterion_group!(benches, bench);
criterion_main!(benches);
