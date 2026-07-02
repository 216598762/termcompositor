//! Compositor — combines a stack of layers into a single [`FrameBuffer`].
//!
//! Concrete CPU (`tiny-skia`) and GPU (`wgpu`) compositors will live behind
//! the [`Compositor`] trait once each candidate crate has been evaluated
//! per `AGENTS.md` §3.

use crate::framebuffer::FrameBuffer;

/// A compositor that resolves a stack of layers into a [`FrameBuffer`].
pub trait Compositor {
    /// Renders layers into `target`. Pure operation — does not mutate
    /// compositor state. Implementations decide blending order and alpha
    /// rules; see `AGENTS.md` §7 for the target architecture.
    fn compose(&self, target: &mut FrameBuffer);
}
