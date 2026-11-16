//! Block types.
//!
//! The list of blocks, their properties and their textures live in
//! `gen_blocks.rs`, generated from the PC game's `assets/data/blocks.toml`, so
//! both editions share the same names, hardness values and drops.

pub use crate::gen_blocks::*;

/// A block type id (index into [`BLOCKS`]). `0` is air.
pub type Block = u8;

/// Texel colour that means "see-through" in cutout textures (leaves, glass).
pub const TRANSPARENT: u16 = 0xF81F;

/// How a block is drawn.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Layer {
    /// Never drawn.
    Air,
    /// Solid cube that hides what's behind it.
    Opaque,
    /// Cube whose texture has see-through texels (leaves, glass).
    Cutout,
    /// Water: tints what is seen through it.
    Water,
    /// Lava: opaque and always fully lit.
    Lava,
}

/// The kind of tool that mines a block fast.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tool {
    /// Any tool or bare hands.
    None,
    /// Pickaxe.
    Pickaxe,
    /// Axe.
    Axe,
    /// Shovel.
    Shovel,
    /// Hoe.
    Hoe,
}

/// Properties of one block type.
#[derive(Clone, Copy, Debug)]
pub struct BlockDef {
    /// Name shown to the player.
    pub name: &'static str,
    /// How the block is drawn.
    pub layer: Layer,
    /// Whether players collide with it.
    pub solid: bool,
    /// Texture index for the top, sides and bottom (into [`TEXTURES`]).
    pub tex: [u8; 3],
    /// Mining difficulty on the Minecraft scale; negative = unbreakable.
    pub hardness: f32,
    /// The tool that mines it fast.
    pub tool: Tool,
    /// Whether it only drops something when mined with the right tool.
    pub needs_tool: bool,
    /// The block given when it is broken (`AIR` = nothing).
    pub drop: Block,
    /// Light the block gives off, 0–15.
    pub light: u8,
}

/// Properties of block `b`.
#[inline]
pub fn def(b: Block) -> &'static BlockDef {
    &BLOCKS[b as usize]
}

/// Whether `b` blocks the view completely (and so casts shadow from the sky).
#[inline]
pub fn is_opaque(b: Block) -> bool {
    matches!(def(b).layer, Layer::Opaque | Layer::Lava)
}

/// Whether a player can place another block into a cell holding `b`.
#[inline]
pub fn is_replaceable(b: Block) -> bool {
    matches!(def(b).layer, Layer::Air | Layer::Water | Layer::Lava)
}

/// Game ticks (1/20 s) needed to mine `b` with bare hands, or `None` if it
/// cannot be mined. Same rule as Minecraft: 1.5 s per hardness point when the
/// block can be harvested by hand, 5 s per point when it needs a tool.
pub fn hand_mining_ticks(b: Block) -> Option<u32> {
    let d = def(b);
    if d.hardness < 0.0 || b == AIR || matches!(d.layer, Layer::Water | Layer::Lava) {
        return None;
    }
    let per_point = if d.needs_tool { 100.0 } else { 30.0 };
    let t = d.hardness * per_point;
    let ticks = t as u32;
    Some(if (ticks as f32) < t { ticks + 1 } else { ticks })
}

/// What mining `b` with bare hands gives (`AIR` = nothing).
pub fn hand_drop(b: Block) -> Block {
    let d = def(b);
    if d.needs_tool { AIR } else { d.drop }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minecraft_mining_times() {
        // Dirt: hardness 0.5 → 0.75 s = 15 ticks by hand.
        assert_eq!(hand_mining_ticks(DIRT), Some(15));
        // Stone needs a pickaxe: 1.5 × 5 s = 7.5 s = 150 ticks, and drops nothing.
        assert_eq!(hand_mining_ticks(STONE), Some(150));
        assert_eq!(hand_drop(STONE), AIR);
        assert_eq!(hand_mining_ticks(BEDROCK), None);
        assert_eq!(hand_drop(GRASS_BLOCK), DIRT);
        assert_eq!(hand_drop(OAK_LOG), OAK_LOG);
    }

    #[test]
    fn table_is_consistent() {
        assert_eq!(BLOCKS.len(), BLOCK_COUNT);
        assert_eq!(def(AIR).layer, Layer::Air);
        for d in BLOCKS.iter() {
            for t in d.tex {
                assert!((t as usize) < TEXTURE_COUNT);
            }
        }
        assert_eq!(def(WATER).layer, Layer::Water);
        assert!(!def(WATER).solid);
        assert_eq!(def(OAK_LEAVES).layer, Layer::Cutout);
    }
}
