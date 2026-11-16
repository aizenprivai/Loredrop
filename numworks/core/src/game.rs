//! The game: screens (title, loading, playing, inventory, pause, settings,
//! controls, death), the 20-ticks-per-second simulation, mining with tools,
//! placing, using blocks (crafting table, furnace), eating, and survival.
//!
//! The platform calls [`Game::frame`] in a loop; it returns `false` when the
//! player quits.

use crate::Platform;
use crate::blocks::*;
use crate::furnace::{Furnace, MAX_FURNACES};
use crate::hud;
use crate::input::{Input, Key};
use crate::inventory::{HOTBAR, Inventory};
use crate::items::*;
use crate::math::{PI, clamp, ifloor};
use crate::player::{MoveInput, Player};
use crate::render::{self, Camera, Target};
use crate::settings::Settings;
use crate::sky::{DAY_TICKS, Sky};
use crate::survival::{Activity, Vitals};
use crate::text::Text;
use crate::ui::{Container, InvScreen};
use crate::world::World;
use crate::worldgen::{HEIGHT, column_info, find_spawn};
use core::fmt::Write;

/// Milliseconds per game tick (20 ticks per second, as on PC).
const TICK_MS: u64 = 50;
/// How far the player can reach blocks (survival / creative), as on PC.
const REACH_SURVIVAL: f32 = 4.5;
const REACH_CREATIVE: f32 = 5.0;
/// Ticks between placed blocks when the use key is held (PC: 4).
const PLACE_REPEAT: u32 = 4;
/// Ticks between broken blocks in creative mode when the key is held (PC: 6).
const CREATIVE_BREAK_REPEAT: u32 = 6;
/// Ticks to eat one food item (PC: 32 = 1.6 s).
const EAT_TICKS: u32 = 32;
/// Columns generated per frame while playing (more = faster loading, slower frames).
const LOAD_PER_FRAME: usize = 24;
/// Items a creative player starts with in the hotbar.
const CREATIVE_HOTBAR: [Item; 9] = [
    GRASS_BLOCK as Item,
    DIRT as Item,
    STONE as Item,
    COBBLESTONE as Item,
    OAK_PLANKS as Item,
    OAK_LOG as Item,
    GLASS as Item,
    STONE_BRICKS as Item,
    DIAMOND_PICKAXE,
];

/// Game mode, as on PC.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    /// Health and hunger; mine at real speeds and collect what you break.
    Survival,
    /// Every item, instant breaking, flying, no damage.
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
    /// Inventory, crafting table, furnace or creative items.
    Inventory,
    /// Pause menu.
    Paused,
    /// Settings list.
    Settings,
    /// Controls help.
    Controls,
    /// "You died" screen.
    Dead,
}

/// The whole game state. Large (the world is ~66 KB): keep it in a `static`.
pub struct Game {
    /// The world.
    pub world: World,
    /// The player's body.
    pub player: Player,
    /// Health, food and breath.
    pub vitals: Vitals,
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
    /// The player's items (`0..9` = hotbar).
    pub inventory: Inventory,
    /// Selected hotbar slot.
    pub selected: usize,
    /// Open inventory screen state.
    pub ui: InvScreen,
    /// Placed furnaces.
    pub furnaces: [Furnace; MAX_FURNACES],
    /// World time in ticks since the world was created.
    pub time: u32,
    /// Where the player (re)spawns.
    spawn: [f32; 3],
    rng: Rng,
    mining: Option<([i32; 3], u32)>,
    eating: u32,
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
    /// What the hearts/food panel last showed (health, food, air, under water, flash, survival).
    last_panel: (u8, u8, u8, bool, bool, bool),
    started: bool,
}

impl Game {
    /// A game on the title screen. `const`, so it can be a `static`.
    pub const fn new() -> Self {
        Self {
            world: World::new(),
            player: Player::new([0.0; 3]),
            vitals: Vitals::new(),
            settings: Settings::DEFAULT,
            input: Input::new(),
            screen: Screen::Title,
            back_to: Screen::Title,
            menu_index: 0,
            mode: Mode::Survival,
            inventory: Inventory::new(),
            selected: 0,
            ui: InvScreen::new(),
            furnaces: [Furnace::EMPTY; MAX_FURNACES],
            time: 0,
            spawn: [0.0; 3],
            rng: Rng(0x1234_5678),
            mining: None,
            eating: 0,
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
            last_panel: (0, 0, 0, false, false, false),
            started: false,
        }
    }

