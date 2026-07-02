//! Framebuffer — flat RGBA pixel buffer representing one composited frame.
//!
//! Stored row-major: index `(y * width + x) * 4` gives the start of the
//! RGBA tuple for pixel `(x, y)`. Each channel is `0..=255`.

/// A flat RGBA pixel buffer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrameBuffer {
    width: u32,
    height: u32,
    /// Pixel data in row-major order; each entry is `[R, G, B, A]`.
    pixels: Vec<[u8; 4]>,
}

impl FrameBuffer {
    /// Creates a new fully transparent framebuffer of the given
    /// dimensions. Uses saturating multiplication to avoid overflow on
    /// absurd input; the resulting buffer is clamped to
    /// `width.saturating_mul(height)` entries.
    pub fn new(width: u32, height: u32) -> Self {
        let count = (width as usize).saturating_mul(height as usize);
        Self {
            width,
            height,
            pixels: vec![[0, 0, 0, 0]; count],
        }
    }

    /// Returns the framebuffer width in pixels.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Returns the framebuffer height in pixels.
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Returns the underlying RGBA pixel slice, row-major.
    pub fn pixels(&self) -> &[[u8; 4]] {
        &self.pixels
    }

    /// Returns the mutable RGBA pixel slice, row-major.
    pub fn pixels_mut(&mut self) -> &mut [[u8; 4]] {
        &mut self.pixels
    }
}
