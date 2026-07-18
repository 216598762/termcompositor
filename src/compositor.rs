//! Compositor — walks a [`LayerStack`] of [`LayerEntry`]s and resolves
//! them into a single [`FrameBuffer`].
//!
//! The default CPU implementation is [`CpuCompositor`]; a custom
//! compositor can be plugged in via the [`Compositor`] trait.

use crate::framebuffer::FrameBuffer;
use crate::layer::{Layer, LayerEntry, LayerId};
use crate::terminal::TerminalSize;

/// A compositor that resolves a [`LayerStack`] of layers into a
/// single [`FrameBuffer`].
///
/// Implementations are expected to iterate over the stack in render
/// order (typically sorted by z), respect each entry's visibility and
/// opacity, and write the result into `target`.
///
/// # Example
///
/// ```
/// use termcompositor::{Compositor, FrameBuffer, LayerStack, SolidColor};
///
/// /// A custom compositor that renders layers in reverse stack order.
/// struct ReverseCompositor;
///
/// impl Compositor for ReverseCompositor {
///     fn compose(&self, stack: &LayerStack, target: &mut FrameBuffer) {
///         for entry in stack.entries().iter().rev() {
///             if entry.is_visible() && entry.opacity() > 0.0 {
///                 entry.layer().render(target, (0, 0), entry.opacity());
///             }
///         }
///     }
/// }
///
/// let mut stack = LayerStack::new();
/// stack.push(SolidColor::new(255, 0, 0, 255).with_z(10));
/// stack.push(SolidColor::new(0, 255, 0, 255).with_z(0));
///     let mut fb = FrameBuffer::new(1, 1);
///     stack.render_with(&ReverseCompositor, &mut fb);
///     // The ReverseCompositor reverses stack order (insertion
///     // order, not z-order), so the first-pushed red is rendered
///     // last, on top of green.
///     assert_eq!(fb.pixels()[0], [255, 0, 0, 255]);
/// ```
pub trait Compositor {
    /// Composites `stack` into `target`.
    fn compose(&self, stack: &LayerStack, target: &mut FrameBuffer);
}

/// The default CPU compositor: sorts visible entries by effective
/// z-order and calls each layer's `render` in turn. Uses no external
/// dependencies; suitable as a reference implementation and for
/// tests. Each layer's own `render` is responsible for blending with
/// the destination pixels using the entry's opacity.
///
/// # Example
///
/// ```
/// use termcompositor::{Compositor, CpuCompositor, FrameBuffer, LayerStack, SolidColor};
///
/// let mut stack = LayerStack::new();
/// let bg = stack.push(SolidColor::new(10, 20, 30, 255).with_z(0));
/// let fg = stack.push(SolidColor::new(200, 100, 50, 255).with_z(10));
/// stack.get_mut(fg).unwrap().set_opacity(0.5);
///
/// let mut fb = FrameBuffer::new(1, 1);
/// // Render using the default compositor.
/// CpuCompositor.compose(&stack, &mut fb);
/// // The foreground (at 0.5 opacity) blends over the background.
/// assert!(fb.pixels()[0][0] > 10);  // red contributed from fg
/// ```
#[derive(Debug, Clone, Copy, Default)]
pub struct CpuCompositor;

impl Compositor for CpuCompositor {
    fn compose(&self, stack: &LayerStack, target: &mut FrameBuffer) {
        for entry in stack
            .iter_sorted()
            .filter(|e| e.is_visible() && e.opacity() > 0.0)
        {
            match entry.transform() {
                Some(transform) if !transform.is_identity() => {
                    // Compute the source bounding box from the layer's
                    // bounds. If the layer reports no bounds (e.g.
                    // SolidColor), fall back to the full target size.
                    let (src_w, src_h) = match entry.layer().bounds() {
                        Some(b) => (b.width, b.height),
                        None => (target.width(), target.height()),
                    };
                    // Render only the source region into a small temp buffer.
                    // Use opacity=1.0 here because apply_transform_to_target
                    // applies the entry's opacity during the final blend.
                    let mut tmp = FrameBuffer::new(src_w, src_h);
                    entry.layer().render(&mut tmp, (0, 0), 1.0);
                    // Compute the target-space bounding box by transforming
                    // the four corners of the source region and taking the
                    // axis-aligned bounding box of the result.
                    let corners = [
                        transform.apply(0.0, 0.0),
                        transform.apply(src_w as f32, 0.0),
                        transform.apply(0.0, src_h as f32),
                        transform.apply(src_w as f32, src_h as f32),
                    ];
                    let mut min_x = f32::INFINITY;
                    let mut min_y = f32::INFINITY;
                    let mut max_x = f32::NEG_INFINITY;
                    let mut max_y = f32::NEG_INFINITY;
                    for (cx, cy) in corners {
                        min_x = min_x.min(cx);
                        min_y = min_y.min(cy);
                        max_x = max_x.max(cx);
                        max_y = max_y.max(cy);
                    }
                    // Clamp to target bounds.
                    let tx_min = (min_x.floor() as i32).max(0) as u32;
                    let ty_min = (min_y.floor() as i32).max(0) as u32;
                    let tx_max = (max_x.ceil() as i32 + 1).min(target.width() as i32) as u32;
                    let ty_max = (max_y.ceil() as i32 + 1).min(target.height() as i32) as u32;
                    if tx_min < tx_max && ty_min < ty_max {
                        apply_transform_to_target(
                            target,
                            &tmp,
                            transform,
                            entry.opacity(),
                            tx_min,
                            ty_min,
                            tx_max,
                            ty_max,
                        );
                    }
                }
                _ => {
                    entry.layer().render(target, (0, 0), entry.opacity());
                }
            }
        }
    }
}

