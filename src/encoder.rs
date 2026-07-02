//! Output protocol selection and framebuffer encoding.
//!
//! The runtime picks a [`Protocol`] (Kitty graphics protocol or
//! Sixel) based on terminal capability detection (via `TERM`,
//! `TERM_PROGRAM`, `COLORTERM`) per AGENTS.md §7, preferring
//! [`Protocol::Kitty`] when the host supports it and falling back
//! to [`Protocol::Sixel`] otherwise.
//!
//! v0.5.0 wires up the Kitty arm via the optional
//! [`little_kitty`](https://crates.io/crates/little-kitty) crate
//! behind the `kitty-encoder` Cargo feature. Sixel is not yet
//! implemented -- calling `encode` on `Protocol::Sixel` returns
//! [`EncoderError::UnsupportedProtocol`] and is the v0.6.0 work.

use crate::framebuffer::FrameBuffer;

/// Terminal graphics protocol used to encode the composited
/// framebuffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protocol {
    /// The kitty graphics protocol -- modern and feature-rich.
    Kitty,
    /// Sixel -- fallback for terminals without kitty support.
    Sixel,
}

impl Protocol {
    /// Returns the protocol name as it appears in docs and
    /// capability probes.
    pub const fn as_str(self) -> &'static str {
        match self {
            Protocol::Kitty => "kitty",
            Protocol::Sixel => "sixel",
        }
    }
}

