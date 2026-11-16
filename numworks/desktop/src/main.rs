//! PC test harness for the Loredrop NumWorks edition.
//!
//! Runs the exact same game code as the calculator against a fake screen and
//! keyboard, so the game can be tested without a calculator:
//!
//! ```text
//! ld_nw_desktop shot  [options] --out file.png   play a script, save the screen
//! ld_nw_desktop bench [options]                  time the 3D view
//! ld_nw_desktop map   [--seed N] --out file.png  top-down biome/height map
//! ```
//!
//! Options:
//! * `--seed N`            world seed (default 1)
//! * `--creative`          start in creative mode
//! * `--script STEPS`      keys to hold, e.g. `8:40,Right+8:10,Ok:30`
//!   (`key[+key...]:frames`, comma-separated; `-` = no key). Each frame is
//!   50 ms of game time (one tick).
//! * `--look YAW PITCH`    set the view (degrees) after loading
//! * `--time TICKS`        set the time of day after loading
//! * `--give ITEMS`        fill the inventory, e.g. `oak_log:10,wooden_pickaxe`
//! * `--summon KINDS`      creatures in a row 4 blocks in front, e.g. `cow,zombie`
//!   (they are placed after `--pos`/`--look`)
//! * `--health N`, `--food N`  set health / food points (0–20)
//! * `--pos X Y Z`         move the player (feet) after loading; with
//!   `--creative` the player starts flying there
//! * `--pixel N`           pixel size setting (2 or 4)
//! * `--view N`            view distance setting
//! * `--frames N`          bench: frames to render (default 60)
//!
//! Host timings say nothing about calculator speed in absolute terms; they
//! are only useful to compare two versions of the renderer.

mod font_gen;

use ld_nw_core::game::{Game, Mode, Screen};
use ld_nw_core::input::{Key, keys};
use ld_nw_core::{Platform, SCREEN_H, SCREEN_W};
use std::time::Instant;

/// Fake calculator: a frame buffer, scripted keys and simulated time.
struct Desktop {
    fb: Vec<u16>,
    keys: u64,
    ms: u64,
    rng: u32,
}

impl Platform for Desktop {
    fn push_rect(&mut self, x: u16, y: u16, w: u16, h: u16, pixels: &[u16]) {
        for r in 0..h as usize {
            for c in 0..w as usize {
                let (sx, sy) = (x as usize + c, y as usize + r);
                if sx < SCREEN_W && sy < SCREEN_H {
                    self.fb[sy * SCREEN_W + sx] = pixels[r * w as usize + c];
                }
            }
        }
    }

    fn fill_rect(&mut self, x: u16, y: u16, w: u16, h: u16, color: u16) {
        for sy in y as usize..(y as usize + h as usize).min(SCREEN_H) {
            for sx in x as usize..(x as usize + w as usize).min(SCREEN_W) {
                self.fb[sy * SCREEN_W + sx] = color;
            }
        }
    }

    fn draw_string(&mut self, text: &str, x: u16, y: u16, large: bool, fg: u16, bg: u16) {
        let (w, h) = if large { (10, 18) } else { (7, 14) };
        for (i, ch) in text.chars().enumerate() {
            let code = ch as u32;
            let idx = if (32..127).contains(&code) { code as usize - 32 } else { b'?' as usize - 32 };
            for r in 0..h {
                let mask = if large { font_gen::LARGE[idx][r] } else { font_gen::SMALL[idx][r] };
                for c in 0..w {
                    let (sx, sy) = (x as usize + i * w + c, y as usize + r);
                    if sx < SCREEN_W && sy < SCREEN_H {
                        self.fb[sy * SCREEN_W + sx] = if mask >> c & 1 == 1 { fg } else { bg };
                    }
                }
            }
        }
    }

    fn keyboard(&mut self) -> u64 {
        self.keys
    }

    fn millis(&mut self) -> u64 {
        self.ms
    }

    fn random(&mut self) -> u32 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 17;
        self.rng ^= self.rng << 5;
        self.rng
    }

    fn sleep(&mut self, _ms: u32) {}
}

