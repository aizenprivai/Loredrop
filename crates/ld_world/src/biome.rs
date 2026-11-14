//! Biome definitions, loaded from `assets/data/biomes.toml`.
//!
//! ## How biomes are chosen (like modern Minecraft)
//! The world generator samples five smooth "climate" values everywhere, each
//! roughly in −1..1:
//!
//! | Parameter         | Meaning                                              |
//! |-------------------|------------------------------------------------------|
//! | `temperature`     | cold ↔ hot                                           |
//! | `humidity`        | dry ↔ wet                                            |
//! | `continentalness` | deep ocean ↔ coast ↔ far inland                      |
//! | `erosion`         | jagged/mountainous ↔ flat/worn down                  |
//! | `weirdness`       | variation; values near 0 carve river valleys         |
//!
//! Each biome lists the range of each parameter it likes. A location gets the
//! biome whose ranges it is *closest* to (distance 0 = inside every range).
//! Ranges may overlap; on a tie the biome listed **first** wins, so list
//! special biomes (rivers, beaches, mountains) before broad ones.
//!
//! The same climate values also shape the terrain height, so biomes always
//! match the land they sit on (no deserts on ocean floors).

use serde::Deserialize;
use std::collections::HashMap;

/// Numeric biome ID (index into the registry).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct BiomeId(pub u8);

/// A climate point or range bound, see module docs.
pub type Range = [f32; 2];

const FULL: Range = [-2.0, 2.0];

/// One biome.
#[derive(Debug, Clone)]
pub struct BiomeDef {
    pub name: String,
    pub display: String,
    pub temperature: Range,
    pub humidity: Range,
    pub continentalness: Range,
    pub erosion: Range,
    pub weirdness: Range,
    /// sRGB colours (0..1) applied to tinted textures.
    pub grass_color: [f32; 3],
    pub foliage_color: [f32; 3],
    pub water_color: [f32; 3],
    /// Block names for the surface layers.
    pub surface: String,
    pub filler: String,
    pub filler_depth: u32,
    /// Below the filler (e.g. sandstone under desert sand).
    pub deep_filler: Option<String>,
    pub deep_filler_depth: u32,
    /// Surface block when under water.
    pub underwater: String,
    /// Freeze water and cover the ground with snow.
    pub snowy: bool,
    /// Average trees per chunk, and which kinds (weights).
    pub tree_density: f32,
    pub trees: HashMap<String, u32>,
    /// Attempts per chunk to place each plant (block name → attempts).
    pub plants: HashMap<String, u32>,
}

#[derive(Deserialize)]
struct BiomesFile {
    biome: Vec<BiomeEntry>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BiomeEntry {
    name: String,
    display: Option<String>,
    temperature: Option<Range>,
    humidity: Option<Range>,
    continentalness: Option<Range>,
    erosion: Option<Range>,
    weirdness: Option<Range>,
    grass_color: String,
    foliage_color: String,
    water_color: String,
    surface: String,
    filler: String,
    filler_depth: Option<u32>,
    deep_filler: Option<String>,
    deep_filler_depth: Option<u32>,
    underwater: Option<String>,
    snowy: Option<bool>,
    tree_density: Option<f32>,
    #[serde(default)]
    trees: HashMap<String, u32>,
    #[serde(default)]
    plants: HashMap<String, u32>,
}

/// Parses `#RRGGBB` into sRGB 0..1.
fn color(s: &str, biome: &str) -> Result<[f32; 3], String> {
    let h = s
        .strip_prefix('#')
        .filter(|h| h.len() == 6)
        .ok_or_else(|| format!("biome '{biome}': colour '{s}' must look like #RRGGBB"))?;
    let v = u32::from_str_radix(h, 16).map_err(|_| format!("biome '{biome}': bad colour '{s}'"))?;
    Ok([(v >> 16) as f32 / 255.0, ((v >> 8) & 255) as f32 / 255.0, (v & 255) as f32 / 255.0])
}

/// All biomes, indexed by [`BiomeId`].
#[derive(Debug, Clone)]
pub struct BiomeRegistry {
    defs: Vec<BiomeDef>,
}

impl BiomeRegistry {
    pub fn from_toml(text: &str) -> Result<Self, String> {
        let file: BiomesFile = toml::from_str(text).map_err(|e| format!("biomes file: {e}"))?;
        if file.biome.is_empty() || file.biome.len() > 255 {
            return Err("biomes file must define between 1 and 255 biomes".into());
        }
        let mut defs = Vec::new();
        for e in file.biome {
            if defs.iter().any(|d: &BiomeDef| d.name == e.name) {
                return Err(format!("biome '{}' is defined twice", e.name));
            }
            defs.push(BiomeDef {
                display: e.display.unwrap_or_else(|| e.name.clone()),
                temperature: e.temperature.unwrap_or(FULL),
                humidity: e.humidity.unwrap_or(FULL),
                continentalness: e.continentalness.unwrap_or(FULL),
                erosion: e.erosion.unwrap_or(FULL),
                weirdness: e.weirdness.unwrap_or(FULL),
                grass_color: color(&e.grass_color, &e.name)?,
                foliage_color: color(&e.foliage_color, &e.name)?,
                water_color: color(&e.water_color, &e.name)?,
                underwater: e.underwater.unwrap_or_else(|| "sand".into()),
                surface: e.surface,
                filler: e.filler,
                filler_depth: e.filler_depth.unwrap_or(3),
                deep_filler: e.deep_filler,
                deep_filler_depth: e.deep_filler_depth.unwrap_or(0),
                snowy: e.snowy.unwrap_or(false),
                tree_density: e.tree_density.unwrap_or(0.0),
                trees: e.trees,
                plants: e.plants,
                name: e.name,
            });
        }
        Ok(Self { defs })
    }

