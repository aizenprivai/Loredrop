//! Looking at, breaking, placing and picking blocks; hotbar selection.
//!
//! Timings follow Minecraft: in Creative, holding the attack button breaks a
//! block every 5 ticks (¼ s) and holding use places one every 4 ticks; the
//! first press always acts immediately. Reach is 5 blocks in Creative and
//! 4.5 in Survival.

use crate::game::{Game, GameMode};
use glam::DVec3;
use ld_core::coords::BlockPos;
use ld_game::mining::break_progress_per_tick;
use ld_game::{ItemKind, ItemStack, ToolInfo, ToolKind};
use ld_input::{Action, InputState};
use ld_physics::{Aabb, raycast};
use ld_world::{BlockId, Shape};

const BREAK_REPEAT_TICKS: u32 = 5;
const PLACE_REPEAT_TICKS: u32 = 4;

impl Game {
    /// How far the player can reach, in blocks.
    pub fn reach(&self) -> f64 {
        match self.mode {
            GameMode::Creative => 5.0,
            GameMode::Survival => 4.5,
        }
    }

    /// Unit vector of the current look direction (Minecraft yaw/pitch).
    pub fn look_dir(&self) -> DVec3 {
        let (ys, yc) = (self.yaw as f64).to_radians().sin_cos();
        let (ps, pc) = (self.pitch as f64).to_radians().sin_cos();
        DVec3::new(-ys * pc, -ps, yc * pc)
    }

    /// True for blocks the crosshair can select (not air, not liquids).
    fn targetable(&self, id: BlockId) -> bool {
        id != BlockId::AIR && Some(id) != self.ids.water && Some(id) != self.ids.lava
    }

    /// Recomputes which block the crosshair points at.
    pub fn update_target(&mut self) {
        let eye = self.player.pos + DVec3::new(0.0, self.eye, 0.0);
        let map = &self.world.map;
        self.target = raycast(eye, self.look_dir(), self.reach(), |p| {
            map.block(p).is_some_and(|id| {
                id != BlockId::AIR && Some(id) != self.ids.water && Some(id) != self.ids.lava
            })
        });
    }

    /// Per-tick handling of hotbar, break, place and pick input.
    pub(crate) fn interact(&mut self, input: &mut InputState) {
        for &a in Action::ALL {
            if let Some(slot) = a.hotbar_slot()
                && input.take_press(a)
            {
                self.inventory.selected = slot;
            }
        }
        self.inventory.scroll(input.take_scroll());
        self.update_target();

        self.attack_cooldown = self.attack_cooldown.saturating_sub(1);
        self.use_cooldown = self.use_cooldown.saturating_sub(1);
        let attack_pressed = input.take_press(Action::Attack);
        match self.mode {
            GameMode::Creative => {
                self.breaking = None;
                if (attack_pressed || (input.is_held(Action::Attack) && self.attack_cooldown == 0))
                    && self.break_target()
                {
                    self.attack_cooldown = BREAK_REPEAT_TICKS;
                }
            }
            GameMode::Survival => self.survival_mine(attack_pressed || input.is_held(Action::Attack)),
        }
        if input.take_press(Action::DropItem) {
            self.drop_held(input.is_held(Action::Sprint)); // Ctrl+Q drops the whole stack
        }
        let use_pressed = input.take_press(Action::UseItem);
        // Using a block (crafting table…) takes priority over placing, unless
        // sneaking — exactly like Minecraft.
        let used = use_pressed && !input.is_held(Action::Sneak) && self.use_target_block();
        // Only place if the click wasn't used on a block (short-circuit order matters).
        let placed = !used
            && (use_pressed || (input.is_held(Action::UseItem) && self.use_cooldown == 0))
            && self.place_held();
        if used || placed {
            self.use_cooldown = PLACE_REPEAT_TICKS;
        }
        if input.take_press(Action::PickBlock) {
            self.pick_block();
        }
    }