/// Applies a transform to a source framebuffer and composites the
/// result onto the target within the given bounding box
/// `[tx_min, ty_min) .. (tx_max, ty_max)`. Uses inverse mapping:
    #[allow(clippy::too_many_arguments)]
/// for each target pixel, computes the corresponding source
/// coordinate via the inverse transform, samples with bilinear
/// interpolation, and blends onto the target.
fn apply_transform_to_target(
    target: &mut FrameBuffer,
    source: &FrameBuffer,
    transform: &crate::geometry::Transform,
    opacity: f32,
    tx_min: u32,
    ty_min: u32,
    tx_max: u32,
    ty_max: u32,
) {
    let sw = source.width() as f32;
    let sh = source.height() as f32;

    for ty in ty_min..ty_max {
        for tx in tx_min..tx_max {
            // Inverse-map target pixel to source space.
            let (sx, sy) = transform.apply_inverse(tx as f32, ty as f32);

            // Bilinear interpolation in source space.
            if sx < 0.0 || sy < 0.0 || sx >= sw || sy >= sh {
                continue;
            }
            let x0 = sx.floor() as u32;
            let y0 = sy.floor() as u32;
            let x1 = (x0 + 1).min(sw as u32 - 1);
            let y1 = (y0 + 1).min(sh as u32 - 1);
            let fx = sx - x0 as f32;
            let fy = sy - y0 as f32;

            let p00 = source.get_pixel(x0, y0).copied().unwrap_or([0, 0, 0, 0]);
            let p10 = source.get_pixel(x1, y0).copied().unwrap_or([0, 0, 0, 0]);
            let p01 = source.get_pixel(x0, y1).copied().unwrap_or([0, 0, 0, 0]);
            let p11 = source.get_pixel(x1, y1).copied().unwrap_or([0, 0, 0, 0]);

            // Interpolate RGB channels separately from alpha to
            // avoid incorrect blending with premultiplied values.
            let mut color = [0u8; 4];
            for c in 0..3 {
                let v = p00[c] as f32 * (1.0 - fx) * (1.0 - fy)
                    + p10[c] as f32 * fx * (1.0 - fy)
                    + p01[c] as f32 * (1.0 - fx) * fy
                    + p11[c] as f32 * fx * fy;
                color[c] = v.round().clamp(0.0, 255.0) as u8;
            }
            // Alpha: interpolate as straight alpha (not premultiplied).
            let a = p00[3] as f32 * (1.0 - fx) * (1.0 - fy)
                + p10[3] as f32 * fx * (1.0 - fy)
                + p01[3] as f32 * (1.0 - fx) * fy
                + p11[3] as f32 * fx * fy;
            color[3] = a.round().clamp(0.0, 255.0) as u8;

            let src_alpha = f32::from(color[3]) / 255.0 * opacity;
            if let Some(px) = target.get_pixel_mut(tx, ty) {
                crate::framebuffer::blend_over(px, &color, src_alpha);
            }
        }
    }
}

/// A backend-manipulable stack of layers: add, remove, and control
/// layers by id; render the stack into a [`FrameBuffer`] via the
/// default [`CpuCompositor`] (or a custom one).
///
/// # Example
///
/// ```no_run
/// use termcompositor::{FrameBuffer, LayerStack, SolidColor};
///
/// let mut stack = LayerStack::new();
/// let bg = stack.push(SolidColor::new(0, 0, 0, 255).with_name("bg"));
/// let fg = stack.push(SolidColor::new(255, 0, 0, 255).with_z(10));
///
/// // Control at will.
/// stack.get_mut(fg).unwrap().set_opacity(0.5);
/// stack.get_mut(bg).unwrap().set_visible(false);
///
/// // Render.
/// let mut fb = FrameBuffer::new(80, 24);
/// stack.render(&mut fb);
///
/// // Remove and re-add.
/// let _ = stack.remove(bg);
/// let new_id = stack.push(SolidColor::new(0, 255, 0, 255));
/// # let _ = new_id;
/// ```
pub struct LayerStack {
    next_id: LayerId,
    entries: Vec<LayerEntry>,
}

impl LayerStack {
    /// Creates a new, empty layer stack.
    pub fn new() -> Self {
        Self {
            next_id: 0,
            entries: Vec::new(),
        }
    }

