//! Per-block data the mesher needs, flattened for speed.

use ld_world::{BiomeId, BiomeRegistry, BlockId, BlockRegistry, RenderLayer, Shape, Tint};

/// What the mesher needs to know about one block type.
#[derive(Debug, Clone, Copy)]
pub struct BlockMeshInfo {
    pub layer: RenderLayer,
    /// Hides neighbouring faces completely.
    pub opaque: bool,
    /// Hide faces between two blocks of this same type (glass).
    pub cull_same: bool,
    /// Texture-array layer per face (indexed by `ld_world::Face`).
    pub textures: [u16; 6],
    /// Tint per face: 0 none, 1 grass, 2 foliage, 3 water.
    pub tints: [u8; 6],
    pub shape: Shape,
}

/// Block info indexed by block ID. Built once at startup and shared
/// (read-only) by all meshing threads.
#[derive(Debug, Clone)]
pub struct MeshTable {
    blocks: Vec<BlockMeshInfo>,
    /// Per biome: packed RGB for [grass, foliage, water].
    biome_colors: Vec<[[f32; 3]; 3]>,
}

/// What lies outside the world (below Y −64): treated as solid so the
/// underside of the world isn't meshed.
pub(crate) const BARRIER: BlockId = BlockId(u16::MAX);

const BARRIER_INFO: BlockMeshInfo = BlockMeshInfo {
    layer: RenderLayer::Invisible,
    opaque: true,
    cull_same: false,
    textures: [0; 6],
    tints: [0; 6],
    shape: Shape::Cube,
};

impl MeshTable {
    /// `texture_layer` maps a texture name to its texture-array layer.
    pub fn new(
        registry: &BlockRegistry,
        biomes: Option<&BiomeRegistry>,
        texture_layer: impl Fn(&str) -> u16,
    ) -> Self {
        let blocks = registry
            .iter()
            .map(|(_, d)| BlockMeshInfo {
                layer: d.layer,
                opaque: d.is_opaque(),
                cull_same: d.cull_same,
                textures: std::array::from_fn(|f| texture_layer(&d.textures[f])),
                tints: std::array::from_fn(|f| match d.tints[f] {
                    Tint::None => 0,
                    Tint::Grass => 1,
                    Tint::Foliage => 2,
                    Tint::Water => 3,
                }),
                shape: d.shape,
            })
            .collect();
        let biome_colors = match biomes {
            Some(b) => b.iter().map(|(_, d)| [d.grass_color, d.foliage_color, d.water_color]).collect(),
            // Without biome data (tests), use plains-like defaults.
            None => vec![[[0.57, 0.74, 0.35], [0.47, 0.67, 0.18], [0.25, 0.46, 0.89]]],
        };
        Self { blocks, biome_colors }
    }

    /// sRGB colour (0..1) of tint `kind` (1 grass, 2 foliage, 3 water) in `biome`.
    #[inline]
    pub fn biome_color(&self, biome: BiomeId, kind: u8) -> [f32; 3] {
        let c = self.biome_colors.get(biome.0 as usize).unwrap_or(&self.biome_colors[0]);
        c[(kind as usize).clamp(1, 3) - 1]
    }

    #[inline]
    pub fn get(&self, id: BlockId) -> &BlockMeshInfo {
        self.blocks.get(id.0 as usize).unwrap_or(&BARRIER_INFO)
    }
}
