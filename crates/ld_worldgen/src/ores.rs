//! Ore and rock-blob placement, configured by `assets/data/ores.toml`.

use crate::noise::Rng;
use ld_world::{BiomeId, BiomeRegistry, BlockId, BlockRegistry};
use serde::Deserialize;

#[derive(Deserialize)]
struct OresFile {
    ore: Vec<OreEntry>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OreEntry {
    block: String,
    deepslate: Option<String>,
    size: u32,
    per_chunk: f32,
    height: [i32; 2],
    shape: Shape,
    #[serde(default)]
    biomes: Vec<String>,
}

#[derive(Deserialize, Clone, Copy, Debug, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Shape {
    Uniform,
    Triangle,
}

/// One resolved ore rule.
#[derive(Clone, Debug)]
pub struct OreRule {
    pub block: BlockId,
    pub deepslate: Option<BlockId>,
    pub size: u32,
    pub per_chunk: f32,
    pub min_y: i32,
    pub max_y: i32,
    pub shape: Shape,
    pub biomes: Vec<BiomeId>,
}

pub fn parse(text: &str, blocks: &BlockRegistry, biomes: &BiomeRegistry) -> Result<Vec<OreRule>, String> {
    let file: OresFile = toml::from_str(text).map_err(|e| format!("ores file: {e}"))?;
    file.ore
        .into_iter()
        .map(|e| {
            if e.height[0] > e.height[1] {
                return Err(format!("ore '{}': height range is reversed", e.block));
            }
            Ok(OreRule {
                block: blocks.require(&e.block)?,
                deepslate: e.deepslate.as_deref().map(|d| blocks.require(d)).transpose()?,
                size: e.size.max(1),
                per_chunk: e.per_chunk,
                min_y: e.height[0],
                max_y: e.height[1],
                shape: e.shape,
                biomes: e
                    .biomes
                    .iter()
                    .map(|b| biomes.id(b).ok_or_else(|| format!("ore '{}': unknown biome '{b}'", e.block)))
                    .collect::<Result<_, _>>()?,
            })
        })
        .collect()
}

impl OreRule {
    /// Picks a Y for one vein according to the distribution shape.
    pub fn pick_y(&self, rng: &mut Rng) -> i32 {
        match self.shape {
            Shape::Uniform => rng.range(self.min_y, self.max_y),
            Shape::Triangle => {
                // Sum of two uniform halves → peaks in the middle.
                let half = (self.max_y - self.min_y) / 2;
                self.min_y + rng.range(0, half) + rng.range(0, self.max_y - self.min_y - half)
            }
        }
    }

    /// How many veins to attempt in a chunk (fractional rates are randomised).
    pub fn attempts(&self, rng: &mut Rng) -> u32 {
        let whole = self.per_chunk.floor();
        whole as u32 + (rng.next_f64() < (self.per_chunk - whole) as f64) as u32
    }
}

/// Offsets of one vein of `size` blocks around its centre: a connected,
/// lumpy blob grown block by block from the centre (each new block touches
/// an existing one), so even tiny veins are solid clusters.
pub fn vein_offsets(size: u32, rng: &mut Rng) -> Vec<[i32; 3]> {
    const DIRS: [[i32; 3]; 6] = [[1, 0, 0], [-1, 0, 0], [0, 1, 0], [0, -1, 0], [0, 0, 1], [0, 0, -1]];
    let mut out = vec![[0, 0, 0]];
    let mut tries = 0;
    while out.len() < size as usize && tries < size * 20 {
        tries += 1;
        let from = out[rng.below(out.len() as u32) as usize];
        let d = DIRS[rng.below(6) as usize];
        let p = [from[0] + d[0], from[1] + d[1], from[2] + d[2]];
        if !out.contains(&p) {
            out.push(p);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn triangle_peaks_in_the_middle() {
        let rule = OreRule {
            block: BlockId(1),
            deepslate: None,
            size: 4,
            per_chunk: 1.0,
            min_y: -100,
            max_y: 100,
            shape: Shape::Triangle,
            biomes: vec![],
        };
        let mut rng = Rng::new(3);
        let ys: Vec<i32> = (0..10_000).map(|_| rule.pick_y(&mut rng)).collect();
        assert!(ys.iter().all(|y| (-100..=100).contains(y)));
        let middle = ys.iter().filter(|y| y.abs() < 25).count();
        let edge = ys.iter().filter(|y| y.abs() > 75).count();
        assert!(middle > edge * 4, "middle {middle} edge {edge}");
    }

    #[test]
    fn vein_size_is_close_to_requested() {
        let mut rng = Rng::new(1);
        for size in [3, 9, 17, 64] {
            let v = vein_offsets(size, &mut rng);
            assert_eq!(v.len() as u32, size);
        }
    }
}
