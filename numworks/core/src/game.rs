//! The game: screens (title, loading, playing, pause, settings, controls),
//! the 20-ticks-per-second simulation, mining and placing, and the hotbar.
//!
//! The platform calls [`Game::frame`] in a loop; it returns `false` when the
//! player quits.

use crate::Platform;
use crate::blocks::*;
use crate::hud::{self, Slot};
use crate::input::{Input, Key};
use crate::math::{PI, clamp, ifloor};
use crate::player::{MoveInput, Player};
use crate::render::{self, Camera, Target};
use crate::settings::Settings;
use crate::sky::{DAY_TICKS, Sky};
use crate::text::Text;
use crate::world::World;
use crate::worldgen::{HEIGHT, column_info, find_spawn};
use core::fmt::Write;

/// Milliseconds per game tick (20 ticks per second, as on PC).
const TICK_MS: u64 = 50;
/// How far the player can reach blocks (survival / creative), as on PC.
const REACH_SURVIVAL: f32 = 4.5;
const REACH_CREATIVE: f32 = 5.0;
/// Ticks between placed blocks when the place key is held (PC: 4).
const PLACE_REPEAT: u32 = 4;
/// Ticks between broken blocks in creative mode when the key is held (PC: 6).
const CREATIVE_BREAK_REPEAT: u32 = 6;
/// Columns generated per frame while playing (more = faster loading, slower frames).
const LOAD_PER_FRAME: usize = 24;
/// Blocks a creative player starts with in the hotbar.
const CREATIVE_HOTBAR: [Block; 9] =
    [GRASS_BLOCK, DIRT, STONE, COBBLESTONE, OAK_PLANKS, OAK_LOG, GLASS, STONE_BRICKS, SAND];

/// Game mode, as on PC.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    /// Mine by hand at real speeds, collect what you break.
    Survival,
    /// Every block, instant breaking, flying.
    Creative,
}

/// Which screen is showing.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Screen {
    /// Title menu.
    Title,
    /// Generating the area around the player.
    Loading,
    /// In the world.
    Playing,
    /// Pause menu.
    Paused,
    /// Settings list.
    Settings,
    /// Controls help.
    Controls,
}

/// The whole game state. Large (the world is ~66 KB): keep it in a `static`.
pub struct Game {
    /// The world.
    pub world: World,
    /// The player.
    pub player: Player,
    /// Options.
    pub settings: Settings,
    input: Input,
    /// Current screen.
    pub screen: Screen,
    /// Screen to return to from Settings / Controls.
    back_to: Screen,
    menu_index: usize,
    /// Survival or creative.
    pub mode: Mode,
    /// Hotbar contents.
    pub hotbar: [Slot; 9],
    /// Selected hotbar slot.
    pub selected: usize,
    /// World time in ticks since the world was created.
    pub time: u32,
    mining: Option<([i32; 3], u32)>,
    place_cooldown: u32,
    break_cooldown: u32,
    sprinting: bool,
    message: Text<48>,
    message_ticks: u32,
    last_ms: u64,
    tick_acc: u64,
    fps_frames: u32,
    fps_since: u64,
    fps: u32,
    /// Bottom bar needs a full redraw.
    hud_dirty: bool,
    /// Menu needs redrawing.
    menu_dirty: bool,
    last_status: Text<64>,
    last_panel: Text<24>,
    started: bool,
}

impl Game {
    /// A game on the title screen. `const`, so it can be a `static`.
    pub const fn new() -> Self {
        Self {
            world: World::new(),
            player: Player::new([0.0; 3]),
            settings: Settings::DEFAULT,
            input: Input::new(),
            screen: Screen::Title,
            back_to: Screen::Title,
            menu_index: 0,
            mode: Mode::Survival,
            hotbar: [Slot::EMPTY; 9],
            selected: 0,
            time: 0,
            mining: None,
            place_cooldown: 0,
            break_cooldown: 0,
            sprinting: false,
            message: Text::new(),
            message_ticks: 0,
            last_ms: 0,
            tick_acc: 0,
            fps_frames: 0,
            fps_since: 0,
            fps: 0,
            hud_dirty: true,
            menu_dirty: true,
            last_status: Text::new(),
            last_panel: Text::new(),
            started: false,
        }
    }