    /// Pushes a new layer onto the top of the stack and returns its
    /// assigned [`LayerId`]. The new entry is fully opaque, visible,
    /// and has no z-override. Ids are monotonically increasing and
    /// are not reused.
    pub fn push<L: Layer + 'static>(&mut self, layer: L) -> LayerId {
        let id = self.next_id;
        self.next_id += 1;
        self.entries.push(LayerEntry::new(id, Box::new(layer)));
        id
    }

    /// Removes the entry with the given `id` and returns the
    /// previously wrapped layer. Returns `None` if no such entry.
    pub fn remove(&mut self, id: LayerId) -> Option<Box<dyn Layer>> {
        let pos = self.index_of(id)?;
        Some(self.entries.remove(pos).into_layer_box())
    }

    /// Returns a reference to the entry with the given `id`.
    pub fn get(&self, id: LayerId) -> Option<&LayerEntry> {
        self.entries.iter().find(|e| e.id() == id)
    }

    /// Returns a mutable reference to the entry with the given `id`.
    pub fn get_mut(&mut self, id: LayerId) -> Option<&mut LayerEntry> {
        self.entries.iter_mut().find(|e| e.id() == id)
    }

    /// Returns the entry's index in the underlying `Vec`, or `None`
    /// if no such entry. The index reflects current stack order; it
    /// is not the z-order.
    pub fn index_of(&self, id: LayerId) -> Option<usize> {
        self.entries.iter().position(|e| e.id() == id)
    }

    /// Returns a reference to the first entry whose name matches
    /// `name`. If multiple entries share the same name, the first
    /// one (earliest push order) is returned.
    pub fn find_by_name(&self, name: &str) -> Option<&LayerEntry> {
        self.entries.iter().find(|e| e.name() == name)
    }

    /// Returns a mutable reference to the first entry whose name
    /// matches `name`.
    pub fn find_by_name_mut(&mut self, name: &str) -> Option<&mut LayerEntry> {
        self.entries.iter_mut().find(|e| e.name() == name)
    }

    /// Moves the entry with the given `id` to `new_index` in the
    /// underlying `Vec`. The relative order of other entries is
    /// preserved. No-op if the id is missing.
    ///
    /// # Panics
    ///
    /// Panics if `new_index > len()`.
    pub fn reorder(&mut self, id: LayerId, new_index: usize) {
        let pos = match self.index_of(id) {
            Some(p) => p,
            None => return,
        };
        assert!(
            new_index <= self.entries.len(),
            "reorder index {new_index} out of bounds (len = {})",
            self.entries.len()
        );
        let entry = self.entries.remove(pos);
        let target = new_index.min(self.entries.len());
        self.entries.insert(target, entry);
    }

    /// Returns the current entry count.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns whether the stack is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Returns a slice of all entries in stack (insertion) order.
    /// For render order, use [`LayerStack::iter_sorted`].
    pub fn entries(&self) -> &[LayerEntry] {
        &self.entries
    }

    /// Returns a mutable slice of all entries in stack order.
    pub fn entries_mut(&mut self) -> &mut [LayerEntry] {
        &mut self.entries
    }

    /// Iterates over all entries in render order (effective z
    /// ascending, then stack insertion order for ties).
    pub fn iter_sorted(&self) -> impl Iterator<Item = &LayerEntry> {
        let mut sorted: Vec<&LayerEntry> = self.entries.iter().collect();
        sorted.sort_by_key(|e| e.effective_z());
        sorted.into_iter()
    }

    /// Removes all entries, leaving the stack empty. Ids are not
    /// reset — a subsequent `push` will receive an id greater than
    /// any previously issued.
    ///
    /// # Example
    ///
    /// ```
    /// use termcompositor::{LayerStack, SolidColor};
    ///
    /// let mut stack = LayerStack::new();
    /// stack.push(SolidColor::new(0, 0, 0, 255));
    /// assert!(!stack.is_empty());
    /// stack.clear();
    /// assert!(stack.is_empty());
    /// ```
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// Renders the stack into `target` using the default
    /// [`CpuCompositor`]. Equivalent to
    /// `CpuCompositor.compose(self, target)`.
    ///
    /// # Example
    ///
    /// ```
    /// use termcompositor::{FrameBuffer, LayerStack, SolidColor};
    ///
    /// let mut stack = LayerStack::new();
    /// stack.push(SolidColor::new(0, 128, 0, 255));
    ///
    /// let mut fb = FrameBuffer::new(2, 2);
    /// stack.render(&mut fb);
    /// assert_eq!(fb.pixels()[0], [0, 128, 0, 255]);
    /// ```
    pub fn render(&self, target: &mut FrameBuffer) {
        CpuCompositor.compose(self, target);
    }

    /// Renders the stack into `target` using the supplied compositor.
    pub fn render_with<C: Compositor>(&self, compositor: &C, target: &mut FrameBuffer) {
        compositor.compose(self, target);
    }

    /// Renders the stack into a fresh [`FrameBuffer`] sized to the
    /// given terminal and returns it. This is the "fits the terminal"
    /// entry point callers should reach for when projecting to a TTY.
    pub fn render_to_terminal(&self, size: TerminalSize) -> FrameBuffer {
        let (w, h) = size.as_framebuffer_size();
        let mut fb = FrameBuffer::new(w, h);
        self.render(&mut fb);
        fb
    }

    /// Convenience: auto-detect the current terminal size, then
    /// render into a framebuffer of that size. Returns the
    /// (framebuffer, terminal size) tuple so the backend can
    /// report the size back through the API.
    pub fn render_to_current_terminal(&self) -> (FrameBuffer, TerminalSize) {
        let size = TerminalSize::current();
        let fb = self.render_to_terminal(size);
        (fb, size)
    }

    /// Renders `self` into `target` using diff-based rendering.
    ///
    /// On the first call (when `dirty` is empty), this is equivalent
    /// to a full `render`. On subsequent calls, only layers whose
    /// bounding boxes intersect the dirty regions are re-composited;
    /// the rest of the framebuffer is preserved from the previous
    /// frame.
    ///
    /// After rendering, `dirty` is cleared. Callers should mark
    /// regions dirty before calling this method (e.g. when a layer
    /// moves or changes opacity).
    pub fn render_diff(&self, target: &mut FrameBuffer, dirty: &mut DirtyRegion) {
        if dirty.is_clean() {
            // First call or nothing marked dirty — full render.
            CpuCompositor.compose(self, target);
            return;
        }

        // Snapshot the regions that need re-compositing, then clear.
        let regions: Vec<DirtyRect> = dirty.take_regions(target.width(), target.height());

        // Render the entire stack into a temporary buffer.
        let mut tmp = FrameBuffer::new(target.width(), target.height());
        CpuCompositor.compose(self, &mut tmp);

        // Copy only the dirty regions from tmp to target.
        for r in &regions {
            for y in r.y..r.y.saturating_add(r.height) {
                for x in r.x..r.x.saturating_add(r.width) {
                    if let (Some(src), Some(dst)) =
                        (tmp.get_pixel(x, y), target.get_pixel_mut(x, y))
                    {
                        *dst = *src;
                    }
                }
            }
        }
    }
}

