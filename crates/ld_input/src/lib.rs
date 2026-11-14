//! # ld_input — input actions and key bindings
//!
//! Gameplay code never asks "is the W key down?". It asks "is
//! [`Action::MoveForward`] active?". This crate translates raw device inputs
//! into those actions using the player's key bindings.
//!
//! Why this indirection matters:
//! * **Rebinding** — any action can be moved to any key without touching
//!   gameplay code.
//! * **Future platforms** — touch controls (mobile) and gamepads only need to
//!   produce actions; nothing else changes.
//!
//! Inputs are identified by plain strings such as `"KeyW"`, `"Space"`,
//! `"ShiftLeft"` or `"MouseLeft"`. These are the same names winit uses for its
//! key codes, and they are what players write in `settings.toml`. Keeping them
//! as strings means this crate has no dependency on any windowing library.

use std::collections::{HashMap, HashSet};

/// Everything the player can *do* with an input device.
///
/// To add an action: add a variant, then add it to [`Action::ALL`] and to the
/// `match` arms of the methods below. The compiler will point out any you miss.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Action {
    MoveForward,
    MoveBackward,
    StrafeLeft,
    StrafeRight,
    Jump,
    Sneak,
    Sprint,
    ToggleDebugOverlay,
    ToggleFullscreen,
    Screenshot,
}

impl Action {
    /// Every action, in the order they appear in settings.
    pub const ALL: &'static [Action] = &[
        Action::MoveForward,
        Action::MoveBackward,
        Action::StrafeLeft,
        Action::StrafeRight,
        Action::Jump,
        Action::Sneak,
        Action::Sprint,
        Action::ToggleDebugOverlay,
        Action::ToggleFullscreen,
        Action::Screenshot,
    ];

    /// Stable name used as the key in `settings.toml` (`[keybinds]` section).
    /// Never rename one of these: it would reset players' custom bindings.
    pub fn config_key(self) -> &'static str {
        match self {
            Action::MoveForward => "move_forward",
            Action::MoveBackward => "move_backward",
            Action::StrafeLeft => "strafe_left",
            Action::StrafeRight => "strafe_right",
            Action::Jump => "jump",
            Action::Sneak => "sneak",
            Action::Sprint => "sprint",
            Action::ToggleDebugOverlay => "toggle_debug_overlay",
            Action::ToggleFullscreen => "toggle_fullscreen",
            Action::Screenshot => "screenshot",
        }
    }

    /// Human-readable name for menus.
    pub fn label(self) -> &'static str {
        match self {
            Action::MoveForward => "Walk forward",
            Action::MoveBackward => "Walk backward",
            Action::StrafeLeft => "Strafe left",
            Action::StrafeRight => "Strafe right",
            Action::Jump => "Jump / fly up",
            Action::Sneak => "Sneak / fly down",
            Action::Sprint => "Sprint",
            Action::ToggleDebugOverlay => "Debug overlay",
            Action::ToggleFullscreen => "Fullscreen",
            Action::Screenshot => "Take screenshot",
        }
    }

    /// One-line explanation shown as a tooltip and as a config-file comment.
    pub fn description(self) -> &'static str {
        match self {
            Action::MoveForward => "Moves you in the direction you are looking.",
            Action::MoveBackward => "Moves you backwards.",
            Action::StrafeLeft => "Steps sideways to the left.",
            Action::StrafeRight => "Steps sideways to the right.",
            Action::Jump => "Jumps. In Creative, double-tap to start/stop flying; hold while flying to rise.",
            Action::Sneak => "Sneaks (slow walk). While flying, hold to descend.",
            Action::Sprint => "Hold while moving forward to run faster.",
            Action::ToggleDebugOverlay => {
                "Shows/hides the debug & performance overlay (FPS, position, GPU info)."
            }
            Action::ToggleFullscreen => "Switches between windowed and borderless fullscreen.",
            Action::Screenshot => "Saves a PNG of the current frame to the screenshots folder.",
        }
    }

    /// Default binding, matching Minecraft Java's defaults where one exists.
    pub fn default_binding(self) -> &'static str {
        match self {
            Action::MoveForward => "KeyW",
            Action::MoveBackward => "KeyS",
            Action::StrafeLeft => "KeyA",
            Action::StrafeRight => "KeyD",
            Action::Jump => "Space",
            Action::Sneak => "ShiftLeft",
            Action::Sprint => "ControlLeft",
            Action::ToggleDebugOverlay => "F3",
            Action::ToggleFullscreen => "F11",
            Action::Screenshot => "F2",
        }
    }
}

/// Maps input names (e.g. `"KeyW"`) to the actions they trigger.
///
/// Several actions may share one input; that's allowed, as players sometimes want it.
#[derive(Debug, Clone, Default)]
pub struct Bindings {
    by_input: HashMap<String, Vec<Action>>,
}

impl Bindings {
    /// Bindings with every action on its default key.
    pub fn defaults() -> Self {
        let mut b = Self::default();
        for &a in Action::ALL {
            b.bind(a, a.default_binding());
        }
        b
    }

