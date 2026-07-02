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
/// use dashcompositor::{Compositor, FrameBuffer, LayerStack, SolidColor};
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
/// use dashcompositor::{Compositor, CpuCompositor, FrameBuffer, LayerStack, SolidColor};
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
            entry.layer().render(target, (0, 0), entry.opacity());
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
/// use dashcompositor::{FrameBuffer, LayerStack, SolidColor};
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
    /// use dashcompositor::{LayerStack, SolidColor};
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
    /// use dashcompositor::{FrameBuffer, LayerStack, SolidColor};
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
}

impl Default for LayerStack {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::LayerStack;
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
}
