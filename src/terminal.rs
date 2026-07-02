//! Terminal capability detection -- exposes the host terminal's
//! row/column dimensions so the compositor can size its framebuffer
//! to fit.
//!
//! The `terminal_size` crate handles the cross-platform `ioctl`
//! (Unix) and console-mode (Windows) call. We adopt it because
//! AGENTS.md section 3 prioritises library reuse over hand-rolling,
//! and `terminal_size` is a tiny, MIT-licensed crate with no
//! transitive dependencies.

/// Reported dimensions of the host terminal in character cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TerminalSize {
    /// Vertical (row) dimension in character cells.
    pub rows: u16,
    /// Horizontal (column) dimension in character cells.
    pub cols: u16,
}

impl TerminalSize {
    /// Returns a fallback size of 80x24 -- the de-facto VT100 default.
    pub const fn fallback() -> Self {
        Self { rows: 24, cols: 80 }
    }

    /// Detects the host terminal's current size. Returns `None` when
    /// the size cannot be determined (e.g. stdout is not a TTY, or
    /// the platform is unsupported).
    pub fn detect() -> Option<Self> {
        let (w, h) = terminal_size::terminal_size()?;
        Some(Self {
            rows: h.0,
            cols: w.0,
        })
    }

    /// Returns the current terminal size, falling back to
    /// [`TerminalSize::fallback`] when detection fails. Never panics.
    pub fn current() -> Self {
        Self::detect().unwrap_or_else(Self::fallback)
    }

    /// Returns the size as `(width, height)` in cells, suitable for
    /// passing to [`FrameBuffer::new`](crate::FrameBuffer::new).
    pub const fn as_framebuffer_size(self) -> (u32, u32) {
        (self.cols as u32, self.rows as u32)
    }
}

#[cfg(test)]
mod tests {
    use super::TerminalSize;

    #[test]
    fn fallback_is_80x24() {
        let s = TerminalSize::fallback();
        assert_eq!(s.rows, 24);
        assert_eq!(s.cols, 80);
    }

    #[test]
    fn as_framebuffer_size_converts() {
        let s = TerminalSize {
            rows: 30,
            cols: 100,
        };
        assert_eq!(s.as_framebuffer_size(), (100, 30));
    }

    #[test]
    fn equality_works() {
        let a = TerminalSize { rows: 24, cols: 80 };
        let b = TerminalSize { rows: 24, cols: 80 };
        let c = TerminalSize { rows: 25, cols: 80 };
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn current_never_panics() {
        let _ = TerminalSize::current();
    }
}