    /// Starts a new world with `seed` in `mode`, beginning on the loading screen.
    pub fn new_world(&mut self, seed: u32, mode: Mode) {
        self.world.reset(seed);
        let (x, y, z) = find_spawn(seed);
        self.player = Player::new([x as f32 + 0.5, y as f32 + 1.0, z as f32 + 0.5]);
        self.mode = mode;
        self.hotbar = [Slot::EMPTY; 9];
        if mode == Mode::Creative {
            for (s, &b) in self.hotbar.iter_mut().zip(CREATIVE_HOTBAR.iter()) {
                *s = Slot { block: b, count: 1 };
            }
        }
        self.selected = 0;
        self.time = 0;
        self.mining = None;
        self.sprinting = false;
        self.screen = Screen::Loading;
        self.menu_dirty = true;
        self.say("Welcome to Loredrop!");
    }

    /// Shows a message in the status line for a few seconds.
    pub fn say(&mut self, text: &str) {
        self.message.clear();
        let _ = self.message.write_str(text);
        self.message_ticks = 60;
    }

    /// Runs one frame: reads keys, advances the game, draws. Returns `false`
    /// when the player chose to quit.
    pub fn frame(&mut self, p: &mut dyn Platform) -> bool {
        let now = p.millis();
        if !self.started {
            self.started = true;
            self.last_ms = now;
            self.fps_since = now;
        }
        let dt = now.saturating_sub(self.last_ms).min(250);
        self.last_ms = now;
        self.input.update(p.keyboard());
        if self.input.pressed(Key::Home) {
            return false;
        }
        match self.screen {
            Screen::Title => self.title(p),
            Screen::Loading => self.loading(p),
            Screen::Playing => self.play(p, dt, now),
            Screen::Paused => self.paused(p),
            Screen::Settings => self.settings_screen(p),
            Screen::Controls => self.controls(p),
        }
    }

    // ----------------------------------------------------------------- menus

    /// Moves the menu cursor with Up/Down; returns true if OK/EXE was pressed.
    fn menu_nav(&mut self, count: usize) -> bool {
        if self.input.pressed(Key::Up) {
            self.menu_index = (self.menu_index + count - 1) % count;
            self.menu_dirty = true;
        }
        if self.input.pressed(Key::Down) {
            self.menu_index = (self.menu_index + 1) % count;
            self.menu_dirty = true;
        }
        self.input.pressed(Key::Ok) || self.input.pressed(Key::Exe)
    }

    fn open(&mut self, screen: Screen) {
        if matches!(screen, Screen::Settings | Screen::Controls) {
            self.back_to = self.screen;
        }
        self.screen = screen;
        self.menu_index = 0;
        self.menu_dirty = true;
        self.hud_dirty = true;
    }

    fn title(&mut self, p: &mut dyn Platform) -> bool {
        const ITEMS: [&str; 3] = ["Play: Survival", "Play: Creative", "Controls"];
        let chosen = self.menu_nav(ITEMS.len());
        if self.menu_dirty {
            self.menu_dirty = false;
            hud::menu_frame(p, "LOREDROP");
            hud::centered_small(p, "NumWorks edition", 40, hud::DIM);
            hud::menu_items(p, &ITEMS, self.menu_index, 80);
            hud::centered_small(p, "Up/Down + OK to choose, Home to quit", 210, hud::DIM);
        }
        if chosen {
            match self.menu_index {
                0 | 1 => {
                    let seed = p.random();
                    let mode = if self.menu_index == 0 { Mode::Survival } else { Mode::Creative };
                    self.new_world(seed, mode);
                }
                _ => self.open(Screen::Controls),
            }
        } else {
            p.sleep(20);
        }
        true
    }

    fn loading(&mut self, p: &mut dyn Platform) -> bool {
        let (x, z) = (ifloor(self.player.pos[0]), ifloor(self.player.pos[2]));
        let missing = self.world.update(x, z, 64);
        let total = crate::world::SIZE * crate::world::SIZE;
        if self.menu_dirty {
            self.menu_dirty = false;
            hud::menu_frame(p, "LOREDROP");
            hud::centered_small(p, "Generating the world...", 100, hud::TEXT);
            p.fill_rect(60, 130, 200, 10, hud::BAR_BG);
        }
        let done = (total - missing.min(total)) * 200 / total;
        p.fill_rect(60, 130, done as u16, 10, hud::GOLD);
        if missing == 0 {
            self.screen = Screen::Playing;
            self.hud_dirty = true;
            self.tick_acc = 0;
        }
        true
    }

