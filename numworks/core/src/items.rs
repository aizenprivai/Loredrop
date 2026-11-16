//! Items: blocks you carry, tools, materials and food, plus mining speed,
//! harvesting and drops (which depend on the tool held).
//!
//! The item table, tool tiers, recipes, fuels and drops are generated from
//! the PC game's data files into `gen_items.rs`. Item ids below
//! `BLOCK_COUNT` are the blocks themselves (item 3 = block 3 = dirt), so
//! placing a block item needs no lookup.

use crate::blocks::{self, BLOCK_COUNT, Block, Tool};
pub use crate::gen_items::*;

/// An item type id (index into [`ITEMS`]). `0` = nothing.
pub type Item = u16;
/// No item.
pub const NONE: Item = 0;
/// In recipes, an ingredient with this bit set is a tag (item group) index.
pub const TAG_FLAG: u16 = 0x8000;

/// A tool tier (wooden, stone...), with Minecraft's numbers.
#[derive(Clone, Copy, Debug)]
pub struct TierDef {
    /// Tier name.
    pub name: &'static str,
    /// Harvest level (0 wood/gold, 1 stone, 2 iron, 3 diamond).
    pub level: u8,
    /// Mining speed multiplier on the right blocks.
    pub speed: f32,
    /// Uses before the tool breaks.
    pub durability: u16,
}

/// What an item is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ItemKind {
    /// A placeable block (the item id is the block id).
    Block,
    /// A tool of some tier (index into [`TIERS`]).
    Tool {
        /// Tool kind.
        tool: Tool,
        /// Tier index.
        tier: u8,
        /// Attack damage.
        damage: f32,
        /// Full-strength attacks per second (Minecraft 1.9+ attack cooldown).
        speed: f32,
    },
    /// Something to eat.
    Food {
        /// Hunger points restored.
        food: u8,
        /// Saturation modifier (saturation gained = food x modifier x 2).
        saturation: f32,
    },
    /// Armour worn in one of four slots.
    Armor {
        /// 0 helmet, 1 chestplate, 2 leggings, 3 boots.
        slot: u8,
        /// Armour points (half a chestplate icon each, as on PC).
        defense: u8,
        /// Armour toughness (reduces strong hits less).
        toughness: f32,
        /// Hits it can take.
        durability: u16,
    },
    /// A crafting material.
    Material,
}

/// Properties of one item type.
#[derive(Clone, Copy, Debug)]
pub struct ItemDef {
    /// Name shown to the player.
    pub name: &'static str,
    /// How many fit in one slot.
    pub max_stack: u8,
    /// Texture used as its icon (into `TEXTURES`).
    pub icon: u8,
    /// What it is.
    pub kind: ItemKind,
    /// Listed in the creative inventory.
    pub creative: bool,
}

/// One possible drop of a block.
#[derive(Clone, Copy, Debug)]
pub struct Drop {
    /// Item dropped.
    pub item: Item,
    /// Smallest count.
    pub min: u8,
    /// Largest count.
    pub max: u8,
    /// Chance (0..1) that it drops at all.
    pub chance: f32,
}

/// A crafting recipe with a fixed pattern (`w` x `h`, row by row in `cells`).
#[derive(Clone, Copy, Debug)]
pub struct Shaped {
    /// Pattern width.
    pub w: u8,
    /// Pattern height.
    pub h: u8,
    /// Ingredients (0 = empty cell; see [`TAG_FLAG`]).
    pub cells: [u16; 9],
    /// Item made.
    pub result: Item,
    /// How many are made.
    pub count: u8,
}

/// A crafting recipe whose ingredients can go anywhere in the grid.
#[derive(Clone, Copy, Debug)]
pub struct Shapeless {
    /// Ingredients (see [`TAG_FLAG`]).
    pub ingredients: &'static [u16],
    /// Item made.
    pub result: Item,
    /// How many are made.
    pub count: u8,
}

/// Some number of one item in a slot. `wear` counts uses of a tool.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Stack {
    /// Item (`NONE` = empty slot).
    pub item: Item,
    /// How many.
    pub count: u8,
    /// Tool wear (uses so far).
    pub wear: u16,
}

impl Stack {
    /// An empty slot.
    pub const EMPTY: Stack = Stack { item: NONE, count: 0, wear: 0 };

    /// `count` of `item`.
    pub const fn new(item: Item, count: u8) -> Stack {
        Stack { item, count, wear: 0 }
    }

