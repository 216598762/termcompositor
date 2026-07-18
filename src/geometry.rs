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

/// An affine 2D transform for layers: rotation, scale, and anchor
/// point. Applied at render time via inverse mapping with bilinear
/// interpolation.
///
/// The transform is defined by:
/// - **Rotation**: clockwise rotation in degrees around the anchor point.
/// - **Scale**: (scale_x, scale_y) factors applied from the anchor point.
/// - **Anchor**: the (x, y) point in layer-local coordinates around
///   which rotation and scaling are applied. Defaults to the layer's
///   top-left corner (0, 0).
///
/// # Example
///
/// ```
/// use termcompositor::geometry::Transform;
///
/// // Rotate 45° around the center of a 100x100 layer.
/// let t = Transform::new()
///     .with_rotation(45.0)
///     .with_scale(1.5, 1.5)
///     .with_anchor(50.0, 50.0);
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Transform {
    /// Rotation in degrees (clockwise).
    rotation: f32,
    /// Horizontal scale factor.
    scale_x: f32,
    /// Vertical scale factor.
    scale_y: f32,
    /// Anchor X coordinate in layer-local space.
    anchor_x: f32,
    /// Anchor Y coordinate in layer-local space.
    anchor_y: f32,
}

impl Transform {
    /// Creates a new identity transform (no rotation, scale = 1.0,
    /// anchor at origin).
    pub const fn new() -> Self {
        Self {
            rotation: 0.0,
            scale_x: 1.0,
            scale_y: 1.0,
            anchor_x: 0.0,
            anchor_y: 0.0,
        }
    }

    /// Returns whether this transform is the identity (no rotation,
    /// scale = 1.0, anchor at origin).
    pub const fn is_identity(&self) -> bool {
        self.rotation == 0.0 && self.scale_x == 1.0 && self.scale_y == 1.0
    }

    /// Builder: sets the rotation in degrees (clockwise).
    #[must_use]
    pub fn with_rotation(mut self, degrees: f32) -> Self {
        self.rotation = degrees;
        self
    }

    /// Builder: sets scale factors.
    #[must_use]
    pub fn with_scale(mut self, sx: f32, sy: f32) -> Self {
        self.scale_x = sx;
        self.scale_y = sy;
        self
    }

    /// Builder: sets the anchor point in layer-local coordinates.
    /// Rotation and scaling are applied around this point.
    #[must_use]
    pub fn with_anchor(mut self, x: f32, y: f32) -> Self {
        self.anchor_x = x;
        self.anchor_y = y;
        self
    }

    /// Returns the rotation in degrees.
    pub const fn rotation(&self) -> f32 {
        self.rotation
    }

    /// Returns the horizontal scale factor.
    pub const fn scale_x(&self) -> f32 {
        self.scale_x
    }

    /// Returns the vertical scale factor.
    pub const fn scale_y(&self) -> f32 {
        self.scale_y
    }

    /// Returns the anchor X coordinate.
    pub const fn anchor_x(&self) -> f32 {
        self.anchor_x
    }

    /// Returns the anchor Y coordinate.
    pub const fn anchor_y(&self) -> f32 {
        self.anchor_y
    }

    /// Transforms a point from layer-local space to target space.
    /// Applies scale, then rotation, then translation to anchor.
    pub fn apply(&self, x: f32, y: f32) -> (f32, f32) {
        let dx = x - self.anchor_x;
        let dy = y - self.anchor_y;
        let sx = dx * self.scale_x;
        let sy = dy * self.scale_y;
        let rad = self.rotation.to_radians();
        let cos = rad.cos();
        let sin = rad.sin();
        let rx = sx * cos - sy * sin;
        let ry = sx * sin + sy * cos;
        (rx + self.anchor_x, ry + self.anchor_y)
    }

    /// Transforms a point from target space back to layer-local
    /// space (inverse mapping). Used for bilinear interpolation
    /// during rendering.
    pub fn apply_inverse(&self, x: f32, y: f32) -> (f32, f32) {
        let dx = x - self.anchor_x;
        let dy = y - self.anchor_y;
        let rad = self.rotation.to_radians();
        let cos = rad.cos();
        let sin = rad.sin();
        let rx = dx * cos + dy * sin;
        let ry = -dx * sin + dy * cos;
        let inv_sx = if self.scale_x.abs() > 1e-6 {
            1.0 / self.scale_x
        } else {
            0.0
        };
        let inv_sy = if self.scale_y.abs() > 1e-6 {
            1.0 / self.scale_y
        } else {
            0.0
        };
        (rx * inv_sx + self.anchor_x, ry * inv_sy + self.anchor_y)
    }
}