    fn paused(&mut self, p: &mut dyn Platform) -> bool {
        let mode = if self.mode == Mode::Survival { "Mode: Survival" } else { "Mode: Creative" };
        let items = ["Resume", mode, "Settings", "Controls", "Quit to title"];
        let chosen = self.menu_nav(items.len());
        if self.input.pressed(Key::Back) {
            self.screen = Screen::Playing;
            self.hud_dirty = true;
            return true;
        }
        if self.menu_dirty {
            self.menu_dirty = false;
            hud::menu_frame(p, "PAUSED");
            hud::menu_items(p, &items, self.menu_index, 50);
            let mut t: Text<64> = Text::new();
            let pos = self.player.pos;
            let biome = column_info(self.world.seed, ifloor(pos[0]), ifloor(pos[2])).biome;
            let _ =
                write!(t, "{}  x {} y {} z {}", biome.name(), ifloor(pos[0]), ifloor(pos[1]), ifloor(pos[2]));
            hud::centered_small(p, t.as_str(), 186, hud::DIM);
            t.clear();
            let _ = write!(
                t,
                "Seed {}  Day {}  Changes {}/{}",
                self.world.seed,
                self.time / DAY_TICKS + 1,
                self.world.edit_count(),
                crate::world::MAX_EDITS
            );
            hud::centered_small(p, t.as_str(), 204, hud::DIM);
            hud::centered_small(p, "The world is not saved when you quit.", 222, hud::DIM);
        }
        if chosen {
            match self.menu_index {
                0 => {
                    self.screen = Screen::Playing;
                    self.hud_dirty = true;
                }
                1 => {
                    self.toggle_mode();
                    self.menu_dirty = true;
                }
                2 => self.open(Screen::Settings),
                3 => self.open(Screen::Controls),
                _ => self.open(Screen::Title),
            }
        } else {
            p.sleep(20);
        }
        true
    }

    fn toggle_mode(&mut self) {
        self.mode = if self.mode == Mode::Survival { Mode::Creative } else { Mode::Survival };
        if self.mode == Mode::Survival {
            self.player.flying = false;
        } else {
            // Fill empty slots so there is something to build with.
            for (s, &b) in self.hotbar.iter_mut().zip(CREATIVE_HOTBAR.iter()) {
                if s.count == 0 {
                    *s = Slot { block: b, count: 1 };
                }
            }
        }
    }

    fn settings_screen(&mut self, p: &mut dyn Platform) -> bool {
        let n = Settings::COUNT;
        self.menu_nav(n);
        if self.input.pressed(Key::Right) || self.input.pressed(Key::Ok) {
            self.settings.change(self.menu_index, 1);
            self.menu_dirty = true;
        }
        if self.input.pressed(Key::Left) {
            self.settings.change(self.menu_index, -1);
            self.menu_dirty = true;
        }
        if self.input.pressed(Key::Back) {
            self.screen = self.back_to;
            self.menu_index = 0;
            self.menu_dirty = true;
            self.hud_dirty = true;
            return true;
        }
        if self.menu_dirty {
            self.menu_dirty = false;
            hud::menu_frame(p, "SETTINGS");
            for i in 0..n {
                let y = 46 + i as u16 * 20;
                let sel = i == self.menu_index;
                let fg = if sel { hud::GOLD } else { hud::TEXT };
                let mut t: Text<40> = Text::new();
                let _ = write!(t, "{} {}", if sel { ">" } else { " " }, Settings::name(i));
                hud::text_padded(p, t.as_str(), 20, y, 22, false, fg, hud::MENU_BG);
                hud::text_padded(p, self.settings.value(i).as_str(), 200, y, 14, false, fg, hud::MENU_BG);
            }
            hud::text_padded(
                p,
                Settings::description(self.menu_index),
                20,
                196,
                42,
                false,
                hud::DIM,
                hud::MENU_BG,
            );
            hud::centered_small(p, "Left/Right: change   Back: done", 218, hud::DIM);
        } else {
            p.sleep(20);
        }
        true
    }