    /// Whether the slot is empty.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.item == NONE || self.count == 0
    }

    /// Takes `n` items away, emptying the slot when none are left.
    pub fn take(&mut self, n: u8) {
        self.count = self.count.saturating_sub(n);
        if self.count == 0 {
            *self = Stack::EMPTY;
        }
    }
}

/// Properties of item `i`.
#[inline]
pub fn item_def(i: Item) -> &'static ItemDef {
    &ITEMS[(i as usize).min(ITEM_COUNT - 1)]
}

/// The block an item places, if it is a block.
#[inline]
pub fn block_of(i: Item) -> Option<Block> {
    if i != NONE && (i as usize) < BLOCK_COUNT { Some(i as Block) } else { None }
}

/// The tool kind and tier of an item, if it is a tool.
pub fn tool_of(i: Item) -> Option<(Tool, &'static TierDef)> {
    match item_def(i).kind {
        ItemKind::Tool { tool, tier, .. } => Some((tool, &TIERS[tier as usize])),
        _ => None,
    }
}

/// How many uses a tool has in total (0 for non-tools).
pub fn durability(i: Item) -> u16 {
    match item_def(i).kind {
        ItemKind::Armor { durability, .. } => durability,
        _ => tool_of(i).map_or(0, |(_, t)| t.durability),
    }
}

/// Attack damage of a held item (1 for bare hands and non-weapons, as on PC).
pub fn attack_damage(i: Item) -> f32 {
    match item_def(i).kind {
        ItemKind::Tool { damage, .. } => damage,
        _ => 1.0,
    }
}

/// Full-strength attacks per second with a held item (bare hands: 4).
pub fn attack_speed(i: Item) -> f32 {
    match item_def(i).kind {
        ItemKind::Tool { speed, .. } => speed,
        _ => 4.0,
    }
}

/// Damage left after armour, with Minecraft's formula: each armour point
/// blocks 4 %, less against strong hits unless the armour is tough.
pub fn armor_reduce(damage: f32, defense: f32, toughness: f32) -> f32 {
    let eff = (defense - damage / (2.0 + toughness / 4.0)).max(defense / 5.0).min(20.0);
    damage * (1.0 - eff / 25.0)
}

/// Whether recipe ingredient `ing` accepts item `i`.
pub fn matches(ing: u16, i: Item) -> bool {
    if ing & TAG_FLAG != 0 { TAGS[(ing & !TAG_FLAG) as usize].contains(&i) } else { ing == i }
}

/// How long `i` burns in a furnace, in ticks (0 = not a fuel).
pub fn fuel_ticks(i: Item) -> u16 {
    FUELS.iter().find(|(f, _)| *f == i).map_or(0, |(_, t)| *t)
}

/// What a furnace makes from `i`, and how long it takes.
pub fn smelt_result(i: Item) -> Option<(Item, u16)> {
    SMELTING.iter().find(|(ing, _, _)| matches(*ing, i)).map(|(_, r, t)| (*r, *t))
}

/// Whether mining `b` while holding `held` gives its drops.
pub fn can_harvest(b: Block, held: Item) -> bool {
    let d = blocks::def(b);
    if d.harvest_level < 0 {
        return true;
    }
    matches!(tool_of(held), Some((tool, tier)) if tool == d.tool && tier.level as i8 >= d.harvest_level)
}

/// Ticks needed to mine `b` while holding `held`, or `None` if it can't be
/// mined. Minecraft's rule: each tick deals `speed / hardness / 30` damage
/// (`/ 100` when the block can't be harvested this way); the block breaks
/// at 1. `speed` is the tool tier's speed with the right tool, 1 otherwise.
pub fn mining_ticks(b: Block, held: Item) -> Option<u32> {
    let d = blocks::def(b);
    if d.hardness < 0.0 || b == blocks::AIR || blocks::is_liquid(b) {
        return None;
    }
    if d.hardness == 0.0 {
        return Some(1);
    }
    let speed = match tool_of(held) {
        Some((tool, tier)) if tool == d.tool && d.tool != Tool::None => tier.speed,
        _ => 1.0,
    };
    let per_tick = speed / d.hardness / if can_harvest(b, held) { 30.0 } else { 100.0 };
    if per_tick >= 1.0 {
        return Some(1);
    }
    let t = 1.0 / per_tick;
    let ticks = t as u32;
    Some(if (ticks as f32) < t { ticks + 1 } else { ticks })
}

