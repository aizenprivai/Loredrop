//! The item registry.
//!
//! Every block that can be held becomes an item automatically (same name),
//! then `items.toml` adds materials and tools. Item IDs are assigned at
//! startup and are not stable between versions — saves will store names.

use ld_world::{BlockId, BlockRegistry, RenderLayer};
use serde::Deserialize;
use std::collections::HashMap;

/// Numeric item ID (index into the registry).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ItemId(pub u16);

/// Kinds of tools.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ToolKind {
    Pickaxe,
    Axe,
    Shovel,
    Hoe,
    Sword,
}

/// A tool's stats.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolInfo {
    pub kind: ToolKind,
    pub tier: String,
    /// Harvest level: 0 wood/gold, 1 stone, 2 iron, 3 diamond.
    pub level: u8,
    /// Mining speed multiplier on blocks this tool is meant for.
    pub speed: f32,
    pub durability: u16,
    pub attack_damage: f32,
    pub attack_speed: f32,
}

/// Which armour slot a piece goes in (index into `Inventory::armor`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ArmorSlot {
    Helmet = 0,
    Chestplate = 1,
    Leggings = 2,
    Boots = 3,
}

/// Armour stats (Minecraft scale).
#[derive(Debug, Clone, PartialEq)]
pub struct ArmorInfo {
    pub slot: ArmorSlot,
    pub defense: i32,
    pub toughness: f32,
    pub durability: u16,
}

/// What an item is.
#[derive(Debug, Clone, PartialEq)]
pub enum ItemKind {
    /// Places this block.
    Block(BlockId),
    Tool(ToolInfo),
    Armor(ArmorInfo),
    /// Materials and everything else.
    Plain,
}

/// One item type.
#[derive(Debug, Clone)]
pub struct ItemDef {
    pub name: String,
    pub display: String,
    pub kind: ItemKind,
    pub max_stack: u16,
    /// Sprite in `textures/item/` (non-block items).
    pub texture: String,
    /// If edible: (hunger points, saturation modifier).
    pub food: Option<(i32, f32)>,
}