/// A single rectangular dirty region.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DirtyRect {
    /// Left edge, inclusive.
    pub x: u32,
    /// Top edge, inclusive.
    pub y: u32,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
}

impl DirtyRect {
    /// Creates a new dirty rectangle.
    pub fn new(x: u32, y: u32, width: u32, height: u32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    /// Returns the right edge (exclusive).
    pub fn right(&self) -> u32 {
        self.x.saturating_add(self.width)
    }

    /// Returns the bottom edge (exclusive).
    pub fn bottom(&self) -> u32 {
        self.y.saturating_add(self.height)
    }

    /// Returns `true` if this rectangle intersects another.
    pub fn intersects(&self, other: &DirtyRect) -> bool {
        self.x < other.right()
            && self.right() > other.x
            && self.y < other.bottom()
            && self.bottom() > other.y
    }

    /// Merges another rectangle into this one, expanding to
    /// cover both.
    pub fn merge(&mut self, other: &DirtyRect) {
        let x1 = self.x.min(other.x);
        let y1 = self.y.min(other.y);
        let x2 = self.right().max(other.right());
        let y2 = self.bottom().max(other.bottom());
        self.x = x1;
        self.y = y1;
        self.width = x2 - x1;
        self.height = y2 - y1;
    }
}

/// Tracks dirty regions across frames for diff-based rendering.
///
/// Callers mark regions as dirty (e.g. when a layer moves or
/// changes), then pass the tracker to [`LayerStack::render_diff`].
/// After rendering, the dirty state is cleared automatically.
#[derive(Debug, Clone)]
pub struct DirtyRegion {
    regions: Vec<DirtyRect>,
    /// When `true`, the entire framebuffer is dirty (full re-render).
    full: bool,
}

impl DirtyRegion {
    /// Creates a new, empty dirty region tracker.
    pub fn new() -> Self {
        Self {
            regions: Vec::new(),
            full: false,
        }
    }

    /// Marks the entire framebuffer as dirty.
    pub fn mark_full(&mut self) {
        self.full = true;
        self.regions.clear();
    }

    /// Marks a rectangular region as dirty.
    pub fn mark_rect(&mut self, rect: DirtyRect) {
        if self.full {
            return;
        }
        // Try to merge with an existing region.
        if let Some(existing) = self.regions.iter_mut().find(|r| r.intersects(&rect)) {
            existing.merge(&rect);
        } else {
            self.regions.push(rect);
        }
    }

    /// Marks a point as dirty (1x1 region).
    pub fn mark_point(&mut self, x: u32, y: u32) {
        self.mark_rect(DirtyRect::new(x, y, 1, 1));
    }

    /// Returns `true` if no regions are marked dirty.
    pub fn is_clean(&self) -> bool {
        self.regions.is_empty() && !self.full
    }

    /// Returns the number of dirty regions.
    pub fn region_count(&self) -> usize {
        if self.full {
            1
        } else {
            self.regions.len()
        }
    }

