//! Block types and the block registry.
//!
//! Block types are **data, not code**: they are read from a TOML file so new
//! blocks (and, later, modded blocks) need no programming. Example entry:
//!
//! ```toml
//! [[block]]
//! name     = "grass_block"         # stable ID, never rename
//! display  = "Grass Block"         # shown to players
//! layer    = "opaque"              # opaque | cutout | translucent | invisible
//! textures = { top = "grass_block_top", bottom = "dirt", side = "grass_block_side" }
//! tint     = { top = "grass" }     # faces coloured by the biome (grass/foliage)
//! ```
//!
//! Texture keys, from least to most specific: `all`, `side` (the four
//! horizontal faces), `top`/`bottom`, then `north`/`south`/`east`/`west`.
//! Texture names follow Minecraft Java resource-pack file names on purpose,
//! so packs like Faithful can be dropped in later (GDD §7.2).

use ahash::AHashMap;
use serde::Deserialize;
use std::collections::HashMap;

/// Numeric block type ID. `0` is always air.
///
/// IDs are assigned in file order at startup and are **not** stable between
/// game versions; save files will store names and remap (Phase 9).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct BlockId(pub u16);

impl BlockId {
    pub const AIR: BlockId = BlockId(0);
}

/// The six faces of a block. The numeric order is shared with the mesher and
/// the shaders, so don't reorder it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Face {
    /// +X
    East = 0,
    /// −X
    West = 1,
    /// +Y
    Up = 2,
    /// −Y
    Down = 3,
    /// +Z
    South = 4,
    /// −Z
    North = 5,
}

impl Face {
    pub const ALL: [Face; 6] = [Face::East, Face::West, Face::Up, Face::Down, Face::South, Face::North];

    /// Unit offset toward the neighbouring block on this side.
    pub fn normal(self) -> [i32; 3] {
        match self {
            Face::East => [1, 0, 0],
            Face::West => [-1, 0, 0],
            Face::Up => [0, 1, 0],
            Face::Down => [0, -1, 0],
            Face::South => [0, 0, 1],
            Face::North => [0, 0, -1],
        }
    }
}

/// How a block is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RenderLayer {
    /// Not drawn at all (air).
    Invisible,
    /// Fully solid; hides whatever is behind it.
    Opaque,
    /// Pixels are either fully visible or fully see-through (leaves, glass).
    Cutout,
    /// Partially see-through (water, stained glass). Drawn last.
    Translucent,
}

/// Colour multiplier applied to a face, e.g. so one grey grass texture can be
/// green in a jungle and yellowish in a savanna (biome tints come in Phase 4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Tint {
    None,
    Grass,
    Foliage,
}

/// Everything known about one block type.
#[derive(Debug, Clone)]
pub struct BlockDef {
    pub name: String,
    pub display: String,
    pub layer: RenderLayer,
    /// Whether entities collide with it.
    pub solid: bool,
    /// For cutout blocks: hide faces between two blocks of the same type
    /// (true for glass, false for leaves so trees look full).
    pub cull_same: bool,
    /// Texture name per face, indexed by [`Face`] as `usize`.
    pub textures: [String; 6],
    pub tints: [Tint; 6],
}

impl BlockDef {
    /// True if this block completely hides the faces of its neighbours.
    pub fn is_opaque(&self) -> bool {
        self.layer == RenderLayer::Opaque
    }
}

// --- File format (serde) ----------------------------------------------------

#[derive(Deserialize)]
struct BlocksFile {
    block: Vec<BlockEntry>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BlockEntry {
    name: String,
    display: Option<String>,
    layer: RenderLayer,
    solid: Option<bool>,
    cull_same: Option<bool>,
    #[serde(default)]
    textures: HashMap<String, String>,
    #[serde(default)]
    tint: HashMap<String, Tint>,
}

/// Resolves `all`/`side`/`top`/… keys into one value per face.
fn per_face<T: Clone>(
    map: &HashMap<String, T>,
    default: T,
    what: &str,
    block: &str,
) -> Result<[T; 6], String> {
    for key in map.keys() {
        if !["all", "side", "top", "bottom", "north", "south", "east", "west"].contains(&key.as_str()) {
            return Err(format!("block '{block}': unknown {what} key '{key}'"));
        }
    }
    let pick = |keys: &[&str]| keys.iter().find_map(|k| map.get(*k)).cloned().unwrap_or(default.clone());
    Ok([
        pick(&["east", "side", "all"]),
        pick(&["west", "side", "all"]),
        pick(&["top", "all"]),
        pick(&["bottom", "all"]),
        pick(&["south", "side", "all"]),
        pick(&["north", "side", "all"]),
    ])
}

/// All block types, indexed by [`BlockId`].
#[derive(Debug, Clone)]
pub struct BlockRegistry {
    defs: Vec<BlockDef>,
    by_name: AHashMap<String, BlockId>,
}

impl BlockRegistry {
    /// Parses a blocks file. `air` is always created as ID 0 and must not be
    /// listed in the file.
    pub fn from_toml(text: &str) -> Result<Self, String> {
        let file: BlocksFile = toml::from_str(text).map_err(|e| format!("blocks file: {e}"))?;
        let mut reg = Self { defs: Vec::new(), by_name: AHashMap::new() };
        reg.push(BlockDef {
            name: "air".into(),
            display: "Air".into(),
            layer: RenderLayer::Invisible,
            solid: false,
            cull_same: false,
            textures: Default::default(),
            tints: [Tint::None; 6],
        })?;
        for e in file.block {
            if e.layer != RenderLayer::Invisible && e.textures.is_empty() {
                return Err(format!("block '{}' is visible but has no textures", e.name));
            }
            let textures = per_face(&e.textures, String::new(), "texture", &e.name)?;
            let tints = per_face(&e.tint, Tint::None, "tint", &e.name)?;
            reg.push(BlockDef {
                display: e.display.unwrap_or_else(|| e.name.clone()),
                solid: e.solid.unwrap_or(e.layer != RenderLayer::Invisible),
                cull_same: e.cull_same.unwrap_or(false),
                name: e.name,
                layer: e.layer,
                textures,
                tints,
            })?;
        }
        Ok(reg)
    }

