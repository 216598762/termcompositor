//! `dashcompositor` — layer-based graphics compositor for the terminal.
//!
//! See [`AGENTS.md`](../AGENTS.md) and the [README](../README.md) for
//! project rules and the target architecture.

pub mod compositor;
pub mod encoder;
pub mod framebuffer;
pub mod layer;

pub use compositor::Compositor;
pub use encoder::Protocol;
pub use framebuffer::FrameBuffer;
pub use layer::Layer;

#[cfg(test)]
mod tests {
    use super::FrameBuffer;

    #[test]
    fn empty_framebuffer_is_zero_sized_pixels() {
        let fb = FrameBuffer::new(2, 3);
        assert_eq!(fb.width(), 2);
        assert_eq!(fb.height(), 3);
        assert_eq!(fb.pixels().len(), 6);
    }
}
