//! Developer tool: prints a few open cave spots near spawn (for screenshots).
//! `cargo run --release -p ld_worldgen --example find_cave -- <seed>`
use ld_core::coords::ChunkPos;
use ld_worldgen::TerrainGenerator;
use std::sync::Arc;

fn main() {
    let seed: u64 = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(12345);
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/data/");
    let reg =
        ld_world::BlockRegistry::from_toml(&std::fs::read_to_string(format!("{dir}blocks.toml")).unwrap())
            .unwrap();
    let biomes = Arc::new(
        ld_world::BiomeRegistry::from_toml(&std::fs::read_to_string(format!("{dir}biomes.toml")).unwrap())
            .unwrap(),
    );
    let ores = std::fs::read_to_string(format!("{dir}ores.toml")).unwrap();
    let g = ld_worldgen::Overworld::new(seed, &reg, biomes, &ores).unwrap();
    let (sx, _, sz) = g.find_spawn();
    let lava = reg.id("lava").unwrap();
    let mut found = 0;
    'outer: for r in 0i32..6 {
        for cx in -r..=r {
            for cz in -r..=r {
                if cx.abs().max(cz.abs()) != r {
                    continue;
                }
                let pos = ChunkPos::new(sx.div_euclid(16) + cx, sz.div_euclid(16) + cz);
                let col = g.generate(pos);
                for y in (-50..40).rev() {
                    // A big open pocket: 5×5×4 of air with a solid floor.
                    let open = (0..5).all(|dx| {
                        (0..5).all(|dz| {
                            (0..4).all(|dy| col.get(4 + dx, y + dy, 4 + dz) == ld_world::BlockId::AIR)
                        })
                    });
                    let floor = col.get(6, y - 1, 6) != ld_world::BlockId::AIR;
                    if open && floor {
                        let near_lava = (0..16)
                            .any(|x| (0..16).any(|z| (y - 3..y + 3).any(|yy| col.get(x, yy, z) == lava)));
                        println!(
                            "cave at {} {} {}{}",
                            pos.x * 16 + 6,
                            y,
                            pos.z * 16 + 6,
                            if near_lava { " (lava nearby)" } else { "" }
                        );
                        found += 1;
                        if found >= 6 {
                            break 'outer;
                        }
                        break;
                    }
                }
            }
        }
    }
}
