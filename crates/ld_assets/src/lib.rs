//! # ld_assets — game files
//!
//! Locates the `assets/` folder and loads what's in it:
//! * `data/blocks.toml` → [`ld_world::BlockRegistry`]
//! * `textures/block/<name>.png` → RGBA pixels for the renderer
//!
//! **Robustness rule:** a missing or broken texture never stops the game. It is
//! replaced by a magenta/black checkerboard (impossible to miss) and a warning
//! is logged. A missing blocks file *is* fatal, because nothing can work without it.
//!
//! Resource packs (Phase 5) will slot in here: a pack is just another folder
//! searched before `assets/`.

use ld_world::{BiomeRegistry, BlockRegistry};
use std::path::{Path, PathBuf};

/// Finds the assets folder. Search order:
/// 1. the `LOREDROP_ASSETS` environment variable,
/// 2. `./assets`,
/// 3. `assets/` next to the executable or in any of its parent folders
///    (so `cargo run` from anywhere in the project works).
pub fn find_assets_dir() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("LOREDROP_ASSETS") {
        let p = PathBuf::from(p);
        if p.is_dir() {
            return Some(p);
        }
        log::warn!("LOREDROP_ASSETS={} is not a folder; searching elsewhere", p.display());
    }
    let mut candidates = vec![PathBuf::from("assets")];
    if let Ok(exe) = std::env::current_exe() {
        candidates.extend(exe.ancestors().skip(1).map(|d| d.join("assets")));
    }
    candidates.into_iter().find(|p| p.join("data").join("blocks.toml").is_file())
}

/// Loads `data/blocks.toml`.
pub fn load_blocks(assets: &Path) -> Result<BlockRegistry, String> {
    let path = assets.join("data").join("blocks.toml");
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    BlockRegistry::from_toml(&text).map_err(|e| format!("{}: {e}", path.display()))
}

/// Loads `data/biomes.toml`.
pub fn load_biomes(assets: &Path) -> Result<BiomeRegistry, String> {
    let path = assets.join("data").join("biomes.toml");
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    BiomeRegistry::from_toml(&text).map_err(|e| format!("{}: {e}", path.display()))
}

/// Reads a text data file from `data/` (e.g. `ores.toml`).
pub fn read_data(assets: &Path, name: &str) -> Result<String, String> {
    let path = assets.join("data").join(name);
    std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))
}

/// Story fragments for lore tablets (`data/lore.toml`), with `{tag}`
/// placeholders already replaced by the names table.
#[derive(Debug, Clone)]
pub struct Lore {
    /// (era, text) in file order.
    pub fragments: Vec<(String, String)>,
}

