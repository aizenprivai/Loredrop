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
        if attack_pressed {
            // A creature in front of the crosshair (and nearer than any block) is hit first.
            let hit_mob = self.attack_creature();
            // Every swing (at a block, a creature or thin air) restarts the
            // attack cooldown, as in Minecraft's 1.9+ combat.
            self.ticks_since_swing = 0;
            if hit_mob {
                return;
            }
        }
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
        let used = used || (use_pressed && self.equip_held_armor());
        let used = used || (use_pressed && self.read_lore_fragment());
        self.update_eating(!used && input.is_held(Action::UseItem));
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

    /// Hits the creature under the crosshair, if any, using the 1.9+ combat
    /// rules (cooldown-scaled damage, critical hits, sword sweep, knockback,
    /// weapon wear). Returns true if a creature was hit.
    fn attack_creature(&mut self) -> bool {
        let eye = self.player.pos + DVec3::new(0.0, self.eye, 0.0);
        let dir = self.look_dir();
        let reach = if self.mode == GameMode::Creative { 5.0 } else { 3.0 };
        let block_dist = self.target.map(|h| h.distance).unwrap_or(f64::MAX);
        let mut best: Option<(usize, f64)> = None;
        for (i, m) in self.mobs.list.iter().enumerate() {
            if m.death_time > 0 {
                continue;
            }
            if let Some(t) = ray_box(eye, dir, &self.mobs.hitbox(m))
                && t <= reach
                && t < block_dist
                && best.is_none_or(|b| t < b.1)
            {
                best = Some((i, t));
            }
        }
        let Some((index, _)) = best else { return false };
        let held = self.inventory.held();
        let (base, _) = ld_game::combat::weapon_stats(held, &self.items);
        let weapon = match held.map(|s| &self.items.get(s.item).kind) {
            Some(ItemKind::Tool(t)) => Some(t.kind),
            _ => None,
        };
        let ctx = ld_game::combat::AttackContext {
            strength: self.attack_strength(),
            falling: self.player.vel.y < 0.0,
            on_ground: self.player.on_ground,
            in_water: self.player.fluid != ld_physics::player::Fluid::None,
            sprinting: self.player.sprinting,
        };
        let hit = ld_game::combat::compute_hit(base, weapon, ctx);
        let knockback = if self.player.sprinting && ctx.strength > 0.9 { 0.9 } else { 0.4 };
        let from = self.player.pos;
        if self.mobs.hurt(index, hit.damage, from, knockback) {
            if hit.sweep {
                let center = self.mobs.list[index].pos;
                for j in 0..self.mobs.list.len() {
                    if j != index
                        && self.mobs.list[j].pos.distance(center) < 1.5
                        && self.mobs.list[j].pos.distance(from) < 4.0
                    {
                        self.mobs.hurt(j, 1.0, from, 0.4);
                    }
                }
            }
            if self.mode == GameMode::Survival {
                self.stats.exhaust(0.1);
                let wear = ld_game::combat::weapon_wear(weapon);
                if let (Some(ItemKind::Tool(t)), Some(stack)) =
                    (held.map(|s| self.items.get(s.item).kind.clone()), self.inventory.held_mut())
                {
                    stack.damage += wear;
                    if stack.damage >= t.durability {
                        *self.inventory.held_mut() = None;
                    }
                }
            }
        }
        true
    }

    /// Right-click with armour in hand: wear it (swapping with what's worn).
    fn equip_held_armor(&mut self) -> bool {
        let Some(stack) = self.inventory.held() else { return false };
        let ItemKind::Armor(a) = &self.items.get(stack.item).kind else { return false };
        let slot = a.slot as usize;
        let old = self.inventory.armor[slot].replace(stack);
        *self.inventory.held_mut() = old;
        true
    }

    /// Right-click with a lore fragment (dropped by Hollows): reads a story
    /// fragment you haven't found yet, adds it to the journal and uses up the
    /// item (Survival). If every fragment is already known, it shows one you
    /// have read and the item is kept. Returns true if the click was used.
    fn read_lore_fragment(&mut self) -> bool {
        let Some(stack) = self.inventory.held() else { return false };
        if self.items.get(stack.item).name != "lore_fragment" {
            return false;
        }
        let mut unread: Vec<(String, usize)> = Vec::new();
        for era in ["golden", "expansion", "deep"] {
            for index in 0..self.lore.era(era).len() {
                if !self.journal.iter().any(|(e, i)| e == era && *i == index) {
                    unread.push((era.to_string(), index));
                }
            }
        }
        let (era, index) = if unread.is_empty() {
            if self.journal.is_empty() {
                return false; // no lore loaded at all
            }
            let k = (self.next_random() * self.journal.len() as f32) as usize % self.journal.len();
            self.journal[k].clone()
        } else {
            let k = (self.next_random() * unread.len() as f32) as usize % unread.len();
            let pick = unread.swap_remove(k);
            self.journal.push(pick.clone());
            if self.mode == GameMode::Survival {
                self.inventory.consume_held();
            }
            pick
        };
        self.open_request = Some(crate::screens::Screen::Tablet { era, index });
        true
    }

    /// Holding use with food: eat after 32 ticks (1.6 s), like Minecraft.
    fn update_eating(&mut self, holding_use: bool) {
        let food = self.inventory.held().and_then(|s| self.items.get(s.item).food);
        let can_eat = self.mode == GameMode::Survival && self.stats.food < ld_game::stats::MAX_FOOD;
        match food {
            Some((points, saturation)) if holding_use && can_eat => {
                self.eating += 1;
                if self.eating >= 32 {
                    self.eating = 0;
                    self.stats.eat(points, saturation);
                    self.inventory.consume_held();
                }
            }
            _ => self.eating = 0,
        }
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
    pub(crate) fn remove_block(&mut self, pos: BlockPos, drop_items: bool) {
        // A furnace spills its contents when broken (in any mode).
        if let Some(f) = self.furnaces.remove(&pos) {
            for s in f.contents() {
                self.entities.drop_from_block(pos.x, pos.y, pos.z, s);
            }
        }
        self.world.set_block(pos, BlockId::AIR);
        // Breaking something a Hollow built angers the Hollows nearby.
        self.mobs.block_broken(pos);
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

    pub(crate) fn next_random_pub(&mut self) -> f32 {
        self.next_random()
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
        let Some(mut per_tick) = break_progress_per_tick(&def, tool.as_ref(), self.player.on_ground) else {
            self.breaking = None; // unbreakable
            return;
        };
        if self.player.fluid == ld_physics::player::Fluid::Water && per_tick < 1.0 {
            per_tick /= 5.0; // mining under water is 5× slower (Minecraft)
        }
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
        self.stats.exhaust(0.005);
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

/// Distance along a ray (unit `dir`) to where it enters `b`, if it does.
pub fn ray_box(origin: DVec3, dir: DVec3, b: &Aabb) -> Option<f64> {
    let (mut t0, mut t1) = (0.0f64, f64::MAX);
    for axis in 0..3 {
        let (o, d, lo, hi) = (origin[axis], dir[axis], b.min[axis], b.max[axis]);
        if d.abs() < 1e-12 {
            if o < lo || o > hi {
                return None;
            }
        } else {
            let (a, c) = ((lo - o) / d, (hi - o) / d);
            t0 = t0.max(a.min(c));
            t1 = t1.min(a.max(c));
            if t0 > t1 {
                return None;
            }
        }
    }
    Some(t0)
}
