//! # ld_config — the settings system
//!
//! **Every setting is defined exactly once**, in [`schema()`]. That one
//! definition carries the setting's label, a plain-language description, its
//! performance cost, its valid range, and how to read/write it. From it we get:
//!
//! * the commented `settings.toml` file ([`Settings::to_toml`]),
//! * validation when loading ([`Settings::from_toml`]),
//! * (Phase 11) the in-game settings menu with tooltips.
//!
//! So a setting can never exist without its explanation.
//!
//! The typed [`Settings`] struct is what game code reads (`settings.graphics.fov`).
//! Its `Default` impl is the single source of default values.
//!
//! ## Adding a setting
//! 1. Add a field to the right `*Settings` struct and give it a default.
//! 2. Add a [`SettingDef`] entry in [`schema()`] using one of the field macros.
//! 3. Read it where it's needed. A unit test checks every entry round-trips.
//!
//! ## Robustness
//! A broken or hand-edited file never stops the game: unknown keys are ignored,
//! wrong types fall back to the default, out-of-range numbers are clamped, and
//! each problem is reported as a warning.

mod schema;
mod toml_io;

pub use schema::{Access, Apply, Kind, SettingDef, Value, schema};

use ld_input::Bindings;

/// All player settings, grouped the same way as the settings menu tabs.
#[derive(Debug, Clone)]
pub struct Settings {
    pub graphics: GraphicsSettings,
    pub performance: PerformanceSettings,
    pub gameplay: GameplaySettings,
    pub controls: ControlSettings,
    pub interface: InterfaceSettings,
    pub keybinds: Bindings,
}

/// Settings in the **Graphics** tab.
#[derive(Debug, Clone)]
pub struct GraphicsSettings {
    pub backend: Backend,
    pub vsync: bool,
    pub fps_limit: u32,
    pub fov: f32,
    pub fullscreen: bool,
    pub fog: bool,
    pub render_distance: u32,
    pub mipmap_levels: u32,
}

/// Settings in the **Performance** tab.
#[derive(Debug, Clone)]
pub struct PerformanceSettings {
    pub worker_threads: u32,
    pub chunk_uploads_per_frame: u32,
}

/// Settings in the **Gameplay** tab.
#[derive(Debug, Clone)]
pub struct GameplaySettings {
    pub difficulty: Difficulty,
    pub game_mode: GameModeSetting,
}

/// Settings in the **Controls** tab.
#[derive(Debug, Clone)]
pub struct ControlSettings {
    pub mouse_sensitivity: u32,
    pub invert_mouse: bool,
    pub toggle_sprint: bool,
    pub toggle_sneak: bool,
}

/// Settings in the **Interface** tab.
#[derive(Debug, Clone)]
pub struct InterfaceSettings {
    pub gui_scale: f32,
    pub show_debug_overlay: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            graphics: GraphicsSettings {
                backend: Backend::Auto,
                vsync: true,
                fps_limit: 0,
                fov: 70.0,
                fullscreen: false,
                fog: true,
                render_distance: 8,
                mipmap_levels: 4,
            },
            performance: PerformanceSettings { worker_threads: 0, chunk_uploads_per_frame: 16 },
            gameplay: GameplaySettings {
                difficulty: Difficulty::Normal,
                game_mode: GameModeSetting::Survival,
            },
            controls: ControlSettings {
                mouse_sensitivity: 100,
                invert_mouse: false,
                toggle_sprint: false,
                toggle_sneak: false,
            },
            interface: InterfaceSettings { gui_scale: 1.0, show_debug_overlay: false },
            keybinds: Bindings::defaults(),
        }
    }
}

/// Defines a "pick one of these" setting type that is stored as a string.
macro_rules! choice_enum {
    ($(#[$m:meta])* $name:ident { $($variant:ident => $text:literal),+ $(,)? }) => {
        $(#[$m])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum $name { $($variant),+ }

        impl $name {
            /// All choices, as written in `settings.toml`.
            pub const OPTIONS: &'static [&'static str] = &[$($text),+];

            /// The string written to `settings.toml`.
            pub fn as_str(self) -> &'static str {
                match self { $(Self::$variant => $text),+ }
            }

            /// Parses a config string (case-insensitive).
            pub fn parse(s: &str) -> Option<Self> {
                $(if s.eq_ignore_ascii_case($text) { return Some(Self::$variant); })+
                None
            }
        }
    };
}

choice_enum! {
    /// Which graphics API to use. Vulkan is preferred (director decision);
    /// the others are fallbacks for hardware or systems without Vulkan.
    Backend {
        Auto => "auto",
        Vulkan => "vulkan",
        Dx12 => "dx12",
        Metal => "metal",
        OpenGl => "opengl",
    }
}

choice_enum! {
    /// How hard the game is (starvation now; creatures from Phase 7).
    Difficulty {
        Peaceful => "peaceful",
        Easy => "easy",
        Normal => "normal",
        Hard => "hard",
    }
}

choice_enum! {
    /// Game mode a new game starts in.
    GameModeSetting {
        Survival => "survival",
        Creative => "creative",
    }
}

impl Settings {
    /// Parses settings from TOML text. Never fails: problems become warnings
    /// and the affected settings keep their defaults.
    pub fn from_toml(text: &str) -> (Settings, Vec<String>) {
        toml_io::load(text)
    }

    /// Serialises all settings to commented TOML.
    pub fn to_toml(&self) -> String {
        toml_io::save(self)
    }
}
