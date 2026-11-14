//! # ld_game — gameplay rules
//!
//! Pure game logic with no rendering or windowing, so it is fully testable:
//! * [`items`] — every item (blocks, materials, tools), from `items.toml`.
//! * [`inventory`] — item stacks, the player inventory and Minecraft's
//!   click rules for moving items around.
//!
//! * [`combat`] — modern attack cooldown, critical hits, sweeping.
//! * [`crafting`] — crafting and smelting recipes, fuels.
//!
//! * [`furnace`] — smelting with fuel, Minecraft's timing.
//! * [`mining`] — break times (Minecraft's formula), harvest tiers, drops.
//! * [`mobs`] — creature definitions (stats, drops, spawns, box models).
//! * [`stats`] — health, hunger, air, damage and regeneration.
//! * [`time`] — the day/night cycle, sun position, sky colours, daylight.

pub mod combat;
pub mod crafting;
pub mod furnace;
pub mod inventory;
pub mod items;
pub mod mining;
pub mod mobs;
pub mod stats;
pub mod time;

pub use crafting::RecipeBook;
pub use inventory::{Click, Inventory, ItemStack};
pub use items::{ArmorInfo, ArmorSlot, ItemDef, ItemId, ItemKind, ItemRegistry, ToolInfo, ToolKind};