    /// Starts a new world with `seed` in `mode`, beginning on the loading screen.
    pub fn new_world(&mut self, seed: u32, mode: Mode) {
        self.world.reset(seed);
        let (x, y, z) = find_spawn(seed);
        self.spawn = [x as f32 + 0.5, y as f32 + 1.0, z as f32 + 0.5];
        self.player = Player::new(self.spawn);
        self.vitals = Vitals::new();
        self.rng = Rng(seed ^ 0x9E37_79B9);
        self.mode = mode;
        self.inventory.clear();
        self.furnaces = [Furnace::EMPTY; MAX_FURNACES];
        if mode == Mode::Creative {
            for (s, &i) in self.inventory.slots.iter_mut().zip(CREATIVE_HOTBAR.iter()) {
                *s = Stack::new(i, 1);
            }
        }
        self.selected = 0;
        self.time = 0;
        self.mining = None;
        self.eating = 0;
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

    /// The stack in the selected hotbar slot.
    fn held(&self) -> Stack {
        self.inventory.slots[self.selected]
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
            Screen::Inventory => self.inventory_screen(p, dt),
            Screen::Paused => self.paused(p),
            Screen::Settings => self.settings_screen(p),
            Screen::Controls => self.controls(p),
            Screen::Dead => self.dead(p),
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

    fn resume(&mut self) {
        self.screen = Screen::Playing;
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
            self.resume();
            self.tick_acc = 0;
        }
        true
    }

    fn paused(&mut self, p: &mut dyn Platform) -> bool {
        let mode = if self.mode == Mode::Survival { "Mode: Survival" } else { "Mode: Creative" };
        let items = ["Resume", mode, "Settings", "Controls", "Quit to title"];
        let chosen = self.menu_nav(items.len());
        if self.input.pressed(Key::Back) {
            self.resume();
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
            hud::centered_small(p, t.as_str(), 176, hud::DIM);
            t.clear();
            // Tick 0 is 6:00 in the morning, as on PC.
            let minutes = ((self.time % DAY_TICKS + 6000) % DAY_TICKS) * 1440 / DAY_TICKS;
            let _ = write!(
                t,
                "Day {}  {:02}:{:02}  Seed {}",
                self.time / DAY_TICKS + 1,
                minutes / 60,
                minutes % 60,
                self.world.seed
            );
            hud::centered_small(p, t.as_str(), 192, hud::DIM);
            t.clear();
            let _ = write!(t, "Changed blocks {}/{}", self.world.edit_count(), crate::world::MAX_EDITS);
            hud::centered_small(p, t.as_str(), 208, hud::DIM);
            hud::centered_small(p, "The world is not saved when you quit.", 224, hud::DIM);
        }
        if chosen {
            match self.menu_index {
                0 => self.resume(),
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
            self.vitals = Vitals::new();
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
                ("0 / 1", "Jump, swim up / sneak"),
                ("3", "Sprint (until you stop)"),
                ("OK (hold)", "Mine the block you look at"),
                ("EXE", "Place, use (table...), eat"),
                ("7 / 9", "Previous / next hotbar slot"),
                ("Var", "Inventory and crafting"),
                ("Toolbox", "Recipes / creative: fly"),
                ("Back", "Pause menu, close screens"),
                ("Home", "Quit"),
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

    fn dead(&mut self, p: &mut dyn Platform) -> bool {
        if self.menu_dirty {
            self.menu_dirty = false;
            p.fill_rect(0, 0, crate::SCREEN_W as u16, crate::SCREEN_H as u16, crate::color::hex(0x3A0C0C));
            let msg = self.vitals.last_cause.map_or("You died.", |c| c.death_message());
            let bg = crate::color::hex(0x3A0C0C);
            p.draw_string("You died!", 115, 70, true, hud::TEXT, bg);
            let w = msg.chars().count() as u16 * hud::SMALL_W;
            p.draw_string(msg, (320 - w) / 2, 104, false, hud::TEXT, bg);
            p.draw_string("Your items were lost.", 86, 124, false, hud::DIM, bg);
            p.draw_string("OK: respawn", 121, 170, false, hud::GOLD, bg);
        }
        if self.input.pressed(Key::Ok) || self.input.pressed(Key::Exe) {
            self.inventory.clear();
            self.vitals = Vitals::new();
            self.player = Player::new(self.spawn);
            self.mining = None;
            self.eating = 0;
            self.screen = Screen::Loading;
            self.menu_dirty = true;
        } else {
            p.sleep(20);
        }
        true
    }

    // -------------------------------------------------------- inventory UI

    fn furnace_index(&self, pos: [i32; 3]) -> Option<usize> {
        self.furnaces.iter().position(|f| f.used && f.pos == pos)
    }

    fn open_container(&mut self, c: Container) {
        self.ui.open(c, self.selected);
        self.screen = Screen::Inventory;
        self.mining = None;
    }

    fn inventory_screen(&mut self, p: &mut dyn Platform, dt: u64) -> bool {
        let furnace_idx = match self.ui.container {
            Container::Furnace(i) => Some(i),
            _ => None,
        };
        let input = self.input;
        let open = {
            let f = furnace_idx.map(|i| &mut self.furnaces[i]);
            self.ui.update(&input, &mut self.inventory, f)
        };
        if !open {
            let lost = self.ui.close(&mut self.inventory);
            if lost > 0 {
                self.say("Inventory full: some items were lost.");
            }
            self.resume();
            return true;
        }
        // The world keeps going while the screen is open (as on PC).
        self.tick_acc += dt;
        let mut n = 0;
        while self.tick_acc >= TICK_MS && n < 5 {
            self.tick_world(false);
            self.tick_acc -= TICK_MS;
            n += 1;
            if furnace_idx.is_some() && self.time.is_multiple_of(10) {
                self.ui.dirty = true;
            }
        }
        if n == 5 {
            self.tick_acc = 0;
        }
        if self.vitals.dead() {
            self.ui.close(&mut self.inventory);
            self.open(Screen::Dead);
            return true;
        }
        let f = furnace_idx.map(|i| &self.furnaces[i]);
        self.ui.draw(p, &self.inventory, f);
        if !self.ui.dirty {
            p.sleep(10);
        }
        true
    }

    // --------------------------------------------------------------- playing

    fn play(&mut self, p: &mut dyn Platform, dt: u64, now: u64) -> bool {
        if self.input.pressed(Key::Back) {
            self.open(Screen::Paused);
            return true;
        }
        if self.input.pressed(Key::Var) {
            self.open_container(if self.mode == Mode::Creative {
                Container::Creative
            } else {
                Container::Inventory
            });
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
        if i.pressed(Key::Seven) || i.pressed(Key::Nine) {
            self.selected =
                if i.pressed(Key::Seven) { (self.selected + 8) % 9 } else { (self.selected + 1) % 9 };
            self.hud_dirty = true;
            self.mining = None;
            self.eating = 0;
        }
        if i.pressed(Key::Three) {
            self.sprinting = !self.sprinting;
        }
        if self.mode == Mode::Creative && i.pressed(Key::Toolbox) {
            self.player.flying = !self.player.flying;
            self.say(if self.player.flying { "Flying" } else { "Walking" });
        }
        if i.pressed(Key::Exe) {
            self.place_cooldown = 0;
            self.use_pressed();
            if self.screen != Screen::Playing {
                return true;
            }
        }

        // Fixed-rate simulation.
        self.tick_acc += dt;
        let mut n = 0;
        while self.tick_acc >= TICK_MS && n < 5 {
            self.tick_world(true);
            self.tick_acc -= TICK_MS;
            n += 1;
        }
        if n == 5 {
            self.tick_acc = 0; // too slow: drop time rather than spiral
        }
        if self.vitals.dead() {
            self.open(Screen::Dead);
            return true;
        }

        // Stream the world around the player.
        let (px, pz) = (ifloor(self.player.pos[0]), ifloor(self.player.pos[2]));
        self.world.update(px, pz, LOAD_PER_FRAME);

        // Draw.
        let held = self.held().item;
        let target = self.target().map(|(cell, _)| Target {
            cell,
            crack: match self.mining {
                Some((c, prog)) if c == cell && self.mode == Mode::Survival => {
                    let need = mining_ticks(self.world.get(c[0], c[1], c[2]), held).unwrap_or(1).max(1);
                    (prog * 10 / need).min(9) as u8 + 1
                }
                _ => 0,
            },
        });
        let sky = Sky::at(self.time);
        let cam = Camera { eye: self.player.eye(), yaw: self.player.yaw, pitch: self.player.pitch };
        render::render(p, &self.world, &sky, cam, &self.settings, target);
        if self.vitals.since_hurt < 4 || self.vitals.fire > 0 {
            // Red (hurt) or orange (burning) frame around the view.
            let c = if self.vitals.since_hurt < 4 {
                crate::color::hex(0xC01818)
            } else {
                crate::color::hex(0xF07018)
            };
            p.fill_rect(0, 0, 320, 3, c);
            p.fill_rect(0, render::VIEW_H as u16 - 3, 320, 3, c);
            p.fill_rect(0, 0, 3, render::VIEW_H as u16, c);
            p.fill_rect(317, 0, 3, render::VIEW_H as u16, c);
        }
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

    /// The use key was just pressed: open a crafting table or furnace if
    /// looking at one, otherwise place or eat (handled in `tick_world`).
    fn use_pressed(&mut self) {
        let Some((cell, _)) = self.target() else { return };
        let b = self.world.get(cell[0], cell[1], cell[2]);
        if b == CRAFTING_TABLE {
            self.open_container(Container::Table);
            self.place_cooldown = PLACE_REPEAT;
        } else if b == FURNACE || b == LIT_FURNACE {
            let idx = self.furnace_index(cell).or_else(|| self.new_furnace(cell));
            match idx {
                Some(i) => self.open_container(Container::Furnace(i)),
                None => self.say("Too many furnaces (8 at most)."),
            }
            self.place_cooldown = PLACE_REPEAT;
        }
    }

    fn new_furnace(&mut self, pos: [i32; 3]) -> Option<usize> {
        let i = self.furnaces.iter().position(|f| !f.used)?;
        self.furnaces[i] = Furnace { used: true, pos, ..Furnace::EMPTY };
        Some(i)
    }

    /// One game tick. `controls` is false while an inventory screen is open
    /// (the player stands still but the world, furnaces and hunger go on).
    fn tick_world(&mut self, controls: bool) {
        let i = self.input;
        let survival = self.mode == Mode::Survival;
        let (forward, strafe, jump, sneak) = if controls {
            (
                (i.down(Key::Eight) as i32 - i.down(Key::Five) as i32) as f32,
                (i.down(Key::Six) as i32 - i.down(Key::Four) as i32) as f32,
                i.down(Key::Zero),
                i.down(Key::One),
            )
        } else {
            (0.0, 0.0, false, false)
        };
        if forward <= 0.0 || (survival && !self.vitals.can_sprint()) {
            self.sprinting = false;
        }
        let mv = MoveInput { forward, strafe, jump, sneak, sprint: self.sprinting };
        self.player.tick(&self.world, mv, self.settings.auto_jump);
        self.time = self.time.wrapping_add(1);
        if self.message_ticks > 0 {
            self.message_ticks -= 1;
        }
        self.place_cooldown = self.place_cooldown.saturating_sub(1);
        self.break_cooldown = self.break_cooldown.saturating_sub(1);

        // Survival: hunger, breath, damage.
        if survival {
            let pl = &self.player;
            let swimming = pl.in_water || pl.in_lava;
            let a = Activity {
                sprint_dist: if self.sprinting && !swimming { pl.moved } else { 0.0 },
                swim_dist: if swimming { pl.moved } else { 0.0 },
                jumped: pl.jumped,
                sprinting: self.sprinting,
                eyes_in_water: pl.eyes_in_water,
                in_water: pl.in_water,
                in_lava: pl.in_lava,
                landed_fall: pl.landed_fall,
            };
            self.vitals.tick(&a);
        }

        // Furnaces.
        for f in self.furnaces.iter_mut().filter(|f| f.used) {
            if f.tick() {
                let [x, y, z] = f.pos;
                let want = if f.lit() { LIT_FURNACE } else { FURNACE };
                if self.world.is_loaded(x, z) && matches!(self.world.get(x, y, z), FURNACE | LIT_FURNACE) {
                    self.world.set(x, y, z, want);
                }
            }
        }

        // Fell out of the world (only possible by digging through bedrock in creative).
        if self.player.pos[1] < -20.0 {
            self.player = Player::new(self.spawn);
        }
        if self.player.pos[1] > HEIGHT as f32 + 20.0 {
            self.player.pos[1] = HEIGHT as f32 + 20.0;
        }

        if !controls {
            return;
        }
        let target = self.target();
        self.tick_mining(target);
        if i.down(Key::Exe) {
            self.tick_use(target);
        } else {
            self.eating = 0;
        }
    }

    /// Holding the use key: eat food, or place the held block.
    fn tick_use(&mut self, target: Option<([i32; 3], [i32; 3])>) {
        let held = self.held();
        if let ItemKind::Food { food, saturation } = item_def(held.item).kind
            && self.mode == Mode::Survival
            && self.vitals.food < crate::survival::MAX_FOOD
        {
            self.eating += 1;
            if self.eating >= EAT_TICKS {
                self.vitals.eat(food, saturation);
                self.inventory.slots[self.selected].take(1);
                self.eating = 0;
                self.hud_dirty = true;
            }
            return;
        }
        if self.place_cooldown == 0 {
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
                self.break_block(cell, b);
                self.break_cooldown = CREATIVE_BREAK_REPEAT;
            }
            return;
        }
        let held = self.held().item;
        let progress = match self.mining {
            Some((c, prog)) if c == cell => prog + 1,
            _ => 1,
        };
        self.mining = Some((cell, progress));
        match mining_ticks(b, held) {
            None => {
                if progress == 1 {
                    self.say("Too hard to break.");
                }
            }
            Some(need) if progress >= need => {
                if !can_harvest(b, held) && def(b).layer != Layer::Cutout {
                    self.say("Needs a better tool to drop anything.");
                }
                self.break_block(cell, b);
                self.mining = None;
            }
            Some(_) => {}
        }
    }

    /// Removes block `b` at `cell`: drops, tool wear, furnace contents.
    fn break_block(&mut self, cell: [i32; 3], b: Block) {
        if !self.world.set(cell[0], cell[1], cell[2], AIR) {
            self.say("Too many changes: far ones get forgotten");
        }
        if let Some(fi) = self.furnace_index(cell) {
            let f = self.furnaces[fi];
            for s in [f.input, f.fuel, f.output] {
                if self.mode == Mode::Survival {
                    self.inventory.add(s);
                }
            }
            self.furnaces[fi] = Furnace::EMPTY;
        }
        if self.mode == Mode::Survival {
            let held = self.held().item;
            let mut lost = false;
            let mut rng = self.rng;
            let inv = &mut self.inventory;
            roll_drops(b, held, &mut rng, |item, n| {
                lost |= inv.add(Stack::new(item, n)) > 0;
            });
            self.rng = rng;
            if lost {
                self.say("Inventory full.");
            }
            // Tool wear.
            let wear = wear_for(b, held);
            if wear > 0 {
                let s = &mut self.inventory.slots[self.selected];
                s.wear += wear;
                if s.wear >= durability(s.item) {
                    let name = item_def(s.item).name;
                    *s = Stack::EMPTY;
                    let mut t: Text<48> = Text::new();
                    let _ = write!(t, "Your {} broke!", name);
                    self.say(t.as_str());
                }
            }
            self.vitals.exhaust(0.005);
        }
        self.hud_dirty = true;
    }

    fn place(&mut self, cell: [i32; 3], normal: [i32; 3]) {
        let slot = self.held();
        let Some(block) = block_of(slot.item).filter(|_| !slot.is_empty()) else { return };
        let at = [cell[0] + normal[0], cell[1] + normal[1], cell[2] + normal[2]];
        if !(0..HEIGHT as i32).contains(&at[1]) {
            return;
        }
        if !is_replaceable(self.world.get(at[0], at[1], at[2])) {
            return;
        }
        if def(block).solid && self.player.overlaps_block(at[0], at[1], at[2]) {
            return;
        }
        if block == FURNACE && self.new_furnace(at).is_none() {
            self.say("Too many furnaces (8 at most).");
            return;
        }
        if !self.world.set(at[0], at[1], at[2], block) {
            self.say("Too many changes: far ones get forgotten");
        }
        if self.mode == Mode::Survival {
            self.inventory.slots[self.selected].take(1);
        }
        self.hud_dirty = true;
    }

    /// Redraws the bottom bar where it changed.
    fn draw_bar(&mut self, p: &mut dyn Platform) {
        let held = self.held();
        let mut status: Text<64> = Text::new();
        if self.message_ticks > 0 {
            let _ = status.write_str(self.message.as_str());
        } else if self.eating > 0 {
            let _ = write!(status, "Eating {}...", item_def(held.item).name);
        } else if let Some((c, prog)) = self.mining {
            let b = self.world.get(c[0], c[1], c[2]);
            if let Some(need) = mining_ticks(b, held.item) {
                let _ = write!(status, "Mining {} {}%", def(b).name, (prog * 100 / need.max(1)).min(100));
            }
        } else if !held.is_empty() {
            let max = durability(held.item);
            if max > 0 {
                let _ = write!(status, "{} {}/{}", item_def(held.item).name, max - held.wear.min(max), max);
            } else if self.mode == Mode::Creative {
                let _ = write!(status, "{}", item_def(held.item).name);
            } else {
                let _ = write!(status, "{} x{}", item_def(held.item).name, held.count);
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
        if self.hud_dirty {
            hud::hotbar(p, &self.inventory.slots[..HOTBAR], self.selected);
        }
        // Hearts and food (survival) or the mode name (creative).
        let flash = self.vitals.since_hurt < 6;
        let v = &self.vitals;
        // Only what the panel shows, so it is redrawn only when that changes.
        let shown = (
            libm::ceilf(v.health) as u8,
            v.food,
            (v.air.max(0) as i32 * 10 / 300) as u8,
            v.air < 300,
            flash,
            self.mode == Mode::Survival,
        );
        if self.hud_dirty || shown != self.last_panel {
            p.fill_rect(hud::PANEL_X, hud::HOTBAR_Y, crate::SCREEN_W as u16 - hud::PANEL_X, 22, hud::BAR_BG);
            if self.mode == Mode::Survival {
                hud::vitals_panel(p, &self.vitals, flash);
            } else {
                p.draw_string("Creative", hud::PANEL_X + 4, hud::HOTBAR_Y + 4, false, hud::DIM, hud::BAR_BG);
            }
            self.last_panel = shown;
        }
        self.hud_dirty = false;
    }
}

impl Default for Game {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use std::boxed::Box;

    /// A survival game with the world around the spawn loaded.
    fn game() -> Box<Game> {
        let mut g = Box::new(Game::new());
        g.new_world(7, Mode::Survival);
        let (x, z) = (ifloor(g.player.pos[0]), ifloor(g.player.pos[2]));
        g.world.update(x, z, usize::MAX);
        g.screen = Screen::Playing;
        g
    }

    /// A free cell next to the player to put test blocks in.
    fn spot(g: &Game) -> [i32; 3] {
        let p = g.player.pos;
        [ifloor(p[0]) + 2, ifloor(p[1]) + 1, ifloor(p[2])]
    }

    #[test]
    fn mining_with_a_tool_drops_and_wears_it() {
        let mut g = game();
        let c = spot(&g);
        g.world.set(c[0], c[1], c[2], STONE);
        g.inventory.slots[0] = Stack::new(WOODEN_PICKAXE, 1);
        g.break_block(c, STONE);
        assert_eq!(g.world.get(c[0], c[1], c[2]), AIR);
        assert_eq!(g.inventory.count(COBBLESTONE as Item), 1);
        assert_eq!(g.inventory.slots[0].wear, 1);
        // By hand, stone gives nothing.
        g.selected = 5;
        g.world.set(c[0], c[1], c[2], STONE);
        g.break_block(c, STONE);
        assert_eq!(g.inventory.count(COBBLESTONE as Item), 1);
    }

    #[test]
    fn tools_break_when_worn_out() {
        let mut g = game();
        let c = spot(&g);
        g.inventory.slots[0] = Stack { item: WOODEN_PICKAXE, count: 1, wear: 58 };
        g.world.set(c[0], c[1], c[2], STONE);
        g.break_block(c, STONE);
        assert!(g.inventory.slots[0].is_empty(), "59 uses for a wooden pickaxe");
    }

    #[test]
    fn eating_takes_32_ticks() {
        let mut g = game();
        g.vitals.food = 10;
        g.inventory.slots[0] = Stack::new(APPLE, 2);
        for _ in 0..31 {
            g.tick_use(None);
        }
        assert_eq!(g.vitals.food, 10);
        g.tick_use(None);
        assert_eq!(g.vitals.food, 14);
        assert_eq!(g.inventory.slots[0].count, 1);
    }

    #[test]
    fn furnace_contents_come_back_when_broken() {
        let mut g = game();
        let c = spot(&g);
        g.inventory.slots[0] = Stack::new(FURNACE as Item, 1);
        // Place it against the block below the spot.
        g.place([c[0], c[1] - 1, c[2]], [0, 1, 0]);
        assert_eq!(g.world.get(c[0], c[1], c[2]), FURNACE);
        let fi = g.furnace_index(c).expect("furnace registered");
        g.furnaces[fi].input = Stack::new(RAW_IRON, 3);
        g.furnaces[fi].fuel = Stack::new(COAL, 1);
        // It lights up and the block turns into a lit furnace.
        g.tick_world(false);
        assert_eq!(g.world.get(c[0], c[1], c[2]), LIT_FURNACE);
        g.inventory.slots[1] = Stack::new(WOODEN_PICKAXE, 1);
        g.selected = 1;
        g.break_block(c, LIT_FURNACE);
        assert_eq!(g.inventory.count(RAW_IRON), 3);
        assert_eq!(g.inventory.count(FURNACE as Item), 1, "a lit furnace drops a furnace");
        assert!(g.furnace_index(c).is_none());
    }

    #[test]
    fn death_and_respawn() {
        let mut g = game();
        g.inventory.slots[0] = Stack::new(DIAMOND, 5);
        g.vitals.health = 1.0;
        g.vitals.hurt(4.0, crate::survival::Cause::Fall);
        assert!(g.vitals.dead());
        struct Null;
        impl Platform for Null {
            fn push_rect(&mut self, _: u16, _: u16, _: u16, _: u16, _: &[u16]) {}
            fn fill_rect(&mut self, _: u16, _: u16, _: u16, _: u16, _: u16) {}
            fn draw_string(&mut self, _: &str, _: u16, _: u16, _: bool, _: u16, _: u16) {}
            fn keyboard(&mut self) -> u64 {
                crate::input::keys(&[Key::Ok])
            }
            fn millis(&mut self) -> u64 {
                0
            }
            fn random(&mut self) -> u32 {
                4
            }
            fn sleep(&mut self, _: u32) {}
        }
        let mut p = Null;
        g.frame(&mut p);
        assert_eq!(g.screen, Screen::Dead);
        g.input = Input::new(); // so OK counts as a fresh press
        g.frame(&mut p);
        assert_eq!(g.screen, Screen::Loading);
        assert_eq!(g.vitals.health, 20.0);
        assert_eq!(g.inventory.count(DIAMOND), 0);
    }
}
