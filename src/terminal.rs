//! Terminal capability detection -- exposes the host terminal's
//! row/column dimensions so the compositor can size its framebuffer
//! to fit.
//!
//! The `terminal_size` crate handles the cross-platform `ioctl`
//! (Unix) and console-mode (Windows) call. We adopt it because it
//! prioritises library reuse over hand-rolling, and `terminal_size`
//! is a tiny, MIT-licensed crate with no transitive dependencies.

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
        detect_with_size(terminal_size::terminal_size)
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

/// Testable inner of [`TerminalSize::detect`]: same logic,
/// but accepts a closure that provides the raw size tuple.
/// `pub(crate)` so unit tests in the same module can call it
/// without requiring a real TTY.
pub(crate) fn detect_with_size<F>(f: F) -> Option<TerminalSize>
where
    F: FnOnce() -> Option<(terminal_size::Width, terminal_size::Height)>,
{
    let (w, h) = f()?;
    Some(TerminalSize {
        rows: h.0,
        cols: w.0,
    })
}

#[cfg(test)]
mod tests {
    use super::{detect_with_size, TerminalSize};

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

    #[test]
    fn detect_returns_some_in_tty() {
        if std::io::IsTerminal::is_terminal(&std::io::stdout()) {
            let s = TerminalSize::detect();
            assert!(s.is_some(), "detect() should return Some when stdout is a TTY");
            let s = s.unwrap();
            assert!(s.rows > 0, "rows must be positive, got {}", s.rows);
            assert!(s.cols > 0, "cols must be positive, got {}", s.cols);
        }
    }

    #[test]
    fn current_returns_positive_dimensions() {
        let s = TerminalSize::current();
        assert!(s.rows > 0, "current() rows must be positive");
        assert!(s.cols > 0, "current() cols must be positive");
    }

    #[test]
    fn current_matches_detect_when_tty() {
        if std::io::IsTerminal::is_terminal(&std::io::stdout()) {
            let detected = TerminalSize::detect();
            let current = TerminalSize::current();
            if let Some(d) = detected {
                assert_eq!(d, current, "current() should equal detect() when TTY");
            }
        }
    }

    #[test]
    fn as_framebuffer_size_zero() {
        let s = TerminalSize { rows: 0, cols: 0 };
        assert_eq!(s.as_framebuffer_size(), (0, 0));
    }

    #[test]
    fn as_framebuffer_size_max_values() {
        let s = TerminalSize {
            rows: u16::MAX,
            cols: u16::MAX,
        };
        assert_eq!(
            s.as_framebuffer_size(),
            (u16::MAX as u32, u16::MAX as u32)
        );
    }

    #[test]
    fn as_framebuffer_size_single_cell() {
        let s = TerminalSize { rows: 1, cols: 1 };
        assert_eq!(s.as_framebuffer_size(), (1, 1));
    }

    #[test]
    fn fallback_dimensions_match() {
        let s = TerminalSize::fallback();
        assert_eq!(s.as_framebuffer_size(), (80, 24));
    }

    #[test]
    fn display_debug_format() {
        let s = TerminalSize { rows: 10, cols: 20 };
        let dbg = format!("{s:?}");
        assert!(dbg.contains("TerminalSize"));
        assert!(dbg.contains("10"));
        assert!(dbg.contains("20"));
    }

    // -- detect_with_size tests (mockable, no TTY needed) --------

    #[test]
    fn detect_with_size_returns_some_for_valid_input() {
        let result = detect_with_size(|| {
            Some((
                terminal_size::Width(120),
                terminal_size::Height(40),
            ))
        });
        assert_eq!(result, Some(TerminalSize { rows: 40, cols: 120 }));
    }

    #[test]
    fn detect_with_size_returns_none_for_none() {
        let result = detect_with_size(|| None);
        assert_eq!(result, None);
    }

    #[test]
    fn detect_with_size_single_cell() {
        let result = detect_with_size(|| {
            Some((terminal_size::Width(1), terminal_size::Height(1)))
        });
        assert_eq!(result, Some(TerminalSize { rows: 1, cols: 1 }));
    }

    #[test]
    fn detect_with_size_zero_dimensions() {
        let result = detect_with_size(|| {
            Some((terminal_size::Width(0), terminal_size::Height(0)))
        });
        assert_eq!(result, Some(TerminalSize { rows: 0, cols: 0 }));
    }

    #[test]
    fn detect_with_size_large_dimensions() {
        let result = detect_with_size(|| {
            Some((
                terminal_size::Width(u16::MAX),
                terminal_size::Height(u16::MAX),
            ))
        });
        assert_eq!(
            result,
            Some(TerminalSize {
                rows: u16::MAX,
                cols: u16::MAX
            })
        );
    }

    #[test]
    fn detect_with_size_asymmetric() {
        let result = detect_with_size(|| {
            Some((terminal_size::Width(200), terminal_size::Height(50)))
        });
        let s = result.unwrap();
        assert_eq!(s.as_framebuffer_size(), (200, 50));
    }

    #[test]
    fn detect_with_size_none_implies_fallback() {
        let result = detect_with_size(|| None);
        let size = result.unwrap_or_else(TerminalSize::fallback);
        assert_eq!(size, TerminalSize::fallback());
    }

    #[test]
    fn detect_with_size_closure_called_exactly_once() {
        use std::cell::Cell;
        let call_count = Cell::new(0u32);
        let _ = detect_with_size(|| {
            call_count.set(call_count.get() + 1);
            Some((terminal_size::Width(80), terminal_size::Height(24)))
        });
        assert_eq!(call_count.get(), 1, "closure must be called exactly once");
    }
}
