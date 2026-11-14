//! # ld_game — gameplay rules
//!
//! Pure game logic with no rendering or windowing, so it is fully testable:
//! * [`items`] — every item (blocks, materials, tools), from `items.toml`.
//! * [`inventory`] — item stacks, the player inventory and Minecraft's
//!   click rules for moving items around.
//!
//! Later in Phase 5: crafting recipes, mining rules (hardness, tool tiers,
//! drops) and smelting.

pub mod inventory;
pub mod items;

pub use inventory::{Click, Inventory, ItemStack};
pub use items::{ItemDef, ItemId, ItemKind, ItemRegistry, ToolInfo, ToolKind};
