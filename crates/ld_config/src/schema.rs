//! The settings schema: one [`SettingDef`] per setting.
//!
//! Descriptions are written for players, not programmers: say what it does,
//! what it looks/feels like, and when to change it.

use crate::{Backend, Difficulty, GameModeSetting, Settings};
use ld_input::Action;

/// A setting's value in a type-erased form, used for loading, saving and menus.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Bool(bool),
    Int(i64),
    Float(f64),
    Text(String),
}

/// What kind of value a setting holds, and its valid range.
#[derive(Debug, Clone, Copy)]
pub enum Kind {
    Bool,
    Int {
        min: i64,
        max: i64,
    },
    Float {
        min: f64,
        max: f64,
    },
    /// One of a fixed list of strings.
    Choice(&'static [&'static str]),
    /// An input name such as `"KeyW"` (see `ld_input`).
    Key,
}

/// When a change takes effect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Apply {
    Immediately,
    OnRestart,
}

/// How to read/write the setting on a [`Settings`] value.
#[derive(Clone, Copy)]
pub enum Access {
    Field { get: fn(&Settings) -> Value, set: fn(&mut Settings, Value) },
    Keybind(Action),
}

/// The complete definition of one setting.
#[derive(Clone, Copy)]
pub struct SettingDef {
    /// TOML table, also the settings-menu tab (`graphics`, `controls`, …).
    pub section: &'static str,
    /// Key inside the section. Never rename: it would reset players' choice.
    pub name: &'static str,
    /// Short name for menus.
    pub label: &'static str,
    /// Plain-language explanation (tooltip + config comment).
    pub description: &'static str,
    /// Performance cost, e.g. "none" or "high on integrated GPUs".
    pub performance: &'static str,
    pub kind: Kind,
    pub apply: Apply,
    pub access: Access,
}

impl SettingDef {
    /// Reads this setting's current value.
    pub fn get(&self, s: &Settings) -> Value {
        match self.access {
            Access::Field { get, .. } => get(s),
            Access::Keybind(a) => Value::Text(s.keybinds.input_for(a).unwrap_or("").to_string()),
        }
    }

    /// Writes a value, after validating it against [`Kind`]. Numbers outside
    /// the range are clamped; a value of the wrong type is rejected.
    pub fn set(&self, s: &mut Settings, v: Value) -> Result<(), String> {
        let v = self.validate(v)?;
        match self.access {
            Access::Field { set, .. } => set(s, v),
            Access::Keybind(a) => {
                if let Value::Text(t) = v {
                    s.keybinds.bind(a, &t);
                }
            }
        }
        Ok(())
    }

    fn validate(&self, v: Value) -> Result<Value, String> {
        match (self.kind, v) {
            (Kind::Bool, v @ Value::Bool(_)) => Ok(v),
            (Kind::Int { min, max }, Value::Int(i)) => Ok(Value::Int(i.clamp(min, max))),
            (Kind::Float { min, max }, Value::Float(f)) if f.is_finite() => {
                Ok(Value::Float(f.clamp(min, max)))
            }
            // Accept `70` where `70.0` is expected; people write it that way.
            (Kind::Float { min, max }, Value::Int(i)) => Ok(Value::Float((i as f64).clamp(min, max))),
            (Kind::Choice(opts), Value::Text(t)) => opts
                .iter()
                .find(|o| o.eq_ignore_ascii_case(&t))
                .map(|o| Value::Text(o.to_string()))
                .ok_or_else(|| format!("must be one of {}", opts.join(", "))),
            (Kind::Key, v @ Value::Text(_)) => Ok(v),
            (k, v) => Err(format!("expected {k:?}, got {v:?}")),
        }
    }

    /// A one-line summary of range/default for the config file and tooltips.
    pub fn range_text(&self) -> String {
        match self.kind {
            Kind::Bool => "true or false".into(),
            Kind::Int { min, max } => format!("{min} – {max}"),
            Kind::Float { min, max } => format!("{min} – {max}"),
            Kind::Choice(opts) => opts.join(" | "),
            Kind::Key => "a key name, e.g. \"KeyW\", \"Space\", \"F3\", \"MouseLeft\"; \"\" = unbound".into(),
        }
    }
}

// --- Accessor helpers -------------------------------------------------------
// Each macro builds the get/set function pair for one struct field. Closures
// that capture nothing convert to plain `fn` pointers, so this costs nothing.