impl Lore {
    /// Fragments belonging to one era (`golden`, `expansion`, `deep`).
    pub fn era(&self, era: &str) -> Vec<&str> {
        self.fragments.iter().filter(|(e, _)| e == era).map(|(_, t)| t.as_str()).collect()
    }
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct LoreFile {
    names: std::collections::HashMap<String, String>,
    fragment: Vec<LoreEntry>,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct LoreEntry {
    era: String,
    text: String,
}

/// Parses lore text: substitutes `{tags}` and rejects unknown tags or eras.
pub fn parse_lore(text: &str) -> Result<Lore, String> {
    let file: LoreFile = toml::from_str(text).map_err(|e| format!("lore file: {e}"))?;
    let mut fragments = Vec::new();
    for f in file.fragment {
        if !["golden", "expansion", "deep"].contains(&f.era.as_str()) {
            return Err(format!("lore: unknown era '{}'", f.era));
        }
        let mut t = f.text.clone();
        for (k, v) in &file.names {
            t = t.replace(&format!("{{{k}}}"), v);
        }
        if let Some(start) = t.find('{') {
            return Err(format!("lore: unknown name tag in \"{}\"", &t[start..t.len().min(start + 30)]));
        }
        fragments.push((f.era, t));
    }
    Ok(Lore { fragments })
}

/// Loads `data/lore.toml`.
pub fn load_lore(assets: &Path) -> Result<Lore, String> {
    parse_lore(&read_data(assets, "lore.toml")?)
}

/// Square RGBA textures, all the same size, ready for a texture array.
pub struct TextureSet {
    /// Width = height, in pixels.
    pub size: u32,
    /// Names in layer order (layer `i` is `names[i]`).
    pub names: Vec<String>,
    /// RGBA8 pixels per layer, `size * size * 4` bytes each.
    pub layers: Vec<Vec<u8>>,
}

impl TextureSet {
    /// Layer index of a texture name (0 = the "missing" texture if unknown).
    pub fn layer_of(&self, name: &str) -> u16 {
        self.names.iter().position(|n| n == name).unwrap_or(0) as u16
    }
}

/// Name of the built-in placeholder texture, always layer 0.
pub const MISSING: &str = "<missing>";

/// Loads every named block texture. Textures of different sizes are scaled
/// (nearest-neighbour, keeping the pixel-art look) to the largest size found.
/// Tall images (animation strips, e.g. 16×512 water) use their first frame
/// until texture animation is implemented.
pub fn load_block_textures(assets: &Path, names: &[String]) -> TextureSet {
    let dir = assets.join("textures").join("block");
    let mut loaded: Vec<(String, Option<image::RgbaImage>)> = Vec::new();
    for name in names {
        let path = dir.join(format!("{name}.png"));
        let img = match image::open(&path) {
            Ok(i) => {
                let mut rgba = i.to_rgba8();
                let w = rgba.width();
                if rgba.height() > w {
                    rgba = image::imageops::crop_imm(&rgba, 0, 0, w, w).to_image();
                }
                Some(rgba)
            }
            Err(e) => {
                log::warn!(
                    "texture '{name}' could not be loaded ({}: {e}); using the missing texture",
                    path.display()
                );
                None
            }
        };
        loaded.push((name.clone(), img));
    }
    let size = loaded.iter().filter_map(|(_, i)| i.as_ref().map(|i| i.width())).max().unwrap_or(16).max(1);

    let mut set = TextureSet { size, names: vec![MISSING.to_string()], layers: vec![missing_texture(size)] };
    for (name, img) in loaded {
        let pixels = match img {
            Some(i) if i.width() == size => i.into_raw(),
            Some(i) => {
                image::imageops::resize(&i, size, size, image::imageops::FilterType::Nearest).into_raw()
            }
            None => missing_texture(size),
        };
        set.names.push(name);
        set.layers.push(pixels);
    }
    set
}

/// Magenta/black checkerboard (2×2 squares).
fn missing_texture(size: u32) -> Vec<u8> {
    let half = (size / 2).max(1);
    let mut px = Vec::with_capacity((size * size * 4) as usize);
    for y in 0..size {
        for x in 0..size {
            let on = (x / half + y / half).is_multiple_of(2);
            px.extend_from_slice(if on { &[248, 0, 248, 255] } else { &[0, 0, 0, 255] });
        }
    }
    px
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assets() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets")
    }

    #[test]
    fn real_blocks_file_loads_and_every_texture_exists() {
        let reg = load_blocks(&assets()).expect("blocks.toml must parse");
        let biomes = load_biomes(&assets()).expect("biomes.toml must parse");
        // Every block a biome names must exist.
        for (_, b) in biomes.iter() {
            for name in [&b.surface, &b.filler, &b.underwater]
                .into_iter()
                .chain(b.deep_filler.as_ref())
                .chain(b.plants.keys())
            {
                assert!(reg.id(name).is_some(), "biome '{}' uses unknown block '{name}'", b.name);
            }
        }
        assert!(reg.id("grass_block").is_some());
        for name in reg.texture_names() {
            let p = assets().join("textures/block").join(format!("{name}.png"));
            assert!(p.is_file(), "texture file missing: {}", p.display());
        }
        let set = load_block_textures(&assets(), &reg.texture_names());
        assert_eq!(set.size, 16);
        assert_eq!(set.layers.len(), reg.texture_names().len() + 1);
        assert!(set.layers.iter().all(|l| l.len() == 16 * 16 * 4));
    }

    #[test]
    fn lore_loads_with_names_substituted() {
        let lore = load_lore(&assets()).expect("lore.toml must parse");
        for era in ["golden", "expansion", "deep"] {
            assert!(lore.era(era).len() >= 3, "era {era} needs fragments");
        }
        assert!(lore.fragments.iter().all(|(_, t)| !t.contains('{')));
        assert!(parse_lore("[names]\n[[fragment]]\nera='golden'\ntext='{nobody}'\n").is_err());
        assert!(parse_lore("[names]\n[[fragment]]\nera='future'\ntext='x'\n").is_err());
    }

    #[test]
    fn missing_texture_falls_back() {
        let set = load_block_textures(&assets(), &["definitely_not_a_texture".into(), "stone".into()]);
        assert_eq!(set.layer_of("definitely_not_a_texture"), 1);
        assert_eq!(set.layers[1], missing_texture(16));
        assert_eq!(set.layer_of("unknown"), 0);
        assert_ne!(set.layers[2], missing_texture(16));
    }
}
