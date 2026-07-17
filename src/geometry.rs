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

    // ── proptest property-based tests ──────────────────────────

    use proptest::prelude::*;

    fn arb_rect() -> impl Strategy<Value = Rect> {
        (0u32..1000, 0u32..1000, 0u32..100, 0u32..100)
            .prop_map(|(x, y, w, h)| Rect::new(x, y, w, h))
    }

    proptest! {
        #[test]
        fn prop_empty_rect_never_intersects(a in arb_rect(), b in arb_rect()) {
            let za = Rect::new(a.x, a.y, 0, a.height);
            let zb = Rect::new(b.x, b.y, b.width, 0);
            prop_assert!(!a.intersects(&za));
            prop_assert!(!a.intersects(&zb));
            prop_assert!(!za.intersects(&a));
            prop_assert!(!zb.intersects(&a));
        }

        #[test]
        fn prop_intersects_is_symmetric(a in arb_rect(), b in arb_rect()) {
            prop_assert_eq!(a.intersects(&b), b.intersects(&a));
        }

        #[test]
        fn prop_intersects_implies_overlapping_bounds(a in arb_rect(), b in arb_rect()) {
            if a.intersects(&b) {
                // Both must be non-empty.
                prop_assert!(!a.is_empty());
                prop_assert!(!b.is_empty());
                // X-axis overlap: a.x < b.right() && b.x < a.right()
                prop_assert!(a.x < b.right(), "a.x={} >= b.right={}", a.x, b.right());
                prop_assert!(b.x < a.right(), "b.x={} >= a.right={}", b.x, a.right());
                // Y-axis overlap: a.y < b.bottom() && b.y < a.bottom()
                prop_assert!(a.y < b.bottom(), "a.y={} >= b.bottom={}", a.y, b.bottom());
                prop_assert!(b.y < a.bottom(), "b.y={} >= a.bottom={}", b.y, a.bottom());
            }
        }

        #[test]
        fn prop_contains_point_inside_implies_bounds(rect in arb_rect(), px in 0u32..1000, py in 0u32..1000) {
            if rect.contains(px, py) {
                prop_assert!(!rect.is_empty());
                prop_assert!(px >= rect.x, "px={} < rect.x={}", px, rect.x);
                prop_assert!(px < rect.right(), "px={} >= rect.right={}", px, rect.right());
                prop_assert!(py >= rect.y, "py={} < rect.y={}", py, rect.y);
                prop_assert!(py < rect.bottom(), "py={} >= rect.bottom={}", py, rect.bottom());
            }
        }

        #[test]
        fn prop_contains_top_left_corner(rect in arb_rect()) {
            if !rect.is_empty() {
                prop_assert!(rect.contains(rect.x, rect.y));
            }
        }

        #[test]
        fn prop_contains_excludes_right_bottom(rect in arb_rect()) {
            if !rect.is_empty() {
                prop_assert!(!rect.contains(rect.right(), rect.y));
                prop_assert!(!rect.contains(rect.x, rect.bottom()));
                prop_assert!(!rect.contains(rect.right(), rect.bottom()));
            }
        }

        #[test]
        fn prop_empty_rect_contains_nothing(rect in arb_rect()) {
            let empty = Rect::new(rect.x, rect.y, 0, 0);
            prop_assert!(!empty.contains(rect.x, rect.y));
            prop_assert!(!empty.contains(0, 0));
        }

        #[test]
        fn prop_self_intersects(rect in arb_rect()) {
            if !rect.is_empty() {
                prop_assert!(rect.intersects(&rect), "non-empty rect must intersect itself");
            }
        }
    }
}