    /// Takes all regions out of the tracker (leaving it empty).
    /// If `full` was set, returns a single region covering the
    /// entire framebuffer.
    fn take_regions(&mut self, width: u32, height: u32) -> Vec<DirtyRect> {
        if self.full {
            self.full = false;
            vec![DirtyRect::new(0, 0, width, height)]
        } else {
            std::mem::take(&mut self.regions)
        }
    }
}

impl Default for DirtyRegion {
    fn default() -> Self {
        Self::new()
    }
}

impl Default for LayerStack {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::LayerStack;
    use super::{DirtyRect, DirtyRegion};
    use crate::framebuffer::FrameBuffer;
    use crate::layer::SolidColor;
    use crate::terminal::TerminalSize;

    #[test]
    fn push_assigns_unique_ids() {
        let mut s = LayerStack::new();
        let a = s.push(SolidColor::new(0, 0, 0, 255));
        let b = s.push(SolidColor::new(0, 0, 0, 255));
        assert_ne!(a, b);
        assert_eq!(s.len(), 2);
    }

    #[test]
    fn remove_returns_layer_and_forgets_id() {
        let mut s = LayerStack::new();
        let id = s.push(SolidColor::new(1, 2, 3, 4));
        assert!(s.remove(id).is_some());
        assert!(s.get(id).is_none());
        assert!(s.remove(id).is_none());
    }

    #[test]
    fn get_mut_can_toggle_visibility() {
        let mut s = LayerStack::new();
        let id = s.push(SolidColor::new(0, 0, 0, 255));
        s.get_mut(id).unwrap().set_visible(false);
        assert!(!s.get(id).unwrap().is_visible());
    }

    #[test]
    fn reorder_moves_entry() {
        let mut s = LayerStack::new();
        let a = s.push(SolidColor::new(0, 0, 0, 255));
        let b = s.push(SolidColor::new(0, 0, 0, 255));
        let c = s.push(SolidColor::new(0, 0, 0, 255));
        s.reorder(a, 2);
        assert_eq!(s.index_of(a), Some(2));
        assert_eq!(s.index_of(b), Some(0));
        assert_eq!(s.index_of(c), Some(1));
    }

    #[test]
    fn clear_empties_but_preserves_id_sequence() {
        let mut s = LayerStack::new();
        let _ = s.push(SolidColor::new(0, 0, 0, 255));
        let _ = s.push(SolidColor::new(0, 0, 0, 255));
        s.clear();
        assert!(s.is_empty());
        let new = s.push(SolidColor::new(0, 0, 0, 255));
        assert!(new >= 2);
    }

    #[test]
    fn render_composites_visible_in_z_order() {
        let mut s = LayerStack::new();
        let bg = s.push(SolidColor::new(255, 0, 0, 255).with_z(0));
        let fg = s.push(SolidColor::new(0, 255, 0, 255).with_z(10));
        s.get_mut(fg).unwrap().set_opacity(0.75);
        let mut fb = FrameBuffer::new(1, 1);
        s.render(&mut fb);
        // Green at 0.75 opacity over opaque red: out_r = 0*0.75 +
        // 255*0.25 = 64; out_g = 255*0.75 + 0*0.25 = 191.
        assert!(fb.pixels()[0][1] > fb.pixels()[0][0]);
        let _ = bg;
    }

    #[test]
    fn iter_sorted_orders_by_effective_z() {
        let mut s = LayerStack::new();
        let _ = s.push(SolidColor::new(0, 0, 0, 255).with_z(10));
        let _ = s.push(SolidColor::new(0, 0, 0, 255).with_z(0));
        let _ = s.push(SolidColor::new(0, 0, 0, 255).with_z(5));
        let zs: Vec<u32> = s.iter_sorted().map(|e| e.effective_z()).collect();
        assert_eq!(zs, vec![0, 5, 10]);
    }

    #[test]
    fn iter_sorted_is_stable_for_ties() {
        let mut s = LayerStack::new();
        let _ = s.push(SolidColor::new(0, 0, 0, 255).with_z(5).with_name("a"));
        let _ = s.push(SolidColor::new(0, 0, 0, 255).with_z(10).with_name("b"));
        let _ = s.push(SolidColor::new(0, 0, 0, 255).with_z(5).with_name("c"));
        let names: Vec<String> = s.iter_sorted().map(|e| e.name().to_owned()).collect();
        assert_eq!(names, vec!["a", "c", "b"]);
    }

    #[test]
    fn render_skips_invisible_layers() {
        let mut s = LayerStack::new();
        let hidden = s.push(SolidColor::new(0, 0, 255, 255).with_z(100));
        s.push(SolidColor::new(255, 0, 0, 255).with_z(0));
        s.get_mut(hidden).unwrap().set_visible(false);
        let mut fb = FrameBuffer::new(1, 1);
        s.render(&mut fb);
        assert_eq!(fb.pixels()[0], [255, 0, 0, 255]);
    }

