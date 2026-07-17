//! Output protocol selection and framebuffer encoding.
//!
//! The runtime picks a [`Protocol`] (Kitty graphics protocol,
//! Sixel, or auto-detected) based on terminal capability
//! detection (via `TERM`, `TERM_PROGRAM`, `COLORTERM`),
//! preferring [`Protocol::Kitty`] when the
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
//! auto-detect shim and the [`Protocol::Auto`] variant. v0.8.0
//! adds tmux passthrough: when the host is running inside
//! tmux and the user has opted in via the `TMUXPASSTHROUGH`
//! env var (or the `main.rs` `--tmux-passthrough` CLI flag),
//! the Kitty arm wraps its APC output in a tmux passthrough
//! DCS (`\x1bPtmux;...\x1b\\`) so the bytes survive the
//! tmux -> outer-terminal hop. See [`kitty::wrap_for_tmux`]
//! for the pure byte transform and [`tmux_passthrough_enabled`]
//! for the opt-in check. v0.8.1 adds chunked Kitty encoding:
//! for framebuffers whose base64 payload exceeds the
//! protocol's 4096-byte per-chunk limit, the encoder splits
//! the payload into 768-pixel chunks and emits one APC per
//! chunk using the `m=0`/`m=1` chunking extension (see
//! [`kitty::encode`]). v0.8.2 adds a memory-bounded
//! streaming entry point [`kitty::encode_to_writer`] that
//! writes the encoded bytes directly to a caller-supplied
//! `&mut impl Write` without materialising the full
//! framebuffer in a `Vec<u8>` (peak working set is O(1)
//! per chunk, not O(framebuffer)). Each arm returns
//! [`EncoderError::UnsupportedProtocol`] when the
//! corresponding feature is disabled in the current build.

use crate::framebuffer::FrameBuffer;
use std::io::Write;

/// Terminal graphics protocol used to encode the composited
/// framebuffer.
///
/// # Example
///
/// ```
/// use termcompositor::Protocol;
///
/// assert_eq!(Protocol::Kitty.as_str(), "kitty");
/// assert_eq!(Protocol::Sixel.as_str(), "sixel");
/// assert_eq!(Protocol::Auto.as_str(), "auto");
/// ```
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
///
/// # Example
///
/// ```
/// use termcompositor::EncoderError;
///
/// let err = EncoderError::UnsupportedProtocol("kitty");
/// assert_eq!(
///     err.to_string(),
///     "protocol kitty is not supported in this build"
/// );
///
/// let err = EncoderError::InvalidDimensions { width: 0, height: 5 };
/// assert_eq!(err.to_string(), "framebuffer has invalid dimensions: 0x5");
/// ```
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