    fn controls(&mut self, p: &mut dyn Platform) -> bool {
        if self.input.pressed(Key::Back) || self.input.pressed(Key::Ok) {
            self.screen = self.back_to;
            self.menu_index = 0;
            self.menu_dirty = true;
            self.hud_dirty = true;
            return true;
        }
        if self.menu_dirty {
            self.menu_dirty = false;
            hud::menu_frame(p, "CONTROLS");
            // Right column fits 29 characters.
            const LINES: [(&str, &str); 11] = [
                ("Arrows", "Look around"),
                ("8 5 4 6", "Walk: fwd, back, left, right"),
                ("0", "Jump, swim up, fly up"),
                ("1", "Sneak, fly down"),
                ("3", "Sprint (until you stop)"),
                ("OK (hold)", "Mine the block you look at"),
                ("EXE", "Place the selected block"),
                ("7 / 9", "Previous / next hotbar slot"),
                ("( / )", "Creative: change the block"),
                ("Toolbox", "Creative: fly on / off"),
                ("Back / Home", "Pause menu / quit"),
            ];
            for (i, (k, a)) in LINES.iter().enumerate() {
                let y = 42 + i as u16 * 16;
                p.draw_string(k, 12, y, false, hud::GOLD, hud::MENU_BG);
                p.draw_string(a, 112, y, false, hud::TEXT, hud::MENU_BG);
            }
            hud::centered_small(p, "Back or OK to return", 222, hud::DIM);
        } else {
            p.sleep(20);
        }
        true
    }

    // --------------------------------------------------------------- playing

    fn play(&mut self, p: &mut dyn Platform, dt: u64, now: u64) -> bool {
        if self.input.pressed(Key::Back) {
            self.open(Screen::Paused);
            return true;
        }
        let i = self.input;

        // Looking around is per frame (smooth at any frame rate).
        let turn = self.settings.turn_speed as f32 * (PI / 180.0) * dt as f32 / 1000.0;
        if i.down(Key::Left) {
            self.player.yaw -= turn;
        }
        if i.down(Key::Right) {
            self.player.yaw += turn;
        }
        if i.down(Key::Up) {
            self.player.pitch += turn * 0.8;
        }
        if i.down(Key::Down) {
            self.player.pitch -= turn * 0.8;
        }
        self.player.pitch = clamp(self.player.pitch, -1.55, 1.55);
        if self.player.yaw > PI {
            self.player.yaw -= 2.0 * PI;
        } else if self.player.yaw < -PI {
            self.player.yaw += 2.0 * PI;
        }

        // Keys that act once per press.
        if i.pressed(Key::Seven) {
            self.selected = (self.selected + 8) % 9;
            self.hud_dirty = true;
            self.mining = None;
        }
        if i.pressed(Key::Nine) {
            self.selected = (self.selected + 1) % 9;
            self.hud_dirty = true;
            self.mining = None;
        }
        if i.pressed(Key::Three) {
            self.sprinting = !self.sprinting;
        }
        if self.mode == Mode::Creative {
            if i.pressed(Key::LeftParen) {
                self.cycle_block(-1);
            }
            if i.pressed(Key::RightParen) {
                self.cycle_block(1);
            }
            if i.pressed(Key::Toolbox) {
                self.player.flying = !self.player.flying;
                self.say(if self.player.flying { "Flying" } else { "Walking" });
            }
        }
        if i.pressed(Key::Exe) {
            self.place_cooldown = 0;
        }

        // Fixed-rate simulation.
        self.tick_acc += dt;
        let mut n = 0;
        while self.tick_acc >= TICK_MS && n < 5 {
            self.tick();
            self.tick_acc -= TICK_MS;
            n += 1;
        }
        if n == 5 {
            self.tick_acc = 0; // too slow: drop time rather than spiral
        }

        // Stream the world around the player.
        let (px, pz) = (ifloor(self.player.pos[0]), ifloor(self.player.pos[2]));
        self.world.update(px, pz, LOAD_PER_FRAME);

        // Draw.
        let target = self.target().map(|(cell, _)| Target {
            cell,
            crack: match self.mining {
                Some((c, prog)) if c == cell && self.mode == Mode::Survival => {
                    let need = hand_mining_ticks(self.world.get(c[0], c[1], c[2])).unwrap_or(1).max(1);
                    (prog * 10 / need).min(9) as u8 + 1
                }
                _ => 0,
            },
        });
        let sky = Sky::at(self.time);
        let cam = Camera { eye: self.player.eye(), yaw: self.player.yaw, pitch: self.player.pitch };
        render::render(p, &self.world, &sky, cam, &self.settings, target);
        self.draw_bar(p);

        self.fps_frames += 1;
        if now - self.fps_since >= 1000 {
            self.fps = (self.fps_frames as u64 * 1000 / (now - self.fps_since)) as u32;
            self.fps_frames = 0;
            self.fps_since = now;
        }
        true
    }

