//! Output protocol selection — Kitty graphics protocol or Sixel.
//!
//! The runtime will pick between these based on terminal capability
//! detection (via `TERM`, `TERM_PROGRAM`, `COLORTERM`) per `AGENTS.md` §7,
//! preferring [`Protocol::Kitty`] when the host supports it and falling
//! back to [`Protocol::Sixel`] otherwise.
/// Terminal graphics protocol used to encode the composited framebuffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protocol {
    /// The kitty graphics protocol — modern and feature-rich.
    Kitty,
    /// Sixel — fallback for terminals without kitty support.
    Sixel,
}

impl Protocol {
    /// Returns the protocol name as it appears in docs and capability
    /// probes.
    pub const fn as_str(self) -> &'static str {
        match self {
            Protocol::Kitty => "kitty",
            Protocol::Sixel => "sixel",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Protocol;

    #[test]
    fn as_str_matches_variant() {
        assert_eq!(Protocol::Kitty.as_str(), "kitty");
        assert_eq!(Protocol::Sixel.as_str(), "sixel");
    }
}