macro_rules! bool_field {
    ($($p:ident).+) => {
        Access::Field {
            get: |s| Value::Bool(s.$($p).+),
            set: |s, v| if let Value::Bool(b) = v { s.$($p).+ = b },
        }
    };
}
macro_rules! int_field {
    ($($p:ident).+ as $t:ty) => {
        Access::Field {
            get: |s| Value::Int(s.$($p).+ as i64),
            set: |s, v| if let Value::Int(i) = v { s.$($p).+ = i as $t },
        }
    };
}
macro_rules! float_field {
    ($($p:ident).+ as $t:ty) => {
        Access::Field {
            get: |s| Value::Float(s.$($p).+ as f64),
            set: |s, v| if let Value::Float(f) = v { s.$($p).+ = f as $t },
        }
    };
}
macro_rules! choice_field {
    ($($p:ident).+ : $e:ty) => {
        Access::Field {
            get: |s| Value::Text(s.$($p).+.as_str().to_string()),
            set: |s, v| if let Value::Text(t) = v {
                if let Some(c) = <$e>::parse(&t) { s.$($p).+ = c; }
            },
        }
    };
}

/// Every setting in the game, in menu order.
pub fn schema() -> Vec<SettingDef> {
    let mut defs = vec![
        // ---------------- Graphics ----------------
        SettingDef {
            section: "graphics",
            name: "backend",
            label: "Graphics API",
            description: "Which graphics system draws the game. \"auto\" tries Vulkan first, then \
                DirectX 12 (Windows) or Metal (macOS), then OpenGL. Force \"opengl\" only if the \
                game fails to start or shows glitches on very old graphics cards.",
            performance: "Vulkan is usually fastest; OpenGL is the most compatible.",
            kind: Kind::Choice(Backend::OPTIONS),
            apply: Apply::OnRestart,
            access: choice_field!(graphics.backend: Backend),
        },
        SettingDef {
            section: "graphics",
            name: "vsync",
            label: "VSync",
            description: "Synchronises frames with your monitor's refresh rate. Removes screen \
                tearing and saves power, but adds a little input delay.",
            performance: "Caps FPS at your refresh rate.",
            kind: Kind::Bool,
            apply: Apply::Immediately,
            access: bool_field!(graphics.vsync),
        },
        SettingDef {
            section: "graphics",
            name: "fps_limit",
            label: "Max framerate",
            description: "Highest frames per second the game will draw. 0 means unlimited. \
                Limiting FPS reduces heat, fan noise and battery use on laptops.",
            performance: "Lower values free up CPU/GPU.",
            kind: Kind::Int { min: 0, max: 1000 },
            apply: Apply::Immediately,
            access: int_field!(graphics.fps_limit as u32),
        },
        SettingDef {
            section: "graphics",
            name: "fov",
            label: "Field of view",
            description: "How wide your view is, in degrees (vertical). 70 matches the classic \
                default; higher values show more around you but distort the edges.",
            performance: "Higher FOV draws slightly more of the world.",
            kind: Kind::Float { min: 30.0, max: 110.0 },
            apply: Apply::Immediately,
            access: float_field!(graphics.fov as f32),
        },
        SettingDef {
            section: "graphics",
            name: "fullscreen",
            label: "Fullscreen",
            description: "Starts the game in borderless fullscreen instead of a window. You can \
                also switch at any time with the fullscreen key (F11 by default).",
            performance: "none",
            kind: Kind::Bool,
            apply: Apply::Immediately,
            access: bool_field!(graphics.fullscreen),
        },
        SettingDef {
            section: "graphics",
            name: "fog",
            label: "Distance fog",
            description: "Fades distant terrain into the sky colour, hiding the edge of the \
                loaded world.",
            performance: "Negligible.",
            kind: Kind::Bool,
            apply: Apply::Immediately,
            access: bool_field!(graphics.fog),
        },
        SettingDef {
            section: "graphics",
            name: "render_distance",
            label: "Render distance",
            description: "How many chunks (16-block columns) are loaded and drawn in every direction \
                around you. 8 chunks = 128 blocks. The single biggest performance setting.",
            performance: "High: CPU, GPU and memory all grow with the square of this value. \
                Old/integrated GPUs: 6–8. Mid-range: 12–16. High-end: 24+.",
            kind: Kind::Int { min: 2, max: 32 },
            apply: Apply::Immediately,
            access: int_field!(graphics.render_distance as u32),
        },
        SettingDef {
            section: "graphics",
            name: "mipmap_levels",
            label: "Mipmap levels",
            description: "Pre-shrunk copies of block textures used for distant blocks. Removes the \
                flickering 'noise' of far-away terrain. 0 turns mipmaps off.",
            performance: "Very small (a little extra video memory).",
            kind: Kind::Int { min: 0, max: 4 },
            apply: Apply::OnRestart,
            access: int_field!(graphics.mipmap_levels as u32),
        },
        // ---------------- Performance ----------------
        SettingDef {
            section: "performance",
            name: "worker_threads",
            label: "Background threads",
            description: "CPU threads used to generate terrain and build chunk meshes in the \
                background. 0 = automatic (number of CPU cores minus one).",
            performance: "More threads load the world faster but leave less CPU for other programs.",
            kind: Kind::Int { min: 0, max: 64 },
            apply: Apply::OnRestart,
            access: int_field!(performance.worker_threads as u32),
        },
        SettingDef {
            section: "performance",
            name: "chunk_uploads_per_frame",
            label: "Chunk uploads per frame",
            description: "Maximum number of finished chunk meshes sent to the graphics card each \
                frame. Lower values give smoother framerates while the world loads; higher \
                values make terrain appear faster.",
            performance: "Lower = fewer stutters, slower loading.",
            kind: Kind::Int { min: 1, max: 256 },
            apply: Apply::Immediately,
            access: int_field!(performance.chunk_uploads_per_frame as u32),
        },
        // ---------------- Gameplay ----------------
        SettingDef {
            section: "gameplay",
            name: "difficulty",
            label: "Difficulty",
            description: "Peaceful: no hunger loss and health regenerates. Easy: starving stops at \
                5 hearts. Normal: starving stops at half a heart. Hard: starvation can kill. \
                (From Phase 7 it also controls how dangerous creatures are.)",
            performance: "none",
            kind: Kind::Choice(Difficulty::OPTIONS),
            apply: Apply::Immediately,
            access: choice_field!(gameplay.difficulty: Difficulty),
        },
        SettingDef {
            section: "gameplay",
            name: "game_mode",
            label: "Starting game mode",
            description: "Survival: gather, craft, eat and stay alive. Creative: unlimited blocks, \
                flying, no damage. You can switch at any time with F4 (development shortcut).",
            performance: "none",
            kind: Kind::Choice(GameModeSetting::OPTIONS),
            apply: Apply::OnRestart,
            access: choice_field!(gameplay.game_mode: GameModeSetting),
        },
        SettingDef {
            section: "gameplay",
            name: "daylight_cycle",
            label: "Day/night cycle",
            description: "Whether time passes. When off, the time of day stays where it is \
                (always day in a new world).",
            performance: "none",
            kind: Kind::Bool,
            apply: Apply::Immediately,
            access: bool_field!(gameplay.daylight_cycle),
        },
        SettingDef {
            section: "gameplay",
            name: "day_length_minutes",
            label: "Day length (minutes)",
            description: "How many real minutes one full day and night takes. Minecraft's is 20 \
                (10 of daylight, 7 of night, plus dusk and dawn). Creatures and furnaces are \
                not affected, only the sun.",
            performance: "none",
            kind: Kind::Int { min: 1, max: 240 },
            apply: Apply::Immediately,
            access: int_field!(gameplay.day_length_minutes as u32),
        },
        // ---------------- Controls ----------------
        SettingDef {
            section: "controls",
            name: "mouse_sensitivity",
            label: "Mouse sensitivity",
            description: "How fast the camera turns when you move the mouse, in percent. \
                100 is the default speed.",
            performance: "none",
            kind: Kind::Int { min: 1, max: 200 },
            apply: Apply::Immediately,
            access: int_field!(controls.mouse_sensitivity as u32),
        },
        SettingDef {
            section: "controls",
            name: "invert_mouse",
            label: "Invert mouse",
            description: "Moving the mouse forward looks down instead of up (flight-sim style).",
            performance: "none",
            kind: Kind::Bool,
            apply: Apply::Immediately,
            access: bool_field!(controls.invert_mouse),
        },
        SettingDef {
            section: "controls",
            name: "toggle_sprint",
            label: "Sprint mode: toggle",
            description: "If true, tap the sprint key once to start sprinting instead of holding \
                it. Sprinting stops when you stop moving forward.",
            performance: "none",
            kind: Kind::Bool,
            apply: Apply::Immediately,
            access: bool_field!(controls.toggle_sprint),
        },
        SettingDef {
            section: "controls",
            name: "toggle_sneak",
            label: "Sneak mode: toggle",
            description: "If true, tap the sneak key to start/stop sneaking instead of holding it.",
            performance: "none",
            kind: Kind::Bool,
            apply: Apply::Immediately,
            access: bool_field!(controls.toggle_sneak),
        },
        // ---------------- Interface ----------------
        SettingDef {
            section: "interface",
            name: "gui_scale",
            label: "GUI scale",
            description: "Size of menus, text and overlays. Increase on high-resolution or small \
                screens.",
            performance: "none",
            kind: Kind::Float { min: 0.5, max: 4.0 },
            apply: Apply::Immediately,
            access: float_field!(interface.gui_scale as f32),
        },
        SettingDef {
            section: "interface",
            name: "show_debug_overlay",
            label: "Debug overlay at start",
            description: "Shows the debug & performance overlay (FPS, frame time, position, GPU) \
                when the game starts. Toggle in-game with F3.",
            performance: "Very small.",
            kind: Kind::Bool,
            apply: Apply::OnRestart,
            access: bool_field!(interface.show_debug_overlay),
        },
    ];

    // ---------------- Key bindings (generated from ld_input) ----------------
    for &a in Action::ALL {
        defs.push(SettingDef {
            section: "keybinds",
            name: a.config_key(),
            label: a.label(),
            description: a.description(),
            performance: "none",
            kind: Kind::Key,
            apply: Apply::Immediately,
            access: Access::Keybind(a),
        });
    }
    defs
}
