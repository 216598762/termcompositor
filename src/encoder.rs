//! Output protocol selection and framebuffer encoding.
//!
//! The runtime picks a [`Protocol`] (Kitty graphics protocol,
//! Sixel, or auto-detected) based on terminal capability
//! detection (via `TERM`, `TERM_PROGRAM`, `COLORTERM`)
//! per AGENTS.md §7, preferring [`Protocol::Kitty`] when the
//! host supports it and falling back to [`Protocol::Sixel`]
//! otherwise. The [`Protocol::Auto`] variant defers the
//! choice to the pure env-var shim [`detect`] (or, when
//! authoritative detection is needed, to the impure probe
//! [`detect_with_probe`]).
//!
//! v0.5.0 wires up the Kitty arm via the optional
//! [`little_kitty`](https://crates.io/crates/little-kitty) crate
//! behind the `kitty-encoder` Cargo feature. v0.6.0 wires up
//! the Sixel arm via the optional
//! [`icy_sixel`](https://crates.io/crates/icy_sixel) crate
//! behind the `sixel-encoder` Cargo feature. v0.7.0 adds the
//! auto-detect shim and the [`Protocol::Auto`] variant. Each
//! arm returns [`EncoderError::UnsupportedProtocol`] when the
//! corresponding feature is disabled in the current build.

use crate::framebuffer::FrameBuffer;

/// Terminal graphics protocol used to encode the composited
/// framebuffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protocol {
    /// The kitty graphics protocol -- modern and feature-rich.
    Kitty,
    /// Sixel -- fallback for terminals without kitty support.
    Sixel,
    /// Auto-detect: defers to the env-var shim ([`detect`]) at
    /// encode time, which picks `Kitty` or `Sixel` based on
    /// `TERM` / `TERM_PROGRAM` / `COLORTERM`. Pure: does no
    /// I/O, so the encoder contract ("no I/O inside `encode`")
    /// is preserved.
    Auto,
}

impl Protocol {
    /// Returns the protocol name as it appears in docs and
    /// capability probes.
    pub const fn as_str(self) -> &'static str {
        match self {
            Protocol::Kitty => "kitty",
            Protocol::Sixel => "sixel",
            Protocol::Auto => "auto",
        }
    }
}

/// Errors produced by [`ProtocolEncoder::encode`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EncoderError {
    /// The requested protocol is not compiled into this build
    /// (e.g. calling `encode` on `Protocol::Kitty` without the
    /// `kitty-encoder` feature, or on `Protocol::Sixel` without
    /// the `sixel-encoder` feature).
    UnsupportedProtocol(&'static str),

    /// The framebuffer has zero width or height and cannot be
    /// encoded.
    InvalidDimensions {
        /// Framebuffer width in pixels.
        width: u32,
        /// Framebuffer height in pixels.
        height: u32,
    },

    /// The underlying encoder crate failed; the wrapped
    /// `String` carries its `Display` output.
    Encode(String),
}

impl std::fmt::Display for EncoderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsupportedProtocol(p) => {
                write!(f, "protocol {p} is not supported in this build")
            }
            Self::InvalidDimensions { width, height } => {
                write!(f, "framebuffer has invalid dimensions: {width}x{height}")
            }
            Self::Encode(msg) => write!(f, "encoder failed: {msg}"),
        }
    }
}

impl std::error::Error for EncoderError {}

// `From` impls for the underlying encoder-crate error types.
// Gated on the respective features so a build that doesn't
// pull in the crate can't reference its error type. The shared
// shape `EncoderError::Encode(String)` lets the per-encoder
// `encode` functions use `?` directly without per-module
// helper closures (the v0.5.0 `io_err` / v0.6.0 `sixel_err`
// helpers have been removed in favour of this pattern).

#[cfg(feature = "kitty-encoder")]
impl From<std::io::Error> for EncoderError {
    fn from(e: std::io::Error) -> Self {
        EncoderError::Encode(e.to_string())
    }
}

#[cfg(feature = "sixel-encoder")]
impl From<icy_sixel::SixelError> for EncoderError {
    fn from(e: icy_sixel::SixelError) -> Self {
        EncoderError::Encode(e.to_string())
    }
}