    /// The targeted block and the face's outward offset.
    fn target(&self) -> Option<([i32; 3], [i32; 3])> {
        let reach = if self.mode == Mode::Creative { REACH_CREATIVE } else { REACH_SURVIVAL };
        render::pick(&self.world, self.player.eye(), self.player.look_dir(), reach)
    }

    /// One game tick: movement, mining, placing, time.
    fn tick(&mut self) {
        let i = self.input;
        let forward = (i.down(Key::Eight) as i32 - i.down(Key::Five) as i32) as f32;
        let strafe = (i.down(Key::Six) as i32 - i.down(Key::Four) as i32) as f32;
        if forward <= 0.0 {
            self.sprinting = false;
        }
        let mv = MoveInput {
            forward,
            strafe,
            jump: i.down(Key::Zero),
            sneak: i.down(Key::One),
            sprint: self.sprinting,
        };
        self.player.tick(&self.world, mv, self.settings.auto_jump);
        self.time = self.time.wrapping_add(1);
        if self.message_ticks > 0 {
            self.message_ticks -= 1;
        }
        self.place_cooldown = self.place_cooldown.saturating_sub(1);
        self.break_cooldown = self.break_cooldown.saturating_sub(1);

        // Fell out of the world (only possible by digging through bedrock in creative).
        if self.player.pos[1] < -20.0 {
            let (x, y, z) = find_spawn(self.world.seed);
            self.player = Player::new([x as f32 + 0.5, y as f32 + 1.0, z as f32 + 0.5]);
        }
        if self.player.pos[1] > HEIGHT as f32 + 20.0 {
            self.player.pos[1] = HEIGHT as f32 + 20.0;
        }

        let target = self.target();
        self.tick_mining(target);
        if i.down(Key::Exe) && self.place_cooldown == 0 {
            if let Some((cell, normal)) = target {
                self.place(cell, normal);
            }
            self.place_cooldown = PLACE_REPEAT;
        }
    }

    fn tick_mining(&mut self, target: Option<([i32; 3], [i32; 3])>) {
        let Some((cell, _)) = target.filter(|_| self.input.down(Key::Ok)) else {
            self.mining = None;
            return;
        };
        let b = self.world.get(cell[0], cell[1], cell[2]);
        if self.mode == Mode::Creative {
            if self.break_cooldown == 0 {
                self.break_block(cell, AIR);
                self.break_cooldown = CREATIVE_BREAK_REPEAT;
            }
            return;
        }
        let progress = match self.mining {
            Some((c, prog)) if c == cell => prog + 1,
            _ => 1,
        };
        self.mining = Some((cell, progress));
        match hand_mining_ticks(b) {
            None => {
                if progress == 1 {
                    self.say("Too hard to break.");
                }
            }
            Some(need) if progress >= need => {
                let drop = hand_drop(b);
                self.break_block(cell, drop);
                if drop == AIR && def(b).needs_tool && def(b).layer != Layer::Cutout {
                    self.say("Needs a tool to drop anything.");
                }
                self.mining = None;
            }
            Some(_) => {}
        }
    }

    fn break_block(&mut self, cell: [i32; 3], drop: Block) {
        if !self.world.set(cell[0], cell[1], cell[2], AIR) {
            self.say("Too many changes: far ones get forgotten");
        }
        if drop != AIR && self.mode == Mode::Survival && !self.give(drop) {
            self.say("Hotbar full.");
        }
        self.hud_dirty = true;
    }

    /// Adds one `block` to the hotbar. Returns false if there was no room.
    pub fn give(&mut self, block: Block) -> bool {
        if let Some(s) = self.hotbar.iter_mut().find(|s| s.block == block && s.count > 0 && s.count < 64) {
            s.count += 1;
            return true;
        }
        if let Some(s) = self.hotbar.iter_mut().find(|s| s.count == 0) {
            *s = Slot { block, count: 1 };
            return true;
        }
        false
    }