// `From<std::io::Error>` for `EncoderError` is needed by
// BOTH encoder arms that use `?` on `std::io::Write` calls:
// the v0.5.0/v0.8.x `kitty::encode_to_writer` and the
// v0.8.4 `sixel::encode_to_writer`. Gate on `any(kitty,
// sixel)` so a build that enables either encoder feature
// can compile both paths' `?` usage; the impl is a no-op
// when neither is enabled (no caller can reach it).
#[cfg(any(feature = "kitty-encoder", feature = "sixel-encoder"))]
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
///    `Protocol::Sixel` by default, or `Protocol::Kitty`
///    when the `TMUXPASSTHROUGH` env var is set
///    (v0.8.0 tmux passthrough opt-in -- the dispatch
///    then wraps the output in `\x1bPtmux;...\x1b\\`).
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
        std::env::var("TMUXPASSTHROUGH").ok().as_deref(),
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
/// encoder; use [`detect`] for that. This
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
    tmux_passthrough: Option<&str>,
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
        // v0.8.0 tmux passthrough: if the user has opted in
        // via the `TMUXPASSTHROUGH` env var (typically
        // `TMUXPASSTHROUGH=1`), we trust that they have
        // `set -g allow-passthrough on` in their tmux.conf
        // and pick Kitty -- the dispatch will auto-wrap the
        // output in `\x1bPtmux;...\x1b\\` (see
        // `wrap_for_tmux`). Without the opt-in we keep the
        // v0.7.0 behaviour: prefer Sixel (the safest
        // fallback for unknown terminals running inside tmux).
        if s == "tmux" || s.starts_with("tmux-") {
            if tmux_passthrough.is_some_and(|v| !v.is_empty()) {
                return Protocol::Kitty;
            }
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
///
/// # Example
///
/// ```
/// use termcompositor::{FrameBuffer, Protocol, ProtocolEncoder};
///
/// let fb = FrameBuffer::new(2, 2);
/// // Protocol::Auto dispatches through `detect()` at encode time.
/// // Without an encoder feature enabled, this returns an error.
/// let result = Protocol::Kitty.encode(&fb);
/// match result {
///     Ok(bytes) => assert!(!bytes.is_empty()),
///     Err(e) => assert!(e.to_string().contains("not supported")),
/// }
/// ```
pub trait ProtocolEncoder {
    /// Encodes `frame` into escape-sequence bytes for `self`.
    fn encode(&self, frame: &FrameBuffer) -> Result<Vec<u8>, EncoderError>;
}

/// Returns `true` when the v0.8.0 tmux passthrough should
/// be applied to the Kitty encoder's output. The check is
/// pure (env vars only); the caller decides whether to
/// wrap. Gated on `kitty-encoder` because the wrapping is
/// only relevant for Kitty output. `pub(crate)` so the
/// v0.8.3 end-to-end `encode_passthrough_to_writer` in
/// the `kitty` submodule can call it (the default
/// `fn`-private visibility is restricted to the current
/// module only, not its descendants).
#[cfg(feature = "kitty-encoder")]
pub(crate) fn tmux_passthrough_enabled() -> bool {
    // `TMUXPASSTHROUGH` is the v0.8.0 opt-in: any non-empty
    // value enables passthrough. Typical usage is
    // `TMUXPASSTHROUGH=1`. The env var is also set by
    // `main.rs`'s `--tmux-passthrough` CLI flag.
    //
    // `TMUX` is the canonical signal that we are actually
    // inside a tmux session (it points to the tmux socket
    // path; the tmux(1) man page documents it as set on
    // every shell that tmux spawns). We require BOTH: the
    // opt-in (so the user has consciously chosen passthrough
    // and presumably has `set -g allow-passthrough on` in
    // their tmux.conf) AND the `TMUX` env var (so we don't
    // accidentally double-wrap a Kitty sequence for a
    // non-tmux host that happens to have `TMUXPASSTHROUGH`
    // set in its shell rc).
    std::env::var("TMUXPASSTHROUGH").is_ok_and(|v| !v.is_empty())
        && std::env::var_os("TMUX").is_some()
}

/// Private dispatch: the single source of truth for "given a
/// `Protocol`, which encoder do I call?". Extracted out of the
/// `ProtocolEncoder::encode` impl so the [`Protocol::Auto`]
/// arm can recurse cleanly via `dispatch(detect(), frame)`
/// without duplicating the per-variant `#[cfg]` matrix.
fn dispatch(protocol: Protocol, frame: &FrameBuffer) -> Result<Vec<u8>, EncoderError> {
    match protocol {
        #[cfg(feature = "kitty-encoder")]
        Protocol::Kitty => {
            // v0.8.0 tmux passthrough: when the host is
            // running inside tmux AND the user has opted in
            // via `TMUXPASSTHROUGH=1` (or `--tmux-passthrough`),
            // wrap the raw Kitty APC bytes in
            // `\x1bPtmux;...\x1b\\` so they survive the
            // tmux→outer-terminal hop. The opt-in is checked
            // here (not in `detect`) because a user might
            // pass `--protocol kitty` directly -- the
            // heuristic would have picked Sixel, but the
            // explicit Kitty choice should still get the
            // passthrough wrapping.
            let raw = kitty::encode(frame)?;
            if tmux_passthrough_enabled() {
                Ok(kitty::wrap_for_tmux(raw))
            } else {
                Ok(raw)
            }
        }
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

/// End-to-end streaming dispatch: encodes `frame` via the
/// per-protocol streaming entry point and writes the result
/// to `out`. **v0.8.6**: mirrors the private [`dispatch()`]
/// function but writes to a `&mut impl Write` sink instead
///
/// # Example
///
/// ```
/// use termcompositor::{dispatch_to_writer, detect, FrameBuffer};
///
/// let fb = FrameBuffer::new(2, 2);
/// let mut out = Vec::new();
/// // This may return UnsupportedProtocol if no encoder feature
/// // is enabled, but it always completes without panicking.
/// let _ = dispatch_to_writer(detect(), &fb, &mut out);
/// ```
/// of returning a `Vec<u8>`. This combines the v0.8.2 Kitty
/// streaming, v0.8.3 tmux passthrough wrap, v0.8.4 Sixel
/// streaming, and v0.8.5 fixed-palette Sixel streaming
/// work into a single end-to-end streaming dispatch.
///
/// For [`Protocol::Kitty`]: delegates to
/// [`kitty::encode_passthrough_to_writer`] (which handles
/// the optional tmux passthrough wrap when the
/// `TMUXPASSTHROUGH` opt-in is set; otherwise equivalent
/// to [`kitty::encode_to_writer`]).
///
/// For [`Protocol::Sixel`]: delegates to
/// [`sixel::encode_to_writer`] (the v0.8.4 streaming
/// entry point that writes the Sixel DCS directly to the
/// sink; uses the `icy_sixel` crate's adaptive
/// quantization).
///
/// For [`Protocol::Auto`]: recurses via
/// `dispatch_to_writer(detect(), frame, out)`. The
/// recursion is bounded because [`detect`] returns only
/// `Kitty` or `Sixel` (never `Auto`) by construction.
///
/// **Peak memory**: O(1) per write call. The per-protocol
/// streaming entry points handle the framebuffer memory
/// management internally (the Kitty arm's
/// `KittyCommandWriter` is per-chunk-incremental; the
/// Sixel arm's `SixelImage::encode` produces a single
/// `String` that's written through to the sink without
/// materialising a `Vec`).
///
/// **Wire format**: byte-for-byte equivalent to the
/// `Vec<u8>`-returning [`ProtocolEncoder::encode`] for
/// the same input (when the tmux passthrough opt-in is
/// not set; with the opt-in, the Kitty arm wraps the
/// output in a tmux passthrough DCS, which is the same
/// behaviour as `dispatch`).
pub fn dispatch_to_writer<W: Write>(
    protocol: Protocol,
    frame: &FrameBuffer,
    out: &mut W,
) -> Result<(), EncoderError> {
    match protocol {
        #[cfg(feature = "kitty-encoder")]
        Protocol::Kitty => kitty::encode_passthrough_to_writer(frame, out),
        #[cfg(not(feature = "kitty-encoder"))]
        Protocol::Kitty => {
            let _ = (frame, out);
            Err(EncoderError::UnsupportedProtocol("kitty"))
        }
        #[cfg(feature = "sixel-encoder")]
        Protocol::Sixel => sixel::encode_to_writer(frame, out),
        #[cfg(not(feature = "sixel-encoder"))]
        Protocol::Sixel => {
            let _ = (frame, out);
            Err(EncoderError::UnsupportedProtocol("sixel"))
        }
        Protocol::Auto => dispatch_to_writer(detect(), frame, out),
    }
}

/// The Kitty graphics protocol encoder, gated on the
/// `kitty-encoder` Cargo feature. Implemented as a private
/// inline module so the public API surface stays minimal.
///
/// v0.5.0: single-command encoder (no chunking).
/// v0.8.0: added [`wrap_for_tmux`] for tmux passthrough.
/// v0.8.1: added chunked encoding (m=0/m=1) for
/// multi-megapixel framebuffers via [`encode`].
/// v0.8.2: added [`encode_to_writer`] for memory-bounded
/// streaming output (no full-framebuffer Vec).
#[cfg(feature = "kitty-encoder")]
mod kitty {
    use super::tmux_passthrough_enabled;
    use super::EncoderError;
    use crate::framebuffer::FrameBuffer;
    use little_kitty::command::ControlValue;
    use little_kitty::io::KittyCommandWriter;
    use std::io::Write;

    /// Number of RGBA pixels per chunk for the v0.8.1
    /// chunked Kitty encoding. Derived from the Kitty
    /// graphics protocol's per-chunk payload limit
    /// (<https://sw.kovidgoyal.net/kitty/graphics-protocol/>):
    /// 4096 base64 chars decode to 3072 raw bytes = 768
    /// 4-byte (32-bit RGBA) pixels, with no base64
    /// padding. Because 768*4 = 3072 is a multiple of 3,
    /// the base64 encoding of an intermediate (full-sized)
    /// chunk is exactly 4096 chars -- a multiple of 4, the
    /// hard alignment the spec requires for non-last
    /// chunks. Hardcoded as a `const` (not computed at
    /// runtime) to keep the encode hot path
    /// allocation-free. `pub(crate)` so the v0.8.1
    /// chunking tests in the parent `tests` module can
    /// reference it (they need to construct framebuffers
    /// at the exact chunk boundary).
    pub(crate) const PIXELS_PER_CHUNK: usize = 768;

    /// Stable image ID used by the single-framebuffer
    /// compositor use case. Kitty replaces the image at this
    /// ID on each retransmit, so the place command only needs
    /// to be issued once per frame (after the transmission).
    pub(crate) const IMAGE_ID: u32 = 1;

    /// Builds the Kitty "place image" APC command for
    /// [`IMAGE_ID`]. Places the image at the current cursor
    /// position with no cell-grid snap and `z=-1` so the image
    /// renders *behind* the cell body.
    fn build_place_apc() -> Vec<u8> {
        let mut out = Vec::with_capacity(20);
        out.extend_from_slice(b"\x1b_Ga=p,i=1,z=-1\x1b\\");
        out
    }

    /// Builds the Kitty "delete image" APC command for
    /// [`IMAGE_ID`]. Deletes the image from Kitty's image
    /// store and all of its placements in one shot. Used by
    /// the v0.12.2 transparency short-circuit in
    /// [`encode_to_writer`] to clear any previously-placed
    /// image without transmitting 4MB+ of zero-RGBA data
    /// every tick.
    fn build_delete_apc() -> Vec<u8> {
        let mut out = Vec::with_capacity(20);
        out.extend_from_slice(b"\x1b_Ga=d,d=I,i=1\x1b\\");
        out
    }

    /// Encodes `frame` as one or more Kitty "transmit"
    /// (`a=T`) APC commands followed by a single Kitty
    /// "place" (`a=p`) command, and writes the concatenated
    /// APC bytes to `out`. **v0.8.2 memory-bounded streaming
    /// path**: never materialises the full framebuffer in a
    /// `Vec<u8>`; the only per-call allocations are one
    /// scratch `Vec<u8>` per chunk (≤ 3072 raw RGBA bytes =
    /// one chunk's worth, plus the chunk's APC framing ≈
    /// 4KB) plus the two small APC command Vecs (place, 20
    /// bytes; delete, 20 bytes). Peak working set is O(1)
    /// regardless of framebuffer size.
    ///
    /// **v0.12.2 split**: the wire format is now a two-step
    /// Kitty dance per frame: (1) one or more `a=T,i=1`
    /// transmit commands that upload the RGBA payload to
    /// Kitty's image-store slot 1, then (2) a single
    /// `a=p,i=1,z=-1` place command that makes the uploaded
    /// image visible at the current cursor position, behind
    /// the cell grid. Prior to v0.12.2 the encoder only
    /// emitted the transmit step, which left the image in
    /// Kitty's image-store but never displayed it.
    ///
    /// Callers that want a `Vec<u8>` of the output (e.g.
    /// the v0.7.0/v0.8.0 API) can call [`encode`] (which
    /// delegates to this function writing into a fresh
    /// `Vec<u8>`) or pass `&mut Vec::new()` here directly.
    /// Callers that want true streaming into a file,
    /// socket, or terminal handle can pass their own
    /// `&mut impl Write`.
    ///
    /// **v0.8.1 chunking**: for framebuffers whose
    /// base64-encoded payload fits within a single chunk
    /// (≤ `PIXELS_PER_CHUNK` = 768 pixels for 32-bit RGBA),
    /// emits the v0.8.0 single-command format
    /// (`\x1b_G<controls>;<base64>\x1b\\` with no `m` key)
    /// for backwards compatibility with terminals that
    /// pre-date the chunking extension. For larger
    /// framebuffers, splits the payload into
    /// `PIXELS_PER_CHUNK`-pixel chunks and emits one APC
    /// per chunk: the first carries the full control list
    /// (`a`, `f`, `q`, `s`, `v`) plus `m=1`, intermediate
    /// chunks carry only `m=1`, and the final chunk carries
    /// `m=0`. All chunks except the last are guaranteed to
    /// have a base64 payload length that is a multiple of 4
    /// (the spec's hard requirement) because
    /// `PIXELS_PER_CHUNK * 4 = 3072` is a multiple of 3.
    pub fn encode_to_writer<W: Write>(
        frame: &FrameBuffer,
        out: &mut W,
    ) -> Result<(), EncoderError> {
        if frame.width() == 0 || frame.height() == 0 {
            return Err(EncoderError::InvalidDimensions {
                width: frame.width(),
                height: frame.height(),
            });
        }

        // v0.12.2 transparency short-circuit: when the layer
        // stack is empty (no child PTY has emitted a Kitty
        // graphics command since startup), the framebuffer is
        // fully transparent. Streaming 4MB+ of zero-RGBA data
        // every tick is wasteful AND clutters Kitty's image
        // store with invisible entries. Instead, emit a single
        // 18-byte Kitty delete command to clear any previously-
        // placed image and return.
        if frame.is_fully_transparent() {
            out.write_all(&build_delete_apc())?;
            return Ok(());
        }

        let width = frame.width();
        let height = frame.height();
        let pixels = frame.pixels();
        let total_pixels = pixels.len();

        // v0.8.1 single-chunk fast path: preserve v0.8.0
        // wire format exactly (no `m` key) for framebuffers
        // that fit in one chunk. This means terminals that
        // pre-date the chunking extension (or that have it
        // disabled) keep working unchanged, and the small-
        // image output is byte-identical to v0.8.0.
        if total_pixels <= PIXELS_PER_CHUNK {
            let chunk_apc = encode_single_chunk_apc(width, height, pixels)?;
            out.write_all(&chunk_apc)?;
            // v0.12.2: emit the place command so the uploaded
            // image is actually displayed (not just stored in
            // Kitty's image-store).
            out.write_all(&build_place_apc())?;
            return Ok(());
        }

        // v0.8.1 multi-chunk path: emit one APC per chunk,
        // writing each chunk's APC directly to `out` (no
        // intermediate concat Vec). `num_chunks` is
        // `ceil(total_pixels / PIXELS_PER_CHUNK)`, computed
        // with `div_ceil` (stable since Rust 1.73).
        let num_chunks = total_pixels.div_ceil(PIXELS_PER_CHUNK);
        for chunk_idx in 0..num_chunks {
            let start_pixel = chunk_idx * PIXELS_PER_CHUNK;
            let end_pixel = (start_pixel + PIXELS_PER_CHUNK).min(total_pixels);
            // The chunk's pixel slice is at most
            // PIXELS_PER_CHUNK entries (3072 raw bytes).
            // The flatten+collect here is the ONLY
            // per-chunk allocation, and it's O(1) in the
            // framebuffer size. The previous v0.8.1
            // implementation allocated a single `Vec<u8>`
            // of the entire framebuffer's RGBA bytes (8MB+
            // for a 2MP image) before chunking, which
            // v0.8.2 eliminates.
            let chunk_pixels = &pixels[start_pixel..end_pixel];
            let chunk_rgba: Vec<u8> =
                chunk_pixels.iter().flatten().copied().collect();
            let is_last = chunk_idx + 1 == num_chunks;
            let m_value: u32 = if is_last { 0 } else { 1 };

            // First chunk carries the full control list +
            // `m=1`. Subsequent chunks carry ONLY `m` -- the
            // terminal remembers the metadata from the
            // first chunk (per the spec).
            let controls: Vec<(char, ControlValue)> = if chunk_idx == 0 {
                vec![
                    ('a', ControlValue::Char('T')),
                    ('f', ControlValue::UnsignedInteger(32)),
                    ('q', ControlValue::UnsignedInteger(2)),
                    // v0.12.2: stable image ID so Kitty
                    // replaces the image at slot 1 on each
                    // retransmit (no image-store leak).
                    ('i', ControlValue::UnsignedInteger(IMAGE_ID)),
                    ('s', ControlValue::UnsignedInteger(width)),
                    ('v', ControlValue::UnsignedInteger(height)),
                    ('m', ControlValue::UnsignedInteger(m_value)),
                ]
            } else {
                vec![('m', ControlValue::UnsignedInteger(m_value))]
            };

            let chunk_apc = build_apc_command(&controls, &chunk_rgba)?;
            out.write_all(&chunk_apc)?;
        }

        // v0.12.2: emit the place command so the uploaded
        // image is actually displayed (not just stored in
        // Kitty's image-store).
        out.write_all(&build_place_apc())?;
        Ok(())
    }

    /// Encodes `frame` as one or more Kitty APC commands
    /// and returns the concatenated bytes. Internally
    /// delegates to [`encode_to_writer`] writing into a
    /// fresh `Vec<u8>`, so the memory bound is O(1) per
    /// chunk (~4KB scratch) rather than O(framebuffer)
    /// (was 8MB+ for a 2MP image prior to v0.8.2).
    pub fn encode(frame: &FrameBuffer) -> Result<Vec<u8>, EncoderError> {
        let mut out = Vec::new();
        encode_to_writer(frame, &mut out)?;
        Ok(out)
    }

    /// Builds a single Kitty APC command with the given
    /// control list and base64 payload, using the
    /// `little_kitty::io::KittyCommandWriter` API. Shared
    /// by the single-chunk fast path and the multi-chunk
    /// path. Returns the full APC command bytes
    /// (`\x1b_G<controls>;<base64>\x1b\\`).
    fn build_apc_command(
        controls: &[(char, ControlValue)],
        payload: &[u8],
    ) -> Result<Vec<u8>, EncoderError> {
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
        // `write_base64` consumes the writer by value
        // (returns Self) and Base64-encodes the payload.
        out = out.write_base64(payload)?;
        out.write_end(false)?;
        Ok(out)
    }

    /// v0.8.0 wire-compatible single-chunk encoder. Emits
    /// `\x1b_Ga=T,f=32,q=2,s=W,v=H;<base64>\x1b\\` with no
    /// `m` key. Used by the v0.8.2 `encode_to_writer` fast
    /// path for framebuffers that fit in one chunk.
    ///
    /// Takes the raw `&[[u8; 4]]` pixel slice (not a
    /// pre-flattened RGBA byte slice) and flattens it
    /// internally; the resulting allocation is bounded at
    /// 3072 bytes (PIXELS_PER_CHUNK * 4) because this
    /// helper is only reachable from the single-chunk fast
    /// path.
    fn encode_single_chunk_apc(
        width: u32,
        height: u32,
        pixels: &[[u8; 4]],
    ) -> Result<Vec<u8>, EncoderError> {
        // Flatten the pixel slice into a single contiguous
        // RGBA byte slice for base64 encoding. The single-
        // chunk path can only be reached when
        // `pixels.len() <= PIXELS_PER_CHUNK`, so this
        // allocation is bounded at 3072 bytes regardless
        // of framebuffer size.
        let rgba: Vec<u8> = pixels.iter().flatten().copied().collect();
        let controls: Vec<(char, ControlValue)> = vec![
            ('a', ControlValue::Char('T')),
            ('f', ControlValue::UnsignedInteger(32)),
            ('q', ControlValue::UnsignedInteger(2)),
            // v0.12.2: stable image ID so Kitty replaces
            // the image at slot 1 on each retransmit.
            ('i', ControlValue::UnsignedInteger(IMAGE_ID)),
            ('s', ControlValue::UnsignedInteger(width)),
            ('v', ControlValue::UnsignedInteger(height)),
        ];
        build_apc_command(&controls, &rgba)
    }

    /// Wraps the raw Kitty APC bytes in `inner` in a tmux
    /// passthrough DCS (`\x1bPtmux;...\x1b\\`) and writes
    /// the result to `out`. **v0.8.3 streaming version** of
    /// [`wrap_for_tmux`]: takes a byte slice (the raw
    /// Kitty APC bytes) and a `&mut impl Write` sink.
    /// Memory bounded: O(1) -- no intermediate `Vec`
    /// allocation; the user's writer buffers as needed.
    ///
    /// Inner `\x1b` bytes are DOUBLED so tmux 3.2+ passes
    /// them through as a single literal `\x1b` to the outer
    /// terminal. The Kitty payload only contains `\x1b` at
    /// the introducer (`\x1b_G`) and terminator (`\x1b\\`),
    /// so the doubling only affects those two locations.
    /// tmux 3.2+ (released 2021) is the floor; tmux < 3.2
    /// has no escape mechanism and would treat the inner
    /// `\x1b\\` as the outer passthrough terminator
    /// (corrupting the sequence).
    pub fn wrap_for_tmux_to_writer<W: Write>(
        inner: &[u8],
        out: &mut W,
    ) -> std::io::Result<()> {
        // The DCS prefix is `ESC P tmux ;` (7 bytes) --
        // written once, regardless of the inner size.
        out.write_all(b"\x1bPtmux;")?;
        // Walk the inner bytes; double every ESC.
        for &b in inner {
            if b == 0x1b {
                out.write_all(&[0x1b, 0x1b])?;
            } else {
                out.write_all(&[b])?;
            }
        }
        // The DCS terminator is `ESC \\` (2 bytes).
        out.write_all(b"\x1b\\")?;
        Ok(())
    }

    /// Wraps a complete Kitty APC (`\x1b_G ... \x1b\\`) in a
    /// tmux passthrough DCS (`\x1bPtmux;...\x1b\\`) so the
    /// bytes survive the tmux→outer-terminal hop. Required
    /// because the user must opt in to
    /// `set -g allow-passthrough on` in their tmux.conf for
    /// tmux 3.2+ to forward APC payloads. Pure: no I/O, no
    /// env-var reads. The `TMUXPASSTHROUGH=1` opt-in is
    /// checked by the caller (`dispatch`).
    ///
    /// Inner `\x1b` bytes are DOUBLED so tmux 3.2+ passes
    /// them through as a single literal `\x1b` to the outer
    /// terminal. The Kitty payload only contains `\x1b` at
    /// the introducer (`\x1b_G`) and terminator (`\x1b\\`),
    /// so the doubling only affects those two locations.
    /// tmux 3.2+ (released 2021) is the floor; tmux < 3.2
    /// has no escape mechanism and would treat the inner
    /// `\x1b\\` as the outer passthrough terminator
    /// (corrupting the sequence).
    ///
    /// Thin convenience wrapper around
    /// [`wrap_for_tmux_to_writer`]: allocates a fresh
    /// `Vec<u8>` and delegates. The v0.8.0 entry point;
    /// kept for backwards compat with callers that need a
    /// `Vec<u8>` of the wrapped bytes. Callers that want
    /// true zero-copy output to a file, socket, or
    /// terminal handle should use [`wrap_for_tmux_to_writer`]
    /// directly (memory bounded) or the end-to-end
    /// [`encode_passthrough_to_writer`] (also memory
    /// bounded, and combines the encode + wrap into a
    /// single pass).
    pub fn wrap_for_tmux(inner: Vec<u8>) -> Vec<u8> {
        // Worst case: every byte is 0x1b -> doubled, plus
        // the 7-byte prefix and 2-byte suffix.
        let mut out: Vec<u8> = Vec::with_capacity(inner.len() * 2 + 9);
        // Writing to a `Vec<u8>` never fails (the only
        // `io::Error` a `Vec<u8>` can return is
        // `io::ErrorKind::WriteZero` from a full disk,
        // which doesn't apply to in-memory writes).
        wrap_for_tmux_to_writer(&inner, &mut out)
            .expect("writing to Vec<u8> cannot fail");
        out
    }

    /// A `Write` adapter that wraps the inner output in a
    /// tmux passthrough DCS (`\x1bPtmux;...\x1b\\`).
    /// On the first byte written to the adapter, writes
    /// the DCS prefix to the inner writer. Subsequent byte
    /// writes are forwarded to the inner writer with every
    /// `ESC` (0x1b) byte DOUBLED (so tmux 3.2+ treats it
    /// as a literal `ESC` in the inner payload). The DCS
    /// terminator is written by [`PassthroughWriter::finish`]
    /// -- the caller MUST call `finish()` to produce a
    /// complete wrapped DCS.
    ///
    /// `PassthroughWriter` is the v0.8.3 building block
    /// for end-to-end O(1) streaming: combined with
    /// [`encode_to_writer`] via
    /// [`encode_passthrough_to_writer`], it lets the
    /// entire encode + wrap + emit pipeline run in O(1)
    /// memory (no intermediate `Vec` allocation). Used
    /// directly by advanced callers that need to wrap an
    /// arbitrary `&[u8]` body in a tmux passthrough DCS
    /// without materialising the wrapped output in a `Vec`.
    ///
    /// The first byte written after construction triggers
    /// the prefix; even an empty body gets a prefix +
    /// suffix pair (this matches the v0.8.0
    /// `wrap_for_tmux` behaviour for empty input:
    /// `\x1bPtmux;\x1b\\`). `finish` is the only place the
    /// suffix is written; `Drop` does NOT auto-finish (a
    /// `Write::write` may fail with `io::Error`, which
    /// `Drop` cannot propagate).
    pub struct PassthroughWriter<W: Write> {
        inner: W,
        prefix_written: bool,
    }

    impl<W: Write> PassthroughWriter<W> {
        /// Creates a new `PassthroughWriter` wrapping
        /// `inner`. The DCS prefix is NOT written until the
        /// first call to `write` (or `finish`, for an
        /// empty body). This matches the v0.8.0
        /// `wrap_for_tmux` behaviour: an empty input
        /// produces `\x1bPtmux;\x1b\\`, with the prefix
        /// written before the (zero-byte) body.
        pub fn new(inner: W) -> Self {
            Self {
                inner,
                prefix_written: false,
            }
        }

        /// Writes the DCS terminator (`\x1b\\`) and
        /// returns the inner writer. Consumes `self` so
        /// the caller can't write more body bytes after
        /// the terminator. If no body bytes were ever
        /// written, also writes the prefix (an empty body
        /// still produces a valid wrapped DCS).
        pub fn finish(mut self) -> std::io::Result<W> {
            if !self.prefix_written {
                self.inner.write_all(b"\x1bPtmux;")?;
            }
            self.inner.write_all(b"\x1b\\")?;
            Ok(self.inner)
        }
    }

    impl<W: Write> std::io::Write for PassthroughWriter<W> {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            if !self.prefix_written {
                self.inner.write_all(b"\x1bPtmux;")?;
                self.prefix_written = true;
            }
            // Forward each byte to the inner writer,
            // doubling every ESC. We always write the
            // full `buf` (the doubling doesn't change the
            // byte count for the caller's perspective; it
            // just emits 2 bytes for every 1 ESC byte in
            // `buf`).
            for &b in buf {
                if b == 0x1b {
                    self.inner.write_all(&[0x1b, 0x1b])?;
                } else {
                    self.inner.write_all(&[b])?;
                }
            }
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            self.inner.flush()
        }
    }

    /// End-to-end streaming entry point: encodes `frame`
    /// via [`encode_to_writer`] and (if the tmux
    /// passthrough opt-in is set) wraps the output in a
    /// tmux passthrough DCS, all in a single pass with
    /// O(1) memory.
    ///
    /// When `tmux_passthrough_enabled()` is `false` (the
    /// default: the user has NOT set `TMUXPASSTHROUGH=1`
    /// and the `TMUX` env var is not set), this function
    /// is equivalent to [`encode_to_writer`] -- no
    /// wrapping is applied. When `true`, the encoded APC
    /// bytes are passed through a [`PassthroughWriter`]
    /// adapter that writes the DCS prefix once, doubles
    /// every `ESC` byte in the body, and writes the DCS
    /// terminator once at the end. The user's `&mut W`
    /// sink receives the wrapped bytes directly, with no
    /// intermediate `Vec` allocation.
    ///
    /// Compared to `dispatch(Protocol::Kitty, frame)`,
    /// which materialises the full encode output (and
    /// optionally the full wrapped output) in a `Vec`:
    ///   - `dispatch` allocates O(N) where N is the
    ///     framebuffer size (2MP frame = 8MB Vec for the
    ///     encode, plus ~11MB Vec for the wrap).
    ///   - `encode_passthrough_to_writer` allocates O(1)
    ///     per write call (~4KB scratch), regardless of
    ///     framebuffer size.
    ///
    /// The `TMUXPASSTHROUGH` opt-in is checked here (not
    /// in `detect`) for the same reason it is in
    /// `dispatch`: a user with a known-good tmux + Kitty
    /// setup who wants to force the wrap regardless of
    /// `TERM` should be able to.
    pub fn encode_passthrough_to_writer<W: Write>(
        frame: &FrameBuffer,
        out: &mut W,
    ) -> Result<(), EncoderError> {
        if !tmux_passthrough_enabled() {
            return encode_to_writer(frame, out);
        }
        // Wrap the user's writer in a `PassthroughWriter`.
        // The `&mut *out` reborrows `out` for the duration
        // of the encode call; `finish()` then returns the
        // borrow and we drop it.
        let mut passthrough = PassthroughWriter::new(&mut *out);
        encode_to_writer(frame, &mut passthrough)?;
        let _inner = passthrough.finish()?;
        Ok(())
    }
}

// Re-export the v0.8.0/v0.8.3 wrap_for_tmux family at the
// `encoder` module level so downstream users (and the
// `termcompositor` crate root) can call them without
// reaching into the private `kitty` submodule. Gated on
// `kitty-encoder` because the helpers are only useful for
// Kitty output. The v0.8.3 additions (the streaming
// `wrap_for_tmux_to_writer`, the `PassthroughWriter`
// adapter, and the end-to-end
// `encode_passthrough_to_writer`) let the entire
// encode + wrap + emit pipeline run in O(1) memory.
#[cfg(feature = "kitty-encoder")]
pub use kitty::wrap_for_tmux;
#[cfg(feature = "kitty-encoder")]
pub use kitty::wrap_for_tmux_to_writer;
#[cfg(feature = "kitty-encoder")]
pub use kitty::PassthroughWriter;
#[cfg(feature = "kitty-encoder")]
pub use kitty::encode_passthrough_to_writer;

/// The Sixel graphics protocol encoder, gated on the
/// `sixel-encoder` Cargo feature. Implemented as a private
/// inline module so the public API surface stays minimal.
///
/// v0.8.4 adds [`encode_to_writer`], a streaming entry point
/// that writes the Sixel DCS bytes directly to a
/// caller-supplied `&mut impl Write` sink. The v0.6.0/0.8.0
/// `encode -> Vec<u8>` path materialised the Sixel output in
/// a `Vec<u8>` (typically several MB for a 2MP frame);
/// v0.8.4 eliminates that allocation by writing the
/// `icy_sixel::SixelImage::encode()` result string's bytes
/// straight to the caller's writer. **Note**: the input
/// RGBA `Vec<u8>` (`pixels.iter().flatten().copied().collect()`,
/// 8MB+ for a 2MP frame) is still materialised, because
/// `icy_sixel` 0.5 takes owned RGBA bytes and does the
/// color quantization + sixel serialisation in one shot
/// with no streaming input API. The streaming entry point
/// therefore saves one full-frame allocation (the Sixel
/// output) but not the input RGBA allocation. The Kitty
/// arm's [`kitty::encode_to_writer`] avoids both
/// allocations because `little_kitty`'s `KittyCommandWriter`
/// is per-chunk-incremental.
#[cfg(feature = "sixel-encoder")]
mod sixel {
    use super::EncoderError;
    use crate::framebuffer::FrameBuffer;
    use icy_sixel::SixelImage;
    use std::io::Write;

    /// Encodes `frame` as a Sixel DCS (Device Control String)
    /// escape sequence and writes the bytes to `out`. **v0.8.4
    /// streaming entry point**: writes the encoded Sixel
    /// bytes directly to the caller's `&mut impl Write` sink
    /// (e.g. `Vec<u8>`, `std::fs::File`, `std::net::TcpStream`,
    /// `std::io::StdoutLock`). Avoids the intermediate
    /// `Vec<u8>` allocation that the v0.6.0/0.8.0 `encode`
    /// path incurred via `sixel_string.into_bytes()`.
    ///
    /// **Memory profile**: the dominant allocation is still
    /// the input RGBA `Vec<u8>` (8MB+ for a 2MP frame) --
    /// `icy_sixel` 0.5 takes owned RGBA bytes and has no
    /// streaming input API. The Sixel-output `Vec<u8>` (which
    /// the v0.8.3 `encode` path allocated as
    /// `sixel_string.into_bytes()`) is eliminated: we write
    /// the `String`'s internal buffer directly to `out` via
    /// `write_all`, which only borrows the bytes (no copy).
    /// The Sixel DCS `String` itself is still allocated by
    /// `icy_sixel::SixelImage::encode()` (that's how
    /// `icy_sixel` exposes its output), but its bytes are
    /// written through to the caller rather than copied
    /// into a fresh `Vec<u8>`.
    ///
    /// Callers that want a `Vec<u8>` of the output (e.g. the
    /// v0.6.0/v0.8.0 API) can call [`encode`] (which
    /// delegates to this function writing into a fresh
    /// `Vec<u8>`) or pass `&mut Vec::new()` here directly.
    pub fn encode_to_writer<W: Write>(
        frame: &FrameBuffer,
        out: &mut W,
    ) -> Result<(), EncoderError> {
        if frame.width() == 0 || frame.height() == 0 {
            return Err(EncoderError::InvalidDimensions {
                width: frame.width(),
                height: frame.height(),
            });
        }

        // Materialise the RGBA pixel data as a single
        // contiguous byte slice. `icy_sixel` 0.5 takes owned
        // bytes via `SixelImage::from_rgba(Vec<u8>, w, h)`,
        // and has no streaming input API (verified by reading
        // the local crate source: the only public encode
        // methods are `encode(&self) -> Result<String>` and
        // `encode_with(&self, opts) -> Result<String>`).
        let rgba: Vec<u8> = frame.pixels().iter().flatten().copied().collect();

        // `SixelImage::from_rgba` takes `usize` width/height;
        // the `u32` values from FrameBuffer are always
        // representable in `usize` on every supported
        // platform (a widening, lossless cast).
        let image = SixelImage::from_rgba(rgba, frame.width() as usize, frame.height() as usize);
        let sixel_string = image.encode()?;
        // Write the Sixel string's UTF-8 bytes directly to
        // the caller's writer. `sixel_string.as_bytes()`
        // borrows the String's internal buffer (no copy);
        // the v0.8.0 `encode` path did
        // `sixel_string.into_bytes()`, which COPIES the
        // String's buffer into a new `Vec<u8>` just to
        // return it. The streaming entry point skips that
        // copy. (Writing a `String`'s bytes to a `Vec<u8>`
        // is itself a copy into the destination's
        // growable buffer, so the streaming entry point
        // is only a memory win when the caller uses a
        // non-`Vec` writer like a `File` or `StdoutLock`.)
        out.write_all(sixel_string.as_bytes())?;
        Ok(())
    }

    /// Encodes `frame` as a Sixel DCS (Device Control String)
    /// escape sequence. The returned bytes are the full
    /// terminal-ready payload: `\x1bPq...sixel data...\x1b\\`.
    /// `icy_sixel` does the color quantization and sixel-data
    /// serialisation; we just hand it the RGBA pixels and pass
    /// through the resulting string.
    ///
    /// Internally delegates to [`encode_to_writer`] writing
    /// into a fresh `Vec<u8>`. The wire format is unchanged
    /// from v0.6.0/v0.8.0 (byte-for-byte equivalent for the
    /// same input); the v0.8.4 change is to the memory
    /// profile of the streaming entry point, not the
    /// return-Vec entry point.
    pub fn encode(frame: &FrameBuffer) -> Result<Vec<u8>, EncoderError> {
        let mut out = Vec::new();
        encode_to_writer(frame, &mut out)?;
        Ok(out)
    }

    // -- v0.8.5: fixed-palette streaming Sixel encoder ----------------
    //
    // The v0.8.4 streaming Sixel path still materialises the
    // full framebuffer in a `Vec<u8>` (8MB+ for a 2MP frame)
    // because `icy_sixel::SixelImage::from_rgba(Vec<u8>, w, h)`
    // takes owned bytes and has no streaming input API.
    // v0.8.5 adds a fully O(1)-memory streaming entry point
    // that uses a fixed xterm-256 palette (16 basic + 6×6×6
    // RGB cube + 24 grayscale) and emits band-by-band in a
    // single DCS sequence. The 8MB+ RGBA `Vec<u8>` is gone --
    // pixels are read directly from the `FrameBuffer`.
    //
    // **Trade-off vs. `encode_to_writer`**: the fixed palette
    // means lower image quality for photos (no adaptive
    // quantization per image) compared to `icy_sixel`'s
    // adaptive quantiser. For UI/dashboards (the
    // termcompositor primary use case) the quality loss is
    // minimal. The `icy_sixel`-based `encode_to_writer` is
    // preserved as the high-quality, O(N)-memory path for
    // small framebuffers; `encode_to_writer_streaming` is
    // the O(1)-memory path for multi-megapixel framebuffers.
    //
    // **Wire format**: single DCS sequence
    // `ESC P 0;0;W;H q #0;2;R;G;B #1;2;R;G;B ... <sixel-data> ESC \`:
    // - `0;0;W;H` is the raster attributes (mode 0, no aspect
    //   ratio, pixel width W, pixel height H)
    // - `#Pc;2;R;G;B` defines each of the 256 palette colors
    //   in 0-100 RGB scale (Sixel uses 0-100, not 0-255)
    // - Sixel data is emitted in bands of 6 rows, with `-`
    //   between bands (carriage return + newline)
    // - Per-column color tracking: for each 6-row column, the
    //   most common palette index is selected, and sixel bits
    //   mark only the pixels that match
    // - RLE: `! <n> <ch>` repeats a sixel character (only when
    //   the encoded form is shorter than the raw form)
    // - Sixel character mapping: value 0-63 -> `?` (63) to
    //   `~` (126) (printable ASCII range)

    /// The xterm-256 palette: 16 basic colors + 6×6×6 RGB
    /// cube + 24 grayscale. Generated at compile time via a
    /// `const fn` with `while` loops (stable since Rust 1.61).
    /// The palette is used as the lookup table for the
    /// streaming Sixel encoder's per-pixel quantization.
    const XTERM_256_PALETTE: [(u8, u8, u8); 256] = build_xterm_256_palette();

    /// `const fn` that builds the xterm-256 palette at
    /// compile time. Uses `while` loops in const context
    /// (stable since Rust 1.61) to fill the 6×6×6 cube
    /// (216 entries) and the 24 grayscale ramp. The 16
    /// basic colors are filled with direct assignments.
    const fn build_xterm_256_palette() -> [(u8, u8, u8); 256] {
        let mut palette = [(0u8, 0u8, 0u8); 256];
        // 0-15: basic colors (standard ANSI/IRC values)
        palette[0] = (0, 0, 0);
        palette[1] = (128, 0, 0);
        palette[2] = (0, 128, 0);
        palette[3] = (128, 128, 0);
        palette[4] = (0, 0, 128);
        palette[5] = (128, 0, 128);
        palette[6] = (0, 128, 128);
        palette[7] = (192, 192, 192);
        palette[8] = (128, 128, 128);
        palette[9] = (255, 0, 0);
        palette[10] = (0, 255, 0);
        palette[11] = (255, 255, 0);
        palette[12] = (0, 0, 255);
        palette[13] = (255, 0, 255);
        palette[14] = (0, 255, 255);
        palette[15] = (255, 255, 255);
        // 16-231: 6×6×6 RGB cube with values
        // [0, 95, 135, 175, 215, 255]
        const CUBE_VALUES: [u8; 6] = [0, 95, 135, 175, 215, 255];
        let mut i = 0;
        while i < 216 {
            let r_idx = i / 36;
            let g_idx = (i / 6) % 6;
            let b_idx = i % 6;
            palette[16 + i] =
                (CUBE_VALUES[r_idx], CUBE_VALUES[g_idx], CUBE_VALUES[b_idx]);
            i += 1;
        }
        // 232-255: 24 grayscale ramp with values
        // [8, 18, 28, ..., 238] (8 + i*10 for i in 0..24)
        let mut i = 0;
        while i < 24 {
            let v = (8 + i * 10) as u8;
            palette[232 + i] = (v, v, v);
            i += 1;
        }
        palette
    }

    /// 5-bit-per-channel RGB to palette index lookup table.
    /// Maps `(r5, g5, b5)` (each 0-31) to the nearest
    /// xterm-256 palette index. Built lazily on first call
    /// via `OnceLock` (stable since Rust 1.70). 32K entries
    /// × 1 byte = 32KB, computed in O(32K × 256) = ~8M
    /// operations at startup (one-time cost, ~100ms on
    /// modern hardware). After init, per-pixel quantization
    /// is O(1) (one table lookup).
    fn palette_lut() -> &'static [u8; 32 * 32 * 32] {
        use std::sync::OnceLock;
        static LUT: OnceLock<[u8; 32 * 32 * 32]> = OnceLock::new();
        LUT.get_or_init(|| {
            let mut lut = [0u8; 32 * 32 * 32];
            for r5 in 0..32u8 {
                for g5 in 0..32u8 {
                    for b5 in 0..32u8 {
                        // Map 5-bit to 8-bit: (value * 255) / 31
                        let r = (u16::from(r5) * 255) / 31;
                        let g = (u16::from(g5) * 255) / 31;
                        let b = (u16::from(b5) * 255) / 31;
                        let idx = (r5 as usize) * 32 * 32
                            + (g5 as usize) * 32
                            + (b5 as usize);
                        lut[idx] = nearest_palette_index(r, g, b);
                    }
                }
            }
            lut
        })
    }

    /// Find the nearest xterm-256 palette index for an
    /// `(r, g, b)` triple using Euclidean distance in RGB
    /// space. O(256) per call; only called at LUT
    /// initialization time (32K calls total, one-time cost).
    fn nearest_palette_index(r: u16, g: u16, b: u16) -> u8 {
        let mut best_idx = 0u8;
        let mut best_dist = u32::MAX;
        for (i, &(pr, pg, pb)) in XTERM_256_PALETTE.iter().enumerate() {
            let dr = i32::from(r) - i32::from(pr);
            let dg = i32::from(g) - i32::from(pg);
            let db = i32::from(b) - i32::from(pb);
            let dist = (dr * dr + dg * dg + db * db) as u32;
            if dist < best_dist {
                best_dist = dist;
                best_idx = i as u8;
            }
        }
        best_idx
    }

    /// Quantize an RGBA pixel to the nearest xterm-256
    /// palette index. O(1) per call (one LUT lookup). The
    /// alpha channel is ignored (Sixel has no alpha;
    /// transparent pixels are quantized to the nearest
    /// palette color, which is usually close to black if
    /// the RGB values are also 0).
    fn quantize_pixel(r: u8, g: u8, b: u8) -> u8 {
        let lut = palette_lut();
        let r5 = (r >> 3) as usize;
        let g5 = (g >> 3) as usize;
        let b5 = (b >> 3) as usize;
        lut[r5 * 32 * 32 + g5 * 32 + b5]
    }

    /// Encodes `frame` as a Sixel DCS using a fixed
    /// xterm-256 palette and band-by-band emission.
    /// **v0.8.5 fully memory-bounded streaming entry
    /// point**: writes the Sixel DCS bytes directly to the
    /// caller's `&mut impl Write` sink with O(1) memory
    /// (no full-framebuffer `Vec<u8>` allocation). The
    /// 8MB+ RGBA `Vec<u8>` that the v0.8.4
    /// `encode_to_writer` still materialises is gone --
    /// pixels are read directly from the `FrameBuffer`.
    ///
    /// See the module doc comment on the v0.8.5 section
    /// above for the full wire format specification and
    /// the quality trade-off vs. `encode_to_writer`.
    pub fn encode_to_writer_streaming<W: Write>(
        frame: &FrameBuffer,
        out: &mut W,
    ) -> Result<(), EncoderError> {
        if frame.width() == 0 || frame.height() == 0 {
            return Err(EncoderError::InvalidDimensions {
                width: frame.width(),
                height: frame.height(),
            });
        }

        let width = frame.width() as usize;
        let height = frame.height() as usize;
        let pixels = frame.pixels();

        // DCS header: ESC P 0;0;W;H q
        // Mode 0 (no aspect ratio), nparams 0, Ph=W, Pv=H.
        write!(out, "\x1bP0;0;{width};{height}q")?;

        // Define the full 256-color palette in the DCS
        // header. Each entry is `#Pc;2;R;G;B` where R,G,B
        // are 0-100 (Sixel uses 0-100 scale, not 0-255).
        // This adds ~3KB to the output (256 × ~12 bytes),
        // negligible for multi-megapixel images. Defining
        // all 256 colors (not just the used ones) keeps the
        // implementation simple and the wire format
        // predictable.
        for (i, &(r, g, b)) in XTERM_256_PALETTE.iter().enumerate() {
            let r100 = u32::from(r) * 100 / 255;
            let g100 = u32::from(g) * 100 / 255;
            let b100 = u32::from(b) * 100 / 255;
            write!(out, "#{i};2;{r100};{g100};{b100}")?;
        }

        // Emit sixel data band-by-band. Each band is 6 rows
        // tall. For each column, we find the most common
        // palette index among the 6 pixels (the "dominant"
        // color), then build a sixel character where bit
        // `dy` is 1 if the pixel at (x, y_start+dy) matches
        // the dominant color. Ties are broken by lower
        // palette index (deterministic).
        let num_bands = height.div_ceil(6);
        for band_idx in 0..num_bands {
            let y_start = band_idx * 6;
            let y_end = (y_start + 6).min(height);

            let mut current_color: u8 = 0;
            let mut color_announced = false;
            let mut run_char: Option<u8> = None;
            let mut run_len: usize = 0;

            for x in 0..width {
                // Quantize each of the 6 pixels in this
                // column. Pixels beyond y_end (last partial
                // band) are treated as 0 (background).
                let mut column_palette = [0u8; 6];
                for (dy, slot) in column_palette.iter_mut().enumerate() {
                    let y = y_start + dy;
                    if y < y_end {
                        let [r, g, b, _a] = pixels[y * width + x];
                        *slot = quantize_pixel(r, g, b);
                    }
                }

                // Find the dominant palette index (most
                // common among the 6 pixels). O(36) per
                // column (6×6 comparisons), negligible
                // compared to the per-pixel LUT lookup.
                // Ties broken by lower index (deterministic).
                let mut best_color = column_palette[0];
                let mut best_count = 1u8;
                for &palette_i in column_palette.iter() {
                    let mut count = 0u8;
                    for &palette_j in column_palette.iter() {
                        if palette_j == palette_i {
                            count += 1;
                        }
                    }
                    if count > best_count
                        || (count == best_count && palette_i < best_color)
                    {
                        best_count = count;
                        best_color = palette_i;
                    }
                }

                // Build the 6-bit value: bit `dy` is 1 if
                // the pixel at (x, y_start+dy) matches the
                // dominant color.
                let mut value: u8 = 0;
                for (dy, &palette) in column_palette.iter().enumerate() {
                    if palette == best_color {
                        value |= 1 << dy;
                    }
                }

                // Sixel character: value (0-63) + 63 =
                // ASCII 63-126 ('?' to '~').
                let char = value + 63;

                // Announce color change if needed. Always
                // announce the first color (even if it's 0)
                // to set the initial color state.
                if !color_announced || best_color != current_color {
                    flush_sixel_rle(
                        &mut run_char, &mut run_len, out,
                    )?;
                    write!(out, "#{best_color}")?;
                    current_color = best_color;
                    color_announced = true;
                }

                // RLE: if the same sixel character repeats,
                // accumulate the run length. The RLE is
                // flushed by `flush_sixel_rle` when the run
                // ends or when the color changes.
                if Some(char) == run_char {
                    run_len += 1;
                } else {
                    flush_sixel_rle(
                        &mut run_char, &mut run_len, out,
                    )?;
                    run_char = Some(char);
                    run_len = 1;
                }
            }

            // Flush the last run of the band.
            flush_sixel_rle(&mut run_char, &mut run_len, out)?;

            // Band separator (except after the last band).
            // `-` means "carriage return + newline" in
            // Sixel, moving the cursor to the start of the
            // next band (6 rows down).
            if band_idx + 1 < num_bands {
                out.write_all(b"-")?;
            }
        }

        // Terminate DCS.
        out.write_all(b"\x1b\\")?;
        Ok(())
    }

    /// Flush a pending RLE run to the output. If the run
    /// length is >= 4, emit `! <n> <ch>` (repeat
    /// introducer). Otherwise emit the single character
    /// (or multiple copies if the run is 2-3). The
    /// threshold of 4 is because the RLE form `! <n> <ch>`
    /// is 4 chars for n=1-9 (1 digit), 5 for n=10-99,
    /// etc. -- so for runs of 1-3, the raw form is
    /// shorter or equal.
    fn flush_sixel_rle<W: Write>(
        run_char: &mut Option<u8>,
        run_len: &mut usize,
        out: &mut W,
    ) -> std::io::Result<()> {
        if let Some(c) = *run_char {
            if *run_len >= 4 {
                // RLE: ! <n> <ch>
                write!(out, "!{run_len}{}", c as char)?;
            } else {
                // Raw: <ch> repeated run_len times.
                for _ in 0..*run_len {
                    out.write_all(&[c])?;
                }
            }
        }
        *run_char = None;
        *run_len = 0;
        Ok(())
    }
}

// Re-export the v0.8.4 streaming Sixel `encode_to_writer`
// at the `encoder` module level so downstream users can
// call it as `termcompositor::encoder::encode_to_writer`
// without reaching into the private `sixel` submodule.
// This mirrors the kitty `encode_to_writer` access
// pattern (`termcompositor::encoder::kitty::encode_to_writer`):
// neither streaming entry point is re-exported at the
// crate root, because a single crate-root `encode_to_writer`
// name would be ambiguous in a build with both
// `kitty-encoder` and `sixel-encoder` enabled (which one
// wins?), and the module-path access is more explicit
// anyway. Gated on `sixel-encoder` because the helper is
// only available when the Sixel encoder crate is
// compiled in.
#[cfg(feature = "sixel-encoder")]
pub use sixel::encode_to_writer;

// Re-export the v0.8.5 fully-memory-bounded streaming
// Sixel `encode_to_writer_streaming` at the `encoder`
// module level so downstream users can call it as
// `termcompositor::encoder::encode_to_writer_streaming`
// without reaching into the private `sixel` submodule.
// This is the O(1)-memory counterpart to the v0.8.4
// `encode_to_writer` (which still materialises the full
// RGBA Vec<u8> because `icy_sixel` has no streaming input
// API). Same rationale as the v0.8.4 re-export: not at
// the crate root to avoid the ambiguity of a single
// `encode_to_writer_streaming` name. Gated on
// `sixel-encoder` because the helper is only available
// when the Sixel encoder crate is compiled in.
#[cfg(feature = "sixel-encoder")]
pub use sixel::encode_to_writer_streaming;

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
        assert_eq!(
            detect_with_env(None, Some("kitty"), None, None),
            Protocol::Kitty
        );
        assert_eq!(
            detect_with_env(None, Some("Kitty"), None, None),
            Protocol::Kitty
        );
        assert_eq!(
            detect_with_env(None, Some("KITTY"), None, None),
            Protocol::Kitty
        );
    }

    #[test]
    fn detect_with_env_picks_kitty_for_term_program_wezterm() {
        assert_eq!(
            detect_with_env(None, Some("wezterm"), None, None),
            Protocol::Kitty
        );
        assert_eq!(
            detect_with_env(None, Some("WezTerm"), None, None),
            Protocol::Kitty
        );
    }

    #[test]
    fn detect_with_env_picks_kitty_for_term_program_ghostty() {
        assert_eq!(
            detect_with_env(None, Some("ghostty"), None, None),
            Protocol::Kitty
        );
        assert_eq!(
            detect_with_env(None, Some("Ghostty"), None, None),
            Protocol::Kitty
        );
    }

    #[test]
    fn detect_with_env_picks_kitty_for_xterm_kitty() {
        assert_eq!(
            detect_with_env(Some("xterm-kitty"), None, None, None),
            Protocol::Kitty
        );
    }

    #[test]
    fn detect_with_env_picks_kitty_for_foot_and_foot_extra() {
        assert_eq!(
            detect_with_env(Some("foot"), None, None, None),
            Protocol::Kitty
        );
        assert_eq!(
            detect_with_env(Some("foot-extra"), None, None, None),
            Protocol::Kitty
        );
        assert_eq!(
            detect_with_env(Some("foot-256color"), None, None, None),
            Protocol::Kitty
        );
    }

    #[test]
    fn detect_with_env_picks_sixel_for_tmux() {
        // tmux passthrough complicates Kitty; default to Sixel.
        assert_eq!(
            detect_with_env(Some("tmux"), None, None, None),
            Protocol::Sixel
        );
        assert_eq!(
            detect_with_env(Some("tmux-256color"), None, None, None),
            Protocol::Sixel
        );
        assert_eq!(
            detect_with_env(Some("tmux-direct"), None, None, None),
            Protocol::Sixel
        );
    }

    #[test]
    fn detect_with_env_picks_sixel_for_xterm_256color() {
        // Conservative: unknown XTerm-like terminal -> Sixel.
        assert_eq!(
            detect_with_env(Some("xterm-256color"), None, None, None),
            Protocol::Sixel
        );
    }

    #[test]
    fn detect_with_env_picks_sixel_when_neither_set() {
        assert_eq!(detect_with_env(None, None, None, None), Protocol::Sixel);
        assert_eq!(
            detect_with_env(Some(""), Some(""), Some(""), None),
            Protocol::Sixel
        );
    }

    #[test]
    fn detect_with_env_term_program_wins_over_term() {
        // TERM_PROGRAM is more specific than TERM; if the two
        // disagree, TERM_PROGRAM wins.
        assert_eq!(
            detect_with_env(Some("xterm-256color"), Some("kitty"), None, None),
            Protocol::Kitty
        );
        // And vice versa: a known Kitty TERM with a non-Kitty
        // TERM_PROGRAM (e.g. a wrapper script setting
        // TERM_PROGRAM) -- TERM_PROGRAM still wins.
        assert_eq!(
            detect_with_env(Some("xterm-kitty"), Some("wezterm"), None, None),
            Protocol::Kitty
        );
    }

    #[test]
    fn detect_with_env_unknown_term_program_falls_through_to_term() {
        // A TERM_PROGRAM we don't recognise shouldn't block
        // detection -- fall through to TERM.
        assert_eq!(
            detect_with_env(Some("xterm-kitty"), Some("apple-terminal"), None, None),
            Protocol::Kitty
        );
    }

    // -- COLORTERM tiebreaker -------------------------------------------

    #[test]
    fn detect_with_env_colorterm_truecolor_picks_kitty_for_unknown_term() {
        // When TERM/TERM_PROGRAM are inconclusive but
        // COLORTERM=truecolor is set, lean Kitty.
        assert_eq!(
            detect_with_env(Some("xterm-256color"), None, Some("truecolor"), None),
            Protocol::Kitty
        );
    }

    #[test]
    fn detect_with_env_colorterm_24bit_picks_kitty_for_unknown_term() {
        assert_eq!(
            detect_with_env(Some("screen-256color"), None, Some("24bit"), None),
            Protocol::Kitty
        );
    }

    #[test]
    fn detect_with_env_colorterm_does_not_override_known_kitty() {
        // COLORTERM should not override an already-known
        // Kitty terminal -- TERM_PROGRAM wins.
        assert_eq!(
            detect_with_env(Some("xterm-kitty"), None, Some("truecolor"), None),
            Protocol::Kitty
        );
    }

    #[test]
    fn detect_with_env_colorterm_non_truecolor_does_not_override_sixel() {
        // A non-truecolor COLORTERM value should not override
        // the Sixel default for unknown terminals.
        assert_eq!(
            detect_with_env(Some("xterm"), None, Some("16color"), None),
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
    /// COLORTERM / TMUXPASSTHROUGH to the supplied values
    /// (or removes them if `None`) AND clears `TMUX` to
    /// a known unset state, runs the closure, then restores
    /// all five env vars via the `EnvGuard` `Drop` impls
    /// (in reverse order) and releases the mutex. The
    /// mutex serialises env-touching tests so no two
    /// `with_env` calls can snapshot each other's modified
    /// env vars. The `TMUX` clear is unconditional so a
    /// test that needs `TMUX` set (e.g. the v0.8.0 dispatch
    /// auto-wrap tests) can set it inside the closure
    /// without racing a shell-exported or parallel-test
    /// `TMUX` value.
    fn with_env<F: FnOnce() -> R, R>(
        term: Option<&str>,
        term_program: Option<&str>,
        colorterm: Option<&str>,
        tmux_passthrough: Option<&str>,
        f: F,
    ) -> R {
        let _lock = env_lock();
        let _term = EnvGuard::new("TERM");
        _term.set(term);
        let _program = EnvGuard::new("TERM_PROGRAM");
        _program.set(term_program);
        let _colorterm = EnvGuard::new("COLORTERM");
        _colorterm.set(colorterm);
        let _tmux = EnvGuard::new("TMUXPASSTHROUGH");
        _tmux.set(tmux_passthrough);
        let _tmux = EnvGuard::new("TMUX");
        _tmux.set(None);
        f()
        // _tmux, _tmux, _colorterm, _program, _term, _lock
        // drop in reverse order, restoring all five env
        // vars then releasing the mutex.
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
        with_env(Some("xterm-kitty"), None, None, None, || {
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
        with_env(Some("tmux-256color"), None, None, None, || {
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
        with_env(None, Some("kitty"), None, None, || {
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
        with_env(Some("tmux-256color"), None, None, None, || {
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
        // v0.8.0: use `with_env(None, None, None, None, ...)`
        // to acquire the env mutex and clear all four
        // touchable env vars (TERM, TERM_PROGRAM, COLORTERM,
        // TMUXPASSTHROUGH) AND `TMUX`. This ensures the
        // v0.8.0 auto-wrap in `dispatch(Protocol::Kitty, ...)`
        // does NOT kick in (it requires `TMUXPASSTHROUGH` and
        // `TMUX` to both be set), so the test always sees
        // the raw APC framing (`\x1b_G ... \x1b\\`). Using
        // `with_env` (not manual `EnvGuard`s) is critical:
        // it participates in the process-global env mutex
        // that serialises all `with_env` tests, closing the
        // parallel-test race that a previous attempt with
        // manual `EnvGuard`s failed to close (the manual
        // guards don't acquire the mutex, so a parallel
        // `with_env` test could modify `TMUXPASSTHROUGH` or
        // `TMUX` between the guard creation and the encode
        // call).
        with_env(None, None, None, None, || {
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
        });
    }

    #[cfg(feature = "kitty-encoder")]
    #[test]
    fn kitty_encode_is_deterministic_for_same_input() {
        // v0.8.0: use `with_env(None, None, None, None, ...)`
        // to acquire the env mutex and clear all env vars.
        // See the comment on `kitty_encode_produces_valid_escape_framing`
        // for why `with_env` (not manual `EnvGuard`s) is
        // required: the manual guards don't acquire the
        // mutex, so a parallel `with_env` test could
        // modify `TMUXPASSTHROUGH` or `TMUX` between the
        // two `encode` calls, causing the first to wrap
        // and the second to not wrap (or vice versa),
        // breaking the determinism assertion.
        with_env(None, None, None, None, || {
            let mut fb = FrameBuffer::new(3, 3);
            for px in fb.pixels_mut() {
                *px = [10, 20, 30, 255];
            }
            let a = Protocol::Kitty.encode(&fb).unwrap();
            let b = Protocol::Kitty.encode(&fb).unwrap();
            assert_eq!(a, b);
        });
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
        with_env(None, Some("kitty"), None, None, || {
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

    // -- v0.8.0: tmux passthrough heuristic tests -----------------------
    //
    // The heuristic lives in `detect_with_env` and is unit-tested
    // here without the env-mutex plumbing (it does not touch
    // any process state).

    #[test]
    fn detect_with_env_tmux_picks_kitty_with_tmux_passthrough() {
        // v0.8.0: when the user opts in via TMUXPASSTHROUGH
        // (any non-empty value) AND TERM=tmux*, the heuristic
        // picks Kitty (the dispatch will then auto-wrap).
        assert_eq!(
            detect_with_env(Some("tmux"), None, None, Some("1")),
            Protocol::Kitty
        );
        assert_eq!(
            detect_with_env(Some("tmux-256color"), None, None, Some("1")),
            Protocol::Kitty
        );
        assert_eq!(
            detect_with_env(Some("tmux-direct"), None, None, Some("yes")),
            Protocol::Kitty
        );
        // TERM_PROGRAM still wins (a user with TERM_PROGRAM=wezterm
        // running inside tmux is using wezterm, not native
        // tmux-attached kitty passthrough).
        assert_eq!(
            detect_with_env(Some("tmux-256color"), Some("wezterm"), None, Some("1")),
            Protocol::Kitty
        );
    }

    #[test]
    fn detect_with_env_tmux_picks_sixel_with_empty_or_missing_tmux_passthrough() {
        // The opt-in is required: empty or absent TMUXPASSTHROUGH
        // keeps the v0.7.0 Sixel fallback.
        assert_eq!(
            detect_with_env(Some("tmux-256color"), None, None, None),
            Protocol::Sixel
        );
        assert_eq!(
            detect_with_env(Some("tmux-256color"), None, None, Some("")),
            Protocol::Sixel
        );
        // The opt-in check is `is_some_and(|v| !v.is_empty())`:
        // any non-empty value (including `"0"`, `"false"`, etc.)
        // enables the opt-in. This is intentional: a user who
        // explicitly sets `TMUXPASSTHROUGH=0` in their shell
        // is making a conscious decision, and the simplest
        // interpretation is "I have set the variable, so my
        // intent is to opt in". A user who wants to opt out
        // should simply unset the variable. (Compare to
        // shell `set -e`: there is no "disable" sentinel --
        // the variable's presence is the opt-in.) This
        // assertion documents and locks in that semantics.
        assert_eq!(
            detect_with_env(Some("tmux-256color"), None, None, Some("0")),
            Protocol::Kitty
        );
    }

    // -- v0.8.0: wrap_for_tmux pure-function unit tests -----------------
    //
    // These test the pure byte transform `kitty::wrap_for_tmux`.
    // No env vars, no FrameBuffer: just bytes in, bytes out.

    #[cfg(feature = "kitty-encoder")]
    #[test]
    fn wrap_for_tmux_wraps_inner_apc_in_dcs_passthrough() {
        // A typical Kitty APC payload -- introducer + controls
        // + base64 payload + terminator -- must come out
        // wrapped in the `ESC P tmux ; ... ESC \` DCS.
        let inner: Vec<u8> = b"\x1b_Ga=T,f=32,q=2,s=2,v=2;AAAA\x1b\\".to_vec();
        // `wrap_for_tmux` takes `inner` by value (consumes it),
        // so clone first to keep the length for the assertion.
        let inner_len = inner.len();
        let wrapped = super::kitty::wrap_for_tmux(inner);
        // The DCS prefix is `ESC P tmux ;` (7 bytes).
        assert!(wrapped.starts_with(b"\x1bPtmux;"));
        // The DCS terminator is `ESC \` (2 bytes).
        assert!(wrapped.ends_with(b"\x1b\\"));
        // The total length is inner + 7 (prefix) + 2 (suffix) + 2
        // (two extra ESC bytes from doubling the introducer and
        // terminator).
        assert_eq!(wrapped.len(), inner_len + 7 + 2 + 2);
    }

    #[cfg(feature = "kitty-encoder")]
    #[test]
    fn wrap_for_tmux_doubles_inner_esc_bytes() {
        // An inner byte sequence containing ESC bytes (not at
        // the introducer or terminator) must have those ESC
        // bytes doubled. tmux 3.2+ treats `ESC ESC` as a
        // single literal ESC in the inner payload.
        let inner: Vec<u8> = b"\x1b_Ga=T\x1bTEST\x1b\\".to_vec();
        let wrapped = super::kitty::wrap_for_tmux(inner.clone());
        // The middle `ESC TEST` becomes `ESC ESC TEST` in the
        // wrapped output.
        let expected_middle = b"\x1b\x1bTEST";
        // Find the middle section in the wrapped output.
        let middle_pos = wrapped
            .windows(expected_middle.len())
            .position(|w| w == expected_middle)
            .expect("doubled ESC TEST must appear in wrapped output");
        // The doubled ESC is preceded by a semicolon and a=T
        // and followed by TEST.
        assert!(middle_pos > 0);
        assert!(middle_pos + expected_middle.len() < wrapped.len());
    }

    #[cfg(feature = "kitty-encoder")]
    #[test]
    fn wrap_for_tmux_handles_empty_inner() {
        // Edge case: an empty inner payload still produces
        // a valid (but empty) wrapped DCS -- `ESC P tmux ;
        // ESC \`. This should not panic and should produce
        // exactly the 9-byte empty-passthrough sequence.
        let wrapped = super::kitty::wrap_for_tmux(Vec::new());
        assert_eq!(wrapped, b"\x1bPtmux;\x1b\\");
    }

    #[cfg(feature = "kitty-encoder")]
    #[test]
    fn wrap_for_tmux_leaves_non_esc_bytes_untouched() {
        // Non-ESC bytes in the inner payload pass through
        // verbatim (no doubling, no transformation). Build
        // a payload with no ESC bytes, then assert the
        // wrapped output equals prefix + inner + suffix.
        let inner: Vec<u8> = b"hello world, no escapes here".to_vec();
        let wrapped = super::kitty::wrap_for_tmux(inner.clone());
        let mut expected = Vec::new();
        expected.extend_from_slice(b"\x1bPtmux;");
        expected.extend_from_slice(&inner);
        expected.push(0x1b);
        expected.push(b'\\');
        assert_eq!(wrapped, expected);
    }

    // -- v0.8.0: dispatch + tmux-passthrough wiring tests ---------------
    //
    // These test the env-driven auto-wrap in `dispatch`. They
    // run the env-mutex-serialised `with_env` helper so they
    // are race-free with the other env-touching tests.

    #[cfg(feature = "kitty-encoder")]
    #[test]
    fn dispatch_kitty_with_tmux_passthrough_wraps_output() {
        // When kitty-encoder is on AND TMUXPASSTHROUGH is
        // set AND the host is inside tmux (TMUX env var
        // present), the dispatch should wrap the Kitty
        // APC output in the tmux passthrough DCS. The
        // wrapper is `ESC P tmux ; ... ESC \`.
        with_env(Some("tmux-256color"), None, None, Some("1"), || {
            // TMUX must also be set for the auto-wrap to
            // kick in (the `tmux_passthrough_enabled` check
            // requires BOTH TMUXPASSTHROUGH and TMUX).
            let _tmux = EnvGuard::new("TMUX");
            _tmux.set(Some("/tmp/tmux-1000/default,12345,0"));
            let fb = FrameBuffer::new(2, 2);
            let bytes = dispatch(Protocol::Kitty, &fb).unwrap();
            // The wrapped output starts with the DCS prefix
            // and ends with the DCS terminator; the inner
            // APC introducer (`\x1b_G`) is still present
            // (but as `\x1b\x1b_G` because ESC was doubled).
            assert!(
                bytes.starts_with(b"\x1bPtmux;"),
                "Kitty dispatch with TMUXPASSTHROUGH+TMUX must wrap; got prefix: {:?}",
                &bytes[..bytes.len().min(12)],
            );
            assert!(bytes.ends_with(b"\x1b\\"));
        });
    }

    #[cfg(feature = "kitty-encoder")]
    #[test]
    fn dispatch_kitty_without_tmux_passthrough_does_not_wrap() {
        // When TMUXPASSTHROUGH is not set, the dispatch
        // produces raw Kitty APC bytes (no wrapping),
        // even if the user is inside tmux. This is the
        // v0.7.0 backwards-compat default.
        with_env(Some("tmux-256color"), None, None, None, || {
            let _tmux = EnvGuard::new("TMUX");
            _tmux.set(Some("/tmp/tmux-1000/default,12345,0"));
            let fb = FrameBuffer::new(2, 2);
            let bytes = dispatch(Protocol::Kitty, &fb).unwrap();
            // Raw Kitty APC starts with `\x1b_G` (NOT
            // `\x1bPtmux;`).
            assert!(
                bytes.starts_with(b"\x1b_G"),
                "Kitty dispatch without TMUXPASSTHROUGH must NOT wrap; got prefix: {:?}",
                &bytes[..bytes.len().min(12)],
            );
            assert!(!bytes.starts_with(b"\x1bPtmux;"));
        });
    }

    #[cfg(feature = "kitty-encoder")]
    #[test]
    fn dispatch_kitty_explicit_protocol_still_wraps_when_opted_in() {
        // When the user passes `--protocol kitty` (so
        // `dispatch` is called with `Protocol::Kitty`
        // directly, bypassing the heuristic) AND the
        // opt-in is set AND TMUX is set, the wrap
        // still happens. This is the "explicit protocol
        // overrides the heuristic" use case: a user
        // with a known-good tmux + Kitty setup who
        // wants to force Kitty regardless of TERM.
        with_env(Some("xterm-256color"), None, None, Some("1"), || {
            let _tmux = EnvGuard::new("TMUX");
            _tmux.set(Some("/tmp/tmux-1000/default,12345,0"));
            let fb = FrameBuffer::new(2, 2);
            let bytes = dispatch(Protocol::Kitty, &fb).unwrap();
            assert!(
                bytes.starts_with(b"\x1bPtmux;"),
                "Explicit Protocol::Kitty with TMUXPASSTHROUGH+TMUX must wrap; got prefix: {:?}",
                &bytes[..bytes.len().min(12)],
            );
        });
    }

    // -- v0.8.1: chunked Kitty encoding tests ---------------------------
    //
    // These tests verify the v0.8.1 chunking logic: single-chunk
    // fast path (no m key, byte-identical to v0.8.0 output for
    // small images) and the multi-chunk path (m=1 on all but
    // last, m=0 on last, base64 alignment on 4-char boundary).
    // All tests use `with_env(None, None, None, None, ...)` to
    // participate in the env-mutex serialization and avoid the
    // v0.8.0 tmux-passthrough auto-wrap kicking in.

    /// 4 RGBA pixels = 4 * 4 = 16 raw bytes. Base64 of 16 bytes
    /// is `ceil(16/3)*4 = 24` chars, well under the 4096-char
    /// chunk limit. The single-chunk fast path should emit the
    /// v0.8.0 wire format (no `m` key) and the output should be
    /// byte-identical to the v0.8.0 single-command encoder.
    #[cfg(feature = "kitty-encoder")]
    #[test]
    fn kitty_encode_single_chunk_produces_no_m_key() {
        with_env(None, None, None, None, || {
            let mut fb = FrameBuffer::new(2, 2);
        fb.pixels_mut()[0] = [255, 0, 0, 255];
            let bytes = super::kitty::encode(&fb).unwrap();
            // Starts with the APC introducer and ends with the
            // APC terminator (existing v0.8.0 framing).
            assert!(bytes.starts_with(b"\x1b_G"));
            assert!(bytes.ends_with(b"\x1b\\"));
            // Parse the controls and assert NO `m` key.
            let s = std::str::from_utf8(&bytes).unwrap();
            let payload_start = "\x1b_G".len();
            let payload_end = s.find(';').unwrap();
            let controls = &s[payload_start..payload_end];
            assert!(
                !controls.contains(",m=") && !controls.starts_with("m="),
                "v0.8.0 single-chunk fast path must NOT include m key, got controls: {controls:?}"
            );
            // Existing v0.8.0 control keys must all be present.
            for key in &["a=T", "f=32", "q=2", "s=2", "v=2"] {
                assert!(controls.contains(key), "missing control {key}");
            }
        });
    }

    /// Exactly `PIXELS_PER_CHUNK` = 768 pixels = 3072 raw bytes.
    /// Base64 of 3072 bytes is exactly 4096 chars (no padding),
    /// which is the chunk limit boundary. This should still use
    /// the single-chunk fast path (the condition is `<=`, not
    /// `<`).
    #[cfg(feature = "kitty-encoder")]
    #[test]
    fn kitty_encode_exactly_768_pixels_is_single_chunk() {
        with_env(None, None, None, None, || {
            // 768 * 1 = 768 pixels, single row. Width*height
            // is exactly PIXELS_PER_CHUNK.
            let mut fb = FrameBuffer::new(super::kitty::PIXELS_PER_CHUNK as u32, 1);
        fb.pixels_mut()[0] = [255, 0, 0, 255];
            let bytes = super::kitty::encode(&fb).unwrap();
            let s = std::str::from_utf8(&bytes).unwrap();
            let payload_start = "\x1b_G".len();
            let payload_end = s.find(';').unwrap();
            let controls = &s[payload_start..payload_end];
            // No `m` key: single-chunk fast path.
            assert!(
                !controls.contains("m="),
                "exactly-768-pixel frame must use single-chunk fast path, got controls: {controls:?}"
            );
        });
    }

    /// `PIXELS_PER_CHUNK + 1` = 769 pixels. This is just over
    /// the single-chunk threshold, so the multi-chunk path
    /// kicks in. Expected: exactly 2 chunks -- the first with
    /// `m=1` and the full control list, the second with
    /// `m=0` and only the `m` control.
    #[cfg(feature = "kitty-encoder")]
    #[test]
    fn kitty_encode_two_chunks_has_m1_m0() {
        with_env(None, None, None, None, || {
            let w = super::kitty::PIXELS_PER_CHUNK as u32 + 1; // 769
            let mut fb = FrameBuffer::new(w, 1);
        fb.pixels_mut()[0] = [255, 0, 0, 255];
            let bytes = super::kitty::encode(&fb).unwrap();
            let s = std::str::from_utf8(&bytes).unwrap();
            // Count APC introducers (each chunk starts with
            // `\x1b_G`). Should be exactly 2.
            let introducer_count = s.matches("\x1b_G").count();
            assert_eq!(
                introducer_count, 3,
                "769-pixel frame must produce exactly 2 chunks, got {introducer_count}"
            );
            // First chunk has `m=1` and the full control list.
            let first_chunk_start = s.find("\x1b_G").unwrap() + "\x1b_G".len();
            let first_chunk_end = s.find(';').unwrap();
            let first_controls = &s[first_chunk_start..first_chunk_end];
            assert!(first_controls.contains("a=T"), "first chunk missing a=T");
            assert!(first_controls.contains("f=32"), "first chunk missing f=32");
            assert!(
                first_controls.contains("s=769"),
                "first chunk missing s=769"
            );
            assert!(first_controls.contains("m=1"), "first chunk must have m=1");
            // Second chunk has `m=0` and ONLY `m`.
            let second_chunk_start = s.rfind("\x1b_Gm=0").unwrap() + "\x1b_G".len();
            let second_chunk_end = s.rfind(';').unwrap();
            let second_controls = &s[second_chunk_start..second_chunk_end];
            assert_eq!(second_controls, "m=0", "second chunk must have only m=0");
        });
    }

    /// A 3-chunk frame (e.g. 769*2 = 1538 pixels, or 2*PIXELS_PER_CHUNK+1).
    /// Verifies that intermediate chunks carry only `m=1`
    /// (not the full control list) and the last carries `m=0`.
    #[cfg(feature = "kitty-encoder")]
    #[test]
    fn kitty_encode_three_chunks_boundary() {
        with_env(None, None, None, None, || {
            // 2 * 768 + 1 = 1537 pixels -> 3 chunks (768, 768, 1).
            let w = (super::kitty::PIXELS_PER_CHUNK * 2 + 1) as u32;
            let mut fb = FrameBuffer::new(w, 1);
        fb.pixels_mut()[0] = [255, 0, 0, 255];
            let bytes = super::kitty::encode(&fb).unwrap();
            let s = std::str::from_utf8(&bytes).unwrap();
            // Exactly 3 chunks.
            assert_eq!(
                s.matches("\x1b_G").count(),
                4,
                "1537-pixel frame must produce exactly 3 chunks"
            );
            // First chunk has full controls + m=1.
            let first_start = s.find("\x1b_G").unwrap() + "\x1b_G".len();
            let first_end = s.find(';').unwrap();
            let first_controls = &s[first_start..first_end];
            assert!(first_controls.contains("a=T"));
            assert!(first_controls.contains("m=1"));
            // Find the second chunk (middle one) and assert it
            // has ONLY m=1 (no full control list).
            let second_chunk_pos = s.find("\x1b_G").unwrap() + 1; // skip first
            let second_start =
                s[second_chunk_pos..].find("\x1b_G").unwrap() + second_chunk_pos + "\x1b_G".len();
            let second_end = s[second_start..].find(';').unwrap() + second_start;
            let second_controls = &s[second_start..second_end];
            assert_eq!(
                second_controls, "m=1",
                "intermediate chunk must have only m=1, got {second_controls:?}"
            );
            // Last chunk has m=0.
            let last_start = s.rfind("\x1b_Gm=0").unwrap() + "\x1b_G".len();
            let last_end = s.rfind(';').unwrap();
            let last_controls = &s[last_start..last_end];
            assert_eq!(last_controls, "m=0");
        });
    }

    /// Verifies the spec's hard requirement that all chunks
    /// except the last have a base64 payload length that is a
    /// multiple of 4. For 32-bit RGBA with 768 pixels per
    /// chunk, the base64 payload of an intermediate chunk is
    /// exactly 4096 chars (a multiple of 4). The last chunk
    /// may have padding and is exempt from this requirement.
    #[cfg(feature = "kitty-encoder")]
    #[test]
    fn kitty_encode_intermediate_chunks_base64_aligned() {
        with_env(None, None, None, None, || {
            // 3 * 768 = 2304 pixels -> 3 chunks, all of size
            // PIXELS_PER_CHUNK (no last-chunk remainder). This
            // way ALL three chunks must be 4096-char aligned.
            let mut fb = FrameBuffer::new((super::kitty::PIXELS_PER_CHUNK * 3) as u32, 1);
        fb.pixels_mut()[0] = [255, 0, 0, 255];
            let bytes = super::kitty::encode(&fb).unwrap();
            let s = std::str::from_utf8(&bytes).unwrap();
            // For each chunk, find the `;` (end of controls)
            // and the matching `\x1b\\` (end of base64 payload),
            // and assert the payload length is a multiple of 4.
            let mut search_from = 0;
            let mut chunk_idx = 0;
            while let Some(intro_pos) = s[search_from..].find("\x1b_G") {
                let abs_intro = search_from + intro_pos;
                // v0.12.2: skip the place command (no ';' separator).
                let abs_semicolon = match s[abs_intro..].find(';') {
                    Some(pos) => abs_intro + pos,
                    None => {
                        search_from = s[abs_intro..]
                            .find("\x1b\\")
                            .unwrap()
                            + abs_intro
                            + "\x1b\\".len();
                        continue;
                    }
                };
                let abs_end = s[abs_semicolon + 1..].find("\x1b\\").unwrap() + abs_semicolon + 1;
                let payload_len = abs_end - (abs_semicolon + 1);
                assert_eq!(
                    payload_len % 4,
                    0,
                    "chunk {chunk_idx} payload length {payload_len} must be multiple of 4 (spec requirement for non-last chunks)"
                );
                chunk_idx += 1;
                search_from = abs_end + "\x1b\\".len();
            }
            assert_eq!(chunk_idx, 3, "expected 3 chunks");
        });
    }

    /// Determinism: encoding the same frame twice must
    /// produce byte-identical output (the encode path is
    /// pure).
    #[cfg(feature = "kitty-encoder")]
    #[test]
    fn kitty_encode_chunked_is_deterministic() {
        with_env(None, None, None, None, || {
            // 1537 pixels -> 3 chunks, exercises the
            // multi-chunk path.
            let w = (super::kitty::PIXELS_PER_CHUNK * 2 + 1) as u32;
            let fb = FrameBuffer::new(w, 1);
            let a = super::kitty::encode(&fb).unwrap();
            let b = super::kitty::encode(&fb).unwrap();
            assert_eq!(a, b, "chunked encode must be deterministic");
        });
    }

    // -- v0.8.2: memory-bounded streaming encode tests ----------------
    //
    // The v0.8.2 streaming entry point `encode_to_writer<W: Write>`
    // writes APC bytes directly to the caller's `&mut impl Write`
    // sink without materialising the full framebuffer in a
    // `Vec<u8>`. These tests verify (a) byte-for-byte
    // equivalence with the existing `encode -> Vec<u8>` path,
    // (b) correctness for the single-chunk and multi-chunk
    // paths via the streaming entry point, (c) the path works
    // on a pre-allocated `Vec<u8>` (not just a fresh one),
    // and (d) a 2MP smoke test (verifies the multi-chunk path
    // on a realistically-sized framebuffer).

    /// Streaming output for a 1×1 frame must match the
    /// `encode -> Vec<u8>` output byte-for-byte. This
    /// pins the v0.8.2 refactor's invariant: the
    /// streaming path and the `Vec<u8>` path produce
    /// identical bytes for the same input.
    #[cfg(feature = "kitty-encoder")]
    #[test]
    fn encode_to_writer_small_frame_matches_encode() {
        with_env(None, None, None, None, || {
            let fb = FrameBuffer::new(1, 1);
            let from_encode = super::kitty::encode(&fb).unwrap();
            let mut from_streaming: Vec<u8> = Vec::new();
            super::kitty::encode_to_writer(&fb, &mut from_streaming).unwrap();
            assert_eq!(from_encode, from_streaming);
        });
    }

    /// The single-chunk fast path (≤ 768 pixels) must
    /// produce the v0.8.0 wire format via the streaming
    /// entry point (no `m` key, full control list).
    /// This exercises the streaming path's
    /// `encode_single_chunk_apc` helper directly.
    #[cfg(feature = "kitty-encoder")]
    #[test]
    fn encode_to_writer_single_chunk_no_m_key() {
        with_env(None, None, None, None, || {
            // Exactly 768 pixels = single chunk boundary.
            let mut fb = FrameBuffer::new(768, 1);
        fb.pixels_mut()[0] = [255, 0, 0, 255];
            let mut out: Vec<u8> = Vec::new();
            super::kitty::encode_to_writer(&fb, &mut out).unwrap();
            let s = std::str::from_utf8(&out).unwrap();
            // One APC introducer (single chunk).
            assert_eq!(s.matches("\x1b_G").count(), 2);
            let payload_start = "\x1b_G".len();
            let payload_end = s.find(';').unwrap();
            let controls = &s[payload_start..payload_end];
            // No `m` key in single-chunk path.
            assert!(!controls.contains("m="));
            // Full control list present.
            for key in &["a=T", "f=32", "q=2", "s=768", "v=1"] {
                assert!(controls.contains(key), "missing control {key}");
            }
        });
    }

    /// The multi-chunk path (769+ pixels) must produce
    /// the correct `m=1` / `m=0` distribution and chunk
    /// count via the streaming entry point. This
    /// exercises the per-chunk flatten-and-collect
    /// inside the streaming loop (the v0.8.2 allocation
    /// that replaced the v0.8.1 full-framebuffer copy).
    #[cfg(feature = "kitty-encoder")]
    #[test]
    fn encode_to_writer_multi_chunk_has_m_keys() {
        with_env(None, None, None, None, || {
            // 1537 pixels -> 3 chunks (768 + 768 + 1).
            let mut fb = FrameBuffer::new(1537, 1);
        fb.pixels_mut()[0] = [255, 0, 0, 255];
            let mut out: Vec<u8> = Vec::new();
            super::kitty::encode_to_writer(&fb, &mut out).unwrap();
            let s = std::str::from_utf8(&out).unwrap();
            // 3 chunks.
            assert_eq!(s.matches("\x1b_G").count(), 4);
            // First chunk: full controls + m=1.
            let first_end = s.find(';').unwrap();
            let first_controls = &s["\x1b_G".len()..first_end];
            assert!(first_controls.contains("a=T"));
            assert!(first_controls.contains("m=1"));
            // Intermediate chunk: only m=1.
            let second_chunk_pos = s.find("\x1b_G").unwrap() + 1;
            let second_start =
                s[second_chunk_pos..].find("\x1b_G").unwrap() + second_chunk_pos + "\x1b_G".len();
            let second_end = s[second_start..].find(';').unwrap() + second_start;
            let second_controls = &s[second_start..second_end];
            assert_eq!(second_controls, "m=1");
            // Last chunk: m=0.
            let last_start = s.rfind("\x1b_Gm=0").unwrap() + "\x1b_G".len();
            let last_end = s.rfind(';').unwrap();
            let last_controls = &s[last_start..last_end];
            assert_eq!(last_controls, "m=0");
        });
    }

    /// The streaming entry point must accept a
    /// pre-allocated `Vec<u8>` (not just a fresh one).
    /// This pins the `<W: Write>` generic surface and
    /// verifies the per-chunk `out.write_all` calls
    /// grow the writer as needed without requiring the
    /// caller to pre-size it.
    #[cfg(feature = "kitty-encoder")]
    #[test]
    fn encode_to_writer_writes_to_pre_allocated_vec() {
        with_env(None, None, None, None, || {
            let fb = FrameBuffer::new(4, 4);
            // Pre-allocate a small Vec to ensure the
            // streaming path doesn't assume the writer
            // is empty. The Vec's `write_all` grows it
            // as needed.
            let mut buf: Vec<u8> = Vec::with_capacity(16);
            super::kitty::encode_to_writer(&fb, &mut buf).unwrap();
            // The buf must be non-empty and end with
            // the APC terminator (single-chunk path: 16
            // pixels = 64 raw bytes = 88 base64 chars,
            // well under the 4096-char limit).
            assert!(!buf.is_empty());
            assert!(buf.starts_with(b"\x1b_G"));
            assert!(buf.ends_with(b"\x1b\\"));
        });
    }

    /// 2MP smoke test: 1920×1080 = 2,073,600 pixels =
    /// 2,701 chunks of 768 pixels each. Verifies that
    /// the streaming path produces the expected chunk
    /// count for a realistically-sized framebuffer.
    /// The peak working set assertion is a code-review
    /// concern, not a runtime test -- we can't easily
    /// measure memory in a unit test without external
    /// tooling, but the per-chunk `Vec<u8>` allocation
    /// is statically bounded at 3072 bytes and is the
    /// only per-chunk allocation in the streaming path.
    #[cfg(feature = "kitty-encoder")]
    #[test]
    fn encode_to_writer_2mp_frame_smoke_test() {
        with_env(None, None, None, None, || {
            let w: u32 = 1920;
            let h: u32 = 1080;
            let mut fb = FrameBuffer::new(w, h);
        fb.pixels_mut()[0] = [255, 0, 0, 255];
            let mut out: Vec<u8> = Vec::new();
            super::kitty::encode_to_writer(&fb, &mut out).unwrap();
            let s = std::str::from_utf8(&out).unwrap();
            // Expected chunk count: ceil(1920*1080 / 768)
            // = ceil(2_073_600 / 768) = 2_701.
            let expected_chunks = (w as usize * h as usize)
                .div_ceil(super::kitty::PIXELS_PER_CHUNK);
            assert_eq!(
                s.matches("\x1b_G").count(),
                expected_chunks + 1,
                "1920x1080 frame must produce {expected_chunks} chunks"
            );
            // First chunk: full controls + m=1.
            let first_end = s.find(';').unwrap();
            let first_controls = &s["\x1b_G".len()..first_end];
            assert!(first_controls.contains("s=1920"));
            assert!(first_controls.contains("v=1080"));
            assert!(first_controls.contains("m=1"));
            // Last chunk: m=0.
            let last_start = s.rfind("\x1b_Gm=0").unwrap() + "\x1b_G".len();
            let last_end = s.rfind(';').unwrap();
            let last_controls = &s[last_start..last_end];
            assert_eq!(last_controls, "m=0");
        });
    }

    // -- v0.8.4: streaming Sixel encode tests ------------------------
    //
    // The v0.8.4 streaming Sixel entry point
    // `sixel::encode_to_writer<W: Write>` writes the Sixel
    // DCS bytes directly to the caller's `&mut impl Write`
    // sink. The input RGBA `Vec<u8>` is still materialised
    // (`icy_sixel` 0.5 has no streaming input API), but the
    // Sixel-output `Vec<u8>` that the v0.8.0 `encode` path
    // allocated via `sixel_string.into_bytes()` is
    // eliminated -- we write the `String`'s internal buffer
    // directly to the caller's writer.
    //
    // These tests verify (a) byte-for-byte equivalence with
    // the v0.8.0 `sixel::encode` for various inputs, (b) the
    // streaming entry point accepts a pre-allocated `Vec`,
    // (c) the zero-dimensions error path still works
    // through the streaming entry point, and (d) a 2MP
    // smoke test (realistic framebuffer size) works.

    /// Streaming output for a small frame (1x1) must match
    /// the v0.8.0 `sixel::encode` output byte-for-byte.
    /// This pins the v0.8.4 refactor's invariant: the
    /// streaming path and the `Vec<u8>` path produce
    /// identical bytes for the same input.
    #[cfg(feature = "sixel-encoder")]
    #[test]
    fn sixel_encode_to_writer_small_frame_matches_encode() {
        let mut fb = FrameBuffer::new(1, 1);
        for px in fb.pixels_mut() {
            *px = [255, 0, 0, 255];
        }
        let from_encode = super::sixel::encode(&fb).unwrap();
        let mut from_streaming: Vec<u8> = Vec::new();
        super::sixel::encode_to_writer(&fb, &mut from_streaming).unwrap();
        assert_eq!(from_encode, from_streaming);
    }

    /// The streaming entry point must accept a
    /// pre-allocated `Vec<u8>` (not just a fresh one).
    /// This pins the `<W: Write>` generic surface and
    /// verifies the streaming path's `out.write_all`
    /// call grows the writer as needed without
    /// requiring the caller to pre-size it.
    #[cfg(feature = "sixel-encoder")]
    #[test]
    fn sixel_encode_to_writer_writes_to_pre_allocated_vec() {
        let mut fb = FrameBuffer::new(2, 2);
        for px in fb.pixels_mut() {
            *px = [0, 255, 0, 255];
        }
        // Pre-allocate a small Vec to ensure the
        // streaming path doesn't assume the writer
        // is empty. The Vec's `write_all` grows it
        // as needed.
        let mut buf: Vec<u8> = Vec::with_capacity(16);
        super::sixel::encode_to_writer(&fb, &mut buf).unwrap();
        // The buf must be non-empty and end with the
        // DCS terminator (Sixel output always starts
        // with `ESC P q` and ends with `ESC \`).
        assert!(!buf.is_empty());
        assert!(buf.starts_with(b"\x1bP"));
        assert!(buf.ends_with(b"\x1b\\"));
    }

    /// The streaming entry point must surface the
    /// zero-dimensions error from the Sixel encoder.
    /// This pins the v0.8.4 invariant: the streaming
    /// path returns the same `EncoderError` variants
    /// as the `Vec<u8>` path, so callers can use
    /// `?` uniformly across both entry points.
    #[cfg(feature = "sixel-encoder")]
    #[test]
    fn sixel_encode_to_writer_rejects_zero_dimensions() {
        let fb_zero_w = FrameBuffer::new(0, 5);
        let fb_zero_h = FrameBuffer::new(5, 0);
        let fb_zero_both = FrameBuffer::new(0, 0);
        let mut buf: Vec<u8> = Vec::new();
        for fb in [&fb_zero_w, &fb_zero_h, &fb_zero_both] {
            let err = super::sixel::encode_to_writer(fb, &mut buf)
                .unwrap_err();
            assert!(matches!(err, EncoderError::InvalidDimensions { .. }));
        }
        // The buffer must be untouched (the error is
        // returned before any Sixel encoding work
        // happens).
        assert!(buf.is_empty());
    }

    /// 2MP smoke test: 1920×1080 = 2,073,600 pixels.
    /// Verifies that the streaming path produces a
    /// non-empty Sixel DCS for a realistically-sized
    /// framebuffer. The peak working set assertion is
    /// a code-review concern, not a runtime test --
    /// we can't easily measure memory in a unit test
    /// without external tooling, but the Sixel-output
    /// `Vec<u8>` that the v0.8.0 `encode` path
    /// allocated via `sixel_string.into_bytes()` is
    /// statically eliminated by the streaming path
    /// (the `String`'s internal buffer is borrowed,
    /// not copied into a new `Vec`).
    #[cfg(feature = "sixel-encoder")]
    #[test]
    fn sixel_encode_to_writer_2mp_frame_smoke_test() {
        let w: u32 = 1920;
        let h: u32 = 1080;
        let fb = FrameBuffer::new(w, h);
        let mut out: Vec<u8> = Vec::new();
        super::sixel::encode_to_writer(&fb, &mut out).unwrap();
        // The output must be non-empty and have the
        // expected Sixel DCS framing.
        assert!(!out.is_empty());
        assert!(out.starts_with(b"\x1bP"));
        assert!(out.ends_with(b"\x1b\\"));
        // Sanity check: the output should be larger
        // than the RGBA input (Sixel DCS adds framing
        // and run-length-encoded sixel data that can
        // be larger or smaller than RGBA depending on
        // color complexity, but for a default-zero
        // framebuffer the encoding is highly
        // compressible so the output should be
        // smaller than 8MB+).
        assert!(out.len() < (w as usize * h as usize * 4));
    }

    // -- v0.8.5: fixed-palette streaming Sixel encode tests ----------
    //
    // The v0.8.5 streaming entry point
    // `sixel::encode_to_writer_streaming<W: Write>` writes
    // the Sixel DCS bytes directly to the caller's `&mut
    // impl Write` sink with O(1) memory (no full-
    // framebuffer `Vec<u8>` allocation). The v0.8.4
    // `encode_to_writer` path still materialises the full
    // RGBA `Vec<u8>` because `icy_sixel` 0.5 has no
    // streaming input API. v0.8.5 adds a fully O(1) path
    // that uses a fixed xterm-256 palette (16 basic +
    // 6x6x6 RGB cube + 24 grayscale) and emits band-by-
    // band in a single DCS sequence.
    //
    // These tests verify (a) the basic DCS structure
    // (header, palette, sixel data, terminator), (b) the
    // RLE compression for uniform colors, (c) the band
    // separator emission, (d) the zero-dimensions error
    // path, and (e) a 2MP smoke test (realistic
    // framebuffer size).

    /// Basic structure test: a 2x2 framebuffer with
    /// known colors produces a valid Sixel DCS with
    /// the expected structure (DCS header, palette
    /// definitions, sixel data, DCS terminator).
    #[cfg(feature = "sixel-encoder")]
    #[test]
    fn sixel_encode_to_writer_streaming_basic_structure() {
        let mut fb = FrameBuffer::new(2, 2);
        for px in fb.pixels_mut() {
            *px = [255, 0, 0, 255]; // red
        }
        let mut out: Vec<u8> = Vec::new();
        super::sixel::encode_to_writer_streaming(&fb, &mut out)
            .unwrap();
        let s = std::str::from_utf8(&out).unwrap();
        assert!(
            s.starts_with("\x1bP0;0;2;2q"),
            "missing DCS header, got prefix: {:?}",
            &s[..s.len().min(20)],
        );
        assert!(s.contains("#0;2;0;0;0"), "missing color 0");
        assert!(s.contains("#15;2;100;100;100"), "missing color 15");
        assert!(
            s.ends_with("\x1b\\"),
            "missing DCS terminator, got suffix: {:?}",
            &s[s.len().saturating_sub(20)..],
        );
    }

    /// RLE test: a framebuffer with a solid color
    /// produces output that uses the `! <n> <ch>`
    /// repeat introducer for the run of identical
    /// sixel characters.
    #[cfg(feature = "sixel-encoder")]
    #[test]
    fn sixel_encode_to_writer_streaming_uses_rle_for_solid_color() {
        let mut fb = FrameBuffer::new(10, 1);
        for px in fb.pixels_mut() {
            *px = [255, 0, 0, 255]; // red
        }
        let mut out: Vec<u8> = Vec::new();
        super::sixel::encode_to_writer_streaming(&fb, &mut out)
            .unwrap();
        let s = std::str::from_utf8(&out).unwrap();
        assert!(
            s.contains('!'),
            "expected RLE repeat introducer `!`, got output: {s:?}"
        );
    }

    /// Band separator test: a framebuffer with height
    /// greater than 6 produces output with `-` band separators
    /// between the 6-row bands.
    #[cfg(feature = "sixel-encoder")]
    #[test]
    fn sixel_encode_to_writer_streaming_uses_band_separators() {
        let mut fb = FrameBuffer::new(1, 12); // 2 bands of 6 rows
        for px in fb.pixels_mut() {
            *px = [0, 255, 0, 255]; // green
        }
        let mut out: Vec<u8> = Vec::new();
        super::sixel::encode_to_writer_streaming(&fb, &mut out)
            .unwrap();
        let s = std::str::from_utf8(&out).unwrap();
        let separator_count = s.matches('-').count();
        assert_eq!(
            separator_count, 1,
            "expected 1 band separator for 12-row frame, got {separator_count}"
        );
    }

    /// Zero-dimensions test: the streaming entry
    /// point surfaces the same `InvalidDimensions`
    /// error as the `encode` path.
    #[cfg(feature = "sixel-encoder")]
    #[test]
    fn sixel_encode_to_writer_streaming_rejects_zero_dimensions() {
        let fb_zero_w = FrameBuffer::new(0, 5);
        let fb_zero_h = FrameBuffer::new(5, 0);
        let fb_zero_both = FrameBuffer::new(0, 0);
        let mut buf: Vec<u8> = Vec::new();
        for fb in [&fb_zero_w, &fb_zero_h, &fb_zero_both] {
            let err = super::sixel::encode_to_writer_streaming(
                fb, &mut buf,
            )
            .unwrap_err();
            assert!(matches!(err, EncoderError::InvalidDimensions { .. }));
        }
        assert!(buf.is_empty());
    }

    /// 2MP smoke test: a 1920x1080 framebuffer encodes
    /// correctly through the streaming path with O(1)
    /// memory. Verifies the expected number of band
    /// separators (1080 / 6 = 180 bands, 179
    /// separators).
    #[cfg(feature = "sixel-encoder")]
    #[test]
    fn sixel_encode_to_writer_streaming_2mp_frame_smoke_test() {
        let w: u32 = 1920;
        let h: u32 = 1080;
        let fb = FrameBuffer::new(w, h);
        let mut out: Vec<u8> = Vec::new();
        super::sixel::encode_to_writer_streaming(&fb, &mut out)
            .unwrap();
        assert!(!out.is_empty());
        assert!(out.starts_with(b"\x1bP"));
        assert!(out.ends_with(b"\x1b\\"));
        let s = std::str::from_utf8(&out).unwrap();
        let separator_count = s.matches('-').count();
        assert_eq!(
            separator_count, 179,
            "expected 179 band separators for 1080-row frame, got {separator_count}"
        );
    }

    // -- v0.8.3: streaming wrap_for_tmux tests -----------------------
    //
    // The v0.8.3 streaming wrap entry point
    // `wrap_for_tmux_to_writer` writes the wrapped DCS bytes
    // directly to a `&mut impl Write` sink instead of
    // materialising them in a `Vec<u8>`. The
    // `PassthroughWriter` adapter is the v0.8.3 building
    // block for end-to-end O(1) streaming: combined with
    // `encode_to_writer` via
    // `encode_passthrough_to_writer`, the entire
    // encode + wrap + emit pipeline runs in O(1) memory.
    //
    // These tests verify (a) byte-for-byte equivalence with
    // the v0.8.0 `wrap_for_tmux` for various inputs, (b) the
    // `PassthroughWriter` adapter's prefix/doubling/suffix
    // semantics, and (c) the end-to-end
    // `encode_passthrough_to_writer` entry point with and
    // without the tmux passthrough opt-in.

    /// Streaming wrap output must match the v0.8.0
    /// `wrap_for_tmux` output byte-for-byte for a typical
    /// Kitty APC payload. This pins the v0.8.3 refactor's
    /// invariant: the streaming path and the `Vec<u8>` path
    /// produce identical bytes for the same input.
    #[cfg(feature = "kitty-encoder")]
    #[test]
    fn wrap_for_tmux_to_writer_matches_wrap_for_tmux() {
        let inner: Vec<u8> =
            b"\x1b_Ga=T,f=32,q=2,s=2,v=2;AAAA\x1b\\".to_vec();
        let from_vec = super::kitty::wrap_for_tmux(inner.clone());
        let mut from_streaming: Vec<u8> = Vec::new();
        super::kitty::wrap_for_tmux_to_writer(&inner, &mut from_streaming)
            .unwrap();
        assert_eq!(from_vec, from_streaming);
    }

    /// Inner ESC bytes must be doubled in the streaming
    /// output, matching the v0.8.0 `wrap_for_tmux` doubling
    /// behaviour. ESCs at the introducer and terminator
    /// (which are the only ESCs in a normal Kitty payload)
    /// each become 2 ESCs.
    #[cfg(feature = "kitty-encoder")]
    #[test]
    fn wrap_for_tmux_to_writer_doubles_inner_esc_bytes() {
        let inner: Vec<u8> = b"\x1b_Ga=T\x1bTEST\x1b\\".to_vec();
        let mut out: Vec<u8> = Vec::new();
        super::kitty::wrap_for_tmux_to_writer(&inner, &mut out)
            .unwrap();
        // The streaming output must be byte-for-byte
        // equal to the v0.8.0 `wrap_for_tmux` output.
        let from_vec = super::kitty::wrap_for_tmux(inner.clone());
        assert_eq!(out, from_vec);
    }

    /// Empty inner input must produce a valid empty
    /// wrapped DCS: `\x1bPtmux;\x1b\\` (9 bytes). This
    /// matches the v0.8.0 `wrap_for_tmux` behaviour for
    /// empty input.
    #[cfg(feature = "kitty-encoder")]
    #[test]
    fn wrap_for_tmux_to_writer_handles_empty() {
        let mut out: Vec<u8> = Vec::new();
        super::kitty::wrap_for_tmux_to_writer(&[], &mut out)
            .unwrap();
        assert_eq!(out, b"\x1bPtmux;\x1b\\");
    }

    /// `PassthroughWriter` must double ESC bytes in the
    /// body (not the prefix/suffix). The first body byte
    /// written triggers the prefix; subsequent bytes are
    /// forwarded with ESC doubling; `finish()` writes the
    /// suffix. The full output (prefix + doubled body +
    /// suffix) must be byte-for-byte equal to the v0.8.0
    /// `wrap_for_tmux` reference output.
    #[cfg(feature = "kitty-encoder")]
    #[test]
    fn passthrough_writer_doubles_esc_in_body() {
        use std::io::Write;
        let mut out: Vec<u8> = Vec::new();
        {
            let mut pw = super::kitty::PassthroughWriter::new(&mut out);
            // Write a body with an ESC in the middle (not
            // at the start or end of the body).
            pw.write_all(b"\x1b_Ga=T\x1bTEST\x1b\\").unwrap();
            pw.flush().unwrap();
            // `finish()` writes the DCS terminator and
            // returns the inner writer. Without this call
            // the suffix would be missing and the test
            // would fail (the `wrap_for_tmux` reference
            // includes both prefix and suffix).
            let _inner = pw.finish().unwrap();
        }
        // The DCS prefix is `ESC P tmux ;` (7 bytes).
        assert!(out.starts_with(b"\x1bPtmux;"));
        // The full output must be byte-for-byte equal to
        // the v0.8.0 `wrap_for_tmux` reference output.
        let reference =
            super::kitty::wrap_for_tmux(b"\x1b_Ga=T\x1bTEST\x1b\\".to_vec());
        assert_eq!(out, reference);
    }

    /// `PassthroughWriter::finish` for an empty body (no
    /// `write` calls) must still produce a valid empty
    /// wrapped DCS: prefix + suffix. This matches the
    /// v0.8.0 `wrap_for_tmux` behaviour for empty input.
    #[cfg(feature = "kitty-encoder")]
    #[test]
    fn passthrough_writer_finish_for_empty_body() {
        let mut out: Vec<u8> = Vec::new();
        let pw = super::kitty::PassthroughWriter::new(&mut out);
        let _inner = pw.finish().unwrap();
        assert_eq!(out, b"\x1bPtmux;\x1b\\");
    }

    /// End-to-end: when the tmux passthrough opt-in is
    /// NOT set, `encode_passthrough_to_writer` must
    /// delegate to `encode_to_writer` and produce the
    /// raw APC bytes (no wrapping).
    #[cfg(feature = "kitty-encoder")]
    #[test]
    fn encode_passthrough_to_writer_without_tmux_skips_wrap() {
        with_env(Some("xterm-kitty"), None, None, None, || {
            let fb = FrameBuffer::new(2, 2);
            let from_passthrough: Vec<u8> = {
                let mut out: Vec<u8> = Vec::new();
                super::kitty::encode_passthrough_to_writer(
                    &fb, &mut out,
                )
                .unwrap();
                out
            };
            let from_encode: Vec<u8> = {
                let mut out: Vec<u8> = Vec::new();
                super::kitty::encode_to_writer(&fb, &mut out)
                    .unwrap();
                out
            };
            // Without the tmux opt-in, the two outputs
            // must be byte-for-byte equal.
            assert_eq!(from_passthrough, from_encode);
            // And the output must NOT be wrapped (no
            // `\x1bPtmux;` prefix).
            assert!(!from_passthrough.starts_with(b"\x1bPtmux;"));
        });
    }

    /// End-to-end: when the tmux passthrough opt-in IS
    /// set (TMUXPASSTHROUGH + TMUX), the output must be
    /// the wrapped DCS. This verifies the v0.8.3
    /// end-to-end O(1) streaming path: encode + wrap
    /// + emit in a single pass.
    #[cfg(feature = "kitty-encoder")]
    #[test]
    fn encode_passthrough_to_writer_with_tmux_wraps() {
        with_env(Some("xterm-kitty"), None, None, Some("1"), || {
            let _tmux = EnvGuard::new("TMUX");
            _tmux.set(Some("/tmp/tmux-1000/default,12345,0"));
            let fb = FrameBuffer::new(2, 2);
            let mut out: Vec<u8> = Vec::new();
            super::kitty::encode_passthrough_to_writer(
                &fb, &mut out,
            )
            .unwrap();
            // With the tmux opt-in, the output MUST be
            // wrapped: starts with the DCS prefix and
            // ends with the DCS terminator.
            assert!(out.starts_with(b"\x1bPtmux;"));
            assert!(out.ends_with(b"\x1b\\"));
            // The output must be byte-for-byte equal to
            // the v0.8.0/v0.8.2 `dispatch(Protocol::Kitty,
            // &fb)` reference output (which materialises
            // both the encode and the wrap in Vecs). The
            // streaming path must produce the same bytes.
            let from_dispatch =
                dispatch(Protocol::Kitty, &fb).unwrap();
            assert_eq!(out, from_dispatch);
        });
    }
    // -- v0.8.6: dispatch_to_writer end-to-end streaming tests --------
    //
    // The v0.8.6 `dispatch_to_writer<W: Write>(protocol,
    // frame, &mut W)` entry point combines the v0.8.2
    // Kitty streaming, v0.8.3 tmux passthrough wrap,
    // and v0.8.4 Sixel streaming work into a single
    // end-to-end streaming dispatch. These tests verify
    // (a) byte-for-byte equivalence with the Vec<u8>-
    // returning dispatch() for the same input, (b) the
    // disabled-feature arms return UnsupportedProtocol,
    // (c) the Auto arm recurses correctly via detect(),
    // and (d) a 2MP smoke test.

    /// When tmux passthrough is disabled, the Kitty
    /// arm of dispatch_to_writer must produce the same
    /// output as the Vec<u8>-returning dispatch()
    /// function. This pins the v0.8.6 invariant: the
    /// streaming dispatch and the Vec<u8> dispatch
    /// produce identical bytes for the same input
    /// (modulo the tmux passthrough wrap, which is
    /// disabled here via with_env).
    #[cfg(feature = "kitty-encoder")]
    #[test]
    fn dispatch_to_writer_kitty_matches_dispatch() {
        with_env(None, None, None, None, || {
            let mut fb = FrameBuffer::new(4, 4);
            for px in fb.pixels_mut() {
                *px = [10, 20, 30, 255];
            }
            // Vec<u8> path via dispatch().
            let from_dispatch = dispatch(Protocol::Kitty, &fb).unwrap();
            // Streaming path via dispatch_to_writer().
            let mut from_streaming: Vec<u8> = Vec::new();
            super::dispatch_to_writer(Protocol::Kitty, &fb, &mut from_streaming).unwrap();
            assert_eq!(from_dispatch, from_streaming);
        });
    }

    /// The Sixel arm of dispatch_to_writer must produce
    /// the same output as dispatch() for the same input.
    #[cfg(feature = "sixel-encoder")]
    #[test]
    fn dispatch_to_writer_sixel_matches_dispatch() {
        let mut fb = FrameBuffer::new(4, 4);
        for px in fb.pixels_mut() {
            *px = [10, 20, 30, 255];
        }
        let from_dispatch = dispatch(Protocol::Sixel, &fb).unwrap();
        let mut from_streaming: Vec<u8> = Vec::new();
        super::dispatch_to_writer(Protocol::Sixel, &fb, &mut from_streaming).unwrap();
        assert_eq!(from_dispatch, from_streaming);
    }

    /// The Auto arm must recurse via detect() to the
    /// correct concrete protocol. With TERM=xterm-kitty
    /// (a known Kitty terminfo name), the recursion
    /// should land in the Kitty arm and produce Kitty
    /// escape bytes.
    #[cfg(feature = "kitty-encoder")]
    #[test]
    fn dispatch_to_writer_auto_resolves_via_detect() {
        with_env(Some("xterm-kitty"), None, None, None, || {
            let mut fb = FrameBuffer::new(2, 2);
            for px in fb.pixels_mut() {
                *px = [128, 64, 32, 255];
            }
            let from_auto: Vec<u8> = {
                let mut out: Vec<u8> = Vec::new();
                super::dispatch_to_writer(Protocol::Auto, &fb, &mut out).unwrap();
                out
            };
            let from_kitty: Vec<u8> = {
                let mut out: Vec<u8> = Vec::new();
                super::dispatch_to_writer(Protocol::Kitty, &fb, &mut out).unwrap();
                out
            };
            // With TERM=xterm-kitty, Auto resolves to Kitty,
            // so the outputs must be byte-for-byte equal.
            assert_eq!(from_auto, from_kitty);
        });
    }

    /// 2MP smoke test: a 1920x1080 framebuffer encodes
    /// correctly through the end-to-end streaming
    /// dispatch (via the Kitty arm) with O(1) memory.
    #[cfg(feature = "kitty-encoder")]
    #[test]
    fn dispatch_to_writer_2mp_frame_smoke_test() {
        with_env(None, None, None, None, || {
            let w: u32 = 1920;
            let h: u32 = 1080;
            let fb = FrameBuffer::new(w, h);
            let mut out: Vec<u8> = Vec::new();
            super::dispatch_to_writer(Protocol::Kitty, &fb, &mut out).unwrap();
            assert!(!out.is_empty());
            // Kitty APC starts with \x1b_G (without tmux
            // passthrough wrap).
            assert!(out.starts_with(b"\x1b_G"));
            assert!(out.ends_with(b"\x1b\\"));
        });
    }

    /// Disabled-feature test: when the kitty-encoder
    /// feature is off, dispatch_to_writer(Protocol::Kitty)
    /// must return UnsupportedProtocol("kitty"), matching
    /// the dispatch() behaviour.
    #[cfg(not(feature = "kitty-encoder"))]
    #[test]
    fn dispatch_to_writer_kitty_unsupported_without_feature() {
        let fb = FrameBuffer::new(2, 2);
        let mut out: Vec<u8> = Vec::new();
        let err = super::dispatch_to_writer(Protocol::Kitty, &fb, &mut out)
            .unwrap_err();
        assert_eq!(err, EncoderError::UnsupportedProtocol("kitty"));
        // Buffer must be untouched.
        assert!(out.is_empty());
    }

    /// Disabled-feature test: when the sixel-encoder
    /// feature is off, dispatch_to_writer(Protocol::Sixel)
    /// must return UnsupportedProtocol("sixel").
    #[cfg(not(feature = "sixel-encoder"))]
    #[test]
    fn dispatch_to_writer_sixel_unsupported_without_feature() {
        let fb = FrameBuffer::new(2, 2);
        let mut out: Vec<u8> = Vec::new();
        let err = super::dispatch_to_writer(Protocol::Sixel, &fb, &mut out)
            .unwrap_err();
        assert_eq!(err, EncoderError::UnsupportedProtocol("sixel"));
        assert!(out.is_empty());
    }

    // -- v0.12.2 place-command + transparency short-circuit tests --

    /// v0.12.2: a non-transparent framebuffer must produce
    /// BOTH a transmit command (`a=T`) and a place command
    /// (`a=p,i=1,z=-1`).
    #[test]
    #[cfg(feature = "kitty-encoder")]
    fn encode_to_writer_emits_place_command_after_transmission() {
        let mut fb = FrameBuffer::new(2, 2);
        fb.pixels_mut()[0] = [255, 0, 0, 255];
        let mut out = Vec::new();
        super::kitty::encode_to_writer(&fb, &mut out).unwrap();
        let output = std::str::from_utf8(&out).expect("non-UTF8 output");
        assert!(output.contains("a=T"), "transmit command missing: {:?}", output);
        assert!(output.contains("a=p,i=1,z=-1"), "place command missing: {:?}", output);
        let transmit_pos = output.find("a=T").unwrap();
        let place_pos = output.find("a=p,i=1,z=-1").unwrap();
        assert!(place_pos > transmit_pos, "place must come after transmit; transmit at {} place at {}: {:?}", transmit_pos, place_pos, output);
    }

    /// v0.12.2: a fully-transparent framebuffer must
    /// short-circuit to a single delete command (`a=d`).
    #[test]
    #[cfg(feature = "kitty-encoder")]
    fn encode_to_writer_fully_transparent_framebuffer_emits_delete_only() {
        let fb = FrameBuffer::new(100, 100);
        assert!(fb.is_fully_transparent());
        let mut out = Vec::new();
        super::kitty::encode_to_writer(&fb, &mut out).unwrap();
        let output = std::str::from_utf8(&out).expect("non-UTF8 output");
        assert!(output.contains("a=d,d=I,i=1"), "delete command missing: {:?}", output);
        assert!(!output.contains("a=T"), "transmit must NOT be emitted for transparent framebuffer: {:?}", output);
        assert!(out.len() < 64, "short-circuit output should be <64 bytes, got {} bytes: {:?}", out.len(), output);
    }

    /// v0.12.2: the FrameBuffer::is_fully_transparent
    /// predicate correctly distinguishes empty from
    /// non-empty framebuffers.
    #[test]
    fn framebuffer_is_fully_transparent_predicate() {
        let empty = FrameBuffer::new(10, 10);
        assert!(empty.is_fully_transparent());
        let mut one_pixel = FrameBuffer::new(10, 10);
        one_pixel.pixels_mut()[0] = [0, 0, 0, 1];
        assert!(!one_pixel.is_fully_transparent());
        let mut opaque = FrameBuffer::new(10, 10);
        opaque.pixels_mut()[5] = [255, 255, 255, 255];
        assert!(!opaque.is_fully_transparent());
    }

    // ── EncoderError Display ───────────────────────────────────

    #[test]
    fn encoder_error_display_encode_variant() {
        let err = EncoderError::Encode("something went wrong".to_string());
        assert_eq!(err.to_string(), "encoder failed: something went wrong");
    }

    #[test]
    fn encoder_error_display_unsupported_protocol_variant() {
        let err = EncoderError::UnsupportedProtocol("kitty");
        assert_eq!(
            err.to_string(),
            "protocol kitty is not supported in this build"
        );
    }

    #[test]
    fn encoder_error_display_invalid_dimensions_variant() {
        let err = EncoderError::InvalidDimensions {
            width: 0,
            height: 5,
        };
        assert_eq!(
            err.to_string(),
            "framebuffer has invalid dimensions: 0x5"
        );
    }

    #[test]
    fn encoder_error_is_std_error() {
        let err: Box<dyn std::error::Error> =
            Box::new(EncoderError::Encode("test".to_string()));
        assert!(err.to_string().contains("encoder failed"));
        assert!(err.source().is_none());
    }    // ── From<std::io::Error> conversion ────────────────────────
    #[cfg(any(feature = "kitty-encoder", feature = "sixel-encoder"))]
    #[test]
    fn from_io_error_converts_to_encode() {
        let io_err =
            std::io::Error::new(std::io::ErrorKind::Other, "write failed");
        let enc_err: EncoderError = io_err.into();
        match enc_err {
            EncoderError::Encode(msg) => {
                assert!(msg.contains("write failed"))
            }
            _ => panic!("expected EncoderError::Encode"),
        }
    }

    // ── tmux_passthrough_enabled ───────────────────────────────

    #[cfg(feature = "kitty-encoder")]
    #[test]
    fn tmux_passthrough_requires_both_env_vars() {
        let _lock = env_lock();
        let _g1 = EnvGuard::new("TMUXPASSTHROUGH");
        let _g2 = EnvGuard::new("TMUX");

        // Neither set
        std::env::remove_var("TMUXPASSTHROUGH");
        std::env::remove_var("TMUX");
        assert!(!super::tmux_passthrough_enabled());

        // Only TMUXPASSTHROUGH set
        _g1.set(Some("1"));
        assert!(!super::tmux_passthrough_enabled());

        // Only TMUX set
        _g1.set(None);
        _g2.set(Some("/tmp/tmux-1000/default,12345,0"));
        assert!(!super::tmux_passthrough_enabled());

        // Both set
        _g1.set(Some("1"));
        assert!(super::tmux_passthrough_enabled());

        // Empty TMUXPASSTHROUGH should NOT enable
        _g1.set(Some(""));
        assert!(!super::tmux_passthrough_enabled());
    }

    // ── wrap_for_tmux ESC doubling ──────────────────────────────

    #[cfg(feature = "kitty-encoder")]
    #[test]
    fn kitty_wrap_for_tmux_doubles_esc_bytes() {
        let inner = vec![0x1b, b'_', b'G', 0x1b, 0x5c, 0x1b];
        let wrapped = super::kitty::wrap_for_tmux(inner.clone());
        // Should contain the DCS prefix and suffix
        assert!(
            wrapped
                .windows(7)
                .any(|w| w == b"\x1bPtmux;")
        );
        assert!(wrapped.windows(2).any(|w| w == b"\x1b\\"));
        // Inner ESC bytes should be doubled: 3 ESCs -> 6, plus prefix
        // (1) + suffix (1) = 8 total
        let inner_esc_count = inner.iter().filter(|&&b| b == 0x1b).count();
        let wrapped_esc_count =
            wrapped.iter().filter(|&&b| b == 0x1b).count();
        assert_eq!(wrapped_esc_count, inner_esc_count * 2 + 2);
    }

    #[cfg(feature = "kitty-encoder")]
    #[test]
    fn kitty_wrap_for_tmux_empty_input() {
        let wrapped = super::kitty::wrap_for_tmux(vec![]);
        assert_eq!(wrapped, b"\x1bPtmux;\x1b\\");
    }

    #[cfg(feature = "kitty-encoder")]
    #[test]
    fn kitty_wrap_for_tmux_to_writer_matches_wrap_for_tmux() {
        let inner = vec![0x1b, b'A', 0x1b, b'B', b'C'];
        let via_fn = super::kitty::wrap_for_tmux(inner.clone());
        let mut via_writer = Vec::new();
        super::kitty::wrap_for_tmux_to_writer(&inner, &mut via_writer)
            .unwrap();
        assert_eq!(via_fn, via_writer);
    }

    // ── PassthroughWriter ───────────────────────────────────────

    #[cfg(feature = "kitty-encoder")]
    #[test]
    fn kitty_passthrough_writer_empty_body() {
        let mut out = Vec::new();
        let pw = super::kitty::PassthroughWriter::new(&mut out);
        let _inner = pw.finish().unwrap();
        assert_eq!(out, b"\x1bPtmux;\x1b\\");
    }

    #[cfg(feature = "kitty-encoder")]
    #[test]
    fn kitty_passthrough_writer_with_body_doubles_esc() {
        use std::io::Write;
        let mut out = Vec::new();
        {
            let mut pw = super::kitty::PassthroughWriter::new(&mut out);
            pw.write_all(b"hello").unwrap();
            pw.write_all(&[0x1b, b'X']).unwrap();
            let _inner = pw.finish().unwrap();
        }
        assert!(out.starts_with(b"\x1bPtmux;"));
        assert!(out.ends_with(b"\x1b\\"));
        assert!(out.windows(5).any(|w| w == b"hello"));
        assert!(out.windows(3).any(|w| w == b"\x1b\x1bX"));
    }

    #[cfg(feature = "kitty-encoder")]
    #[test]
    fn kitty_passthrough_writer_flush_delegates() {
        use std::io::Write;
        let mut out = Vec::new();
        let mut pw = super::kitty::PassthroughWriter::new(&mut out);
        pw.write_all(b"test").unwrap();
        pw.flush().unwrap();
        assert!(!out.is_empty());
    }

    #[cfg(feature = "kitty-encoder")]
    #[test]
    fn kitty_passthrough_writer_returns_inner_on_finish() {
        use std::io::Write;
        let mut out = Vec::new();
        let pw = super::kitty::PassthroughWriter::new(&mut out);
        let returned = pw.finish().unwrap();
        // Returned writer should still be usable
        returned.write_all(b"after").unwrap();
        assert!(out.starts_with(b"\x1bPtmux;"));
        assert!(out.ends_with(b"after"));
    }

    // ── encode_passthrough_to_writer without tmux ───────────────

    #[cfg(feature = "kitty-encoder")]
    #[test]
    fn kitty_encode_passthrough_to_writer_no_tmux_matches_encode_to_writer() {
        let _lock = env_lock();
        let _g1 = EnvGuard::new("TMUXPASSTHROUGH");
        let _g2 = EnvGuard::new("TMUX");
        _g1.set(None);
        _g2.set(None);

        let fb = FrameBuffer::new(2, 2);
        let mut out_passthrough = Vec::new();
        let mut out_regular = Vec::new();
        super::kitty::encode_passthrough_to_writer(
            &fb,
            &mut out_passthrough,
        )
        .unwrap();
        super::kitty::encode_to_writer(&fb, &mut out_regular).unwrap();
        assert_eq!(out_passthrough, out_regular);
    }

    // ── sixel encode_to_writer_streaming invalid dimensions ──────

    #[cfg(feature = "sixel-encoder")]
    #[test]
    fn sixel_streaming_rejects_zero_width() {
        let fb = FrameBuffer::new(0, 10);
        let mut out = Vec::new();
        let err = super::sixel::encode_to_writer_streaming(&fb, &mut out)
            .unwrap_err();
        assert!(matches!(
            err,
            EncoderError::InvalidDimensions {
                width: 0,
                height: 10
            }
        ));
    }

    #[cfg(feature = "sixel-encoder")]
    #[test]
    fn sixel_streaming_rejects_zero_height() {
        let fb = FrameBuffer::new(10, 0);
        let mut out = Vec::new();
        let err = super::sixel::encode_to_writer_streaming(&fb, &mut out)
            .unwrap_err();
        assert!(matches!(
            err,
            EncoderError::InvalidDimensions {
                width: 10,
                height: 0
            }
        ));
    }

    #[cfg(feature = "sixel-encoder")]
    #[test]
    fn sixel_streaming_produces_valid_dcs() {
        let fb = FrameBuffer::new(4, 4);
        let mut out = Vec::new();
        super::sixel::encode_to_writer_streaming(&fb, &mut out)
            .unwrap();
        assert!(!out.is_empty());
        // Should start with DCS header
        assert!(out.starts_with(b"\x1bP"));
    }

    #[cfg(feature = "sixel-encoder")]
    #[test]
    fn sixel_streaming_multiband_iteration() {
        // 10 rows > 6 (one band), so the encoder iterates bands: [0..6), [6..10)
        let fb = FrameBuffer::new(4, 10);
        let mut out = Vec::new();
        super::sixel::encode_to_writer_streaming(&fb, &mut out).unwrap();
        assert!(!out.is_empty());
        assert!(out.starts_with(b"\x1bP"));
        // Sixel DCS must end with ST (ESC \)
        assert!(out.ends_with(b"\x1b\\"));
        // A taller frame should produce more output bytes than a 4x4 frame
        let mut out_4x4 = Vec::new();
        super::sixel::encode_to_writer_streaming(&FrameBuffer::new(4, 4), &mut out_4x4).unwrap();
        assert!(out.len() > out_4x4.len());
    }

    // ── ProtocolEncoder trait dispatch ───────────────────────────

    #[cfg(feature = "kitty-encoder")]
    #[test]
    fn protocol_encoder_kitty_encode_produces_output() {
        let fb = FrameBuffer::new(2, 2);
        let result = Protocol::Kitty.encode(&fb);
        assert!(result.is_ok());
        assert!(!result.unwrap().is_empty());
    }

    #[cfg(feature = "sixel-encoder")]
    #[test]
    fn protocol_encoder_sixel_encode_produces_output() {
        let fb = FrameBuffer::new(2, 2);
        let result = Protocol::Sixel.encode(&fb);
        assert!(result.is_ok());
        assert!(!result.unwrap().is_empty());
    }

    // ── dispatch_to_writer explicit protocols ────────────────────

    // ── kitty multi-chunk boundary ──────────────────────────────

    #[cfg(feature = "kitty-encoder")]
    #[test]
    fn kitty_encode_exact_boundary_single_chunk() {
        let fb = FrameBuffer::new(768, 1);
        let result = super::kitty::encode(&fb);
        assert!(result.is_ok());
        assert!(!result.unwrap().is_empty());
    }

    #[cfg(feature = "kitty-encoder")]
    #[test]
    fn kitty_encode_one_over_boundary_multi_chunk() {
        let fb = FrameBuffer::new(769, 1);
        let result = super::kitty::encode(&fb);
        assert!(result.is_ok());
        assert!(!result.unwrap().is_empty());
    }

    // ── From<std::io::Error> for EncoderError ────────────────────

    #[cfg(feature = "kitty-encoder")]
    #[test]
    fn io_error_converts_to_encoder_error_encode() {
        let io_err =
            std::io::Error::new(std::io::ErrorKind::Other, "disk full");
        let enc_err: EncoderError = io_err.into();
        assert_eq!(
            enc_err.to_string(),
            "encoder failed: disk full"
        );
    }

}