/// Pure env-var-based terminal-capability detection.
///
/// Reads `TERM`, `TERM_PROGRAM`, and `COLORTERM` from the
/// process environment and returns a [`Protocol`] suggestion.
/// Always available, no I/O, never panics. This is the shim
/// that [`Protocol::Auto::encode`] dispatches through; callers
/// who want authoritative detection (e.g. for a TUI picker)
/// can use [`detect_with_probe`] instead.
///
/// Heuristics (in priority order):
/// 1. `TERM_PROGRAM` (most specific -- set by the terminal
///    app): `kitty` / `wezterm` / `ghostty` (case-insensitive)
///    -> `Protocol::Kitty`.
/// 2. `TERM` (terminfo name): `xterm-kitty` / `foot` / `foot-*`
///    -> `Protocol::Kitty`; `tmux` / `tmux-*` ->
///    `Protocol::Sixel` (Kitty via tmux needs passthrough
///    setup; the current encoder does not handle it, so prefer
///    Sixel when in tmux).
/// 3. `COLORTERM` tiebreaker (weak signal): `truecolor` /
///    `24bit` -> `Protocol::Kitty` when `TERM` / `TERM_PROGRAM`
///    are inconclusive. Modern truecolor terminals are more
///    likely to support the Kitty graphics protocol than the
///    average XTerm-like terminal.
/// 4. Default -> `Protocol::Sixel` (most universal fallback).
pub fn detect() -> Protocol {
    detect_with_env(
        std::env::var("TERM").ok().as_deref(),
        std::env::var("TERM_PROGRAM").ok().as_deref(),
        std::env::var("COLORTERM").ok().as_deref(),
    )
}

/// Like [`detect`], but additionally probes the terminal for
/// Kitty support via the query-response protocol
/// (`little_kitty::Command::is_supported()`). Returns the
/// env-var result when the env-var already says `Kitty`
/// (avoids an unnecessary probe in the common case).
///
/// This function performs I/O on the terminal's stdin/stdout --
/// it writes a Kitty query and reads a response, blocking
/// until the terminal answers. Do NOT call it from a pure
/// encoder (see AGENTS.md §7); use [`detect`] for that. This
/// is the right entry point for a one-shot startup probe
/// (e.g. the `main.rs` demo's `--probe` flag).
#[cfg(feature = "kitty-encoder")]
pub fn detect_with_probe() -> Result<Protocol, EncoderError> {
    let env_result = detect();
    if env_result == Protocol::Kitty {
        // Env-var already says Kitty; trust it. Avoids the
        // probe entirely in the common case (Kitty terminal,
        // where TERM_PROGRAM=kitty is set).
        return Ok(env_result);
    }
    little_kitty::command::Command::default()
        .is_supported()
        .map(|kitty_supported| {
            if kitty_supported {
                Protocol::Kitty
            } else {
                env_result
            }
        })
        .map_err(|e| EncoderError::Encode(format!("kitty probe failed: {e}")))
}

/// Testable inner of [`detect`]: same heuristics, but with the
/// env values passed in explicitly. `pub(crate)` so unit tests
/// in the same module can call it without racing on
/// `std::env::set_var` (which is process-global and unsafe
/// under parallel tests).
pub(crate) fn detect_with_env(
    term: Option<&str>,
    term_program: Option<&str>,
    colorterm: Option<&str>,
) -> Protocol {
    // 1. TERM_PROGRAM wins (most specific -- set by the
    //    terminal application itself, not the terminfo
    //    database).
    if let Some(s) = term_program {
        if s.eq_ignore_ascii_case("kitty") {
            return Protocol::Kitty;
        }
        if s.eq_ignore_ascii_case("wezterm") {
            return Protocol::Kitty;
        }
        if s.eq_ignore_ascii_case("ghostty") {
            return Protocol::Kitty;
        }
    }
    // 2. TERM-based heuristics (terminfo name).
    if let Some(s) = term {
        if s == "xterm-kitty" {
            return Protocol::Kitty;
        }
        if s == "foot" || s.starts_with("foot-") {
            return Protocol::Kitty;
        }
        // tmux passthrough complicates Kitty; prefer Sixel.
        // The encoder could be extended later to handle the
        // tmux passthrough wrapping (`ESC Ptmux;...ESC \\`).
        if s == "tmux" || s.starts_with("tmux-") {
            return Protocol::Sixel;
        }
    }
    // 3. COLORTERM tiebreaker (weak signal -- see the
    //    `detect` doc comment).
    if let Some(c) = colorterm {
        if c.eq_ignore_ascii_case("truecolor") || c.eq_ignore_ascii_case("24bit") {
            return Protocol::Kitty;
        }
    }
    // 4. Default to Sixel (most universal fallback -- most
    //    XTerm-like terminals support Sixel even when they
    //    do not support the Kitty graphics protocol).
    Protocol::Sixel
}