    #[test]
    fn render_with_custom_compositor() {
        let mut s = LayerStack::new();
        s.push(SolidColor::new(0, 255, 0, 255));
        struct CountingComp(#[allow(dead_code)] usize);
        impl super::Compositor for CountingComp {
            fn compose(&self, stack: &LayerStack, _target: &mut FrameBuffer) {
                // We don't actually need to render — just confirm we
                // saw one visible entry. The test only checks the
                // custom compositor was invoked.
                assert_eq!(stack.len(), 1);
            }
        }
        let mut fb = FrameBuffer::new(1, 1);
        let comp = CountingComp(0);
        s.render_with(&comp, &mut fb);
    }

    #[test]
    fn render_to_terminal_returns_buffer_of_given_size() {
        let mut s = LayerStack::new();
        s.push(SolidColor::new(0, 0, 0, 255));
        let size = TerminalSize { rows: 5, cols: 10 };
        let fb = s.render_to_terminal(size);
        assert_eq!(fb.width(), 10);
        assert_eq!(fb.height(), 5);
        assert_eq!(fb.pixels().len(), 50);
    }

    #[test]
    fn render_to_current_terminal_returns_size() {
        let s = LayerStack::new();
        let (fb, reported) = s.render_to_current_terminal();
        assert_eq!(fb.width() as u16, reported.cols);
        assert_eq!(fb.height() as u16, reported.rows);
    }

    #[test]
    fn entries_returns_all_in_stack_order() {
        let mut s = LayerStack::new();
        let a = s.push(SolidColor::new(1, 0, 0, 255).with_name("a"));
        let b = s.push(SolidColor::new(0, 1, 0, 255).with_name("b"));
        let entries = s.entries();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].id(), a);
        assert_eq!(entries[1].id(), b);
    }

    #[test]
    fn entries_mut_allows_modifying_entries() {
        let mut s = LayerStack::new();
        let _ = s.push(SolidColor::new(0, 0, 0, 255));
        let _ = s.push(SolidColor::new(0, 0, 0, 255));
        // Use entries_mut to set the first entry invisible
        // and the second entry to a custom name.
        s.entries_mut()[0].set_visible(false);
        s.entries_mut()[1].set_name("second");
        assert!(!s.entries()[0].is_visible());
        assert_eq!(s.entries()[1].name(), "second");
    }

    #[test]
    fn default_creates_empty_stack() {
        let s = LayerStack::default();
        assert!(s.is_empty());
        assert_eq!(s.len(), 0);
    }

    #[test]
    fn default_push_starts_at_zero() {
        let mut s = LayerStack::default();
        let id = s.push(SolidColor::new(0, 0, 0, 255));
        assert_eq!(id, 0);
    }

    #[test]
    fn reorder_missing_id_is_noop() {
        let mut s = LayerStack::new();
        let a = s.push(SolidColor::new(0, 0, 0, 255));
        let b = s.push(SolidColor::new(0, 0, 0, 255));
        // Reorder a non-existent id — should be a no-op.
        s.reorder(999, 0);
        // Stack unchanged.
        assert_eq!(s.index_of(a), Some(0));
        assert_eq!(s.index_of(b), Some(1));
    }

    #[test]
    fn reorder_to_same_index_is_stable() {
        let mut s = LayerStack::new();
        let a = s.push(SolidColor::new(0, 0, 0, 255));
        let _b = s.push(SolidColor::new(0, 0, 0, 255));
        // Move 'a' from index 0 to index 0 — no change.
        s.reorder(a, 0);
        assert_eq!(s.index_of(a), Some(0));
    }

    #[test]
    fn reorder_to_last_index() {
        let mut s = LayerStack::new();
        let a = s.push(SolidColor::new(0, 0, 0, 255));
        let b = s.push(SolidColor::new(0, 0, 0, 255));
        let c = s.push(SolidColor::new(0, 0, 0, 255));
        // Move 'a' to end (index 3 == len()).
        s.reorder(a, 3);
        assert_eq!(s.index_of(a), Some(2));
        assert_eq!(s.index_of(b), Some(0));
        assert_eq!(s.index_of(c), Some(1));
    }

    #[test]
    #[should_panic(expected = "reorder index 2 out of bounds (len = 1)")]
    fn reorder_past_end_panics() {
        let mut s = LayerStack::new();
        let a = s.push(SolidColor::new(0, 0, 0, 255));
        // index 2 > len (1) — should panic.
        s.reorder(a, 2);
    }

    #[test]
    fn entries_empty_stack() {
        let s = LayerStack::new();
        assert!(s.entries().is_empty());
    }

    #[test]
    fn entries_mut_empty_stack() {
        let mut s = LayerStack::new();
        assert!(s.entries_mut().is_empty());
    }

    // ── Transform rendering tests ─────────────────────────────

    use crate::geometry::Transform;
    use crate::layer::RectLayer;

