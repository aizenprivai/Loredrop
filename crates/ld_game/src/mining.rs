//! Mining rules: how long a block takes to break and what it drops.
//!
//! Follows Minecraft Java's formula so mining feels identical:
//!
//! ```text
//! speed  = tool's tier speed if it's the right tool for the block, else 1
//! speed /= 5 if not standing on the ground (and later: if under water)
//! progress per tick = speed / hardness / (30 if harvestable else 100)
//! ```
//! The block breaks when progress reaches 1. Hardness 0 breaks instantly;
//! negative hardness (bedrock, lore tablets) never breaks in Survival.
//! "Harvestable" means the block will drop something: blocks with a
//! `harvest_level` need the right tool of at least that tier (an iron ore
//! needs a stone pickaxe or better).

use crate::inventory::ItemStack;
use crate::items::{ItemId, ItemRegistry, ToolInfo, ToolKind};
use ld_world::{BlockDef, BlockId, BlockRegistry};

/// One possible drop of a block.
#[derive(Debug, Clone, PartialEq)]
pub enum Drop {
    Nothing,
    /// `item`, a random count in `min..=max`, with probability `chance`.
    Item {
        item: ItemId,
        min: u16,
        max: u16,
        chance: f32,
    },
}

/// Resolved drop for every block, indexed by block ID.
#[derive(Debug, Clone)]
pub struct DropTable {
    /// Every possible drop of each block (each rolled independently).
    drops: Vec<Vec<Drop>>,
}

fn tool_name(kind: ToolKind) -> &'static str {
    match kind {
        ToolKind::Pickaxe => "pickaxe",
        ToolKind::Axe => "axe",
        ToolKind::Shovel => "shovel",
        ToolKind::Hoe => "hoe",
        ToolKind::Sword => "sword",
    }
}

/// Parses `"none"`, `"item"`, `"item:2-5"` or `"item@0.02"`.
fn parse_drop(spec: &str, items: &ItemRegistry) -> Result<Drop, String> {
    if spec == "none" {
        return Ok(Drop::Nothing);
    }
    let (rest, chance) = match spec.split_once('@') {
        Some((r, c)) => (r, c.parse::<f32>().map_err(|_| format!("bad drop chance in '{spec}'"))?),
        None => (spec, 1.0),
    };
    let (name, min, max) = match rest.split_once(':') {
        Some((n, range)) => {
            let (a, b) =
                range.split_once('-').ok_or_else(|| format!("bad drop count in '{spec}' (use min-max)"))?;
            (
                n,
                a.parse().map_err(|_| format!("bad drop count in '{spec}'"))?,
                b.parse().map_err(|_| format!("bad drop count in '{spec}'"))?,
            )
        }
        None => (rest, 1, 1),
    };
    let item = items.id(name).ok_or_else(|| format!("drop '{spec}': unknown item '{name}'"))?;
    Ok(Drop::Item { item, min, max, chance })
}

impl DropTable {
    pub fn new(blocks: &BlockRegistry, items: &ItemRegistry) -> Result<Self, String> {
        let mut drops = Vec::with_capacity(blocks.len());
        for (id, b) in blocks.iter() {
            let d = match &b.drops {
                Some(spec) => spec
                    .split(',')
                    .map(|part| parse_drop(part.trim(), items))
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|e| format!("block '{}': {e}", b.name))?,
                None => match items.for_block(id) {
                    Some(item) => vec![Drop::Item { item, min: 1, max: 1, chance: 1.0 }],
                    None => vec![Drop::Nothing],
                },
            };
            drops.push(d);
        }
        Ok(Self { drops })
    }

    /// Items dropped when `block` is mined with `tool`. `roll` is a source
    /// of random numbers in 0..1 (for counts and chances).
    pub fn drops(
        &self,
        id: BlockId,
        block: &BlockDef,
        tool: Option<&ToolInfo>,
        mut roll: impl FnMut() -> f32,
    ) -> Vec<ItemStack> {
        if !can_harvest(block, tool) {
            return Vec::new();
        }
        let mut out = Vec::new();
        for d in &self.drops[id.0 as usize] {
            if let Drop::Item { item, min, max, chance } = d {
                if roll() >= *chance {
                    continue;
                }
                let n = min + ((roll() * (max - min + 1) as f32) as u16).min(max - min);
                if n > 0 {
                    out.push(ItemStack::new(*item, n));
                }
            }
        }
        out
    }
}

/// Whether `tool` is the kind of tool `block` wants.
pub fn is_effective(block: &BlockDef, tool: Option<&ToolInfo>) -> bool {
    match (tool, &block.tool) {
        (Some(t), Some(wanted)) => tool_name(t.kind) == wanted,
        _ => false,
    }
}