/// Encodes a [`FrameBuffer`] into the byte stream a terminal
/// expects for a chosen [`Protocol`].
///
/// Implementors return a `Vec<u8>` of escape sequences the
/// caller writes to stdout; the encoding does no I/O itself.
pub trait ProtocolEncoder {
    /// Encodes `frame` into escape-sequence bytes for `self`.
    fn encode(&self, frame: &FrameBuffer) -> Result<Vec<u8>, EncoderError>;
}

/// Private dispatch: the single source of truth for "given a
/// `Protocol`, which encoder do I call?". Extracted out of the
/// `ProtocolEncoder::encode` impl so the [`Protocol::Auto`]
/// arm can recurse cleanly via `dispatch(detect(), frame)`
/// without duplicating the per-variant `#[cfg]` matrix.
fn dispatch(protocol: Protocol, frame: &FrameBuffer) -> Result<Vec<u8>, EncoderError> {
    match protocol {
        #[cfg(feature = "kitty-encoder")]
        Protocol::Kitty => kitty::encode(frame),
        #[cfg(not(feature = "kitty-encoder"))]
        Protocol::Kitty => {
            let _ = frame;
            Err(EncoderError::UnsupportedProtocol("kitty"))
        }
        #[cfg(feature = "sixel-encoder")]
        Protocol::Sixel => sixel::encode(frame),
        #[cfg(not(feature = "sixel-encoder"))]
        Protocol::Sixel => {
            let _ = frame;
            Err(EncoderError::UnsupportedProtocol("sixel"))
        }
        Protocol::Auto => {
            // Recurse: `detect()` returns `Kitty` or `Sixel`
            // (never `Auto`), so the recursion is guaranteed
            // to terminate. The `detect_with_env` heuristics
            // guarantee this -- see the doc comment on
            // `detect_with_env`.
            dispatch(detect(), frame)
        }
    }
}

impl ProtocolEncoder for Protocol {
    fn encode(&self, frame: &FrameBuffer) -> Result<Vec<u8>, EncoderError> {
        dispatch(*self, frame)
    }
}

/// The Kitty graphics protocol encoder, gated on the
/// `kitty-encoder` Cargo feature. Implemented as a private
/// inline module so the public API surface stays minimal.
#[cfg(feature = "kitty-encoder")]
mod kitty {
    use super::EncoderError;
    use crate::framebuffer::FrameBuffer;
    use little_kitty::command::ControlValue;
    use little_kitty::io::KittyCommandWriter;
    use std::io::Write;

    /// Encodes `frame` as a single Kitty "transmit and display"
    /// command using raw RGBA pixel data (format code 32 per
    /// the Kitty graphics protocol spec). The returned bytes
    /// are the full escape-sequence payload ready to be
    /// written to the terminal.
    pub fn encode(frame: &FrameBuffer) -> Result<Vec<u8>, EncoderError> {
        if frame.width() == 0 || frame.height() == 0 {
            return Err(EncoderError::InvalidDimensions {
                width: frame.width(),
                height: frame.height(),
            });
        }

        // Materialise the RGBA pixel data as a single
        // contiguous byte slice. A streaming encode can be
        // added later if the per-frame allocation becomes a
        // hotspot.
        let rgba: Vec<u8> = frame.pixels().iter().flatten().copied().collect();

        // Build the control list. The Kitty graphics protocol
        // accepts a comma-separated list of key=value pairs
        // before the payload separator (`;`). We use:
        //   a=T   -- action: transmit and put (display)
        //   f=32  -- format: 32-bit RGBA
        //   q=2   -- quiet: suppress terminal OK/error
        //            responses
        //   s=W   -- image width in pixels
        //   v=H   -- image height in pixels
        let controls: Vec<(char, ControlValue)> = vec![
            ('a', ControlValue::Char('T')),
            ('f', ControlValue::UnsignedInteger(32)),
            ('q', ControlValue::UnsignedInteger(2)),
            ('s', ControlValue::UnsignedInteger(frame.width())),
            ('v', ControlValue::UnsignedInteger(frame.height())),
        ];

        let mut out = Vec::new();
        out.write_start(false, None)?;
        for (i, (key, value)) in controls.iter().enumerate() {
            if i > 0 {
                out.write_all(b",")?;
            }
            write!(out, "{key}=")?;
            value.write(&mut out)?;
        }
        out.write_all(b";")?;
        // write_base64 consumes the writer by value (returns
        // Self) and Base64-encodes the payload per the Kitty
        // graphics protocol.
        // TODO(v0.5.x): chunk large images (m=0 more-chunks /
        // m=1 last chunk) to support multi-megapixel
        // framebuffers; the current single-command encoder
        // will hit terminal size limits for very large frames.
        out = out.write_base64(&rgba)?;
        out.write_end(false)?;
        Ok(out)
    }
}

