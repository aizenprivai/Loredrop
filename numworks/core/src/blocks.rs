//! Block types.
//!
//! The list of blocks, their properties and their textures live in
//! `gen_blocks.rs`, generated from the PC game's `assets/data/blocks.toml`, so
//! both editions share the same names, hardness values and harvest rules.
//! Mining speed and drops (which depend on the tool held) are in `items`.

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
    /// Two crossed see-through planes (plants, torches); walked through.
    Cross,
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
    /// Sword.
    Sword,
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
    /// If 0 or more, the block only drops something when mined with `tool`
    /// of at least this tier level (0 wood/gold, 1 stone, 2 iron, 3 diamond).
    pub harvest_level: i8,
    /// Light the block gives off, 0–15.
    pub light: u8,
    /// Placing a block here replaces it (air, liquids, short grass, ferns).
    pub replaceable: bool,
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
    def(b).replaceable || matches!(def(b).layer, Layer::Air | Layer::Water | Layer::Lava)
}

/// Whether `b` is a liquid (water or lava).
#[inline]
pub fn is_liquid(b: Block) -> bool {
    matches!(def(b).layer, Layer::Water | Layer::Lava)
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(def(STONE).harvest_level, 0);
        assert_eq!(def(DIRT).harvest_level, -1);
        assert_eq!(def(DIAMOND_ORE).harvest_level, 2);
    }
}
