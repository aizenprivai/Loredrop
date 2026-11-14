//! Looking at, breaking, placing and picking blocks; hotbar selection.
//!
//! Timings follow Minecraft: in Creative, holding the attack button breaks a
//! block every 5 ticks (¼ s) and holding use places one every 4 ticks; the
//! first press always acts immediately. Reach is 5 blocks in Creative and
//! 4.5 in Survival.

use crate::game::{Game, GameMode};
use glam::DVec3;
use ld_core::coords::BlockPos;
use ld_game::{ItemKind, ItemStack};
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
        if (attack_pressed || (input.is_held(Action::Attack) && self.attack_cooldown == 0))
            && self.mode == GameMode::Creative
            && self.break_target()
        {
            self.attack_cooldown = BREAK_REPEAT_TICKS;
        }
        let use_pressed = input.take_press(Action::UseItem);
        if (use_pressed || (input.is_held(Action::UseItem) && self.use_cooldown == 0)) && self.place_held() {
            self.use_cooldown = PLACE_REPEAT_TICKS;
        }
        if input.take_press(Action::PickBlock) {
            self.pick_block();
        }
    }

    /// Removes the targeted block. Returns true if something was broken.
    pub fn break_target(&mut self) -> bool {
        let Some(hit) = self.target else { return false };
        self.world.set_block(hit.pos, BlockId::AIR);
        // Plants standing on the removed block lose their support and pop off.
        let above = BlockPos::new(hit.pos.x, hit.pos.y + 1, hit.pos.z);
        if let Some(id) = self.world.map.block(above)
            && self.world.registry.get(id).shape == Shape::Cross
        {
            self.world.set_block(above, BlockId::AIR);
        }
        self.update_target();
        true
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
