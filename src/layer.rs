//! Layer — a single compositable layer in the compositor.
//!
//! Concrete [`Layer`] implementations will include raster images, text
//! glyphs, vector shapes, and sprites. Each layer is keyed by a
//! [`Layer::z_order`] value: higher draws later (on top).

/// A single layer that can be drawn into a [`crate::FrameBuffer`].
pub trait Layer {
    /// Returns the z-order of this layer. Higher values are drawn later
    /// (on top); ties resolve via implementation-defined rules.
    /// Conceptually non-negative, hence [`u32`].
    fn z_order(&self) -> u32;
}