    /// Binds `action` to `input`, removing any previous binding of `action`.
    /// An empty `input` string leaves the action unbound.
    pub fn bind(&mut self, action: Action, input: &str) {
        for actions in self.by_input.values_mut() {
            actions.retain(|&a| a != action);
        }
        self.by_input.retain(|_, v| !v.is_empty());
        if !input.is_empty() {
            self.by_input.entry(input.to_string()).or_default().push(action);
        }
    }

    /// The actions bound to `input` (possibly none).
    pub fn actions_for(&self, input: &str) -> &[Action] {
        self.by_input.get(input).map(Vec::as_slice).unwrap_or(&[])
    }

    /// The input bound to `action`, if any.
    pub fn input_for(&self, action: Action) -> Option<&str> {
        self.by_input.iter().find(|(_, v)| v.contains(&action)).map(|(k, _)| k.as_str())
    }
}

/// The live state of all actions, fed by device events.
///
/// Two kinds of queries exist:
/// * [`is_held`](Self::is_held) — "is it down right now?" (movement keys).
/// * [`take_press`](Self::take_press) — "was it pressed since I last asked?"
///   (toggles like the debug overlay). Presses are *queued* until consumed,
///   so a quick tap between two 20 Hz game ticks is never lost.
#[derive(Debug, Default)]
pub struct InputState {
    held_inputs: HashSet<String>,
    held: HashMap<Action, u32>,
    pending_presses: HashMap<Action, u32>,
    mouse_delta: (f64, f64),
}

impl InputState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed a raw press/release. OS key-repeat events (a held key re-sending
    /// "pressed") are ignored so they don't count as new presses.
    pub fn handle_input(&mut self, bindings: &Bindings, input: &str, pressed: bool) {
        if pressed {
            if !self.held_inputs.insert(input.to_string()) {
                return; // already held: OS auto-repeat
            }
            for &a in bindings.actions_for(input) {
                *self.held.entry(a).or_default() += 1;
                *self.pending_presses.entry(a).or_default() += 1;
            }
        } else if self.held_inputs.remove(input) {
            for &a in bindings.actions_for(input) {
                if let Some(n) = self.held.get_mut(&a) {
                    *n = n.saturating_sub(1);
                    if *n == 0 {
                        self.held.remove(&a);
                    }
                }
            }
        }
    }

    /// Accumulates raw mouse movement (in device units, roughly pixels).
    pub fn add_mouse_delta(&mut self, dx: f64, dy: f64) {
        self.mouse_delta.0 += dx;
        self.mouse_delta.1 += dy;
    }

    /// Returns and resets the mouse movement accumulated since the last call.
    pub fn take_mouse_delta(&mut self) -> (f64, f64) {
        std::mem::take(&mut self.mouse_delta)
    }

    /// Whether `action` is currently held down.
    pub fn is_held(&self, action: Action) -> bool {
        self.held.contains_key(&action)
    }

    /// Consumes one queued press of `action`; returns whether there was one.
    pub fn take_press(&mut self, action: Action) -> bool {
        match self.pending_presses.get_mut(&action) {
            Some(n) if *n > 0 => {
                *n -= 1;
                true
            }
            _ => false,
        }
    }

    /// Releases everything — call when the window loses focus, otherwise a key
    /// released while unfocused would stay "stuck" down.
    pub fn release_all(&mut self) {
        self.held_inputs.clear();
        self.held.clear();
        self.pending_presses.clear();
        self.mouse_delta = (0.0, 0.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_action_has_unique_config_key_and_default() {
        let mut keys = HashSet::new();
        for &a in Action::ALL {
            assert!(keys.insert(a.config_key()), "duplicate key {}", a.config_key());
            assert!(!a.default_binding().is_empty());
        }
    }

    #[test]
    fn rebinding_moves_action() {
        let mut b = Bindings::defaults();
        b.bind(Action::Jump, "KeyJ");
        assert!(b.actions_for("Space").is_empty());
        assert_eq!(b.actions_for("KeyJ"), &[Action::Jump]);
        assert_eq!(b.input_for(Action::Jump), Some("KeyJ"));
    }

    #[test]
    fn press_queue_and_auto_repeat() {
        let b = Bindings::defaults();
        let mut s = InputState::new();
        s.handle_input(&b, "F3", true);
        s.handle_input(&b, "F3", true); // auto-repeat, must be ignored
        assert!(s.is_held(Action::ToggleDebugOverlay));
        s.handle_input(&b, "F3", false);
        assert!(!s.is_held(Action::ToggleDebugOverlay));
        assert!(s.take_press(Action::ToggleDebugOverlay)); // tap survived release
        assert!(!s.take_press(Action::ToggleDebugOverlay));
    }

    #[test]
    fn shared_binding_counts_correctly() {
        let mut b = Bindings::defaults();
        b.bind(Action::Sprint, "KeyW"); // W = forward + sprint
        let mut s = InputState::new();
        s.handle_input(&b, "KeyW", true);
        assert!(s.is_held(Action::MoveForward) && s.is_held(Action::Sprint));
        s.handle_input(&b, "KeyW", false);
        assert!(!s.is_held(Action::Sprint));
    }
}
