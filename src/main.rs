//! `dashcompositor` CLI — scaffold entry point.
//!
//! The current binary deliberately does nothing more than confirm the
//! crate builds end-to-end. Future work will:
//!
//! 1. Detect host terminal capabilities (per `AGENTS.md` §7).
//! 2. Compose a stack of [`dashcompositor::Layer`]s into a
//!    [`dashcompositor::FrameBuffer`].
//! 3. Encode the framebuffer via the chosen
//!    [`dashcompositor::Protocol`] and write the escape sequence to
//!    stdout.

fn main() {
    // Scaffold placeholder. No I/O yet.
}