#[derive(Deserialize)]
struct ItemsFile {
    tiers: HashMap<String, TierEntry>,
    item: Vec<ItemEntry>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TierEntry {
    level: u8,
    speed: f32,
    durability: u16,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ItemEntry {
    name: String,
    display: Option<String>,
    max_stack: Option<u16>,
    texture: Option<String>,
    tool: Option<ToolKind>,
    tier: Option<String>,
    attack_damage: Option<f32>,
    attack_speed: Option<f32>,
    food: Option<i32>,
    saturation: Option<f32>,
    armor: Option<ArmorSlot>,
    defense: Option<i32>,
    toughness: Option<f32>,
    durability: Option<u16>,
}

/// All items, indexed by [`ItemId`].
#[derive(Debug, Clone)]
pub struct ItemRegistry {
    defs: Vec<ItemDef>,
    by_name: HashMap<String, ItemId>,
    by_block: HashMap<BlockId, ItemId>,
}

impl ItemRegistry {
    pub fn new(blocks: &BlockRegistry, items_toml: &str) -> Result<Self, String> {
        let file: ItemsFile = toml::from_str(items_toml).map_err(|e| format!("items file: {e}"))?;
        let mut reg = Self { defs: Vec::new(), by_name: HashMap::new(), by_block: HashMap::new() };
        // 1. Block items (air and liquids can't be held).
        for (id, b) in blocks.iter() {
            if id == BlockId::AIR
                || b.layer == RenderLayer::Invisible
                || b.name == "water"
                || b.name == "lava"
            {
                continue;
            }
            reg.push(ItemDef {
                name: b.name.clone(),
                display: b.display.clone(),
                kind: ItemKind::Block(id),
                max_stack: 64,
                texture: String::new(),
                food: None,
            })?;
            reg.by_block.insert(id, ItemId(reg.defs.len() as u16 - 1));
        }
        // 2. Materials and tools from the file.
        for e in file.item {
            let kind = match (e.tool, &e.tier) {
                (Some(kind), Some(tier)) => {
                    let t = file
                        .tiers
                        .get(tier)
                        .ok_or_else(|| format!("item '{}': unknown tier '{tier}'", e.name))?;
                    ItemKind::Tool(ToolInfo {
                        kind,
                        tier: tier.clone(),
                        level: t.level,
                        speed: t.speed,
                        durability: t.durability,
                        attack_damage: e.attack_damage.unwrap_or(1.0),
                        attack_speed: e.attack_speed.unwrap_or(4.0),
                    })
                }
                (None, None) => match e.armor {
                    Some(slot) => ItemKind::Armor(ArmorInfo {
                        slot,
                        defense: e.defense.unwrap_or(0),
                        toughness: e.toughness.unwrap_or(0.0),
                        durability: e.durability.unwrap_or(100),
                    }),
                    None => ItemKind::Plain,
                },
                _ => return Err(format!("item '{}': `tool` and `tier` must be given together", e.name)),
            };
            let max_stack =
                if matches!(kind, ItemKind::Tool(_)) { 1 } else { e.max_stack.unwrap_or(64).clamp(1, 64) };
            let food = e.food.map(|f| (f, e.saturation.unwrap_or(0.6)));
            reg.push(ItemDef {
                food,
                display: e.display.unwrap_or_else(|| e.name.clone()),
                texture: e.texture.unwrap_or_else(|| e.name.clone()),
                name: e.name,
                kind,
                max_stack,
            })?;
        }
        Ok(reg)
    }

    fn push(&mut self, def: ItemDef) -> Result<(), String> {
        if self.by_name.contains_key(&def.name) {
            return Err(format!("item '{}' is defined twice (blocks are items automatically)", def.name));
        }
        self.by_name.insert(def.name.clone(), ItemId(self.defs.len() as u16));
        self.defs.push(def);
        Ok(())
    }

    pub fn get(&self, id: ItemId) -> &ItemDef {
        &self.defs[id.0 as usize]
    }

    pub fn id(&self, name: &str) -> Option<ItemId> {
        self.by_name.get(name).copied()
    }

    /// The item that places `block`, if any.
    pub fn for_block(&self, block: BlockId) -> Option<ItemId> {
        self.by_block.get(&block).copied()
    }

    pub fn len(&self) -> usize {
        self.defs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.defs.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = (ItemId, &ItemDef)> {
        self.defs.iter().enumerate().map(|(i, d)| (ItemId(i as u16), d))
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub fn registries() -> (BlockRegistry, ItemRegistry) {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/data/");
        let blocks =
            BlockRegistry::from_toml(&std::fs::read_to_string(format!("{dir}blocks.toml")).unwrap()).unwrap();
        let items = ItemRegistry::new(&blocks, &std::fs::read_to_string(format!("{dir}items.toml")).unwrap())
            .unwrap();
        (blocks, items)
    }

    #[test]
    fn blocks_become_items_and_tools_have_stats() {
        let (blocks, items) = registries();
        let stone = items.id("stone").unwrap();
        assert_eq!(items.get(stone).kind, ItemKind::Block(blocks.id("stone").unwrap()));
        assert_eq!(items.for_block(blocks.id("stone").unwrap()), Some(stone));
        assert!(items.id("water").is_none() && items.id("air").is_none());
        let pick = items.get(items.id("iron_pickaxe").unwrap());
        match &pick.kind {
            ItemKind::Tool(t) => {
                assert_eq!((t.kind, t.level, t.durability), (ToolKind::Pickaxe, 2, 250));
            }
            other => panic!("not a tool: {other:?}"),
        }
        assert_eq!(pick.max_stack, 1);
        assert_eq!(items.get(items.id("stick").unwrap()).max_stack, 64);
    }

    #[test]
    fn every_item_sprite_exists() {
        let (_, items) = registries();
        for (_, d) in items.iter() {
            if !matches!(d.kind, ItemKind::Block(_)) {
                let p =
                    format!("{}/../../assets/textures/item/{}.png", env!("CARGO_MANIFEST_DIR"), d.texture);
                assert!(std::path::Path::new(&p).is_file(), "missing sprite {p}");
            }
        }
    }
}