/// Wear a tool takes from breaking `b` (Minecraft: 1, swords 2; nothing for
/// blocks that break instantly).
pub fn wear_for(b: Block, held: Item) -> u16 {
    match tool_of(held) {
        Some(_) if blocks::def(b).hardness == 0.0 => 0,
        Some((Tool::Sword, _)) => 2,
        Some(_) => 1,
        None => 0,
    }
}

/// Small deterministic random number generator (xorshift32).
#[derive(Clone, Copy)]
pub struct Rng(pub u32);

impl Rng {
    /// Next random 32-bit value.
    pub fn next_u32(&mut self) -> u32 {
        let mut x = self.0.max(1);
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        x
    }

    /// Random float in `0..1`.
    pub fn unit(&mut self) -> f32 {
        (self.next_u32() >> 8) as f32 / 16_777_216.0
    }
}

/// Rolls the drops of `b` mined while holding `held`, calling `give(item,
/// count)` for each.
pub fn roll_drops(b: Block, held: Item, rng: &mut Rng, mut give: impl FnMut(Item, u8)) {
    if !can_harvest(b, held) {
        return;
    }
    for d in DROPS[b as usize] {
        if d.chance < 1.0 && rng.unit() >= d.chance {
            continue;
        }
        let n =
            if d.max > d.min { d.min + (rng.next_u32() % (d.max - d.min + 1) as u32) as u8 } else { d.min };
        if n > 0 {
            give(d.item, n);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blocks::*;

    #[test]
    fn minecraft_mining_times() {
        // Dirt by hand: hardness 0.5 → 0.75 s = 15 ticks.
        assert_eq!(mining_ticks(DIRT, NONE), Some(15));
        // Stone by hand: needs a pickaxe → 1.5 × 5 s = 150 ticks, no drop.
        assert_eq!(mining_ticks(STONE, NONE), Some(150));
        assert!(!can_harvest(STONE, NONE));
        // Wooden pickaxe: speed 2 → 1.5 × 1.5 / 2 s = 22.5 → 23 ticks, drops cobblestone.
        assert_eq!(mining_ticks(STONE, WOODEN_PICKAXE), Some(23));
        assert!(can_harvest(STONE, WOODEN_PICKAXE));
        // Iron ore needs stone tier or better.
        assert!(!can_harvest(IRON_ORE, WOODEN_PICKAXE));
        assert!(can_harvest(IRON_ORE, STONE_PICKAXE));
        // Diamond needs iron.
        assert!(!can_harvest(DIAMOND_ORE, STONE_PICKAXE));
        assert!(can_harvest(DIAMOND_ORE, IRON_PICKAXE));
        // Wrong tool doesn't speed up.
        assert_eq!(mining_ticks(DIRT, WOODEN_PICKAXE), Some(15));
        assert_eq!(mining_ticks(BEDROCK, DIAMOND_PICKAXE), None);
        assert_eq!(durability(WOODEN_PICKAXE), 59);
    }

    #[test]
    fn drops() {
        let mut rng = Rng(42);
        let mut got = [0u32; ITEM_COUNT];
        roll_drops(COAL_ORE, WOODEN_PICKAXE, &mut rng, |i, n| got[i as usize] += n as u32);
        assert_eq!(got[COAL as usize], 1);
        let mut got = [0u32; ITEM_COUNT];
        roll_drops(COAL_ORE, NONE, &mut rng, |i, n| got[i as usize] += n as u32);
        assert_eq!(got.iter().sum::<u32>(), 0, "no drop by hand");
        // Lapis gives 4–9.
        for _ in 0..50 {
            let mut n = 0;
            roll_drops(LAPIS_ORE, STONE_PICKAXE, &mut rng, |_, c| n += c);
            assert!((4..=9).contains(&n));
        }
        // Grass gives dirt.
        let mut item = NONE;
        roll_drops(GRASS_BLOCK, NONE, &mut rng, |i, _| item = i);
        assert_eq!(item, DIRT as Item);
    }

    #[test]
    fn fuels_and_smelting() {
        assert_eq!(fuel_ticks(COAL), 1600);
        assert_eq!(fuel_ticks(DIRT as Item), 0);
        assert_eq!(smelt_result(RAW_IRON), Some((IRON_INGOT, 200)));
        assert_eq!(smelt_result(OAK_LOG as Item).map(|r| r.0), Some(CHARCOAL));
        assert_eq!(block_of(STONE as Item), Some(STONE));
        assert_eq!(block_of(STICK), None);
    }
}
