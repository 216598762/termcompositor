//! `dashcompositor` -- layer-based graphics compositor for the
//! terminal.
//!
//! See [`AGENTS.md`](../AGENTS.md) and the
//! [README](../README.md) for project rules and the target
//! architecture.

pub mod compositor;
pub mod encoder;
pub mod framebuffer;
pub mod geometry;
pub mod layer;
pub mod terminal;

pub use compositor::{Compositor, CpuCompositor, LayerStack};
pub use encoder::Protocol;
pub use framebuffer::{blend_over, FrameBuffer};
pub use geometry::Rect;
pub use layer::{Layer, LayerEntry, LayerId, RectLayer, SolidColor, TextLayer};
#[cfg(feature = "image-decoder")]
pub use layer::ImageLayer;
pub use terminal::TerminalSize;

// Re-export the gated `ImageLayer` only when the feature is on; the
// other layer types are always available.

#[cfg(test)]
mod tests {
    use super::{FrameBuffer, LayerStack, RectLayer, SolidColor, TextLayer};

    #[test]
    fn empty_framebuffer_is_zero_sized_pixels() {
        let fb = FrameBuffer::new(2, 3);
        assert_eq!(fb.width(), 2);
        assert_eq!(fb.height(), 3);
        assert_eq!(fb.pixels().len(), 6);
    }

    #[test]
    fn end_to_end_add_remove_control_render() {
        let mut stack = LayerStack::new();
        let bg = stack.push(SolidColor::new(0, 0, 0, 255).with_name("bg"));
        let fg = stack.push(RectLayer::new(10, 5, 5, 5, [255, 255, 255, 255]).with_z(10));

        // Control: fade and hide.
        stack.get_mut(fg).unwrap().set_opacity(0.25);
        stack.get_mut(bg).unwrap().set_visible(false);

        // Remove and re-add.
        assert!(stack.remove(bg).is_some());
        let accent = stack.push(SolidColor::new(255, 0, 0, 255));
        stack.get_mut(accent).unwrap().set_z_override(99);

        // Render.
        let mut fb = FrameBuffer::new(20, 10);
        stack.render(&mut fb);
        // The accent (red, full alpha) covers everything because
        // it's on top and the rect is faded out.
        assert_eq!(fb.pixels()[0][0], 255);
    }

    #[test]
    fn end_to_end_rect_and_text_layers() {
        let mut stack = LayerStack::new();
        stack.push(SolidColor::new(0, 0, 0, 255));
        let id_rect = stack.push(RectLayer::new(2, 2, 3, 3, [0, 255, 0, 255]).with_z(10));
        let id_text = stack.push(TextLayer::new(0, 0, "hi", [255, 255, 255, 255]).with_z(20));
        let mut fb = FrameBuffer::new(10, 10);
        stack.render(&mut fb);
        // The green rect's center pixel is green; the text was
        // drawn at (0, 0) with the white placeholder block.
        assert_eq!(fb.get_pixel(3, 3), Some(&[0, 255, 0, 255]));
        assert_eq!(fb.get_pixel(0, 0), Some(&[255, 255, 255, 255]));
        assert_eq!(fb.get_pixel(1, 0), Some(&[255, 255, 255, 255]));
        let _ = (id_rect, id_text);
    }
}