/// The Sixel graphics protocol encoder, gated on the
/// `sixel-encoder` Cargo feature. Implemented as a private
/// inline module so the public API surface stays minimal.
#[cfg(feature = "sixel-encoder")]
mod sixel {
    use super::EncoderError;
    use crate::framebuffer::FrameBuffer;
    use icy_sixel::SixelImage;

    /// Encodes `frame` as a Sixel DCS (Device Control String)
    /// escape sequence. The returned bytes are the full
    /// terminal-ready payload: `\x1bPq...sixel data...\x1b\\`.
    /// `icy_sixel` does the color quantization and sixel-data
    /// serialisation; we just hand it the RGBA pixels and pass
    /// through the resulting string.
    pub fn encode(frame: &FrameBuffer) -> Result<Vec<u8>, EncoderError> {
        if frame.width() == 0 || frame.height() == 0 {
            return Err(EncoderError::InvalidDimensions {
                width: frame.width(),
                height: frame.height(),
            });
        }

        // Materialise the RGBA pixel data as a single
        // contiguous byte slice. `icy_sixel` takes owned
        // bytes.
        let rgba: Vec<u8> = frame.pixels().iter().flatten().copied().collect();

        // `SixelImage::from_rgba` takes `usize` width/height;
        // the `u32` values from FrameBuffer are always
        // representable in `usize` on every supported
        // platform (a widening, lossless cast).
        let image = SixelImage::from_rgba(rgba, frame.width() as usize, frame.height() as usize);
        let sixel_string = image.encode()?;
        Ok(sixel_string.into_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::{detect_with_env, dispatch, EncoderError, Protocol, ProtocolEncoder};
    use crate::framebuffer::FrameBuffer;
    use std::sync::{Mutex, MutexGuard, OnceLock};

    #[test]
    fn as_str_matches_variant() {
        assert_eq!(Protocol::Kitty.as_str(), "kitty");
        assert_eq!(Protocol::Sixel.as_str(), "sixel");
        assert_eq!(Protocol::Auto.as_str(), "auto");
    }

    #[test]
    fn encoder_error_display_includes_context() {
        let e = EncoderError::UnsupportedProtocol("sixel");
        assert_eq!(
            e.to_string(),
            "protocol sixel is not supported in this build"
        );

        let e = EncoderError::InvalidDimensions {
            width: 0,
            height: 5,
        };
        assert_eq!(e.to_string(), "framebuffer has invalid dimensions: 0x5");
    }

    // -- detect_with_env heuristic coverage -----------------------------

    #[test]
    fn detect_with_env_picks_kitty_for_term_program_kitty() {
        assert_eq!(detect_with_env(None, Some("kitty"), None), Protocol::Kitty);
        assert_eq!(detect_with_env(None, Some("Kitty"), None), Protocol::Kitty);
        assert_eq!(detect_with_env(None, Some("KITTY"), None), Protocol::Kitty);
    }

    #[test]
    fn detect_with_env_picks_kitty_for_term_program_wezterm() {
        assert_eq!(
            detect_with_env(None, Some("wezterm"), None),
            Protocol::Kitty
        );
        assert_eq!(
            detect_with_env(None, Some("WezTerm"), None),
            Protocol::Kitty
        );
    }

    #[test]
    fn detect_with_env_picks_kitty_for_term_program_ghostty() {
        assert_eq!(
            detect_with_env(None, Some("ghostty"), None),
            Protocol::Kitty
        );
        assert_eq!(
            detect_with_env(None, Some("Ghostty"), None),
            Protocol::Kitty
        );
    }

    #[test]
    fn detect_with_env_picks_kitty_for_xterm_kitty() {
        assert_eq!(
            detect_with_env(Some("xterm-kitty"), None, None),
            Protocol::Kitty
        );
    }

    #[test]
    fn detect_with_env_picks_kitty_for_foot_and_foot_extra() {
        assert_eq!(detect_with_env(Some("foot"), None, None), Protocol::Kitty);
        assert_eq!(
            detect_with_env(Some("foot-extra"), None, None),
            Protocol::Kitty
        );
        assert_eq!(
            detect_with_env(Some("foot-256color"), None, None),
            Protocol::Kitty
        );
    }

    #[test]
    fn detect_with_env_picks_sixel_for_tmux() {
        // tmux passthrough complicates Kitty; default to Sixel.
        assert_eq!(detect_with_env(Some("tmux"), None, None), Protocol::Sixel);
        assert_eq!(
            detect_with_env(Some("tmux-256color"), None, None),
            Protocol::Sixel
        );
        assert_eq!(
            detect_with_env(Some("tmux-direct"), None, None),
            Protocol::Sixel
        );
    }

    #[test]
    fn detect_with_env_picks_sixel_for_xterm_256color() {
        // Conservative: unknown XTerm-like terminal -> Sixel.
        assert_eq!(
            detect_with_env(Some("xterm-256color"), None, None),
            Protocol::Sixel
        );
    }

    #[test]
    fn detect_with_env_picks_sixel_when_neither_set() {
        assert_eq!(detect_with_env(None, None, None), Protocol::Sixel);
        assert_eq!(
            detect_with_env(Some(""), Some(""), Some("")),
            Protocol::Sixel
        );
    }

    #[test]
    fn detect_with_env_term_program_wins_over_term() {
        // TERM_PROGRAM is more specific than TERM; if the two
        // disagree, TERM_PROGRAM wins.
        assert_eq!(
            detect_with_env(Some("xterm-256color"), Some("kitty"), None),
            Protocol::Kitty
        );
        // And vice versa: a known Kitty TERM with a non-Kitty
        // TERM_PROGRAM (e.g. a wrapper script setting
        // TERM_PROGRAM) -- TERM_PROGRAM still wins.
        assert_eq!(
            detect_with_env(Some("xterm-kitty"), Some("wezterm"), None),
            Protocol::Kitty
        );
    }

    #[test]
    fn detect_with_env_unknown_term_program_falls_through_to_term() {
        // A TERM_PROGRAM we don't recognise shouldn't block
        // detection -- fall through to TERM.
        assert_eq!(
            detect_with_env(Some("xterm-kitty"), Some("apple-terminal"), None),
            Protocol::Kitty
        );
    }

    // -- COLORTERM tiebreaker -------------------------------------------

    #[test]
    fn detect_with_env_colorterm_truecolor_picks_kitty_for_unknown_term() {
        // When TERM/TERM_PROGRAM are inconclusive but
        // COLORTERM=truecolor is set, lean Kitty.
        assert_eq!(
            detect_with_env(Some("xterm-256color"), None, Some("truecolor")),
            Protocol::Kitty
        );
    }

    #[test]
    fn detect_with_env_colorterm_24bit_picks_kitty_for_unknown_term() {
        assert_eq!(
            detect_with_env(Some("screen-256color"), None, Some("24bit")),
            Protocol::Kitty
        );
    }

    #[test]
    fn detect_with_env_colorterm_does_not_override_known_kitty() {
        // COLORTERM should not override an already-known
        // Kitty terminal -- TERM_PROGRAM wins.
        assert_eq!(
            detect_with_env(Some("xterm-kitty"), None, Some("truecolor")),
            Protocol::Kitty
        );
    }

    #[test]
    fn detect_with_env_colorterm_non_truecolor_does_not_override_sixel() {
        // A non-truecolor COLORTERM value should not override
        // the Sixel default for unknown terminals.
        assert_eq!(
            detect_with_env(Some("xterm"), None, Some("16color")),
            Protocol::Sixel
        );
    }

    // -- dispatch + Auto encode tests (env-var-driven) -------------------
    //
    // These tests touch `std::env::set_var`, which is
    // process-global and racy under parallel tests if two of
    // them snapshot each other's modified env vars. The
    // `cargo test` harness runs tests on multiple threads by
    // default; without serialisation, two `with_env` calls
    // can stomp on each other:
    //   Test X's `EnvGuard::new("TERM")` saves the original
    //     value.
    //   Test Y's `EnvGuard::new("TERM")` saves X's modified
    //     value (not the original).
    //   X drops, restores the original -- correct.
    //   Y drops, restores X's value -- WRONG; the env var now
    //     leaks X's value to any subsequent parallel test.
    //
    // v0.7.1 closes this race by acquiring a process-global
    // `Mutex<()>` (`env_mutex()`) before any env var is
    // touched and holding it until the closure returns. The
    // `EnvGuard` struct (RAII save/restore) is still in place
    // for the panic-safety guarantee: if the test panics
    // while holding the lock + the env guards, both the lock
    // and the env vars are restored in `Drop` order. The
    // `Mutex::lock` call uses `unwrap_or_else(|e|
    // e.into_inner())` to recover from a poisoned mutex
    // (e.g. a previous test panicked while holding the lock).

    /// Process-global mutex that serialises the env-touching
    /// test bodies. Returned by `env_mutex()` on first use.
    /// Held by `with_env` for the duration of the closure
    /// (set-env / run / restore-env).
    fn env_mutex() -> &'static Mutex<()> {
        static M: OnceLock<Mutex<()>> = OnceLock::new();
        M.get_or_init(|| Mutex::new(()))
    }

    /// Acquires the env mutex, recovering from poisoning.
    /// The returned guard is held until the end of the
    /// enclosing scope (the `with_env` call site); the lock
    /// is released when the guard is dropped.
    fn env_lock() -> MutexGuard<'static, ()> {
        env_mutex()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Saves the current value of one env var on construction
    /// and restores it on `Drop`. A test can `set` a new value
    /// (or `remove`) via the `set` method; the saved value is
    /// always restored, even on panic.
    struct EnvGuard {
        name: &'static str,
        saved: Option<String>,
    }

    impl EnvGuard {
        fn new(name: &'static str) -> Self {
            let saved = std::env::var(name).ok();
            Self { name, saved }
        }
        fn set(&self, value: Option<&str>) {
            match value {
                Some(v) => std::env::set_var(self.name, v),
                None => std::env::remove_var(self.name),
            }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match self.saved.as_ref() {
                Some(v) => std::env::set_var(self.name, v),
                None => std::env::remove_var(self.name),
            }
        }
    }

    /// Panic-safe, race-free env-var fixture. Acquires the
    /// process-global env mutex, sets TERM / TERM_PROGRAM /
    /// COLORTERM to the supplied values (or removes them if
    /// `None`), runs the closure, then restores all three
    /// env vars via the `EnvGuard` `Drop` impls (in reverse
    /// order) and releases the mutex. The mutex serialises
    /// env-touching tests so no two `with_env` calls can
    /// snapshot each other's modified env vars.
    fn with_env<F: FnOnce() -> R, R>(
        term: Option<&str>,
        term_program: Option<&str>,
        colorterm: Option<&str>,
        f: F,
    ) -> R {
        let _lock = env_lock();
        let _term = EnvGuard::new("TERM");
        _term.set(term);
        let _program = EnvGuard::new("TERM_PROGRAM");
        _program.set(term_program);
        let _colorterm = EnvGuard::new("COLORTERM");
        _colorterm.set(colorterm);
        f()
        // _colorterm, _program, _term, _lock drop in reverse
        // order, restoring all three env vars then releasing
        // the mutex.
    }

    #[cfg(feature = "kitty-encoder")]
    #[test]
    fn dispatch_auto_recurses_through_detect_resolves_to_kitty() {
        // The Auto arm recurses via `dispatch(detect(), frame)`.
        // Set `TERM=xterm-kitty` (a known Kitty terminfo name)
        // so `detect()` deterministically returns Kitty, then
        // assert the dispatch output starts with `\x1b_G`
        // (Kitty's APC introducer). Without this, the previous
        // v0.7.0 `dispatch_auto_recurses_through_detect` test
        // only verified the dispatch terminates (no infinite
        // loop), not that the recursion actually resolves
        // correctly.
        with_env(Some("xterm-kitty"), None, None, || {
            let fb = FrameBuffer::new(2, 2);
            let bytes = dispatch(Protocol::Auto, &fb).unwrap();
            assert!(
                bytes.starts_with(b"\x1b_G"),
                "Auto with TERM=xterm-kitty must dispatch to Kitty, got prefix: {:?}",
                &bytes[..bytes.len().min(8)],
            );
        });
    }

    #[cfg(feature = "sixel-encoder")]
    #[test]
    fn dispatch_auto_recurses_through_detect_resolves_to_sixel() {
        // Mirror of the Kitty-side recursion test. Set
        // `TERM=tmux-256color` (a known Sixel-fallback
        // terminfo name) so `detect()` deterministically
        // returns Sixel, then assert the dispatch output
        // starts with `\x1bP` (Sixel's DCS introducer). Catches
        // a regression that would make the recursion land in
        // the wrong arm on the Sixel side.
        with_env(Some("tmux-256color"), None, None, || {
            let fb = FrameBuffer::new(2, 2);
            let bytes = dispatch(Protocol::Auto, &fb).unwrap();
            assert!(
                bytes.starts_with(b"\x1bP"),
                "Auto with TERM=tmux-256color must dispatch to Sixel, got prefix: {:?}",
                &bytes[..bytes.len().min(8)],
            );
        });
    }

    // Without `kitty-encoder`, the recursion's Kitty arm
    // returns `Err(UnsupportedProtocol("kitty"))` -- which
    // also proves the dispatch terminates. The
    // `dispatch_auto_with_term_tmux_delegates_to_sixel` test
    // below covers the sixel-side recursion with a known env.

    #[test]
    fn dispatch_auto_with_term_program_kitty_delegates_to_kitty() {
        with_env(None, Some("kitty"), None, || {
            // When kitty-encoder is on, dispatch should
            // produce Kitty escape bytes (start with
            // `\x1b_G`).
            #[cfg(feature = "kitty-encoder")]
            {
                let fb = FrameBuffer::new(2, 2);
                let bytes = dispatch(Protocol::Auto, &fb).unwrap();
                assert!(
                    bytes.starts_with(b"\x1b_G"),
                    "Auto with TERM_PROGRAM=kitty must dispatch to Kitty, got prefix: {:?}",
                    &bytes[..bytes.len().min(8)],
                );
            }
            // When kitty-encoder is off, dispatch should
            // report the kitty feature is missing (the
            // recursion lands in the disabled-feature Kitty
            // arm).
            #[cfg(not(feature = "kitty-encoder"))]
            {
                let fb = FrameBuffer::new(2, 2);
                let err = dispatch(Protocol::Auto, &fb).unwrap_err();
                assert_eq!(err, EncoderError::UnsupportedProtocol("kitty"));
            }
        });
    }

    #[test]
    fn dispatch_auto_with_term_tmux_delegates_to_sixel() {
        with_env(Some("tmux-256color"), None, None, || {
            // When sixel-encoder is on, dispatch should
            // produce Sixel escape bytes (start with
            // `\x1bP`).
            #[cfg(feature = "sixel-encoder")]
            {
                let fb = FrameBuffer::new(2, 2);
                let bytes = dispatch(Protocol::Auto, &fb).unwrap();
                assert!(
                    bytes.starts_with(b"\x1bP"),
                    "Auto with TERM=tmux-256color must dispatch to Sixel, got prefix: {:?}",
                    &bytes[..bytes.len().min(8)],
                );
            }
            // When sixel-encoder is off, dispatch should
            // report the sixel feature is missing.
            #[cfg(not(feature = "sixel-encoder"))]
            {
                let fb = FrameBuffer::new(2, 2);
                let err = dispatch(Protocol::Auto, &fb).unwrap_err();
                assert_eq!(err, EncoderError::UnsupportedProtocol("sixel"));
            }
        });
    }

    // -- existing per-encoder tests (kept verbatim from v0.6.0) --------

    #[cfg(not(feature = "sixel-encoder"))]
    #[test]
    fn sixel_encode_is_unsupported_without_feature() {
        let fb = FrameBuffer::new(2, 2);
        let err = Protocol::Sixel.encode(&fb).unwrap_err();
        assert_eq!(err, EncoderError::UnsupportedProtocol("sixel"));
    }

    #[cfg(not(feature = "kitty-encoder"))]
    #[test]
    fn kitty_encode_is_unsupported_without_feature() {
        let fb = FrameBuffer::new(2, 2);
        let err = Protocol::Kitty.encode(&fb).unwrap_err();
        assert_eq!(err, EncoderError::UnsupportedProtocol("kitty"));
    }

    #[cfg(feature = "kitty-encoder")]
    #[test]
    fn kitty_encode_rejects_zero_dimensions() {
        let fb_zero_w = FrameBuffer::new(0, 5);
        let fb_zero_h = FrameBuffer::new(5, 0);
        let fb_zero_both = FrameBuffer::new(0, 0);
        for fb in [&fb_zero_w, &fb_zero_h, &fb_zero_both] {
            let err = Protocol::Kitty.encode(fb).unwrap_err();
            assert!(matches!(err, EncoderError::InvalidDimensions { .. }));
        }
    }

    #[cfg(feature = "kitty-encoder")]
    #[test]
    fn kitty_encode_produces_valid_escape_framing() {
        let mut fb = FrameBuffer::new(2, 2);
        for px in fb.pixels_mut() {
            *px = [255, 0, 0, 255];
        }
        let bytes = Protocol::Kitty.encode(&fb).unwrap();
        assert!(!bytes.is_empty());
        assert!(bytes.starts_with(b"\x1b_G"));
        assert!(bytes.ends_with(b"\x1b\\"));
        let s = std::str::from_utf8(&bytes).unwrap_or("");
        let payload_start = "\x1b_G".len();
        let payload_end = s.find(';').unwrap_or(s.len());
        let controls = &s[payload_start..payload_end];
        for key in &["a=T", "f=32", "q=2", "s=2", "v=2"] {
            assert!(
                controls.contains(key),
                "controls must include `{key}`, got: {controls:?}",
            );
        }
    }

    #[cfg(feature = "kitty-encoder")]
    #[test]
    fn kitty_encode_is_deterministic_for_same_input() {
        let mut fb = FrameBuffer::new(3, 3);
        for px in fb.pixels_mut() {
            *px = [10, 20, 30, 255];
        }
        let a = Protocol::Kitty.encode(&fb).unwrap();
        let b = Protocol::Kitty.encode(&fb).unwrap();
        assert_eq!(a, b);
    }

    #[cfg(feature = "sixel-encoder")]
    #[test]
    fn sixel_encode_rejects_zero_dimensions() {
        let fb_zero_w = FrameBuffer::new(0, 5);
        let fb_zero_h = FrameBuffer::new(5, 0);
        let fb_zero_both = FrameBuffer::new(0, 0);
        for fb in [&fb_zero_w, &fb_zero_h, &fb_zero_both] {
            let err = Protocol::Sixel.encode(fb).unwrap_err();
            assert!(matches!(err, EncoderError::InvalidDimensions { .. }));
        }
    }

    #[cfg(feature = "sixel-encoder")]
    #[test]
    fn sixel_encode_produces_valid_dcs_framing() {
        let mut fb = FrameBuffer::new(2, 2);
        for px in fb.pixels_mut() {
            *px = [255, 0, 0, 255];
        }
        let bytes = Protocol::Sixel.encode(&fb).unwrap();
        assert!(!bytes.is_empty());
        assert!(bytes.starts_with(b"\x1bP"));
        assert!(bytes.ends_with(b"\x1b\\"));
        let header_end = bytes.iter().position(|&b| b == b'#').unwrap_or(bytes.len());
        let header = &bytes[..header_end];
        assert!(header.contains(&b'q'));
        assert!(bytes.len() > 16);
        let s = std::str::from_utf8(&bytes).unwrap_or("");
        assert!(s.contains('2'));
    }

    #[cfg(feature = "sixel-encoder")]
    #[test]
    fn sixel_encode_is_deterministic_for_same_input() {
        let mut fb = FrameBuffer::new(3, 3);
        for px in fb.pixels_mut() {
            *px = [10, 20, 30, 255];
        }
        let a = Protocol::Sixel.encode(&fb).unwrap();
        let b = Protocol::Sixel.encode(&fb).unwrap();
        assert_eq!(a, b);
    }

    // -- detect_with_probe short-circuit test ----------------------------

    #[cfg(feature = "kitty-encoder")]
    #[test]
    fn detect_with_probe_short_circuits_when_env_already_kitty() {
        // When env-var already says Kitty, `detect_with_probe`
        // must return Ok(Kitty) WITHOUT invoking the I/O
        // probe. We can't directly observe the probe (it
        // would block), but we can verify the function
        // returns quickly with the env-var result and doesn't
        // error.
        with_env(None, Some("kitty"), None, || {
            let proto = super::detect_with_probe().expect("probe short-circuits");
            assert_eq!(proto, Protocol::Kitty);
        });
    }

    // -- end-to-end Protocol::Auto.encode test (gated on both features) --

    #[cfg(all(feature = "kitty-encoder", feature = "sixel-encoder"))]
    #[test]
    fn auto_encode_through_trait_delegates_to_dispatch() {
        // The `ProtocolEncoder for Protocol` impl is a
        // one-line `dispatch(*self, frame)` wrapper. This
        // test verifies that the trait entry point and the
        // direct dispatch call produce byte-identical output
        // for `Protocol::Auto`. Without this test, a
        // regression that accidentally inlined a different
        // path in the trait impl would not be caught by the
        // existing tests (which all call `dispatch`
        // directly).
        let mut fb = FrameBuffer::new(2, 2);
        for px in fb.pixels_mut() {
            *px = [128, 64, 32, 255];
        }
        let through_trait = Protocol::Auto.encode(&fb).unwrap();
        let through_dispatch = dispatch(Protocol::Auto, &fb).unwrap();
        assert_eq!(
            through_trait, through_dispatch,
            "Protocol::Auto.encode must go through dispatch"
        );
    }
}