impl Default for Transform {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::{Rect, Transform};

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

    // ── Transform tests ───────────────────────────────────────

    #[test]
    fn transform_identity() {
        let t = Transform::new();
        assert!(t.is_identity());
        assert_eq!(t.rotation(), 0.0);
        assert_eq!(t.scale_x(), 1.0);
        assert_eq!(t.scale_y(), 1.0);
    }

    #[test]
    fn transform_builder_chain() {
        let t = Transform::new()
            .with_rotation(45.0)
            .with_scale(2.0, 3.0)
            .with_anchor(10.0, 20.0);
        assert_eq!(t.rotation(), 45.0);
        assert_eq!(t.scale_x(), 2.0);
        assert_eq!(t.scale_y(), 3.0);
        assert_eq!(t.anchor_x(), 10.0);
        assert_eq!(t.anchor_y(), 20.0);
        assert!(!t.is_identity());
    }

    #[test]
    fn transform_apply_identity() {
        let t = Transform::new();
        let (x, y) = t.apply(5.0, 10.0);
        assert!((x - 5.0).abs() < 1e-5);
        assert!((y - 10.0).abs() < 1e-5);
    }

    #[test]
    fn transform_apply_scale() {
        let t = Transform::new().with_scale(2.0, 3.0);
        let (x, y) = t.apply(5.0, 10.0);
        assert!((x - 10.0).abs() < 1e-5);
        assert!((y - 30.0).abs() < 1e-5);
    }

    #[test]
    fn transform_apply_rotation_90() {
        let t = Transform::new().with_rotation(90.0);
        let (x, y) = t.apply(1.0, 0.0);
        assert!((x - 0.0).abs() < 1e-5);
        assert!((y - 1.0).abs() < 1e-5);
    }

    #[test]
    fn transform_apply_inverse_roundtrip() {
        let t = Transform::new()
            .with_rotation(45.0)
            .with_scale(2.0, 3.0)
            .with_anchor(10.0, 20.0);
        let (x, y) = t.apply(5.0, 10.0);
        let (bx, by) = t.apply_inverse(x, y);
        assert!((bx - 5.0).abs() < 1e-5);
        assert!((by - 10.0).abs() < 1e-5);
    }

    #[test]
    fn transform_apply_with_anchor() {
        let t = Transform::new()
            .with_scale(2.0, 2.0)
            .with_anchor(10.0, 10.0);
        let (x, y) = t.apply(10.0, 10.0);
        assert!((x - 10.0).abs() < 1e-5);
        assert!((y - 10.0).abs() < 1e-5);
        let (x, y) = t.apply(15.0, 10.0);
        assert!((x - 20.0).abs() < 1e-5);
        assert!((y - 10.0).abs() < 1e-5);
    }

    #[test]
    fn transform_apply_rotation_around_anchor() {
        let t = Transform::new().with_rotation(90.0).with_anchor(0.0, 0.0);
        let (x, y) = t.apply(1.0, 0.0);
        assert!((x - 0.0).abs() < 1e-5);
        assert!((y - 1.0).abs() < 1e-5);
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
                prop_assert!(!a.is_empty());
                prop_assert!(!b.is_empty());
                prop_assert!(a.x < b.right());
                prop_assert!(b.x < a.right());
                prop_assert!(a.y < b.bottom());
                prop_assert!(b.y < a.bottom());
            }
        }

        #[test]
        fn prop_contains_top_left_corner(rect in arb_rect()) {
            if !rect.is_empty() {
                prop_assert!(rect.contains(rect.x, rect.y));
            }
        }

        #[test]
        fn prop_empty_rect_contains_nothing(rect in arb_rect()) {
            let empty = Rect::new(rect.x, rect.y, 0, 0);
            prop_assert!(!empty.contains(rect.x, rect.y));
        }

        #[test]
        fn prop_self_intersects(rect in arb_rect()) {
            if !rect.is_empty() {
                prop_assert!(rect.intersects(&rect));
            }
        }
    }
}
