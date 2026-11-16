//! Fixed-size text buffer, so text can be formatted with `write!` without a
//! heap.

use core::fmt;

/// Up to `N` bytes of UTF-8 text. Extra text is silently cut off.
pub struct Text<const N: usize> {
    buf: [u8; N],
    len: usize,
}

impl<const N: usize> Default for Text<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> Text<N> {
    /// An empty buffer.
    pub const fn new() -> Self {
        Self { buf: [0; N], len: 0 }
    }

    /// The text written so far.
    pub fn as_str(&self) -> &str {
        // Only whole `&str`s are ever copied in, cut at a char boundary.
        core::str::from_utf8(&self.buf[..self.len]).unwrap_or("")
    }

    /// Empties the buffer.
    pub fn clear(&mut self) {
        self.len = 0;
    }
}

impl<const N: usize> fmt::Write for Text<N> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        let room = N - self.len;
        let mut n = s.len().min(room);
        while !s.is_char_boundary(n) {
            n -= 1;
        }
        self.buf[self.len..self.len + n].copy_from_slice(&s.as_bytes()[..n]);
        self.len += n;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::fmt::Write;

    #[test]
    fn formats_and_truncates() {
        let mut t = Text::<8>::new();
        write!(t, "x={} y={}", 12, 345).unwrap();
        assert_eq!(t.as_str(), "x=12 y=3");
        t.clear();
        write!(t, "é").unwrap();
        assert_eq!(t.as_str(), "é");
    }
}