/// Whether mining `block` with `tool` gives drops.
pub fn can_harvest(block: &BlockDef, tool: Option<&ToolInfo>) -> bool {
    match block.harvest_level {
        None => true,
        Some(level) => is_effective(block, tool) && tool.is_some_and(|t| t.level >= level),
    }
}

/// Fraction of the block broken per tick (≥ 1 = instant), or `None` if the
/// block can't be broken at all.
pub fn break_progress_per_tick(block: &BlockDef, tool: Option<&ToolInfo>, on_ground: bool) -> Option<f32> {
    if block.hardness < 0.0 {
        return None;
    }
    if block.hardness == 0.0 {
        return Some(1.0);
    }
    let mut speed = if is_effective(block, tool) { tool.map(|t| t.speed).unwrap_or(1.0) } else { 1.0 };
    if !on_ground {
        speed /= 5.0;
    }
    let divisor = if can_harvest(block, tool) { 30.0 } else { 100.0 };
    Some(speed / block.hardness / divisor)
}

/// Ticks needed to break (rounded up), for tests and the debug overlay.
pub fn break_ticks(block: &BlockDef, tool: Option<&ToolInfo>, on_ground: bool) -> Option<u32> {
    break_progress_per_tick(block, tool, on_ground).map(|p| (1.0 / p).ceil() as u32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::items::ItemKind;
    use crate::items::tests::registries;

    fn tool<'a>(items: &'a ItemRegistry, name: &str) -> &'a ToolInfo {
        match &items.get(items.id(name).unwrap()).kind {
            ItemKind::Tool(t) => t,
            _ => panic!(),
        }
    }

    #[test]
    fn break_times_match_minecraft() {
        let (blocks, items) = registries();
        let b = |n: &str| blocks.get(blocks.id(n).unwrap());
        // Minecraft wiki: stone by hand 7.5 s (150 ticks), wooden pickaxe 1.15 s, stone pickaxe 0.6 s.
        assert_eq!(break_ticks(b("stone"), None, true), Some(150));
        assert_eq!(break_ticks(b("stone"), Some(tool(&items, "wooden_pickaxe")), true), Some(23));
        assert_eq!(break_ticks(b("stone"), Some(tool(&items, "stone_pickaxe")), true), Some(12));
        // Dirt by hand: 0.75 s (15 ticks); logs by hand 3 s (60 ticks).
        assert_eq!(break_ticks(b("dirt"), None, true), Some(15));
        assert_eq!(break_ticks(b("oak_log"), None, true), Some(60));
        // In the air everything is 5× slower.
        assert_eq!(break_ticks(b("dirt"), None, false), Some(75));
        assert_eq!(break_ticks(b("bedrock"), Some(tool(&items, "diamond_pickaxe")), true), None);
        assert_eq!(break_ticks(b("short_grass"), None, true), Some(1));
    }

    #[test]
    fn harvest_levels_and_drops() {
        let (blocks, items) = registries();
        let table = DropTable::new(&blocks, &items).unwrap();
        let id = |n: &str| blocks.id(n).unwrap();
        let drop = |block: &str, t: Option<&str>| {
            let tool = t.map(|n| tool(&items, n));
            table.drops(id(block), blocks.get(id(block)), tool, || 0.5)
        };
        assert!(drop("stone", None).is_empty(), "stone by hand drops nothing");
        assert_eq!(drop("stone", Some("wooden_pickaxe"))[0].item, items.id("cobblestone").unwrap());
        assert!(drop("iron_ore", Some("wooden_pickaxe")).is_empty(), "iron needs stone tier");
        assert_eq!(drop("iron_ore", Some("stone_pickaxe"))[0].item, items.id("raw_iron").unwrap());
        assert!(drop("diamond_ore", Some("stone_pickaxe")).is_empty());
        assert_eq!(drop("diamond_ore", Some("iron_pickaxe"))[0].item, items.id("diamond").unwrap());
        assert_eq!(drop("grass_block", None)[0].item, items.id("dirt").unwrap());
        assert!(drop("glass", None).is_empty(), "glass shatters");
        let lapis = drop("lapis_ore", Some("stone_pickaxe"))[0];
        assert!((4..=9).contains(&lapis.count));
        assert_eq!(drop("oak_log", None)[0].item, items.id("oak_log").unwrap(), "logs drop by hand");
        // Leaves: rare sticks and apples, each rolled separately.
        let leaves = id("oak_leaves");
        let lucky = table.drops(leaves, blocks.get(leaves), None, || 0.001);
        assert_eq!(
            lucky.iter().map(|s| s.item).collect::<Vec<_>>(),
            [items.id("stick").unwrap(), items.id("apple").unwrap()]
        );
        assert!(table.drops(leaves, blocks.get(leaves), None, || 0.5).is_empty());
    }
}