    fn place(&mut self, cell: [i32; 3], normal: [i32; 3]) {
        let slot = self.hotbar[self.selected];
        if slot.count == 0 || slot.block == AIR {
            return;
        }
        let at = [cell[0] + normal[0], cell[1] + normal[1], cell[2] + normal[2]];
        if !(0..HEIGHT as i32).contains(&at[1]) {
            return;
        }
        if !is_replaceable(self.world.get(at[0], at[1], at[2])) {
            return;
        }
        if def(slot.block).solid && self.player.overlaps_block(at[0], at[1], at[2]) {
            return;
        }
        if !self.world.set(at[0], at[1], at[2], slot.block) {
            self.say("Too many changes: far ones get forgotten");
        }
        if self.mode == Mode::Survival {
            let s = &mut self.hotbar[self.selected];
            s.count -= 1;
            if s.count == 0 {
                s.block = AIR;
            }
        }
        self.hud_dirty = true;
    }

    /// Creative: changes the selected slot's block to the next/previous one.
    fn cycle_block(&mut self, dir: i32) {
        let s = &mut self.hotbar[self.selected];
        let mut b = s.block as i32;
        loop {
            b = (b + dir).rem_euclid(BLOCK_COUNT as i32);
            if b != AIR as i32 && b != LIT_FURNACE as i32 {
                break;
            }
        }
        *s = Slot { block: b as Block, count: 1 };
        self.hud_dirty = true;
        let name = def(b as Block).name;
        self.say(name);
    }

    /// Redraws the bottom bar where it changed.
    fn draw_bar(&mut self, p: &mut dyn Platform) {
        let mut status: Text<64> = Text::new();
        if self.message_ticks > 0 {
            let _ = status.write_str(self.message.as_str());
        } else if let Some((c, prog)) = self.mining {
            let b = self.world.get(c[0], c[1], c[2]);
            if let Some(need) = hand_mining_ticks(b) {
                let _ = write!(status, "Mining {} {}%", def(b).name, (prog * 100 / need.max(1)).min(100));
            }
        } else {
            let s = self.hotbar[self.selected];
            if s.count > 0 {
                if self.mode == Mode::Creative {
                    let _ = write!(status, "{}", def(s.block).name);
                } else {
                    let _ = write!(status, "{} x{}", def(s.block).name, s.count);
                }
            }
        }
        let mut right: Text<12> = Text::new();
        if self.settings.show_fps {
            // The counter needs one full second of play before it has a value.
            if self.fps == 0 {
                let _ = write!(right, "-- FPS");
            } else {
                let _ = write!(right, "{} FPS", self.fps);
            }
        }
        let mut both: Text<64> = Text::new();
        let _ = write!(both, "{}|{}", status.as_str(), right.as_str());
        if self.hud_dirty || both.as_str() != self.last_status.as_str() {
            if self.hud_dirty {
                p.fill_rect(0, hud::STATUS_Y, crate::SCREEN_W as u16, 18, hud::BAR_BG);
            }
            hud::status_line(p, status.as_str(), right.as_str());
            self.last_status = both;
        }

        // Clock: tick 0 is 6:00 in the morning, as on PC.
        let day_ticks = (self.time % DAY_TICKS + 6000) % DAY_TICKS;
        let minutes = day_ticks * 1440 / DAY_TICKS;
        let mut panel: Text<24> = Text::new();
        let mode = if self.mode == Mode::Survival { "Survival" } else { "Creative" };
        let _ = write!(panel, "{:02}:{:02} {}", minutes / 60, minutes % 60, mode);
        if self.hud_dirty || panel.as_str() != self.last_panel.as_str() {
            if self.hud_dirty {
                hud::hotbar(p, &self.hotbar, self.selected, panel.as_str());
            } else {
                p.draw_string(
                    panel.as_str(),
                    2 + 9 * hud::SLOT + 4,
                    hud::HOTBAR_Y + 4,
                    false,
                    hud::DIM,
                    hud::BAR_BG,
                );
            }
            self.last_panel = panel;
        }
        self.hud_dirty = false;
    }
}

impl Default for Game {
    fn default() -> Self {
        Self::new()
    }
}
