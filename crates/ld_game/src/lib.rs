//! # ld_game — gameplay rules
//!
//! Pure game logic with no rendering or windowing, so it is fully testable:
//! * [`items`] — every item (blocks, materials, tools), from `items.toml`.
//! * [`inventory`] — item stacks, the player inventory and Minecraft's
//!   click rules for moving items around.
//!
//! * [`crafting`] — crafting and smelting recipes, fuels.
//!
//! * [`furnace`] — smelting with fuel, Minecraft's timing.
//! * [`mining`] — break times (Minecraft's formula), harvest tiers, drops.

pub mod crafting;
pub mod furnace;
pub mod inventory;
pub mod items;
pub mod mining;

pub use crafting::RecipeBook;
pub use inventory::{Click, Inventory, ItemStack};
pub use items::{ItemDef, ItemId, ItemKind, ItemRegistry, ToolInfo, ToolKind};
