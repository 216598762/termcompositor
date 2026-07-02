//! Geometry primitives shared by the compositor, layers, and any
//! future region/clip/transform code. Lives in its own module so it
//! can be reused without dragging in framebuffer or layer types.

/// An axis-aligned rectangle in 2-D integer space. Used by
/// [`crate::layer::Layer::bounds`] and anywhere else a region needs
/// to be described (clip regions, hit-test rectangles, etc.).
///
/// Coordinates are in cells or pixels: the unit depends on the
/// caller. `width` and `height` are non-negative, and a `Rect` with
/// `width == 0 || height == 0` is considered empty and
/// intersects nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Rect {
    /// Left edge, inclusive.
    pub x: u32,
    /// Top edge, inclusive.
    pub y: u32,
    /// Width in cells/pixels. `0` means empty.
    pub width: u32,
    /// Height in cells/pixels. `0` means empty.
    pub height: u32,
}

impl Rect {
    /// Creates a new rectangle.
    #[inline]
    pub const fn new(x: u32, y: u32, width: u32, height: u32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    /// Returns the exclusive right edge (`x + width`). Saturates at
    /// `u32::MAX` if the addition would overflow.
    #[inline]
    pub const fn right(&self) -> u32 {
        self.x.saturating_add(self.width)
    }

    /// Returns the exclusive bottom edge (`y + height`). Saturates
    /// at `u32::MAX` if the addition would overflow.
    #[inline]
    pub const fn bottom(&self) -> u32 {
        self.y.saturating_add(self.height)
    }

    /// Returns whether the rectangle is empty (zero area).
    #[inline]
    pub const fn is_empty(&self) -> bool {
        self.width == 0 || self.height == 0
    }

    /// Returns whether `(x, y)` is contained in this rectangle
    /// (inclusive of the left/top edge, exclusive of the
    /// right/bottom).
    #[inline]
    pub const fn contains(&self, x: u32, y: u32) -> bool {
        !self.is_empty() && x >= self.x && x < self.right() && y >= self.y && y < self.bottom()
    }

    /// Returns whether this rectangle and `other` overlap by at
    /// least one cell/pixel. Two empty rectangles never intersect.
    pub fn intersects(&self, other: &Rect) -> bool {
        if self.is_empty() || other.is_empty() {
            return false;
        }
        self.x < other.right()
            && other.x < self.right()
            && self.y < other.bottom()
            && other.y < self.bottom()
    }
}

#[cfg(test)]
mod tests {
    use super::Rect;

    #[test]
    fn new_stores_fields() {
        let r = Rect::new(1, 2, 3, 4);
        assert_eq!(r.x, 1);
        assert_eq!(r.y, 2);
        assert_eq!(r.width, 3);
        assert_eq!(r.height, 4);
        assert_eq!(r.right(), 4);
        assert_eq!(r.bottom(), 6);
    }

    #[test]
    fn empty_is_zero_width_or_height() {
        assert!(Rect::new(0, 0, 0, 5).is_empty());
        assert!(Rect::new(0, 0, 5, 0).is_empty());
        assert!(!Rect::new(0, 0, 5, 5).is_empty());
    }

    #[test]
    fn contains_is_inclusive_left_top_exclusive_right_bottom() {
        let r = Rect::new(10, 10, 5, 5);
        assert!(r.contains(10, 10));
        assert!(r.contains(14, 14));
        assert!(!r.contains(15, 14));
        assert!(!r.contains(14, 15));
        assert!(!r.contains(9, 10));
    }

    #[test]
    fn intersects_overlap_and_disjoint() {
        let a = Rect::new(0, 0, 10, 10);
        let b = Rect::new(5, 5, 10, 10);
        let c = Rect::new(20, 20, 5, 5);
        assert!(a.intersects(&b));
        assert!(b.intersects(&a));
        assert!(!a.intersects(&c));
    }

    #[test]
    fn empty_never_intersects() {
        let a = Rect::new(0, 0, 10, 10);
        let z = Rect::new(0, 0, 0, 0);
        assert!(!a.intersects(&z));
        assert!(!z.intersects(&a));
    }
}