impl Desktop {
    fn save_png(&self, path: &str) {
        let file = std::fs::File::create(path).unwrap_or_else(|e| panic!("can't create {path}: {e}"));
        let mut enc = png::Encoder::new(std::io::BufWriter::new(file), SCREEN_W as u32, SCREEN_H as u32);
        enc.set_color(png::ColorType::Rgb);
        enc.set_depth(png::BitDepth::Eight);
        let mut w = enc.write_header().unwrap();
        let mut data = Vec::with_capacity(SCREEN_W * SCREEN_H * 3);
        for &c in &self.fb {
            let (r, g, b) = ((c >> 11) & 31, (c >> 5) & 63, c & 31);
            data.extend_from_slice(&[
                (r << 3 | r >> 2) as u8,
                (g << 2 | g >> 4) as u8,
                (b << 3 | b >> 2) as u8,
            ]);
        }
        w.write_image_data(&data).unwrap();
    }
}

fn key_by_name(name: &str) -> Key {
    use Key::*;
    match name {
        "Left" => Left,
        "Up" => Up,
        "Down" => Down,
        "Right" => Right,
        "Ok" => Ok,
        "Back" => Back,
        "Home" => Home,
        "Exe" => Exe,
        "Toolbox" => Toolbox,
        "Var" => Var,
        "Backspace" => Backspace,
        "(" => LeftParen,
        ")" => RightParen,
        "0" => Zero,
        "1" => One,
        "2" => Two,
        "3" => Three,
        "4" => Four,
        "5" => Five,
        "6" => Six,
        "7" => Seven,
        "8" => Eight,
        "9" => Nine,
        other => panic!("unknown key '{other}'"),
    }
}

/// Parses `key+key:frames,...` into (key state, frame count) steps.
fn parse_script(s: &str) -> Vec<(u64, u32)> {
    s.split(',')
        .filter(|p| !p.trim().is_empty())
        .map(|step| {
            let (k, n) = step.trim().rsplit_once(':').unwrap_or((step, "1"));
            let held: Vec<Key> = if k == "-" { vec![] } else { k.split('+').map(key_by_name).collect() };
            (keys(&held), n.parse().expect("frame count"))
        })
        .collect()
}

struct Options {
    seed: u32,
    creative: bool,
    script: String,
    look: Option<(f32, f32)>,
    time: Option<u32>,
    pixel: Option<u8>,
    view: Option<u8>,
    frames: u32,
    out: String,
    pos: Option<[f32; 3]>,
    give: String,
    summon: String,
    health: Option<f32>,
    food: Option<u8>,
}

fn parse_args(args: &[String]) -> Options {
    let mut o = Options {
        seed: 1,
        creative: false,
        script: String::new(),
        look: None,
        time: None,
        pixel: None,
        view: None,
        frames: 60,
        out: "shot.png".into(),
        pos: None,
        give: String::new(),
        summon: String::new(),
        health: None,
        food: None,
    };
    let mut i = 0;
    let next = |i: &mut usize| -> String {
        *i += 1;
        args.get(*i).cloned().expect("missing option value")
    };
    while i < args.len() {
        match args[i].as_str() {
            "--seed" => o.seed = next(&mut i).parse().unwrap(),
            "--creative" => o.creative = true,
            "--script" => o.script = next(&mut i),
            "--look" => {
                let y = next(&mut i).parse().unwrap();
                let p = next(&mut i).parse().unwrap();
                o.look = Some((y, p));
            }
            "--time" => o.time = Some(next(&mut i).parse().unwrap()),
            "--give" => o.give = next(&mut i),
            "--summon" => o.summon = next(&mut i),
            "--health" => o.health = Some(next(&mut i).parse().unwrap()),
            "--food" => o.food = Some(next(&mut i).parse().unwrap()),
            "--pos" => {
                let x = next(&mut i).parse().unwrap();
                let y = next(&mut i).parse().unwrap();
                let z = next(&mut i).parse().unwrap();
                o.pos = Some([x, y, z]);
            }
            "--pixel" => o.pixel = Some(next(&mut i).parse().unwrap()),
            "--view" => o.view = Some(next(&mut i).parse().unwrap()),
            "--frames" => o.frames = next(&mut i).parse().unwrap(),
            "--out" => o.out = next(&mut i),
            other => panic!("unknown option {other}"),
        }
        i += 1;
    }
    o
}