    /// A registry with no biomes (placeholder; `get` must not be called).
    pub fn empty() -> Self {
        Self { defs: Vec::new() }
    }

    pub fn get(&self, id: BiomeId) -> &BiomeDef {
        self.defs.get(id.0 as usize).unwrap_or(&self.defs[0])
    }

    pub fn id(&self, name: &str) -> Option<BiomeId> {
        self.defs.iter().position(|d| d.name == name).map(|i| BiomeId(i as u8))
    }

    pub fn len(&self) -> usize {
        self.defs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.defs.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = (BiomeId, &BiomeDef)> {
        self.defs.iter().enumerate().map(|(i, d)| (BiomeId(i as u8), d))
    }

    /// The biome best matching a climate point `[t, h, c, e, w]`.
    pub fn select(&self, p: [f32; 5]) -> BiomeId {
        let dist = |v: f32, r: Range| {
            if v < r[0] {
                r[0] - v
            } else if v > r[1] {
                v - r[1]
            } else {
                0.0
            }
        };
        let mut best = (f32::MAX, 0usize);
        for (i, d) in self.defs.iter().enumerate() {
            let ranges = [d.temperature, d.humidity, d.continentalness, d.erosion, d.weirdness];
            let score: f32 = p.iter().zip(ranges).map(|(&v, r)| dist(v, r).powi(2)).sum();
            if score < best.0 {
                best = (score, i);
                if score == 0.0 {
                    break; // first exact match wins
                }
            }
        }
        BiomeId(best.1 as u8)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r##"
        [[biome]]
        name = "river"
        continentalness = [-0.1, 2.0]
        weirdness = [-0.05, 0.05]
        grass_color = "#88BB66"
        foliage_color = "#77AA33"
        water_color = "#3366DD"
        surface = "grass_block"
        filler = "dirt"

        [[biome]]
        name = "plains"
        continentalness = [-0.1, 2.0]
        grass_color = "#90BC59"
        foliage_color = "#77AB2F"
        water_color = "#3F76E4"
        surface = "grass_block"
        filler = "dirt"
        tree_density = 0.1
        trees = { oak = 1 }

        [[biome]]
        name = "ocean"
        continentalness = [-2.0, -0.2]
        grass_color = "#8EB971"
        foliage_color = "#71A74D"
        water_color = "#3F76E4"
        surface = "sand"
        filler = "sand"
    "##;

    #[test]
    fn selects_first_exact_match_then_nearest() {
        let r = BiomeRegistry::from_toml(SAMPLE).unwrap();
        assert_eq!(r.get(r.select([0.0, 0.0, 0.5, 0.0, 0.01])).name, "river", "listed first");
        assert_eq!(r.get(r.select([0.0, 0.0, 0.5, 0.0, 0.5])).name, "plains");
        assert_eq!(r.get(r.select([0.0, 0.0, -0.8, 0.0, 0.5])).name, "ocean");
        // In the gap between ocean (≤ -0.2) and land (≥ -0.1): nearest wins.
        assert_eq!(r.get(r.select([0.0, 0.0, -0.18, 0.0, 0.5])).name, "ocean");
        let p = r.get(r.id("plains").unwrap());
        assert!((p.grass_color[0] - 0x90 as f32 / 255.0).abs() < 1e-6);
        assert_eq!(p.trees.get("oak"), Some(&1));
    }

    #[test]
    fn rejects_bad_colours_and_duplicates() {
        assert!(BiomeRegistry::from_toml(&SAMPLE.replace("#88BB66", "green")).is_err());
        let dup = SAMPLE.replace("name = \"ocean\"", "name = \"plains\"");
        assert!(BiomeRegistry::from_toml(&dup).is_err());
    }
}
