//! Keyboard state. Key numbers are the bit positions used by the calculator's
//! keyboard scan (NumWorks EADK `eadk_key_t`), so the app passes the raw scan
//! through unchanged.

/// A calculator key (value = bit index in the keyboard state).
#[allow(missing_docs)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum Key {
    Left = 0,
    Up = 1,
    Down = 2,
    Right = 3,
    Ok = 4,
    Back = 5,
    Home = 6,
    OnOff = 8,
    Shift = 12,
    Alpha = 13,
    Xnt = 14,
    Var = 15,
    Toolbox = 16,
    Backspace = 17,
    Exp = 18,
    Ln = 19,
    Log = 20,
    Imaginary = 21,
    Comma = 22,
    Power = 23,
    Sine = 24,
    Cosine = 25,
    Tangent = 26,
    Pi = 27,
    Sqrt = 28,
    Square = 29,
    Seven = 30,
    Eight = 31,
    Nine = 32,
    LeftParen = 33,
    RightParen = 34,
    Four = 36,
    Five = 37,
    Six = 38,
    Multiply = 39,
    Divide = 40,
    One = 42,
    Two = 43,
    Three = 44,
    Plus = 45,
    Minus = 46,
    Zero = 48,
    Dot = 49,
    Ee = 50,
    Ans = 51,
    Exe = 52,
}

/// Keys held this frame and last frame, to tell "held" from "just pressed".
#[derive(Clone, Copy, Default)]
pub struct Input {
    now: u64,
    before: u64,
}

impl Input {
    /// An input state with no key held.
    pub const fn new() -> Self {
        Self { now: 0, before: 0 }
    }

    /// Starts a new frame with the latest keyboard scan.
    pub fn update(&mut self, scan: u64) {
        self.before = self.now;
        self.now = scan;
    }

    /// Whether `k` is held down.
    #[inline]
    pub fn down(&self, k: Key) -> bool {
        (self.now >> k as u8) & 1 == 1
    }

    /// Whether `k` went down this frame.
    #[inline]
    pub fn pressed(&self, k: Key) -> bool {
        self.down(k) && (self.before >> k as u8) & 1 == 0
    }

    /// Whether any key is held.
    pub fn any(&self) -> bool {
        self.now != 0
    }
}

/// Builds a keyboard state with the given keys held (used by tests and the
/// PC harness's scripted input).
pub fn keys(list: &[Key]) -> u64 {
    list.iter().fold(0, |acc, k| acc | 1 << *k as u8)
}