/// Builds a game already in the world (loading finished).
fn start(o: &Options, d: &mut Desktop) -> Box<Game> {
    let mut g = Box::new(Game::new());
    g.new_world(o.seed, if o.creative { Mode::Creative } else { Mode::Survival });
    while g.screen == Screen::Loading {
        g.frame(d);
        d.ms += 50;
    }
    if let Some((yaw, pitch)) = o.look {
        g.player.yaw = yaw.to_radians();
        g.player.pitch = pitch.to_radians();
    }
    if let Some(t) = o.time {
        g.time = t;
    }
    // `--give oak_log:10,wooden_pickaxe`: item names as shown in game,
    // with underscores for spaces (case doesn't matter).
    for part in o.give.split(',').filter(|s| !s.is_empty()) {
        use ld_nw_core::items::{ITEM_COUNT, Item, Stack, item_def};
        let (name, n) = part.split_once(':').unwrap_or((part, "1"));
        let wanted = name.replace('_', " ").to_lowercase();
        let item = (1..ITEM_COUNT as Item)
            .find(|&i| item_def(i).name.to_lowercase() == wanted)
            .unwrap_or_else(|| panic!("unknown item '{name}'"));
        g.inventory.add(Stack::new(item, n.parse().expect("count")));
    }
    if let Some(h) = o.health {
        g.vitals.health = h;
    }
    if let Some(f) = o.food {
        g.vitals.food = f;
    }
    if let Some(pos) = o.pos {
        g.player.pos = pos;
        g.player.flying = o.creative;
        while g.world.update(pos[0] as i32, pos[2] as i32, usize::MAX) > 0 {}
    }
    if let Some(p) = o.pixel {
        g.settings.pixel_size = p;
    }
    if let Some(v) = o.view {
        g.settings.view_distance = v;
    }
    // Creatures in a row in front of the player, standing on the ground,
    // facing the player.
    let kinds: Vec<&str> = o.summon.split(',').filter(|s| !s.is_empty()).collect();
    let fwd = [-g.player.yaw.sin(), g.player.yaw.cos()];
    let side = [fwd[1], -fwd[0]];
    for (n, name) in kinds.iter().enumerate() {
        use ld_nw_core::mobs::{MOB_COUNT, mob_def};
        let k = (0..MOB_COUNT as u8)
            .find(|&k| mob_def(k).name.eq_ignore_ascii_case(name))
            .unwrap_or_else(|| panic!("unknown creature '{name}'"));
        let off = (n as f32 - (kinds.len() as f32 - 1.0) / 2.0) * 2.2;
        let x = g.player.pos[0] + fwd[0] * 4.5 + side[0] * off;
        let z = g.player.pos[2] + fwd[1] * 4.5 + side[1] * off;
        let (bx, bz) = (x.floor() as i32, z.floor() as i32);
        let y =
            (0..47).rev().find(|&y| ld_nw_core::blocks::def(g.world.get(bx, y, bz)).solid).unwrap_or(20) + 1;
        if let Some(i) = g.entities.spawn(k, [x, y as f32, z]) {
            g.entities.mobs[i].yaw = g.player.yaw + std::f32::consts::PI;
        }
    }
    g
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cmd = args.first().cloned().unwrap_or_default();
    let o = parse_args(&args[1.min(args.len())..]);
    let mut d = Desktop { fb: vec![0; SCREEN_W * SCREEN_H], keys: 0, ms: 0, rng: o.seed.max(1) };
    match cmd.as_str() {
        "shot" => {
            let mut g = start(&o, &mut d);
            for (k, n) in parse_script(&o.script) {
                d.keys = k;
                for _ in 0..n {
                    g.frame(&mut d);
                    d.ms += 50;
                }
            }
            // One more frame with the last keys still held (so e.g. mining
            // cracks are visible), unless there was no script.
            g.frame(&mut d);
            d.save_png(&o.out);
            let p = g.player.pos;
            let mobs: Vec<&str> = g
                .entities
                .mobs
                .iter()
                .filter(|m| m.used)
                .map(|m| ld_nw_core::mobs::mob_def(m.kind).name)
                .collect();
            println!(
                "saved {} (player at {:.1} {:.1} {:.1}, health {}, screen {:?}, creatures {:?})",
                o.out, p[0], p[1], p[2], g.vitals.health, g.screen, mobs
            );
        }
        "title" => {
            let mut g = Box::new(Game::new());
            for (k, n) in parse_script(&o.script) {
                d.keys = k;
                for _ in 0..n {
                    g.frame(&mut d);
                    d.ms += 50;
                }
            }
            d.keys = 0;
            g.frame(&mut d);
            d.save_png(&o.out);
            println!("saved {} (screen {:?})", o.out, g.screen);
        }
        "bench" => {
            let mut g = start(&o, &mut d);
            let t0 = Instant::now();
            d.keys = keys(&[Key::Right]);
            for _ in 0..o.frames {
                g.frame(&mut d);
                d.ms += 50;
            }
            let per = t0.elapsed().as_secs_f64() * 1000.0 / o.frames as f64;
            println!(
                "{} frames, {:.2} ms per frame on this PC (pixel size {}, view {})",
                o.frames,
                per,
                if g.settings.pixel_size == 0 { g.auto_pixel } else { g.settings.pixel_size },
                g.settings.view_distance
            );
        }
        "find" => {
            // Prints a deep-water spot and a cave spot near the origin, for
            // use with `--pos` in screenshots.
            use ld_nw_core::blocks::AIR;
            use ld_nw_core::worldgen::{HEIGHT, SEA_LEVEL, column_info, generate_column};
            let mut col = [AIR; HEIGHT];
            let (mut water, mut cave) = (None, None);
            'outer: for r in 0..400i32 {
                for i in -r..=r {
                    for (x, z) in [(i * 2, -r * 2), (r * 2, i * 2)] {
                        let info = column_info(o.seed, x, z);
                        if water.is_none() && info.height < SEA_LEVEL - 6 {
                            water = Some((x, z));
                        }
                        if cave.is_none() && info.height > SEA_LEVEL {
                            generate_column(o.seed, x, z, &mut col);
                            // Three air blocks in a row well under the surface.
                            if let Some(y) = (6..info.height as usize - 6)
                                .find(|&y| col[y..y + 3].iter().all(|&b| b == AIR))
                            {
                                cave = Some((x, y, z));
                            }
                        }
                        if water.is_some() && cave.is_some() {
                            break 'outer;
                        }
                    }
                }
            }
            println!("deep water column: {water:?}  cave air at: {cave:?}");
            let ruin = ld_nw_core::structures::nearest_tablet(o.seed, 0, 0, true);
            let vault = ld_nw_core::structures::nearest_tablet(o.seed, 0, 0, false);
            println!("nearest surface ruin tablet: {ruin:?}  deep vault tablet: {vault:?}");
        }
        "map" => {
            use ld_nw_core::worldgen::{Biome, SEA_LEVEL, column_info};
            for y in 0..SCREEN_H {
                for x in 0..SCREEN_W {
                    let (wx, wz) = ((x as i32 - 160) * 8, (y as i32 - 120) * 8);
                    let info = column_info(o.seed, wx, wz);
                    let base: u32 = match info.biome {
                        Biome::Ocean => 0x2F5FC8,
                        Biome::Beach => 0xE0D090,
                        Biome::Plains => 0x7DB04A,
                        Biome::Forest => 0x3E7A2A,
                        Biome::Desert => 0xD8C078,
                        Biome::Snowy => 0xE8F0F8,
                        Biome::Mountains => 0x8A8A8A,
                    };
                    let shade = (180 + (info.height - SEA_LEVEL) * 3).clamp(100, 300) as u32;
                    d.fb[y * SCREEN_W + x] = ld_nw_core::color::scale(ld_nw_core::color::hex(base), shade);
                }
            }
            d.save_png(&o.out);
            println!("saved {} (8 blocks per pixel, origin at centre)", o.out);
        }
        _ => {
            eprintln!("usage: ld_nw_desktop shot|title|bench|map [options]  (see source header)");
            std::process::exit(2);
        }
    }
}