/// Errors produced by [`ProtocolEncoder::encode`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EncoderError {
    /// The requested protocol is not compiled into this build
    /// (e.g. calling `encode` on `Protocol::Kitty` without the
    /// `kitty-encoder` feature, or on `Protocol::Sixel` until the
    /// Sixel encoder lands in v0.6.0).
    UnsupportedProtocol(&'static str),

    /// The framebuffer has zero width or height and cannot be
    /// encoded.
    InvalidDimensions {
        /// Framebuffer width in pixels.
        width: u32,
        /// Framebuffer height in pixels.
        height: u32,
    },

    /// The underlying encoder crate failed; the wrapped `String`
    /// carries its `Display` output.
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

/// Encodes a [`FrameBuffer`] into the byte stream a terminal
/// expects for a chosen [`Protocol`].
///
/// Implementors return a `Vec<u8>` of escape sequences the caller
/// writes to stdout; the encoding does no I/O itself.
pub trait ProtocolEncoder {
    /// Encodes `frame` into escape-sequence bytes for `self`.
    fn encode(&self, frame: &FrameBuffer) -> Result<Vec<u8>, EncoderError>;
}

impl ProtocolEncoder for Protocol {
    fn encode(&self, frame: &FrameBuffer) -> Result<Vec<u8>, EncoderError> {
        match self {
            #[cfg(feature = "kitty-encoder")]
            Protocol::Kitty => kitty::encode(frame),
            #[cfg(not(feature = "kitty-encoder"))]
            Protocol::Kitty => Err(EncoderError::UnsupportedProtocol("kitty")),
            Protocol::Sixel => {
                let _ = frame;
                Err(EncoderError::UnsupportedProtocol("sixel"))
            },
        }
    }
}

/// The Kitty graphics protocol encoder, gated on the
/// `kitty-encoder` Cargo feature. Implemented as a private inline
/// module so the public API surface stays minimal.
#[cfg(feature = "kitty-encoder")]
mod kitty {
    use super::EncoderError;
    use crate::framebuffer::FrameBuffer;
    use little_kitty::command::ControlValue;
    use little_kitty::io::KittyCommandWriter;
    use std::io::Write;

    /// Convert a `std::io::Error` from the `little_kitty` writer
    /// into our [`EncoderError::Encode`]. Local helper so the
    /// `encode` function below stays readable.
    fn io_err(e: std::io::Error) -> EncoderError {
        EncoderError::Encode(e.to_string())
    }

    /// Encodes `frame` as a single Kitty "transmit and display"
    /// command using raw RGBA pixel data (format code 32 per the
    /// Kitty graphics protocol spec). The returned bytes are the
    /// full escape-sequence payload ready to be written to the
    /// terminal.
    pub fn encode(frame: &FrameBuffer) -> Result<Vec<u8>, EncoderError> {
        if frame.width() == 0 || frame.height() == 0 {
            return Err(EncoderError::InvalidDimensions {
                width: frame.width(),
                height: frame.height(),
            });
        }

        // Materialise the RGBA pixel data as a single contiguous
        // byte slice. A streaming encode can be added later if
        // the per-frame allocation becomes a hotspot.
        let rgba: Vec<u8> = frame.pixels().iter().flatten().copied().collect();

        // Build the control list. The Kitty graphics protocol
        // accepts a comma-separated list of key=value pairs
        // before the payload separator (`;`). We use:
        //   a=T   -- action: transmit and put (display)
        //   f=32  -- format: 32-bit RGBA
        //   q=2   -- quiet: suppress terminal OK/error responses
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
        out.write_start(false, None).map_err(io_err)?;
        for (i, (key, value)) in controls.iter().enumerate() {
            if i > 0 {
                out.write_all(b",").map_err(io_err)?;
            }
            write!(out, "{key}=").map_err(io_err)?;
            value.write(&mut out).map_err(io_err)?;
        }
        out.write_all(b";").map_err(io_err)?;
        out = out.write_base64(&rgba).map_err(io_err)?;
        out.write_end(false).map_err(io_err)?;
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::{EncoderError, Protocol, ProtocolEncoder};
    use crate::framebuffer::FrameBuffer;

    #[test]
    fn as_str_matches_variant() {
        assert_eq!(Protocol::Kitty.as_str(), "kitty");
        assert_eq!(Protocol::Sixel.as_str(), "sixel");
    }

    #[test]
    fn encoder_error_display_includes_context() {
        let e = EncoderError::UnsupportedProtocol("sixel");
        assert_eq!(e.to_string(), "protocol sixel is not supported in this build");

        let e = EncoderError::InvalidDimensions { width: 0, height: 5 };
        assert_eq!(e.to_string(), "framebuffer has invalid dimensions: 0x5");
    }

    #[test]
    fn sixel_encode_is_unsupported_in_v050() {
        // Sixel is the v0.6.0 work; the encoder must return
        // UnsupportedProtocol for any input, including a valid
        // framebuffer.
        let fb = FrameBuffer::new(2, 2);
        let err = Protocol::Sixel.encode(&fb).unwrap_err();
        assert_eq!(err, EncoderError::UnsupportedProtocol("sixel"));
    }

    #[cfg(not(feature = "kitty-encoder"))]
    #[test]
    fn kitty_encode_is_unsupported_without_feature() {
        // When the kitty-encoder feature is off, the Kitty arm
        // must return UnsupportedProtocol instead of producing
        // a (non-existent) encoder.
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
        // 2x2 fully-opaque red framebuffer.
        let mut fb = FrameBuffer::new(2, 2);
        for px in fb.pixels_mut() {
            *px = [255, 0, 0, 255];
        }
        let bytes = Protocol::Kitty.encode(&fb).unwrap();
        assert!(!bytes.is_empty(), "encoded output must not be empty");
        // The Kitty graphics protocol APC starts with ESC _G and
        // ends with ESC \\. See
        // https://sw.kovidgoyal.net/kitty/graphics-protocol/
        assert!(
            bytes.starts_with(b"\x1b_G"),
            "encoded output must start with the Kitty APC start (\\x1b_G), got: {:?}",
            &bytes[..bytes.len().min(8)],
        );
        assert!(
            bytes.ends_with(b"\x1b\\"),
            "encoded output must end with the Kitty APC terminator (\\x1b\\\\), got tail: {:?}",
            &bytes[bytes.len().saturating_sub(8)..],
        );
        // Decode the control payload (between the APC start and
        // the `;` separator) as UTF-8 and verify it contains the
        // expected keys and values for a 2x2 32-bit RGBA
        // transmit-and-display command.
        let s = std::str::from_utf8(&bytes).unwrap_or("");
        let payload_start = "\x1b_G".len();
        let payload_end = s.find(';').unwrap_or(s.len());
        let controls = &s[payload_start..payload_end];
        assert!(
            controls.contains("a=T"),
            "controls must include `a=T` (transmit and put), got: {controls:?}",
        );
        assert!(
            controls.contains("f=32"),
            "controls must include `f=32` (32-bit RGBA), got: {controls:?}",
        );
        assert!(
            controls.contains("q=2"),
            "controls must include `q=2` (suppress responses), got: {controls:?}",
        );
        assert!(
            controls.contains("s=2"),
            "controls must include `s=2` (width 2), got: {controls:?}",
        );
        assert!(
            controls.contains("v=2"),
            "controls must include `v=2` (height 2), got: {controls:?}",
        );
    }

    #[cfg(feature = "kitty-encoder")]
    #[test]
    fn kitty_encode_is_deterministic_for_same_input() {
        // Two calls with the same input must produce identical
        // bytes (the encoder is pure with respect to the frame).
        let mut fb = FrameBuffer::new(3, 3);
        for px in fb.pixels_mut() {
            *px = [10, 20, 30, 255];
        }
        let a = Protocol::Kitty.encode(&fb).unwrap();
        let b = Protocol::Kitty.encode(&fb).unwrap();
        assert_eq!(a, b);
    }
}