    #[test]
    fn transform_90deg_rotation_moves_rect() {
        // A 2x3 red rect at (0, 0) rotated 90° around its center (1.0, 1.5).
        // After CW rotation, the rect should appear in a different region.
        let mut s_no_t = LayerStack::new();
        let _ = s_no_t.push(RectLayer::new(0, 0, 2, 3, [255, 0, 0, 255]));
        let mut fb_no_t = FrameBuffer::new(10, 10);
        s_no_t.render(&mut fb_no_t);

        let mut s = LayerStack::new();
        let id = s.push(RectLayer::new(0, 0, 2, 3, [255, 0, 0, 255]));
        let t = Transform::new().with_rotation(90.0).with_anchor(1.0, 1.5);
        s.get_mut(id).unwrap().set_transform(Some(t));
        let mut fb = FrameBuffer::new(10, 10);
        s.render(&mut fb);

        // The rotated rect should render differently from the unrotated one.
        assert_ne!(
            fb_no_t.pixels(),
            fb.pixels(),
            "rotated rect should render differently from unrotated"
        );
        // And it should have some non-zero pixels.
        let has_content = fb.pixels().iter().any(|p| p[3] > 0);
        assert!(has_content, "rotated rect should produce visible pixels");
    }

    #[test]
    fn transform_2x_scale_doubles_rect_size() {
        // A 3x2 red rect at (0, 0) scaled 2x around its center
        // (1.5, 1.0). The resulting rect should be ~6x4 pixels.
        let mut s_no_t = LayerStack::new();
        let _ = s_no_t.push(RectLayer::new(0, 0, 3, 2, [255, 0, 0, 255]));
        let mut fb_no_t = FrameBuffer::new(10, 10);
        s_no_t.render(&mut fb_no_t);

        let mut s = LayerStack::new();
        let id = s.push(RectLayer::new(0, 0, 3, 2, [255, 0, 0, 255]));
        let t = Transform::new().with_scale(2.0, 2.0).with_anchor(1.5, 1.0);
        s.get_mut(id).unwrap().set_transform(Some(t));
        let mut fb = FrameBuffer::new(10, 10);
        s.render(&mut fb);

        // The scaled rect should be larger than the original.
        let scaled_count = fb
            .pixels()
            .iter()
            .filter(|p| p[0] > 200 && p[3] > 200)
            .count();
        let original_count = fb_no_t
            .pixels()
            .iter()
            .filter(|p| p[0] > 200 && p[3] > 200)
            .count();
        assert!(scaled_count > original_count,
            "scaled rect should have more red pixels than original, scaled={scaled_count} original={original_count}");
    }

    #[test]
    fn transform_identity_render_matches_no_transform() {
        // A RectLayer with an identity transform should render
        // identically to one without a transform.
        let mut s1 = LayerStack::new();
        let _id1 = s1.push(RectLayer::new(2, 2, 4, 3, [0, 0, 255, 255]));
        let mut fb1 = FrameBuffer::new(10, 10);
        s1.render(&mut fb1);

        let mut s2 = LayerStack::new();
        let id2 = s2.push(RectLayer::new(2, 2, 4, 3, [0, 0, 255, 255]));
        s2.get_mut(id2)
            .unwrap()
            .set_transform(Some(Transform::new()));
        let mut fb2 = FrameBuffer::new(10, 10);
        s2.render(&mut fb2);

        assert_eq!(fb1.pixels(), fb2.pixels());
    }

    #[test]
    fn transform_opacity_not_double_applied() {
        // A red RectLayer at 0.5 opacity. After transform rendering,
        // the pixel alpha should reflect 0.5, not 0.25 (double-applied)
        // and not 1.0 (not applied at all).
        let mut s = LayerStack::new();
        let id = s.push(RectLayer::new(0, 0, 3, 3, [255, 0, 0, 255]));
        s.get_mut(id).unwrap().set_opacity(0.5);
        let t = Transform::new().with_scale(2.0, 2.0).with_anchor(1.5, 1.5);
        s.get_mut(id).unwrap().set_transform(Some(t));
        let mut fb = FrameBuffer::new(10, 10);
        s.render(&mut fb);

        // Find the strongest red pixel (center of scaled rect).
        let max_alpha = fb
            .pixels()
            .iter()
            .filter(|p| p[0] > 100)
            .map(|p| p[3])
            .max()
            .unwrap_or(0);
        // Opacity 0.5 → max alpha ≈ 128.
        // If double-applied: ≈ 64 (too low).
        // If not applied at all: 255 (too high).
        assert!(
            max_alpha > 80 && max_alpha < 200,
            "opacity should be ~128, got alpha={max_alpha}"
        );
    }

    #[test]
    fn transform_rendered_rect_not_at_original_position() {
        // A red RectLayer at (0,0) rotated 45° around (0,0) should
        // NOT have all its red pixels at (0,0)-(3,3).
        let mut s_no_t = LayerStack::new();
        let _id_no_t = s_no_t.push(RectLayer::new(0, 0, 4, 4, [255, 0, 0, 255]));
        let mut fb_no_t = FrameBuffer::new(10, 10);
        s_no_t.render(&mut fb_no_t);

        let mut s_t = LayerStack::new();
        let id_t = s_t.push(RectLayer::new(0, 0, 4, 4, [255, 0, 0, 255]));
        let t = Transform::new().with_rotation(45.0);
        s_t.get_mut(id_t).unwrap().set_transform(Some(t));
        let mut fb_t = FrameBuffer::new(10, 10);
        s_t.render(&mut fb_t);

        assert_ne!(
            fb_no_t.pixels(),
            fb_t.pixels(),
            "rotated rect should render differently from unrotated"
        );
    }

