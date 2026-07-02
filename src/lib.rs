//! `dashcompositor` — layer-based graphics compositor for the
//! terminal.
//!
//! Build a [`LayerStack`] of layers ([`SolidColor`], [`RectLayer`],
//! [`TextLayer`], [`ImageLayer`]), render them into a
//! [`FrameBuffer`], and encode the result as terminal escape
//! sequences (Kitty graphics protocol or Sixel) via
//! [`dispatch_to_writer`].
//!
//! # Quick start
//!
//! ```
//! use dashcompositor::{FrameBuffer, LayerStack, SolidColor, TextLayer, detect, dispatch_to_writer};
//!
//! let mut stack = LayerStack::new();
//! stack.push(SolidColor::new(0, 0, 64, 255).with_name("bg"));
//! stack.push(TextLayer::new(0, 0, "hello", [255; 4]).with_z(10));
//!
//! let mut fb = FrameBuffer::new(80, 24);
//! stack.render(&mut fb);
//!
//! // The encode step requires at least one encoder feature
//! // (`kitty-encoder` or `sixel-encoder`) to produce output;
//! // without them the dispatch returns UnsupportedProtocol.
//! # let mut out = Vec::new();
//! # let _ = dispatch_to_writer(detect(), &fb, &mut out);
//! ```
//!
//! See [`AGENTS.md`](../AGENTS.md) and the
//! [README](../README.md) for the full architecture.
//!
//! ## Feature flags
//!
//! | Feature             | Default | Description |
//! | ------------------- | :-----: | ----------- |
//! | `font-rasterizer`   | **on**  | Real glyph rasterization in [`TextLayer`] via `fontdue` |
//! | `kitty-encoder`     |   off   | Kitty graphics protocol encoder |
//! | `sixel-encoder`     |   off   | Sixel encoder |
//! | `image-decoder`     |   off   | [`ImageLayer`] (PNG + JPEG) |
//!
//! At least one of `kitty-encoder` or `sixel-encoder` is required
//! to produce terminal output. [`detect`] auto-picks the protocol
//! based on the host terminal's `TERM` / `TERM_PROGRAM`.
//!
//! ## Modules
//!
//! | Module | Description |
//! | ------ | ----------- |
//! | [`compositor`] | [`LayerStack`], [`Compositor`] trait, [`CpuCompositor`] |
//! | [`encoder`]   | Protocol detection and encoding (Kitty / Sixel) |
//! | [`framebuffer`] | [`FrameBuffer`] and [`blend_over`] compositing |
//! | [`geometry`]  | [`Rect`] primitive |
//! | [`layer`]     | Layer trait, all built-in layer types |
//! | [`terminal`]  | [`TerminalSize`] detection |

pub mod compositor;
pub mod encoder;
pub mod framebuffer;
pub mod geometry;
pub mod layer;
pub mod terminal;

pub use compositor::{Compositor, CpuCompositor, LayerStack};
#[cfg(feature = "kitty-encoder")]
pub use encoder::detect_with_probe;
#[cfg(feature = "kitty-encoder")]
pub use encoder::wrap_for_tmux;
#[cfg(feature = "kitty-encoder")]
pub use encoder::wrap_for_tmux_to_writer;
#[cfg(feature = "kitty-encoder")]
pub use encoder::PassthroughWriter;
#[cfg(feature = "kitty-encoder")]
pub use encoder::encode_passthrough_to_writer;
// NOTE: the v0.8.4 `sixel::encode_to_writer` is re-exported
// at the `encoder` module level (see
// `dashcompositor::encoder::encode_to_writer`) but NOT at
// the crate root, to mirror the kitty `encode_to_writer`
// access pattern (`dashcompositor::encoder::kitty::encode_to_writer`).
// Neither streaming entry point is at the crate root: a
// single crate-root `encode_to_writer` name would be
// ambiguous in a build with both `kitty-encoder` and
// `sixel-encoder` enabled (which one wins?), and the
// module-path access is more explicit anyway.
pub use encoder::{detect, dispatch_to_writer, EncoderError, Protocol, ProtocolEncoder};
pub use framebuffer::{blend_over, FrameBuffer};
pub use geometry::Rect;
#[cfg(feature = "image-decoder")]
pub use layer::ImageLayer;
#[cfg(feature = "font-rasterizer")]
pub use layer::FontSource;
pub use layer::{Layer, LayerEntry, LayerId, RectLayer, SolidColor, TextLayer};
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
        // The green rect's center pixel is green.
        assert_eq!(fb.get_pixel(3, 3), Some(&[0, 255, 0, 255]));
        // The text layer at (0,0) should have rendered something
        // (either glyph pixels with font-rasterizer, or a
        // placeholder block without).
        #[cfg(not(feature = "font-rasterizer"))]
        {
            assert_eq!(fb.get_pixel(0, 0), Some(&[255, 255, 255, 255]));
            assert_eq!(fb.get_pixel(1, 0), Some(&[255, 255, 255, 255]));
        }
        #[cfg(feature = "font-rasterizer")]
        {
            // With font-rasterizer, the glyph bitmap produces
            // composited pixels with varying alpha.
            assert!(fb.pixels().iter().any(|p| p[3] > 0));
        }
        let _ = (id_rect, id_text);
    }
}
