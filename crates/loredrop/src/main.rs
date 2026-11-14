//! Loredrop executable entry point.
//!
//! This crate wires the engine crates together and should stay thin:
//! * `app`        — window, event loop, frame timing (winit)
//! * `game`       — the fixed-rate simulation (player, game mode rules)
//! * `world`      — loaded chunks + background generation/meshing pipeline
//! * `overlay`    — debug/performance overlay and crosshair (egui)
//! * `frame_stats`— FPS / frame-time measurement
//!
//! Command-line options (mainly for testing):
//! ```text
//! --settings <file>          settings file to use (default: settings.toml)
//! --screenshot-after <n>     save a screenshot after n frames, then exit
//! --screenshot-path <file>   where to save that screenshot (default: autotest.png)
//! --autopilot                walk/jump automatically (tests the game loop)
//! --exit-after <seconds>     quit automatically after a time
//! --bench-world <distance>   no window: time generating + meshing that render distance
//! --seed <number or text>    world seed (default: random)
//! --pos <x> <y> <z>          start at this position (developer option)
//! --look <yaw> <pitch>       start looking this way (developer option)
//! --fly                      start flying (developer option)
//! --map <blocks> <file.png>  no window: draw a top-down map of the world around spawn
//! ```

mod app;
mod frame_stats;
mod game;
mod overlay;
mod settings_file;
mod world;

use std::path::PathBuf;

/// Options parsed from the command line.
#[derive(Debug, Clone)]
pub struct Options {
    pub settings_path: PathBuf,
    pub screenshot_after: Option<u64>,
    pub screenshot_path: PathBuf,
    pub autopilot: bool,
    pub exit_after: Option<f64>,
    pub bench_world: Option<u32>,
    pub seed: Option<u64>,
    pub start_pos: Option<[f64; 3]>,
    pub start_look: Option<[f32; 2]>,
    pub fly: bool,
    pub map: Option<(u32, PathBuf)>,
}

fn parse_args() -> Result<Options, String> {
    let mut o = Options {
        settings_path: "settings.toml".into(),
        screenshot_after: None,
        screenshot_path: "autotest.png".into(),
        autopilot: false,
        exit_after: None,
        bench_world: None,
        seed: None,
        start_pos: None,
        start_look: None,
        fly: false,
        map: None,
    };
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        let mut value = |name: &str| args.next().ok_or(format!("{name} needs a value"));
        match a.as_str() {
            "--settings" => o.settings_path = value("--settings")?.into(),
            "--screenshot-after" => {
                o.screenshot_after =
                    Some(value("--screenshot-after")?.parse().map_err(|_| "bad frame count")?)
            }
            "--screenshot-path" => o.screenshot_path = value("--screenshot-path")?.into(),
            "--autopilot" => o.autopilot = true,
            "--exit-after" => o.exit_after = Some(value("--exit-after")?.parse().map_err(|_| "bad seconds")?),
            "--bench-world" => {
                o.bench_world = Some(value("--bench-world")?.parse().map_err(|_| "bad render distance")?)
            }
            "--seed" => o.seed = Some(parse_seed(&value("--seed")?)),
            "--pos" => {
                let mut v = [0.0; 3];
                for c in &mut v {
                    *c = args.next().and_then(|a| a.parse().ok()).ok_or("--pos needs x y z")?;
                }
                o.start_pos = Some(v);
            }
            "--look" => {
                let mut v = [0.0; 2];
                for c in &mut v {
                    *c = args.next().and_then(|a| a.parse().ok()).ok_or("--look needs yaw pitch")?;
                }
                o.start_look = Some(v);
            }
            "--fly" => o.fly = true,
            "--map" => {
                let size = args.next().and_then(|a| a.parse().ok()).ok_or("--map needs <blocks> <file>")?;
                let file = args.next().ok_or("--map needs <blocks> <file>")?;
                o.map = Some((size, file.into()));
            }
            "--help" | "-h" => {
                println!("{}", include_str!("help.txt"));
                std::process::exit(0);
            }
            other => return Err(format!("unknown option {other} (try --help)")),
        }
    }
    Ok(o)
}

/// Numbers are used as-is; any other text is hashed into a number, so
/// "my cool world" is a valid, repeatable seed.
fn parse_seed(s: &str) -> u64 {
    if let Ok(n) = s.trim().parse::<i64>() {
        return n as u64;
    }
    // FNV-1a: simple, stable across platforms and versions.
    s.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ b as u64).wrapping_mul(0x0100_0000_01b3))
}

fn main() {
    // Our crates log at info; the GPU libraries only report warnings and errors.
    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("info,wgpu_core=warn,wgpu_hal=warn,naga=warn"),
    )
    .init();

    let options = match parse_args() {
        Ok(o) => o,
        Err(e) => {
            eprintln!("loredrop: {e}");
            std::process::exit(2);
        }
    };
    log::info!("Loredrop {} starting", env!("CARGO_PKG_VERSION"));
    let settings = settings_file::load_or_create(&options.settings_path);

    if let Some((size, path)) = &options.map {
        app::draw_map(*size, path, &settings, options.seed);
        return;
    }
    if let Some(rd) = options.bench_world {
        app::bench_world(rd, &settings, options.seed);
        return;
    }
    if let Err(e) = app::run(options, settings) {
        log::error!("{e}");
        eprintln!("Loredrop could not start: {e}");
        std::process::exit(1);
    }
}
