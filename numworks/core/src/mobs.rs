//! Creature kinds and lore text, generated from the PC game's `mobs.toml`
//! and `lore.toml` into `gen_mobs.rs` (same numbers, same box models, same
//! textures as on PC). The creatures' behaviour is in `entities`.

pub use crate::gen_mobs::*;
use crate::items::{Drop, Item};
use crate::worldgen::Biome;

/// How a creature treats the player.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Category {
    /// Never attacks; runs away when hurt.
    Passive,
    /// Attacks the player on sight.
    Hostile,
    /// Ignores the player unless provoked (the Hollow).
    Neutral,
}

/// How a model part moves when the creature walks.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Role {
    /// Doesn't move.
    None,
    /// The head.
    Head,
    /// Swings back and forth with the walk.
    Leg,
    /// Swings opposite to the legs.
    Arm,
    /// Flaps (chicken).
    Wing,
}

/// Which age of the Builders' story a lore fragment belongs to (decides
/// which ruins hold it).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Era {
    /// Surface ruins: the age of building.
    Golden,
    /// Desert halls: the gateways and the burning realm.
    Expansion,
    /// Deep vaults: what they became.
    Deep,
}

/// One box of a creature model, in blocks (x across, y up from the feet,
/// z forward), rotating around `pivot`.
#[derive(Clone, Copy, Debug)]
pub struct Part {
    /// Corner with the smallest coordinates.
    pub min: [f32; 3],
    /// Size.
    pub size: [f32; 3],
    /// Point the part rotates around.
    pub pivot: [f32; 3],
    /// Fixed rotation in radians (x, y, z).
    pub rot: [f32; 3],
    /// How it animates.
    pub role: Role,
    /// Which half of the walk cycle it swings in (0 or 1).
    pub phase: u8,
    /// Texture for every face (into `TEXTURES`).
    pub tex: u8,
    /// Texture for the front (+z) face.
    pub front: u8,
}

/// One creature kind.
#[derive(Clone, Copy, Debug)]
pub struct MobDef {
    /// Name shown to the player.
    pub name: &'static str,
    /// Passive, hostile or neutral.
    pub category: Category,
    /// Health points (2 = one heart).
    pub health: f32,
    /// Movement speed attribute (Minecraft scale).
    pub speed: f32,
    /// Hitbox width.
    pub width: f32,
    /// Hitbox height.
    pub height: f32,
    /// Melee damage (Normal difficulty); for ranged creatures, arrow damage.
    pub attack: f32,
    /// Shoots arrows instead of hitting.
    pub ranged: bool,
    /// Catches fire under open sky by day.
    pub burns_in_daylight: bool,
    /// Won't attack in daylight unless hurt (spider).
    pub calm_in_light: bool,
    /// Speed multiplier when provoked (neutral creatures).
    pub angry_speed: f32,
    /// What it drops when killed.
    pub drops: &'static [Drop],
    /// Blocks it picks up and moves around (the Hollow).
    pub carries: &'static [Item],
    /// Biomes where it lives on the surface (passive creatures).
    pub surface: &'static [Biome],
    /// Appears in the dark (night or caves).
    pub dark: bool,
    /// How many appear together (min, max).
    pub group: (u8, u8),
    /// How often it is picked compared with the others.
    pub weight: u16,
    /// Model boxes.
    pub parts: &'static [Part],
}

/// Properties of creature kind `k`.
#[inline]
pub fn mob_def(k: u8) -> &'static MobDef {
    &MOBS[k as usize]
}
