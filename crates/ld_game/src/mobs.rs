//! Creature definitions, loaded from `assets/data/mobs.toml`: stats, drops,
//! spawn rules and box models. Behaviour (AI) lives in the game executable;
//! this module is pure data, shared by spawning, AI and rendering.

use crate::items::ItemRegistry;
use crate::mining::{Drop, parse_drop};
use ld_world::{BiomeId, BiomeRegistry};
use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Category {
    Passive,
    Hostile,
}

/// How a model part moves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum PartRole {
    /// Turns to look around / at targets.
    Head,
    /// Swings while walking.
    Leg,
    /// Swings while walking (and when attacking).
    Arm,
    /// Flaps (chickens).
    Wing,
    #[default]
    None,
}

/// One box of a creature model, in pixels (16 = one block).
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelPart {
    pub name: String,
    /// `[x, y, z, width, height, depth]`.
    #[serde(rename = "box")]
    pub cuboid: [f32; 6],
    #[serde(default)]
    pub pivot: [f32; 3],
    /// Fixed rotation in degrees.
    #[serde(default)]
    pub rot: [f32; 3],
    #[serde(default)]
    pub role: PartRole,
    #[serde(default)]
    pub phase: u8,
    pub tex: String,
    pub front: Option<String>,
}

/// Where and how often a creature appears.
#[derive(Debug, Clone, Default)]
pub struct SpawnRule {
    /// Biomes where it appears when a chunk is first generated.
    pub surface: Vec<BiomeId>,
    /// Appears in dark places (no sky above) near the player.
    pub dark: bool,
    pub group: [u32; 2],
    pub weight: u32,
}

#[derive(Debug, Clone)]
pub struct MobDef {
    pub name: String,
    pub display: String,
    pub category: Category,
    pub health: f32,
    pub speed: f32,
    pub width: f64,
    pub height: f64,
    pub attack_damage: f32,
    pub ranged: bool,
    pub drops: Vec<Drop>,
    pub spawn: SpawnRule,
    pub parts: Vec<ModelPart>,
}

/// Numeric creature type ID.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MobKind(pub u16);

#[derive(Deserialize)]
struct MobsFile {
    mob: Vec<MobEntry>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SpawnEntry {
    #[serde(default)]
    surface: Vec<String>,
    #[serde(default)]
    dark: bool,
    group: [u32; 2],
    weight: u32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MobEntry {
    name: String,
    display: Option<String>,
    category: Category,
    health: f32,
    speed: f32,
    width: f64,
    height: f64,
    attack_damage: Option<f32>,
    ranged: Option<bool>,
    #[serde(default)]
    drops: Vec<String>,
    spawn: Option<SpawnEntry>,
    parts: Vec<ModelPart>,
}

#[derive(Debug, Clone)]
pub struct MobRegistry {
    defs: Vec<MobDef>,
}

impl MobRegistry {
    pub fn from_toml(text: &str, items: &ItemRegistry, biomes: &BiomeRegistry) -> Result<Self, String> {
        let file: MobsFile = toml::from_str(text).map_err(|e| format!("mobs file: {e}"))?;
        let mut defs: Vec<MobDef> = Vec::new();
        for e in file.mob {
            if defs.iter().any(|d| d.name == e.name) {
                return Err(format!("mob '{}' is defined twice", e.name));
            }
            if e.parts.is_empty() {
                return Err(format!("mob '{}' has no model parts", e.name));
            }
            let drops = e
                .drops
                .iter()
                .map(|d| parse_drop(d, items))
                .collect::<Result<_, _>>()
                .map_err(|err| format!("mob '{}': {err}", e.name))?;
            let spawn = match e.spawn {
                Some(s) => SpawnRule {
                    surface: s
                        .surface
                        .iter()
                        .map(|b| biomes.id(b).ok_or_else(|| format!("mob '{}': unknown biome '{b}'", e.name)))
                        .collect::<Result<_, _>>()?,
                    dark: s.dark,
                    group: [s.group[0].max(1), s.group[1].max(s.group[0]).max(1)],
                    weight: s.weight,
                },
                None => SpawnRule::default(),
            };
            defs.push(MobDef {
                display: e.display.unwrap_or_else(|| e.name.clone()),
                name: e.name,
                category: e.category,
                health: e.health,
                speed: e.speed,
                width: e.width,
                height: e.height,
                attack_damage: e.attack_damage.unwrap_or(0.0),
                ranged: e.ranged.unwrap_or(false),
                drops,
                spawn,
                parts: e.parts,
            });
        }
        Ok(Self { defs })
    }

    pub fn get(&self, kind: MobKind) -> &MobDef {
        &self.defs[kind.0 as usize]
    }

    pub fn id(&self, name: &str) -> Option<MobKind> {
        self.defs.iter().position(|d| d.name == name).map(|i| MobKind(i as u16))
    }

    pub fn iter(&self) -> impl Iterator<Item = (MobKind, &MobDef)> {
        self.defs.iter().enumerate().map(|(i, d)| (MobKind(i as u16), d))
    }

    /// Every texture name used by any model (for the texture array).
    pub fn texture_names(&self) -> Vec<String> {
        let mut v: Vec<String> = self
            .defs
            .iter()
            .flat_map(|d| d.parts.iter().flat_map(|p| std::iter::once(p.tex.clone()).chain(p.front.clone())))
            .collect();
        v.sort();
        v.dedup();
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::items::tests::registries;

    #[test]
    fn mobs_file_loads_with_models_drops_and_spawns() {
        let (_, items) = registries();
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/data/");
        let biomes =
            BiomeRegistry::from_toml(&std::fs::read_to_string(format!("{dir}biomes.toml")).unwrap()).unwrap();
        let mobs = MobRegistry::from_toml(
            &std::fs::read_to_string(format!("{dir}mobs.toml")).unwrap(),
            &items,
            &biomes,
        )
        .unwrap();
        let cow = mobs.get(mobs.id("cow").unwrap());
        assert_eq!((cow.category, cow.health), (Category::Passive, 10.0));
        assert!(!cow.spawn.surface.is_empty());
        assert_eq!(cow.parts.iter().filter(|p| p.role == PartRole::Leg).count(), 4);
        let zombie = mobs.get(mobs.id("zombie").unwrap());
        assert!(zombie.spawn.dark && zombie.category == Category::Hostile && zombie.attack_damage > 0.0);
        assert!(mobs.get(mobs.id("skeleton").unwrap()).ranged);
        for name in mobs.texture_names() {
            let p = format!("{}/../../assets/textures/entity/{name}.png", env!("CARGO_MANIFEST_DIR"));
            assert!(std::path::Path::new(&p).is_file(), "missing creature texture {p}");
        }
    }
}
