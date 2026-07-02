//! Framebuffer — flat RGBA pixel buffer representing one composited frame.
//!
//! Stored row-major: index `(y * width + x)` gives the pixel for
//! `(x, y)`. Each pixel is `[R, G, B, A]` with channels in `0..=255`.

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
    /// dimensions. Uses saturating multiplication to avoid overflow
    /// on absurd input; the resulting buffer is clamped to
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

    /// Resets every pixel to fully transparent (`[0, 0, 0, 0]`).
    pub fn clear(&mut self) {
        for px in &mut self.pixels {
            *px = [0, 0, 0, 0];
        }
    }

    /// Returns a shared reference to the pixel at `(x, y)`, or
    /// `None` if `(x, y)` is outside the framebuffer. Free
    /// bounds-checking for layers that may draw partially
    /// off-screen.
    pub fn get_pixel(&self, x: u32, y: u32) -> Option<&[u8; 4]> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let idx = (y as usize) * (self.width as usize) + (x as usize);
        self.pixels.get(idx)
    }

    /// Returns a mutable reference to the pixel at `(x, y)`, or
    /// `None` if `(x, y)` is outside the framebuffer.
    pub fn get_pixel_mut(&mut self, x: u32, y: u32) -> Option<&mut [u8; 4]> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let idx = (y as usize) * (self.width as usize) + (x as usize);
        self.pixels.get_mut(idx)
    }
}

/// Blends `src` (straight RGBA) over `dst` (in place) using the
/// given `src_alpha` (in `0.0..=1.0`).
///
/// Uses standard over-compositing math in straight
/// (non-premultiplied) alpha. Both inputs and the destination are
/// non-premultiplied; the math handles the premultiplied-equivalent
/// operations internally.
///
/// `src_alpha` is clamped to `0.0..=1.0`. A `src_alpha` of `0.0`
/// is a no-op; a `src_alpha` of `1.0` writes the source colour
/// directly (overwriting the destination's RGB and alpha).
pub fn blend_over(dst: &mut [u8; 4], src: &[u8; 4], src_alpha: f32) {
    let a = src_alpha.clamp(0.0, 1.0);
    if a == 0.0 {
        return;
    }
    let dst_a = f32::from(dst[3]) / 255.0;
    let out_a = a + dst_a * (1.0 - a);
    if out_a == 0.0 {
        return;
    }
    for i in 0..3 {
        let s = f32::from(src[i]);
        let d = f32::from(dst[i]);
        let out_c = (s * a + d * dst_a * (1.0 - a)) / out_a;
        dst[i] = out_c.round().clamp(0.0, 255.0) as u8;
    }
    dst[3] = (out_a * 255.0).round().clamp(0.0, 255.0) as u8;
}

#[cfg(test)]
mod tests {
    use super::{blend_over, FrameBuffer};

    #[test]
    fn empty_framebuffer_is_zero_sized_pixels() {
        let fb = FrameBuffer::new(2, 3);
        assert_eq!(fb.width(), 2);
        assert_eq!(fb.height(), 3);
        assert_eq!(fb.pixels().len(), 6);
        assert!(fb.pixels().iter().all(|p| *p == [0, 0, 0, 0]));
    }

    #[test]
    fn clear_resets_to_transparent() {
        let mut fb = FrameBuffer::new(2, 2);
        fb.pixels_mut()[0] = [10, 20, 30, 40];
        fb.clear();
        assert!(fb.pixels().iter().all(|p| *p == [0, 0, 0, 0]));
    }

    #[test]
    fn blend_opaque_overwrites() {
        let mut dst = [10, 20, 30, 40];
        blend_over(&mut dst, &[200, 100, 50, 255], 1.0);
        assert_eq!(dst, [200, 100, 50, 255]);
    }

    #[test]
    fn blend_zero_alpha_is_noop() {
        let mut dst = [10, 20, 30, 40];
        blend_over(&mut dst, &[200, 100, 50, 255], 0.0);
        assert_eq!(dst, [10, 20, 30, 40]);
    }

    #[test]
    fn blend_translucent_preserves_alpha() {
        let mut dst = [0, 0, 0, 128];
        blend_over(&mut dst, &[255, 0, 0, 128], 0.5);
        assert!((dst[3] as i32 - 191).abs() <= 1, "alpha = {}", dst[3]);
    }

    #[test]
    fn get_pixel_in_bounds_round_trip() {
        let mut fb = FrameBuffer::new(3, 2);
        if let Some(px) = fb.get_pixel_mut(2, 1) {
            *px = [1, 2, 3, 4];
        }
        assert_eq!(fb.get_pixel(2, 1), Some(&[1, 2, 3, 4]));
    }

    #[test]
    fn get_pixel_out_of_bounds_returns_none() {
        let fb = FrameBuffer::new(2, 2);
        assert_eq!(fb.get_pixel(2, 0), None);
        assert_eq!(fb.get_pixel(0, 2), None);
        assert_eq!(fb.get_pixel(u32::MAX, 0), None);
    }

    #[test]
    fn get_pixel_mut_writes_back() {
        let mut fb = FrameBuffer::new(2, 2);
        if let Some(px) = fb.get_pixel_mut(1, 1) {
            *px = [9, 8, 7, 6];
        }
        assert_eq!(fb.get_pixel(1, 1), Some(&[9, 8, 7, 6]));
    }
}