    fn push(&mut self, def: BlockDef) -> Result<(), String> {
        if self.by_name.contains_key(&def.name) {
            return Err(format!("block '{}' is defined twice", def.name));
        }
        if self.defs.len() > u16::MAX as usize {
            return Err("too many block types".into());
        }
        self.by_name.insert(def.name.clone(), BlockId(self.defs.len() as u16));
        self.defs.push(def);
        Ok(())
    }

    /// The definition of `id`. Unknown IDs return air's definition.
    pub fn get(&self, id: BlockId) -> &BlockDef {
        self.defs.get(id.0 as usize).unwrap_or(&self.defs[0])
    }

    /// Looks a block up by its stable name.
    pub fn id(&self, name: &str) -> Option<BlockId> {
        self.by_name.get(name).copied()
    }

    /// Like [`id`](Self::id) but returns an error naming the missing block.
    pub fn require(&self, name: &str) -> Result<BlockId, String> {
        self.id(name).ok_or_else(|| format!("required block '{name}' is missing from the blocks file"))
    }

    pub fn len(&self) -> usize {
        self.defs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.defs.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = (BlockId, &BlockDef)> {
        self.defs.iter().enumerate().map(|(i, d)| (BlockId(i as u16), d))
    }

    /// Every distinct texture name used by any block, sorted (the renderer
    /// turns each into one texture-array layer).
    pub fn texture_names(&self) -> Vec<String> {
        let mut names: Vec<String> =
            self.defs.iter().flat_map(|d| d.textures.iter().cloned()).filter(|t| !t.is_empty()).collect();
        names.sort();
        names.dedup();
        names
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
        [[block]]
        name = "stone"
        layer = "opaque"
        textures = { all = "stone" }

        [[block]]
        name = "grass_block"
        display = "Grass Block"
        layer = "opaque"
        textures = { top = "grass_block_top", bottom = "dirt", side = "grass_block_side" }
        tint = { top = "grass" }

        [[block]]
        name = "oak_leaves"
        layer = "cutout"
        textures = { all = "oak_leaves" }
        tint = { all = "foliage" }
    "#;

    #[test]
    fn parses_and_resolves_faces() {
        let r = BlockRegistry::from_toml(SAMPLE).unwrap();
        assert_eq!(r.len(), 4);
        assert_eq!(r.id("air"), Some(BlockId::AIR));
        let g = r.get(r.id("grass_block").unwrap());
        assert_eq!(g.textures[Face::Up as usize], "grass_block_top");
        assert_eq!(g.textures[Face::Down as usize], "dirt");
        assert_eq!(g.textures[Face::North as usize], "grass_block_side");
        assert_eq!(g.tints[Face::Up as usize], Tint::Grass);
        assert_eq!(g.tints[Face::East as usize], Tint::None);
        assert!(g.solid && g.is_opaque());
        assert_eq!(r.texture_names(), ["dirt", "grass_block_side", "grass_block_top", "oak_leaves", "stone"]);
    }

    #[test]
    fn rejects_mistakes() {
        assert!(BlockRegistry::from_toml("[[block]]\nname='a'\nlayer='opaque'\n").is_err(), "no textures");
        let dup = "[[block]]\nname='a'\nlayer='opaque'\ntextures={all='x'}\n".repeat(2);
        assert!(BlockRegistry::from_toml(&dup).is_err(), "duplicate");
        let typo = "[[block]]\nname='a'\nlayer='opaque'\ntextures={tpo='x'}\n";
        assert!(BlockRegistry::from_toml(typo).is_err(), "unknown face key");
    }

    #[test]
    fn face_normals_are_unit_and_distinct() {
        let mut seen = std::collections::HashSet::new();
        for f in Face::ALL {
            let n = f.normal();
            assert_eq!(n.iter().map(|c| c.abs()).sum::<i32>(), 1);
            assert!(seen.insert(n));
        }
    }
}