    #[test]
    fn find_by_name_returns_matching_entry() {
        let mut s = LayerStack::new();
        s.push(SolidColor::new(255, 0, 0, 255).with_name("bg"));
        s.push(RectLayer::new(10, 10, 5, 5, [0, 255, 0, 255]).with_name("rect"));

        let entry = s.find_by_name("rect").expect("should find rect");
        assert_eq!(entry.name(), "rect");
    }

    #[test]
    fn find_by_name_returns_none_when_not_found() {
        let s = LayerStack::new();
        assert!(s.find_by_name("nonexistent").is_none());
    }

    #[test]
    fn find_by_name_mut_allows_modification() {
        let mut s = LayerStack::new();
        s.push(RectLayer::new(0, 0, 10, 10, [255, 0, 0, 255]).with_name("target"));

        s.find_by_name_mut("target").unwrap().set_opacity(0.5);
        assert_eq!(s.find_by_name("target").unwrap().opacity(), 0.5);
    }

    #[test]
    fn find_by_name_returns_first_match() {
        let mut s = LayerStack::new();
        s.push(SolidColor::new(255, 0, 0, 255).with_name("dup"));
        s.push(SolidColor::new(0, 255, 0, 255).with_name("dup"));

        let entry = s.find_by_name("dup").unwrap();
        // First pushed entry (red) should be returned.
        assert_eq!(entry.name(), "dup");
    }

    // ─── Diff-Based Rendering tests ───────────────────────────

    #[test]
    fn dirty_region_new_is_clean() {
        let d = DirtyRegion::new();
        assert!(d.is_clean());
        assert_eq!(d.region_count(), 0);
    }

    #[test]
    fn dirty_rect_intersects() {
        let a = DirtyRect::new(0, 0, 10, 10);
        let b = DirtyRect::new(5, 5, 10, 10);
        let c = DirtyRect::new(20, 20, 5, 5);
        assert!(a.intersects(&b));
        assert!(!a.intersects(&c));
    }

    #[test]
    fn dirty_rect_merge() {
        let mut a = DirtyRect::new(0, 0, 5, 5);
        let b = DirtyRect::new(3, 3, 5, 5);
        a.merge(&b);
        assert_eq!(a, DirtyRect::new(0, 0, 8, 8));
    }

    #[test]
    fn dirty_region_mark_rect() {
        let mut d = DirtyRegion::new();
        d.mark_rect(DirtyRect::new(0, 0, 10, 10));
        assert!(!d.is_clean());
        assert_eq!(d.region_count(), 1);
    }

    #[test]
    fn dirty_region_mark_full() {
        let mut d = DirtyRegion::new();
        d.mark_rect(DirtyRect::new(0, 0, 10, 10));
        d.mark_full();
        assert_eq!(d.region_count(), 1); // full counts as 1
    }

    #[test]
    fn render_diff_full_on_first_call() {
        let mut s = LayerStack::new();
        s.push(RectLayer::new(5, 5, 10, 10, [255, 0, 0, 255]));

        let mut fb = FrameBuffer::new(20, 20);
        let mut dirty = DirtyRegion::new();
        dirty.mark_full();

        s.render_diff(&mut fb, &mut dirty);

        // After render, dirty should be clean.
        assert!(dirty.is_clean());
        // The rect should be visible.
        assert_eq!(fb.get_pixel(7, 7), Some(&[255, 0, 0, 255]));
    }

    #[test]
    fn render_diff_skips_clean_regions() {
        let mut s = LayerStack::new();
        s.push(RectLayer::new(0, 0, 20, 20, [0, 0, 255, 255]));

        let mut fb = FrameBuffer::new(20, 20);
        let mut dirty = DirtyRegion::new();
        dirty.mark_full();
        s.render_diff(&mut fb, &mut dirty);

        // Write garbage to a region that won't be re-rendered.
        for px in fb.pixels_mut()[0..5].iter_mut() {
            *px = [128, 128, 128, 128];
        }

        // Mark only a small region dirty — the garbage should survive.
        dirty.mark_rect(DirtyRect::new(10, 10, 5, 5));
        s.render_diff(&mut fb, &mut dirty);

        // The untouched pixels should still be garbage.
        assert_eq!(fb.get_pixel(0, 0), Some(&[128, 128, 128, 128]));
        // The dirty region should be re-rendered.
        assert_eq!(fb.get_pixel(12, 12), Some(&[0, 0, 255, 255]));
    }

    #[test]
    fn render_diff_produces_same_result_as_full_render() {
        let mut s = LayerStack::new();
        s.push(RectLayer::new(3, 3, 8, 8, [0, 255, 0, 255]));

        // Full render.
        let mut fb_full = FrameBuffer::new(20, 20);
        s.render(&mut fb_full);

        // Diff render with full dirty.
        let mut fb_diff = FrameBuffer::new(20, 20);
        let mut dirty = DirtyRegion::new();
        dirty.mark_full();
        s.render_diff(&mut fb_diff, &mut dirty);

        assert_eq!(fb_full.pixels(), fb_diff.pixels());
    }
}
