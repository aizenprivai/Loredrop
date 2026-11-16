//! The game: screens (title, loading, playing, inventory, pause, settings,
//! controls, death), the 20-ticks-per-second simulation, mining with tools,
//! placing, using blocks (crafting table, furnace), eating, and survival.
//!
//! The platform calls [`Game::frame`] in a loop; it returns `false` when the
//! player quits.

use crate::Platform;
use crate::blocks::*;
use crate::entities::{Entities, Target as MobTarget};
use crate::furnace::{Furnace, MAX_FURNACES};
use crate::hud;
use crate::input::{Input, Key};
use crate::inventory::{HOTBAR, Inventory};
use crate::items::*;
use crate::math::{PI, clamp, ifloor};
use crate::mobs::LORE;
use crate::models::{self, EntityScene};
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
/// Columns generated per frame while playing (more = faster loading, slower
/// frames). Nearest columns come first; 12 per frame keeps up with walking
/// and sprinting at calculator frame rates (flying fast may show the edge
/// of the loaded area for a moment).
const LOAD_PER_FRAME: usize = 12;
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
    /// Reading a lore fragment.
    Lore,
    /// List of the lore fragments found so far.
    Journal,
    /// Results of the built-in speed test.
    SpeedTest,
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
    /// Creatures, dropped items and arrows.
    pub entities: Entities,
    /// Entities prepared for drawing (scratch space, rebuilt every frame).
    scene: EntityScene,
    /// Screen rows being prepared before they are sent to the display.
    lines: [u16; crate::SCREEN_W * render::BATCH_ROWS],
    /// Lore fragments found (bit i = `LORE[i]`).
    pub lore_found: u16,
    /// Fragment being read, and where to go back to.
    lore_shown: usize,
    lore_back: Screen,
    /// Ticks since the last attack (for the attack cooldown).
    attack_ticks: u32,
    /// Pixel size chosen by the automatic setting (1–4).
    pub auto_pixel: u8,
    /// Longest view distance automatic quality allows (lowered only when
    /// pixel size 4 is still too slow).
    pub auto_view: u8,
    /// Milliseconds spent this second in the 3D view, the game logic and
    /// the bottom bar (summed over frames), and the averages last shown.
    time_sums: [u64; 3],
    /// Average ms per frame of the last second (3D view, game, bar), x10.
    pub timings: [u32; 3],
    /// Last speed test: screen, 3D compute, 3D complete (ms x10), pixel size, view.
    pub speed_results: [u32; 5],
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
            entities: Entities::new(),
            scene: EntityScene::new(),
            lines: [0; crate::SCREEN_W * render::BATCH_ROWS],
            lore_found: 0,
            lore_shown: 0,
            lore_back: Screen::Playing,
            attack_ticks: 100,
            auto_pixel: 2,
            auto_view: 14,
            time_sums: [0; 3],
            timings: [0; 3],
            speed_results: [0; 5],
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
        self.entities.clear(seed);
        self.lore_found = 0;
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
            Screen::Lore => self.lore_screen(p),
            Screen::Journal => self.journal(p),
            Screen::SpeedTest => self.speed_results_screen(p),
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
        // Time spent in menus or loading must not count in the FPS figure.
        self.fps_frames = 0;
        self.fps_since = self.last_ms;
        self.time_sums = [0; 3];
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
        let mut journal: Text<24> = Text::new();
        let _ = write!(journal, "Lore journal {}/{}", self.lore_found.count_ones(), LORE.len());
        let items = ["Resume", mode, journal.as_str(), "Settings", "Controls", "Quit to title"];
        let chosen = self.menu_nav(items.len());
        if self.input.pressed(Key::Back) {
            self.resume();
            return true;
        }
        if self.menu_dirty {
            self.menu_dirty = false;
            hud::menu_frame(p, "PAUSED");
            hud::menu_items(p, &items, self.menu_index, 40);
            let mut t: Text<64> = Text::new();
            let pos = self.player.pos;
            let biome = column_info(self.world.seed, ifloor(pos[0]), ifloor(pos[2])).biome;
            let _ =
                write!(t, "{}  x {} y {} z {}", biome.name(), ifloor(pos[0]), ifloor(pos[1]), ifloor(pos[2]));
            hud::centered_small(p, t.as_str(), 184, hud::DIM);
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
            hud::centered_small(p, t.as_str(), 200, hud::DIM);
            t.clear();
            let _ = write!(t, "Changed blocks {}/{}", self.world.edit_count(), crate::world::MAX_EDITS);
            hud::centered_small(p, t.as_str(), 214, hud::DIM);
            hud::centered_small(p, "The world is not saved when you quit.", 227, hud::DIM);
        }
        if chosen {
            match self.menu_index {
                0 => self.resume(),
                1 => {
                    self.toggle_mode();
                    self.menu_dirty = true;
                }
                2 => self.open(Screen::Journal),
                3 => self.open(Screen::Settings),
                4 => self.open(Screen::Controls),
                _ => self.open(Screen::Title),
            }
        } else {
            p.sleep(20);
        }
        true
    }

    /// Shows lore fragment `i` and records it in the journal.
    fn read_lore(&mut self, i: usize, back: Screen) {
        let new = self.lore_found & (1 << i) == 0;
        self.lore_found |= 1 << i;
        self.lore_shown = i;
        self.lore_back = back;
        self.screen = Screen::Lore;
        self.menu_dirty = true;
        self.mining = None;
        if new {
            self.say("New entry in your lore journal.");
        }
    }

    fn lore_screen(&mut self, p: &mut dyn Platform) -> bool {
        if self.menu_dirty {
            self.menu_dirty = false;
            hud::menu_frame(p, "THE BUILDERS");
            let mut t: Text<24> = Text::new();
            let era = match LORE[self.lore_shown].0 {
                crate::mobs::Era::Golden => "The golden age",
                crate::mobs::Era::Expansion => "The gateways",
                crate::mobs::Era::Deep => "What they became",
            };
            let _ = write!(t, "{}  ({}/{})", era, self.lore_shown + 1, LORE.len());
            hud::centered_small(p, t.as_str(), 40, hud::DIM);
            // Word-wrap to 42 characters per line.
            let text = LORE[self.lore_shown].1;
            let (mut line_start, mut last_space, mut y) = (0usize, None::<usize>, 66u16);
            let bytes = text.as_bytes();
            let mut i = 0;
            while i <= bytes.len() {
                let end = i == bytes.len();
                if end || bytes[i] == b' ' {
                    if i - line_start > 42
                        && let Some(s) = last_space
                    {
                        p.draw_string(&text[line_start..s], 16, y, false, hud::TEXT, hud::MENU_BG);
                        y += 18;
                        line_start = s + 1;
                    }
                    last_space = Some(i);
                }
                if end {
                    p.draw_string(&text[line_start..], 16, y, false, hud::TEXT, hud::MENU_BG);
                }
                i += 1;
            }
            hud::centered_small(p, "Back or OK to close", 222, hud::DIM);
        }
        if self.input.pressed(Key::Back) || self.input.pressed(Key::Ok) {
            self.screen = self.lore_back;
            self.menu_dirty = true;
            self.hud_dirty = true;
        } else {
            p.sleep(20);
        }
        true
    }

    fn journal(&mut self, p: &mut dyn Platform) -> bool {
        let n = LORE.len();
        let chosen = self.menu_nav(n);
        if self.input.pressed(Key::Back) {
            self.open(Screen::Paused);
            return true;
        }
        if self.menu_dirty {
            self.menu_dirty = false;
            hud::menu_frame(p, "LORE JOURNAL");
            for (i, (_, words)) in LORE.iter().enumerate() {
                let y = 40 + i as u16 * 14;
                let sel = i == self.menu_index;
                let mut t: Text<48> = Text::new();
                if self.lore_found & (1 << i) != 0 {
                    let cut = words.char_indices().nth(36).map_or(words.len(), |(k, _)| k);
                    let _ = write!(t, "{}{:2}. {}...", if sel { ">" } else { " " }, i + 1, &words[..cut]);
                } else {
                    let _ = write!(t, "{}{:2}. ???", if sel { ">" } else { " " }, i + 1);
                }
                hud::text_padded(
                    p,
                    t.as_str(),
                    6,
                    y,
                    44,
                    false,
                    if sel { hud::GOLD } else { hud::TEXT },
                    hud::MENU_BG,
                );
            }
            hud::centered_small(p, "Find tablets in ruins. OK: read", 222, hud::DIM);
        }
        if chosen && self.lore_found & (1 << self.menu_index) != 0 {
            self.read_lore(self.menu_index, Screen::Journal);
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
        // The options, then one more line that runs the speed test.
        let n = Settings::COUNT + 1;
        let ok = self.menu_nav(n);
        // (Checked after moving the cursor: it can wrap onto this line.)
        let on_test = self.menu_index == Settings::COUNT;
        if on_test && ok {
            self.speed_test(p);
            return true;
        }
        if !on_test && (self.input.pressed(Key::Right) || self.input.pressed(Key::Ok)) {
            self.settings.change(self.menu_index, 1);
            self.menu_dirty = true;
        }
        if !on_test && self.input.pressed(Key::Left) {
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
                let y = 40 + i as u16 * 17;
                let sel = i == self.menu_index;
                let fg = if sel { hud::GOLD } else { hud::TEXT };
                let mut t: Text<40> = Text::new();
                let name = if i == Settings::COUNT { "Run speed test" } else { Settings::name(i) };
                let _ = write!(t, "{} {}", if sel { ">" } else { " " }, name);
                hud::text_padded(p, t.as_str(), 20, y, 22, false, fg, hud::MENU_BG);
                let value = if i == Settings::COUNT { Text::new() } else { self.settings.value(i) };
                hud::text_padded(p, value.as_str(), 200, y, 14, false, fg, hud::MENU_BG);
            }
            let desc = if on_test {
                "Measures screen and 3D speed (OK)."
            } else {
                Settings::description(self.menu_index)
            };
            hud::text_padded(p, desc, 20, 200, 42, false, hud::DIM, hud::MENU_BG);
            hud::centered_small(p, "Left/Right: change   Back: done", 220, hud::DIM);
        } else {
            p.sleep(20);
        }
        true
    }

    /// Built-in speed test (Settings → Run speed test): times, on this
    /// calculator, sending full screens to the display, computing the 3D
    /// view without sending it, and the complete 3D view, with the current
    /// pixel size and view distance. The results screen stays until Back,
    /// so they can be reported.
    fn speed_test(&mut self, p: &mut dyn Platform) {
        const RUNS: u64 = 8;
        hud::menu_frame(p, "SPEED TEST");
        hud::centered_small(p, "Testing, please wait...", 110, hud::TEXT);
        let pixel = if self.settings.pixel_size == crate::settings::PIXEL_AUTO {
            self.auto_pixel
        } else {
            self.settings.pixel_size
        } as usize;
        let mut view_settings = self.settings;
        view_settings.view_distance = self.view_distance();
        let sky = Sky::at(self.time);
        let cam = Camera { eye: self.player.eye(), yaw: self.player.yaw, pitch: self.player.pitch };
        let (sun, cave) = render::light_levels(&sky, &self.settings);
        let view = self.view_distance() as f32;

        // 1. Display only: full 3D-view-sized screens, 8 rows per send.
        let t0 = p.millis();
        for k in 0..RUNS {
            self.lines.fill(if k % 2 == 0 { 0x0000 } else { 0x4208 });
            for row in (0..render::VIEW_H).step_by(render::BATCH_ROWS) {
                let h = render::BATCH_ROWS.min(render::VIEW_H - row);
                p.push_rect(
                    0,
                    row as u16,
                    crate::SCREEN_W as u16,
                    h as u16,
                    &self.lines[..crate::SCREEN_W * h],
                );
            }
        }
        let display = (p.millis() - t0) * 10 / RUNS;

        // 2. 3D computed but not sent to the display.
        let t1 = p.millis();
        for _ in 0..RUNS {
            models::build(&mut self.scene, &self.entities, &self.world, cam.eye, view, sun, cave);
            let mut quiet = NoDisplay(&mut *p);
            render::render(
                &mut quiet,
                &self.world,
                &sky,
                cam,
                &view_settings,
                None,
                &mut self.scene,
                pixel,
                &mut self.lines,
            );
        }
        let compute = (p.millis() - t1) * 10 / RUNS;

        // 3. The complete 3D view.
        let t2 = p.millis();
        for _ in 0..RUNS {
            models::build(&mut self.scene, &self.entities, &self.world, cam.eye, view, sun, cave);
            render::render(
                p,
                &self.world,
                &sky,
                cam,
                &view_settings,
                None,
                &mut self.scene,
                pixel,
                &mut self.lines,
            );
        }
        let full = (p.millis() - t2) * 10 / RUNS;

        self.speed_results = [display as u32, compute as u32, full as u32, pixel as u32, view as u32];
        self.screen = Screen::SpeedTest;
        self.menu_dirty = true;
    }

    /// Shows the speed test results until Back is pressed.
    fn speed_results_screen(&mut self, p: &mut dyn Platform) -> bool {
        if self.input.pressed(Key::Back) || self.input.pressed(Key::Ok) {
            self.screen = Screen::Settings;
            self.menu_dirty = true;
            return true;
        }
        if self.menu_dirty {
            self.menu_dirty = false;
            let [display, compute, full, pixel, view] = self.speed_results;
            hud::menu_frame(p, "SPEED TEST");
            let rows: [(&str, u32); 3] =
                [("Screen only", display), ("3D compute only", compute), ("3D complete", full)];
            for (i, (name, v)) in rows.iter().enumerate() {
                let mut t: Text<40> = Text::new();
                let _ = write!(t, "{:<16}{:>4}.{} ms", name, v / 10, v % 10);
                p.draw_string(t.as_str(), 40, 60 + i as u16 * 24, false, hud::TEXT, hud::MENU_BG);
            }
            let mut t: Text<44> = Text::new();
            let fps = 10_000u32.checked_div(full).unwrap_or(0);
            let _ = write!(t, "Pixel {}  View {}  ->  about {} FPS", pixel, view, fps);
            hud::centered_small(p, t.as_str(), 150, hud::GOLD);
            hud::centered_small(p, "Please send these numbers. Back: return", 210, hud::DIM);
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
                ("OK", "Hit a creature, mine (hold)"),
                ("EXE", "Place, use (table...), eat"),
                ("7 / 9", "Previous / next hotbar slot"),
                ("Var", "Inventory and crafting"),
                ("Toolbox", "Recipes / creative: fly"),
                ("Backspace", "Throw one held item"),
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

    fn dead(&mut self, p: &mut dyn Platform) -> bool {
        if self.menu_dirty {
            self.menu_dirty = false;
            p.fill_rect(0, 0, crate::SCREEN_W as u16, crate::SCREEN_H as u16, crate::color::hex(0x3A0C0C));
            let mut msg: Text<48> = Text::new();
            match self.vitals.last_cause {
                Some(c) => c.death_message(&mut msg),
                None => {
                    let _ = msg.write_str("You died.");
                }
            }
            let bg = crate::color::hex(0x3A0C0C);
            p.draw_string("You died!", 115, 70, true, hud::TEXT, bg);
            let w = msg.as_str().chars().count() as u16 * hud::SMALL_W;
            p.draw_string(msg.as_str(), (320u16).saturating_sub(w) / 2, 104, false, hud::TEXT, bg);
            p.draw_string("Your items lie where you died", 58, 124, false, hud::DIM, bg);
            p.draw_string("(for 5 minutes).", 102, 140, false, hud::DIM, bg);
            p.draw_string("OK: respawn", 121, 180, false, hud::GOLD, bg);
        }
        if self.input.pressed(Key::Ok) || self.input.pressed(Key::Exe) {
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

    /// The player died: everything carried falls on the ground (as on PC;
    /// at most as many stacks as the item list holds), then the death screen.
    fn die(&mut self) {
        let pos = self.player.pos;
        let stacks = self.inventory.slots.iter().chain(self.inventory.armor.iter()).copied();
        for s in stacks.filter(|s| !s.is_empty()) {
            self.entities.drop_item(s, [pos[0], pos[1] + 1.0, pos[2]], 10);
        }
        self.inventory.clear();
        self.open(Screen::Dead);
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
            let (pos, ents) = (self.player.pos, &mut self.entities);
            let lost =
                self.ui.close(&mut self.inventory, |s| ents.drop_item(s, [pos[0], pos[1] + 1.0, pos[2]], 40));
            if lost > 0 {
                self.say("Inventory full: items dropped.");
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
            self.ui.close(&mut self.inventory, |_| {});
            self.die();
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
        // Attack the creature in front (OK), if it's closer than any block.
        if i.pressed(Key::Ok)
            && let Some(m) = self.mob_target()
        {
            self.attack(m);
        }
        // Throw one of the held item (Backspace, like Q on PC).
        if i.pressed(Key::Backspace) && !self.held().is_empty() {
            let s = self.held();
            let one = Stack { count: 1, ..s };
            self.inventory.slots[self.selected].take(1);
            self.entities.throw_item(one, self.player.eye(), self.player.look_dir());
            self.hud_dirty = true;
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
            self.die();
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
        let (sun, cave) = render::light_levels(&sky, &self.settings);
        let view = self.view_distance() as f32;
        models::build(&mut self.scene, &self.entities, &self.world, cam.eye, view, sun, cave);
        let t_render = p.millis();
        let pixel = if self.settings.pixel_size == crate::settings::PIXEL_AUTO {
            self.auto_pixel
        } else {
            self.settings.pixel_size
        };
        // Automatic quality may shorten the view distance.
        let mut view_settings = self.settings;
        view_settings.view_distance = self.view_distance();
        render::render(
            p,
            &self.world,
            &sky,
            cam,
            &view_settings,
            target,
            &mut self.scene,
            pixel as usize,
            &mut self.lines,
        );
        let t_bar = p.millis();
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
        let t_end = p.millis();
        self.time_sums[0] += t_bar - t_render;
        self.time_sums[1] += t_render - now;
        self.time_sums[2] += t_end - t_bar;

        self.fps_frames += 1;
        if now - self.fps_since >= 1000 {
            let frames = self.fps_frames.max(1) as u64;
            self.fps = (frames * 1000 / (now - self.fps_since)) as u32;
            for k in 0..3 {
                self.timings[k] = (self.time_sums[k] * 10 / frames) as u32;
            }
            self.time_sums = [0; 3];
            self.fps_frames = 0;
            self.fps_since = now;
            self.adapt_pixel_size();
        }
        true
    }

    /// Automatic pixel size: bigger pixels when the game runs below 12
    /// frames per second, smaller ones again when the estimate for the
    /// sharper size (cost grows with the square of 1 / pixel size) stays
    /// above 15. The gap between the two thresholds stops it flickering.
    fn adapt_pixel_size(&mut self) {
        if self.settings.pixel_size != crate::settings::PIXEL_AUTO || self.fps == 0 {
            return;
        }
        let ps = self.auto_pixel as u32;
        let view = self.view_distance() as u32;
        if self.fps < 12 {
            // Too slow: bigger pixels first, then a shorter view (down to 8).
            if ps < 4 {
                self.auto_pixel += 1;
            } else if view > 8 {
                self.auto_view = (view - 2) as u8;
            }
        } else if view < self.settings.view_distance as u32 {
            // Fast enough again: give back the view distance first (the
            // cost grows about in step with the distance).
            if self.fps * view / (view + 2) >= 15 {
                self.auto_view = (view + 2) as u8;
            }
        } else if ps > 1 {
            // (Full resolution, size 1, is only chosen when it should still
            // run at 15 FPS or more.)
            let sharper = ps - 1;
            let predicted = self.fps * sharper * sharper / (ps * ps);
            if predicted >= 15 {
                self.auto_pixel -= 1;
            }
        }
    }

    /// View distance in use: the setting, shortened by automatic quality
    /// if even the biggest pixels were too slow.
    pub fn view_distance(&self) -> u8 {
        if self.settings.pixel_size == crate::settings::PIXEL_AUTO {
            self.settings.view_distance.min(self.auto_view)
        } else {
            self.settings.view_distance
        }
    }

    /// The targeted block and the face's outward offset (none when a
    /// creature is in the way).
    fn target(&self) -> Option<([i32; 3], [i32; 3])> {
        let reach = if self.mode == Mode::Creative { REACH_CREATIVE } else { REACH_SURVIVAL };
        let hit = render::pick(&self.world, self.player.eye(), self.player.look_dir(), reach)?;
        if let Some((_, tm)) = self.entities.pick(self.player.eye(), self.player.look_dir(), 3.0) {
            let e = self.player.eye();
            let c =
                [hit.0[0] as f32 + 0.5 - e[0], hit.0[1] as f32 + 0.5 - e[1], hit.0[2] as f32 + 0.5 - e[2]];
            if tm * tm < c[0] * c[0] + c[1] * c[1] + c[2] * c[2] {
                return None;
            }
        }
        Some(hit)
    }

    /// The creature the player is aiming at within attack reach (3 blocks,
    /// as on PC), if no block is closer.
    fn mob_target(&self) -> Option<usize> {
        let (eye, dir) = (self.player.eye(), self.player.look_dir());
        let (m, tm) = self.entities.pick(eye, dir, 3.0)?;
        match render::pick(&self.world, eye, dir, tm) {
            Some(_) => None,
            None => Some(m),
        }
    }

    /// Hits creature `m` with the held item. Damage follows Minecraft's
    /// attack cooldown: a full-strength hit needs `20 / attack speed` ticks
    /// since the last one, quicker hits do less (down to 20 %).
    fn attack(&mut self, m: usize) {
        let held = self.held().item;
        let full = 20.0 / attack_speed(held);
        let charge = ((self.attack_ticks as f32 + 0.5) / full).min(1.0);
        let damage = attack_damage(held) * (0.2 + charge * charge * 0.8);
        self.attack_ticks = 0;
        let killed = self.entities.hurt(m, damage, Some(self.player.pos));
        if self.mode == Mode::Survival {
            self.vitals.exhaust(0.1);
            if let Some((tool, _)) = tool_of(held) {
                let s = &mut self.inventory.slots[self.selected];
                s.wear += if tool == Tool::Sword { 1 } else { 2 };
                if s.wear >= durability(s.item) {
                    *s = Stack::EMPTY;
                    self.say("Your tool broke!");
                }
                self.hud_dirty = true;
            }
        }
        if killed {
            self.mining = None;
        }
    }

    /// The use key was just pressed: open a crafting table or furnace if
    /// looking at one, otherwise place or eat (handled in `tick_world`).
    fn use_pressed(&mut self) {
        // Reading a lore fragment item: it reveals a fragment not yet found
        // (or a random one) and is used up.
        if self.held().item == LORE_FRAGMENT {
            let unread: u32 = (0..LORE.len()).filter(|&k| self.lore_found & (1 << k) == 0).count() as u32;
            let r = self.rng.next_u32();
            let pick = if unread > 0 {
                (0..LORE.len())
                    .filter(|&k| self.lore_found & (1 << k) == 0)
                    .nth((r % unread) as usize)
                    .unwrap_or(0)
            } else {
                r as usize % LORE.len()
            };
            if self.mode == Mode::Survival {
                self.inventory.slots[self.selected].take(1);
            }
            self.hud_dirty = true;
            self.read_lore(pick, Screen::Playing);
            return;
        }
        let Some((cell, _)) = self.target() else { return };
        let b = self.world.get(cell[0], cell[1], cell[2]);
        if b == LORE_TABLET {
            let f = crate::structures::fragment_at(self.world.seed, cell[0], cell[1], cell[2]);
            self.read_lore(f, Screen::Playing);
            self.place_cooldown = PLACE_REPEAT;
        } else if b == CRAFTING_TABLE {
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

        // Creatures, items on the ground, arrows.
        self.attack_ticks = self.attack_ticks.saturating_add(1);
        let daylight = Sky::at(self.time).daylight;
        let me = MobTarget { pos: self.player.pos, attackable: survival };
        let ev = self.entities.tick(&mut self.world, me, daylight);
        if ev.damage > 0.0 && survival {
            let (def, tough) = self.inventory.armor_points();
            let dmg = armor_reduce(ev.damage, def, tough);
            if self.vitals.hurt(dmg, ev.cause.unwrap_or(crate::survival::Cause::Fall)) {
                for k in 0..3 {
                    self.player.vel[k] += ev.knock[k];
                }
                if def > 0.0 && self.inventory.wear_armor(ev.damage) {
                    self.say("Your armour broke!");
                }
            }
        }
        if self.entities.pickup(self.player.pos, &mut self.inventory) {
            self.hud_dirty = true;
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
        // A torch or plant standing on this block falls off too.
        let above = self.world.get(cell[0], cell[1] + 1, cell[2]);
        if def(above).layer == Layer::Cross && b != AIR {
            let held = self.held().item;
            self.world.set(cell[0], cell[1] + 1, cell[2], AIR);
            if self.mode == Mode::Survival {
                let at = [cell[0] as f32 + 0.5, cell[1] as f32 + 1.25, cell[2] as f32 + 0.5];
                let mut rng = self.rng;
                let ents = &mut self.entities;
                roll_drops(above, held, &mut rng, |item, n| ents.drop_item(Stack::new(item, n), at, 10));
                self.rng = rng;
            }
        }
        // Drops land on the ground at the block, as on PC.
        let at = [cell[0] as f32 + 0.5, cell[1] as f32 + 0.25, cell[2] as f32 + 0.5];
        if let Some(fi) = self.furnace_index(cell) {
            let f = self.furnaces[fi];
            for s in [f.input, f.fuel, f.output] {
                self.entities.drop_item(s, at, 10);
            }
            self.furnaces[fi] = Furnace::EMPTY;
        }
        if self.entities.block_broken(cell) {
            self.say("The Hollow saw you break its work...");
        }
        if self.mode == Mode::Survival {
            let held = self.held().item;
            let mut rng = self.rng;
            let ents = &mut self.entities;
            roll_drops(b, held, &mut rng, |item, n| ents.drop_item(Stack::new(item, n), at, 10));
            self.rng = rng;
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
        // Aiming at short grass replaces it instead of placing next to it.
        let aimed = self.world.get(cell[0], cell[1], cell[2]);
        let at = if def(aimed).replaceable && def(aimed).layer == Layer::Cross {
            cell
        } else {
            [cell[0] + normal[0], cell[1] + normal[1], cell[2] + normal[2]]
        };
        if !(0..HEIGHT as i32).contains(&at[1]) {
            return;
        }
        if !is_replaceable(self.world.get(at[0], at[1], at[2])) {
            return;
        }
        // Torches and plants stand on top of a solid block (no wall torches yet).
        if def(block).layer == Layer::Cross && !def(self.world.get(at[0], at[1] - 1, at[2])).solid {
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
        let mut right: Text<40> = Text::new();
        if self.settings.show_timings {
            // ms per frame: 3D view, game logic, bottom bar; then the pixel
            // size and view distance in use.
            let [v, g, b] = self.timings;
            let ps = if self.settings.pixel_size == 0 { self.auto_pixel } else { self.settings.pixel_size };
            let _ = write!(right, "3D{} G{} B{} P{} V{} ", v / 10, g / 10, b / 10, ps, self.view_distance());
        } else {
            let armor = self.inventory.armor_points().0 as u32;
            if armor > 0 && self.mode == Mode::Survival {
                let _ = write!(right, "Armor {}  ", armor);
            }
        }
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

/// Wraps a platform so nothing is drawn (used to time the 3D maths alone).
struct NoDisplay<'a>(&'a mut dyn Platform);

impl Platform for NoDisplay<'_> {
    fn push_rect(&mut self, _: u16, _: u16, _: u16, _: u16, _: &[u16]) {}
    fn fill_rect(&mut self, _: u16, _: u16, _: u16, _: u16, _: u16) {}
    fn draw_string(&mut self, _: &str, _: u16, _: u16, _: bool, _: u16, _: u16) {}
    fn keyboard(&mut self) -> u64 {
        self.0.keyboard()
    }
    fn millis(&mut self) -> u64 {
        self.0.millis()
    }
    fn random(&mut self) -> u32 {
        self.0.random()
    }
    fn sleep(&mut self, ms: u32) {
        self.0.sleep(ms)
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

    /// Walks over every item on the ground, picking it up.
    fn collect(g: &mut Game) {
        for k in 0..crate::entities::MAX_ITEMS {
            let it = g.entities.items[k];
            if it.used {
                g.entities.items[k].age = 100;
                g.entities.pickup([it.pos[0], it.pos[1] - 0.5, it.pos[2]], &mut g.inventory);
            }
        }
    }

    #[test]
    fn mining_with_a_tool_drops_and_wears_it() {
        let mut g = game();
        let c = spot(&g);
        g.world.set(c[0], c[1], c[2], STONE);
        g.inventory.slots[0] = Stack::new(WOODEN_PICKAXE, 1);
        g.break_block(c, STONE);
        assert_eq!(g.world.get(c[0], c[1], c[2]), AIR);
        // The cobblestone lies on the ground until picked up.
        assert_eq!(g.inventory.count(COBBLESTONE as Item), 0);
        collect(&mut g);
        assert_eq!(g.inventory.count(COBBLESTONE as Item), 1);
        assert_eq!(g.inventory.slots[0].wear, 1);
        // By hand, stone gives nothing.
        g.selected = 5;
        g.world.set(c[0], c[1], c[2], STONE);
        g.break_block(c, STONE);
        collect(&mut g);
        assert_eq!(g.inventory.count(COBBLESTONE as Item), 1);
    }

    /// Flattens a 9x9 area around the player and returns the player's feet y.
    fn flatten(g: &mut Game) -> i32 {
        let p = g.player.pos;
        let (px, py, pz) = (ifloor(p[0]), ifloor(p[1]), ifloor(p[2]));
        for x in px - 4..=px + 4 {
            for z in pz - 4..=pz + 4 {
                g.world.set(x, py - 1, z, STONE);
                for y in py..py + 4 {
                    g.world.set(x, y, z, AIR);
                }
            }
        }
        py
    }

    #[test]
    fn killing_a_cow_with_a_sword() {
        let mut g = game();
        let y = flatten(&mut g) as f32;
        g.entities.mobs = [crate::entities::Mob::EMPTY; crate::entities::MAX_MOBS];
        let p = g.player.pos;
        let cow = g.entities.spawn(crate::mobs::COW, [p[0], y, p[2] + 2.0]).unwrap();
        g.player.yaw = 0.0; // facing +Z
        g.player.pitch = -0.3;
        assert_eq!(g.mob_target(), Some(cow));
        assert!(g.target().is_none(), "the cow hides the ground behind it");
        g.inventory.slots[0] = Stack::new(DIAMOND_SWORD, 1);
        // Diamond sword: 7 damage at full charge; a cow has 10 health.
        for _ in 0..2 {
            g.attack_ticks = 100;
            g.entities.mobs[cow].hurt = 0;
            g.attack(cow);
        }
        assert!(!g.entities.mobs[cow].used, "the cow died");
        assert!(g.entities.items.iter().any(|it| it.used && it.stack.item == BEEF));
        assert_eq!(g.inventory.slots[0].wear, 2, "swords wear 1 per hit");
        // Spamming does little damage (attack cooldown).
        let c2 = g.entities.spawn(crate::mobs::COW, [p[0], y, p[2] + 2.0]).unwrap();
        g.attack_ticks = 0;
        g.attack(c2);
        assert!(g.entities.mobs[c2].health > 8.0, "uncharged hit: {}", g.entities.mobs[c2].health);
    }

    #[test]
    fn torches_light_and_fall_with_their_block() {
        let mut g = game();
        let floor = flatten(&mut g); // first air layer above the stone floor
        let p = g.player.pos;
        let c = [ifloor(p[0]) + 2, floor, ifloor(p[2])];
        g.inventory.slots[0] = Stack::new(TORCH as Item, 2);
        // Torches can't hang in the air...
        g.place([c[0], c[1] + 2, c[2]], [1, 0, 0]);
        assert_eq!(g.inventory.slots[0].count, 2);
        // ...but stand on the ground.
        g.place([c[0], c[1] - 1, c[2]], [0, 1, 0]);
        assert_eq!(g.world.get(c[0], c[1], c[2]), TORCH);
        let t = crate::render::Torches::near(&g.world, g.player.pos, 20);
        assert_eq!(t.count, 1);
        assert_eq!(t.light(c[0], c[1], c[2]), 256);
        assert!(t.light(c[0] + 5, c[1], c[2]) > 0);
        assert_eq!(t.light(c[0] + 14, c[1], c[2]), 0);
        // Breaking the block under it drops the torch.
        g.break_block([c[0], c[1] - 1, c[2]], STONE);
        assert_eq!(g.world.get(c[0], c[1], c[2]), AIR);
        collect(&mut g);
        assert_eq!(g.inventory.count(TORCH as Item), 2);
    }

    #[test]
    fn automatic_pixel_size() {
        let mut g = game();
        assert_eq!(g.settings.pixel_size, crate::settings::PIXEL_AUTO);
        assert_eq!(g.auto_pixel, 2);
        g.fps = 8;
        g.adapt_pixel_size();
        assert_eq!(g.auto_pixel, 3, "too slow: bigger pixels");
        g.adapt_pixel_size();
        assert_eq!(g.auto_pixel, 4);
        g.fps = 20;
        g.adapt_pixel_size();
        assert_eq!(g.auto_pixel, 4, "20 FPS at 4 would be ~11 at 3: stay");
        g.fps = 40;
        g.adapt_pixel_size();
        assert_eq!(g.auto_pixel, 3, "fast enough: sharper again");
        // A fixed setting is never changed.
        // Still too slow at size 4: the view gets shorter, down to 8.
        g.auto_pixel = 4;
        g.fps = 5;
        for _ in 0..5 {
            g.adapt_pixel_size();
        }
        assert_eq!(g.auto_pixel, 4);
        assert_eq!(g.view_distance(), 8);
        // Faster again: the view distance comes back first.
        g.fps = 30;
        g.adapt_pixel_size();
        assert_eq!(g.view_distance(), 10);
        assert_eq!(g.auto_pixel, 4);
        // A fixed setting is never changed.
        g.settings.pixel_size = 2;
        g.fps = 3;
        g.adapt_pixel_size();
        assert_eq!(g.auto_pixel, 4);
        assert_eq!(g.view_distance(), g.settings.view_distance);
    }

    #[test]
    fn death_drops_the_inventory() {
        let mut g = game();
        g.inventory.slots[0] = Stack::new(DIAMOND, 5);
        g.inventory.armor[1] = Stack::new(IRON_CHESTPLATE, 1);
        g.die();
        assert_eq!(g.inventory.count(DIAMOND), 0);
        let on_ground = g.entities.items.iter().filter(|it| it.used).count();
        assert_eq!(on_ground, 2);
        collect(&mut g);
        assert_eq!(g.inventory.count(DIAMOND), 5);
    }

    #[test]
    fn armour_reduces_creature_damage() {
        // Minecraft: full diamond (20 points, toughness 8) leaves 24 % of a
        // 4-damage hit: 20 - 4 / (2 + 8/4) = 19 effective points → 76 % blocked.
        assert!((armor_reduce(4.0, 20.0, 8.0) - 0.96).abs() < 1e-4);
        assert_eq!(armor_reduce(4.0, 0.0, 0.0), 4.0);
        let mut inv = Inventory::new();
        inv.armor = [
            Stack::new(IRON_HELMET, 1),
            Stack::new(IRON_CHESTPLATE, 1),
            Stack::new(IRON_LEGGINGS, 1),
            Stack::new(IRON_BOOTS, 1),
        ];
        assert_eq!(inv.armor_points().0, 15.0, "full iron = 15 points");
    }

    #[test]
    fn reading_a_tablet_fills_the_journal() {
        let mut g = game();
        let c = spot(&g);
        g.world.set(c[0], c[1], c[2], LORE_TABLET);
        // Look straight at it.
        let e = g.player.eye();
        let d = [c[0] as f32 + 0.5 - e[0], c[1] as f32 + 0.5 - e[1], c[2] as f32 + 0.5 - e[2]];
        let len = crate::math::sqrt(d[0] * d[0] + d[1] * d[1] + d[2] * d[2]);
        g.player.yaw = libm::atan2f(-d[0], d[2]);
        g.player.pitch = libm::asinf(d[1] / len);
        g.use_pressed();
        assert_eq!(g.screen, Screen::Lore);
        assert_eq!(g.lore_found.count_ones(), 1);
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
        flatten(&mut g); // no grass tuft in the way
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
        collect(&mut g);
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
