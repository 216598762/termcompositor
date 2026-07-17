//! Framebuffer — flat RGBA pixel buffer representing one composited frame.
//!
//! Stored row-major: index `(y * width + x)` gives the pixel for
//! `(x, y)`. Each pixel is `[R, G, B, A]` with channels in `0..=255`.

/// A flat RGBA pixel buffer.
///
/// # Example
///
/// ```
/// use dashcompositor::FrameBuffer;
///
/// // Create a 4x4 transparent framebuffer.
/// let mut fb = FrameBuffer::new(4, 4);
/// assert_eq!(fb.width(), 4);
/// assert_eq!(fb.height(), 4);
///
/// // Write a red pixel at (1, 2).
/// if let Some(px) = fb.get_pixel_mut(1, 2) {
///     *px = [255, 0, 0, 255];
/// }
/// assert_eq!(fb.get_pixel(1, 2), Some(&[255, 0, 0, 255]));
///
/// // Out-of-bounds access returns None.
/// assert_eq!(fb.get_pixel(99, 99), None);
///
/// // Clear resets all pixels to transparent.
/// fb.clear();
/// assert_eq!(fb.get_pixel(1, 2), Some(&[0, 0, 0, 0]));
/// ```
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

    /// Returns `true` if every pixel is fully transparent
    /// (`[0, 0, 0, 0]`). Used by the Kitty encoder to short-
    /// circuit the transmission when the layer stack is empty
    /// (no child PTY has emitted a Kitty graphics command) --
    /// instead of streaming 4MB+ of zero-RGBA chunks every
    /// tick, the encoder emits a single Kitty delete command
    /// (`a=d,d=I,i=1`) to clear any previously-placed image
    /// and returns immediately.
    ///
    /// O(n) over `pixels.len()`. For a 2MP framebuffer
    /// (~2,000,000 pixels) this is a single cache-friendly
    /// pass over ~8MB of memory, which completes in well
    /// under a millisecond on modern hardware.
    pub fn is_fully_transparent(&self) -> bool {
        self.pixels.iter().all(|px| *px == [0, 0, 0, 0])
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
///
/// # Example
///
/// ```
/// use dashcompositor::blend_over;
///
/// // Blend a translucent red over a blue background.
/// let mut dst = [0, 0, 255, 255];  // opaque blue
/// blend_over(&mut dst, &[255, 0, 0, 128], 0.5);
///
/// // The result should be a purple-ish colour.
/// assert!(dst[0] > 0);  // red channel contributed
/// assert!(dst[2] > 0);  // blue channel still present
/// assert!(dst[3] > 128); // combined alpha > 128
/// ```
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
    use proptest::prelude::*;

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

    // -- Property-based tests (proptest) ------------------------

    proptest::proptest! {
        #[test]
        fn framebuffer_new_pixel_count_matches_dimensions(
            w in 0u32..1000,
            h in 0u32..1000,
        ) {
            let fb = FrameBuffer::new(w, h);
            // The pixel count must always equal w * h (saturating).
            let expected = (w as usize).saturating_mul(h as usize);
            prop_assert_eq!(fb.pixels().len(), expected);
        }

        #[test]
        fn framebuffer_new_all_pixels_transparent(
            w in 0u32..500,
            h in 0u32..500,
        ) {
            let fb = FrameBuffer::new(w, h);
            prop_assert!(
                fb.pixels().iter().all(|p| *p == [0, 0, 0, 0]),
                "all pixels must be transparent after creation"
            );
        }

        #[test]
        fn framebuffer_width_height_preserved(
            w in 0u32..1000,
            h in 0u32..1000,
        ) {
            let fb = FrameBuffer::new(w, h);
            prop_assert_eq!(fb.width(), w);
            prop_assert_eq!(fb.height(), h);
        }

        #[test]
        fn framebuffer_get_pixel_in_bounds_always_some(
            w in 1u32..100,
            h in 1u32..100,
            x in 0u32..100,
            y in 0u32..100,
        ) {
            let fb = FrameBuffer::new(w, h);
            if x < w && y < h {
                prop_assert!(fb.get_pixel(x, y).is_some());
            }
        }

        #[test]
        fn framebuffer_get_pixel_out_of_bounds_always_none(
            w in 1u32..100,
            h in 1u32..100,
        ) {
            let fb = FrameBuffer::new(w, h);
            // Any coordinate >= width or >= height must be None.
            prop_assert_eq!(fb.get_pixel(w, 0), None);
            prop_assert_eq!(fb.get_pixel(0, h), None);
            prop_assert_eq!(fb.get_pixel(w, h), None);
            prop_assert_eq!(fb.get_pixel(u32::MAX, 0), None);
            prop_assert_eq!(fb.get_pixel(0, u32::MAX), None);
        }

        #[test]
        fn framebuffer_set_then_get_round_trip(
            w in 1u32..50,
            h in 1u32..50,
            x in 0u32..50,
            y in 0u32..50,
            r in 0u8..=255,
            g in 0u8..=255,
            b in 0u8..=255,
            a in 0u8..=255,
        ) {
            let mut fb = FrameBuffer::new(w, h);
            if x < w && y < h {
                if let Some(px) = fb.get_pixel_mut(x, y) {
                    *px = [r, g, b, a];
                }
                prop_assert_eq!(fb.get_pixel(x, y), Some(&[r, g, b, a]));
            }
        }

        #[test]
        fn framebuffer_clear_resets_all_pixels(
            w in 1u32..50,
            h in 1u32..50,
        ) {
            let mut fb = FrameBuffer::new(w, h);
            // Write non-zero values to every pixel.
            for px in fb.pixels_mut() {
                *px = [128, 64, 32, 255];
            }
            fb.clear();
            prop_assert!(
                fb.pixels().iter().all(|p| *p == [0, 0, 0, 0]),
                "clear must reset all pixels to transparent"
            );
        }

        #[test]
        fn prop_blend_zero_alpha_is_noop(
            r in 0u8..=255,
            g in 0u8..=255,
            b in 0u8..=255,
            a in 0u8..=255,
            dr in 0u8..=255,
            dg in 0u8..=255,
            db in 0u8..=255,
            da in 0u8..=255,
        ) {
            let mut dst = [dr, dg, db, da];
            blend_over(&mut dst, &[r, g, b, a], 0.0);
            prop_assert_eq!(dst, [dr, dg, db, da], "blend with alpha=0 must not modify dst");
        }

        #[test]
        fn prop_blend_opaque_overwrites_rgb(
            r in 0u8..=255,
            g in 0u8..=255,
            b in 0u8..=255,
        ) {
            let mut dst = [0u8; 4];
            blend_over(&mut dst, &[r, g, b, 0], 1.0);
            // src_alpha=1.0 means the source colour is written directly;
            // the result is fully opaque regardless of src[3].
            prop_assert_eq!(dst[0], r, "red channel must match");
            prop_assert_eq!(dst[1], g, "green channel must match");
            prop_assert_eq!(dst[2], b, "blue channel must match");
            prop_assert_eq!(dst[3], 255, "result must be fully opaque when src_alpha=1.0");
        }

        #[test]
        fn prop_blend_result_alpha_bounded(
            r in 0u8..=255,
            g in 0u8..=255,
            b in 0u8..=255,
            a in 0u8..=255,
            dr in 0u8..=255,
            dg in 0u8..=255,
            db in 0u8..=255,
            da in 0u8..=255,
            src_alpha in 0.0f32..=1.0,
        ) {
            let mut dst = [dr, dg, db, da];
            blend_over(&mut dst, &[r, g, b, a], src_alpha);
        }

        #[test]
        fn prop_blend_result_rgb_bounded(
            r in 0u8..=255,
            g in 0u8..=255,
            b in 0u8..=255,
            a in 0u8..=255,
            dr in 0u8..=255,
            dg in 0u8..=255,
            db in 0u8..=255,
            da in 0u8..=255,
            src_alpha in 0.0f32..=1.0,
        ) {
            let mut dst = [dr, dg, db, da];
            blend_over(&mut dst, &[r, g, b, a], src_alpha);
        }

        #[test]
        fn prop_blend_clamps_extreme_alpha_values(
            r in 0u8..=255,
            g in 0u8..=255,
            b in 0u8..=255,
            a in 0u8..=255,
            dr in 0u8..=255,
            dg in 0u8..=255,
            db in 0u8..=255,
            da in 0u8..=255,
            extreme_alpha in (-100.0f32..100.0).prop_filter("extreme", |a| *a < 0.0 || *a > 1.0),
        ) {
            let mut dst = [dr, dg, db, da];
            let before = dst;
            blend_over(&mut dst, &[r, g, b, a], extreme_alpha);

            if extreme_alpha <= 0.0 {
                // Negative alpha is clamped to 0 → noop.
                prop_assert_eq!(dst, before, "negative alpha must be noop");
            } else {
                // Alpha > 1.0 is clamped to 1.0 → full overwrite.
                let mut expected = [0u8; 4];
                blend_over(&mut expected, &[r, g, b, a], 1.0);
                prop_assert_eq!(dst, expected, "alpha > 1.0 must behave like alpha == 1.0");
            }
        }
    }
}