    /// Right-click on an interactive block. Returns true if it did something.
    fn use_target_block(&mut self) -> bool {
        let Some(hit) = self.target else { return false };
        let id = self.world.map.block(hit.pos);
        if id.is_some() && id == self.ids.crafting_table {
            self.open_request = Some(crate::screens::Screen::CraftingTable);
            return true;
        }
        if id.is_some() && id == self.ids.lore_tablet {
            let (era, index) = self.tablet_fragment(hit.pos);
            if !self.journal.iter().any(|(e, i)| *e == era && *i == index) {
                self.journal.push((era.clone(), index));
            }
            self.open_request = Some(crate::screens::Screen::Tablet { era, index });
            return true;
        }
        if id.is_some() && (id == self.ids.furnace || id == self.ids.lit_furnace) {
            self.furnaces.entry(hit.pos).or_default();
            self.open_request = Some(crate::screens::Screen::Furnace(hit.pos));
            return true;
        }
        false
    }

    /// Which story fragment a tablet holds. The era comes from the ruin it
    /// stands in (recognised by its pedestal); the fragment within the era is
    /// derived from the tablet's position, so it never changes.
    fn tablet_fragment(&self, pos: BlockPos) -> (String, usize) {
        let below = self.world.map.block(BlockPos::new(pos.x, pos.y - 1, pos.z));
        let era = if below.is_some() && below == self.ids.deepslate_bricks {
            "deep"
        } else if (below.is_some() && below == self.ids.chiseled_sandstone) || pos.y < ld_worldgen::SEA_LEVEL
        {
            "expansion" // desert halls and (underwater) sunken towers
        } else {
            "golden"
        };
        let count = self.lore.era(era).len();
        (era.to_string(), ld_worldgen::structures::tablet_fragment(pos.x, pos.y, pos.z, count))
    }

    /// Removes the targeted block (Creative: no drops). Returns true if broken.
    pub fn break_target(&mut self) -> bool {
        let Some(hit) = self.target else { return false };
        self.remove_block(hit.pos, false);
        true
    }

    /// Removes a block; plants standing on it lose their support and pop
    /// off too. With `drop_items`, popped plants drop as items (Survival).
    fn remove_block(&mut self, pos: BlockPos, drop_items: bool) {
        // A furnace spills its contents when broken (in any mode).
        if let Some(f) = self.furnaces.remove(&pos) {
            for s in f.contents() {
                self.entities.drop_from_block(pos.x, pos.y, pos.z, s);
            }
        }
        self.world.set_block(pos, BlockId::AIR);
        let above = BlockPos::new(pos.x, pos.y + 1, pos.z);
        if let Some(id) = self.world.map.block(above)
            && self.world.registry.get(id).shape == Shape::Cross
        {
            self.world.set_block(above, BlockId::AIR);
            if drop_items {
                self.spawn_drops(above, id, None);
            }
        }
        self.update_target();
    }

    fn next_random(&mut self) -> f32 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        (self.rng >> 40) as f32 / (1u64 << 24) as f32
    }

    /// Spawns the items a block drops when mined with `tool`.
    fn spawn_drops(&mut self, pos: BlockPos, id: BlockId, tool: Option<&ToolInfo>) {
        let drops = self.drops.clone();
        let def = self.world.registry.get(id).clone();
        let stacks = drops.drops(id, &def, tool, || self.next_random());
        for s in stacks {
            self.entities.drop_from_block(pos.x, pos.y, pos.z, s);
        }
    }

    /// The tool in hand, if the held item is one.
    fn held_tool(&self) -> Option<ToolInfo> {
        match self.inventory.held().map(|s| &self.items.get(s.item).kind) {
            Some(ItemKind::Tool(t)) => Some(t.clone()),
            _ => None,
        }
    }

    /// Survival mining: hold attack to make progress at Minecraft's rate;
    /// the block breaks (and drops) when progress reaches 1.
    fn survival_mine(&mut self, attacking: bool) {
        if self.break_delay > 0 {
            self.break_delay -= 1;
            self.breaking = None;
            return;
        }
        let (Some(hit), true) = (self.target, attacking) else {
            self.breaking = None;
            return;
        };
        let Some(id) = self.world.map.block(hit.pos) else { return };
        let def = self.world.registry.get(id).clone();
        let tool = self.held_tool();
        let Some(per_tick) = break_progress_per_tick(&def, tool.as_ref(), self.player.on_ground) else {
            self.breaking = None; // unbreakable
            return;
        };
        let progress = match self.breaking {
            Some((pos, p)) if pos == hit.pos => p + per_tick,
            _ => per_tick,
        };
        if progress < 1.0 {
            self.breaking = Some((hit.pos, progress));
            return;
        }
        // Broken!
        self.breaking = None;
        if per_tick < 1.0 {
            self.break_delay = 5; // Minecraft's pause between blocks (not for instant ones)
        }
        self.spawn_drops(hit.pos, id, tool.as_ref());
        self.remove_block(hit.pos, true);
        // Tools wear out: 1 durability per block (2 for swords), not for instant blocks.
        if let Some(t) = tool
            && def.hardness > 0.0
            && let Some(stack) = self.inventory.held_mut()
        {
            stack.damage += if t.kind == ToolKind::Sword { 2 } else { 1 };
            if stack.damage >= t.durability {
                *self.inventory.held_mut() = None; // the tool breaks
            }
        }
    }

    /// Q: throw one held item (or the whole stack) in front of the player.
    fn drop_held(&mut self, whole_stack: bool) {
        let Some(mut stack) = self.inventory.held() else { return };
        let n = if whole_stack { stack.count } else { 1 };
        stack.count = n;
        for _ in 0..n {
            self.inventory.consume_held();
        }
        let eye = self.player.pos + DVec3::new(0.0, self.eye, 0.0);
        let look = self.look_dir();
        self.entities.throw(eye, look, stack);
    }

    /// Places the held block against the targeted face. Returns true if placed.
    pub fn place_held(&mut self) -> bool {
        let (Some(hit), Some(stack)) = (self.target, self.inventory.held()) else { return false };
        let ItemKind::Block(block) = self.items.get(stack.item).kind else { return false };
        let reg = &self.world.registry;
        // Clicking a replaceable block (tall grass) replaces it instead of placing next to it.
        let target_id = self.world.map.block(hit.pos).unwrap_or(BlockId::AIR);
        let pos = if reg.get(target_id).replaceable { hit.pos } else { hit.adjacent() };
        let Some(current) = self.world.map.block(pos) else { return false };
        if !reg.get(current).replaceable {
            return false;
        }
        let def = reg.get(block);
        // Don't place a solid block inside the player.
        if def.solid && Aabb::block(pos.x, pos.y, pos.z).intersects(&self.player.aabb()) {
            return false;
        }
        // Plants need solid ground under them.
        if def.shape == Shape::Cross {
            let below = self.world.map.block(BlockPos::new(pos.x, pos.y - 1, pos.z)).unwrap_or(BlockId::AIR);
            if !reg.get(below).is_opaque() {
                return false;
            }
        }
        self.world.set_block(pos, block);
        if self.mode == GameMode::Survival {
            self.inventory.consume_held();
        }
        self.update_target();
        true
    }

    /// Middle click: put the targeted block's item in hand (Creative: from
    /// nowhere; Survival: only if it's already in the hotbar).
    pub fn pick_block(&mut self) {
        let Some(hit) = self.target else { return };
        let Some(id) = self.world.map.block(hit.pos) else { return };
        if !self.targetable(id) {
            return;
        }
        let Some(item) = self.items.for_block(id) else { return };
        if let Some(slot) = (0..9).find(|&i| self.inventory.slots[i].is_some_and(|s| s.item == item)) {
            self.inventory.selected = slot;
        } else if self.mode == GameMode::Creative {
            // Prefer an empty hotbar slot; otherwise replace the held item.
            if let Some(empty) = (0..9).find(|&i| self.inventory.slots[i].is_none()) {
                self.inventory.selected = empty;
            }
            *self.inventory.held_mut() = Some(ItemStack::new(item, 1));
        }
    }
}
