//! Layer — a single compositable layer in the compositor.
//!
//! Concrete [`Layer`] implementations include solid fills, raster
//! images, text glyphs, and vector shapes. Each layer is identified
//! by a [`LayerId`] when wrapped in a [`LayerEntry`] inside a
//! [`crate::LayerStack`].
//!
//! Layers are pure drawing primitives: the trait exposes z-order,
//! name, an optional bounding box ([`Layer::bounds`]), and a
//! single `render` call. Per-layer state that the backend might
//! want to tweak at runtime — opacity, visibility, z-override,
//! custom name — lives on [`LayerEntry`], not on the trait, so the
//! backend can adjust them through the [`crate::LayerStack`] API
//! without downcasting.

use crate::framebuffer::FrameBuffer;
use crate::geometry::Rect;

/// A unique handle for a layer inside a [`crate::LayerStack`].
///
/// Ids are assigned by the stack when a layer is pushed and remain
/// stable until the entry is removed. Ids are not reused within
/// the lifetime of a stack.
pub type LayerId = usize;

// ─── Accessibility Metadata ─────────────────────────────────────

/// Semantic role for accessibility metadata.
///
/// Describes the purpose or meaning of a layer for screen readers
/// and other assistive technologies. This allows headless terminals
/// and accessibility tools to convey the content's meaning without
/// rendering the visual output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum SemanticRole {
    /// No specific role; the layer is decorative.
    #[default]
    None,
    /// A text label or heading.
    Text,
    /// A button or interactive element.
    Button,
    /// An image or icon.
    Image,
    /// A container grouping child layers.
    Container,
    /// A separator or divider.
    Separator,
    /// A progress indicator or status display.
    Status,
    /// A navigation element.
    Navigation,
    /// A custom role with a static string label.
    Custom(&'static str),
}

/// Accessibility metadata for a layer.
///
/// Attach alt-text and a semantic role to any layer so that screen
/// readers and headless terminals can convey the content's meaning.
///
/// # Example
///
/// ```ignore
/// use termcompositor::{AccessibilityMetadata, SemanticRole, RectLayer};
///
/// let rect = RectLayer::new(0, 0, 10, 5, [255, 0, 0, 255])
///     .with_accessibility(AccessibilityMetadata::new()
///         .with_alt_text("Status indicator")
///         .with_role(SemanticRole::Status));
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AccessibilityMetadata {
    /// Alternative text describing the layer's content.
    alt_text: Option<String>,
    /// Semantic role describing the layer's purpose.
    role: SemanticRole,
}

impl AccessibilityMetadata {
    /// Creates new empty accessibility metadata (no alt-text,
    /// role = [`SemanticRole::None`]).
    pub fn new() -> Self {
        Self::default()
    }

    /// Builder: sets the alt-text.
    #[must_use]
    pub fn with_alt_text(mut self, text: impl Into<String>) -> Self {
        self.alt_text = Some(text.into());
        self
    }

    /// Builder: sets the semantic role.
    #[must_use]
    pub fn with_role(mut self, role: SemanticRole) -> Self {
        self.role = role;
        self
    }

    /// Returns a reference to the alt-text, if set.
    pub fn alt_text(&self) -> Option<&str> {
        self.alt_text.as_deref()
    }

    /// Returns the semantic role.
    pub fn role(&self) -> SemanticRole {
        self.role
    }

    /// Sets the alt-text.
    pub fn set_alt_text(&mut self, text: impl Into<String>) {
        self.alt_text = Some(text.into());
    }

    /// Clears the alt-text.
    pub fn clear_alt_text(&mut self) {
        self.alt_text = None;
    }

    /// Sets the semantic role.
    pub fn set_role(&mut self, role: SemanticRole) {
        self.role = role;
    }
}

// ─── Layer Trait ────────────────────────────────────────────────

/// A single layer that can be drawn into a [`FrameBuffer`].
///
/// Implementations should be pure with respect to the rest of
/// the layer stack: the compositor handles ordering, visibility,
/// and opacity. A layer's [`Layer::render`] is expected to read
/// the destination's current state from `target` and write its
/// contribution blended at the given `opacity`.
pub trait Layer {
    /// The default z-order of this layer. Higher values are
    /// drawn later (on top); ties resolve by stack insertion
    /// order. The [`LayerEntry`] wrapper can override this with
    /// [`LayerEntry::set_z_override`].
    fn z_order(&self) -> u32;

    /// A human-readable name for the layer, used in error
    /// messages and debugging.
    fn name(&self) -> &str {
        "<unnamed layer>"
    }

    /// The layer's intrinsic bounding box in layer-local
    /// coordinates, or `None` for layers that have no finite
    /// footprint (e.g. a solid-colour fill that always covers
    /// the whole target). Compositors MAY use `bounds()` for
    /// clipping, culling, hit-testing, or animation work, but
    /// must not rely on it for correctness — a layer that draws
    /// outside its reported bounding box is still well-defined.
    fn bounds(&self) -> Option<Rect> {
        None
    }

    /// Renders this layer into `target`, alpha-blending with
    /// the destination pixels using `opacity` (in
    /// `0.0..=1.0`). The `offset` parameter is an additive
    /// translation applied on top of the layer's intrinsic
    /// position; layers without a position (e.g. a full-frame
    /// solid colour) MAY ignore it. Implementations must
    /// respect `opacity`: at `0.0` the target must be unchanged;
    /// at `1.0` the layer's own alpha determines the blend.
    /// Implementations should clip writes that fall outside
    /// `target` (do not panic on off-screen coordinates).
    fn render(&self, target: &mut FrameBuffer, offset: (u32, u32), opacity: f32);
}

// ─── SolidColor ─────────────────────────────────────────────────

/// A solid-colour layer: fills the entire target framebuffer
/// with one RGBA colour, alpha-blended using the layer's
/// effective opacity. `bounds()` returns `None` because a
/// solid fill has no finite footprint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SolidColor {
    /// `[R, G, B, A]` in `0..=255` per channel.
    pub color: [u8; 4],
    z: u32,
    name: String,
}

impl SolidColor {
    /// Creates a new solid-color layer with the given RGBA channels.
    pub fn new(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self {
            color: [r, g, b, a],
            z: 0,
            name: format!("SolidColor(r={r}, g={g}, b={b}, a={a})"),
        }
    }

    /// Builder: sets the default z-order. The override in
    /// [`LayerEntry`] (if any) wins.
    #[must_use]
    pub fn with_z(mut self, z: u32) -> Self {
        self.z = z;
        self
    }

    /// Builder: sets a human-readable name.
    #[must_use]
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }
}

impl Layer for SolidColor {
    fn z_order(&self) -> u32 {
        self.z
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn render(&self, target: &mut FrameBuffer, _offset: (u32, u32), opacity: f32) {
        let effective = (f32::from(self.color[3]) / 255.0 * opacity).clamp(0.0, 1.0);
        for pixel in target.pixels_mut() {
            crate::framebuffer::blend_over(pixel, &self.color, effective);
        }
    }
}

// ─── RectLayer ──────────────────────────────────────────────────

/// A solid-colour rectangle at a specific position and size.
/// `bounds()` returns the rectangle itself; `render` writes
/// only inside the rect (writes outside `target` are clipped
/// silently).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RectLayer {
    /// Left edge, inclusive.
    pub x: u32,
    /// Top edge, inclusive.
    pub y: u32,
    /// Width in cells/pixels.
    pub width: u32,
    /// Height in cells/pixels.
    pub height: u32,
    /// `[R, G, B, A]` in `0..=255` per channel.
    pub color: [u8; 4],
    /// Corner radius in pixels. `0` means sharp corners.
    pub border_radius: u32,
    z: u32,
    name: String,
}

impl RectLayer {
    /// Creates a new rectangle layer at `(x, y)` with the given
    /// `width x height` and RGBA `color`.
    pub fn new(x: u32, y: u32, width: u32, height: u32, color: [u8; 4]) -> Self {
        Self {
            x,
            y,
            width,
            height,
            color,
            border_radius: 0,
            z: 0,
            name: format!("Rect({x},{y},{width}x{height})"),
        }
    }

    /// Builder: sets the default z-order.
    #[must_use]
    pub fn with_z(mut self, z: u32) -> Self {
        self.z = z;
        self
    }

    /// Builder: sets a human-readable name.
    #[must_use]
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    /// Builder: sets the corner radius in pixels.
    ///
    /// When `radius > 0`, the four corners of the rectangle are
    /// clipped to circular arcs. The effective radius is clamped
    /// to `min(width, height) / 2`.
    #[must_use]
    pub fn with_border_radius(mut self, radius: u32) -> Self {
        self.border_radius = radius;
        self
    }

    /// Returns `true` if `(sx, sy)` falls outside the rounded
    /// corner arcs.  `r` is the clamped radius, and `w`/`h` are
    /// the rectangle dimensions.
    fn is_outside_radius(sx: u32, sy: u32, w: u32, h: u32, r: u32) -> bool {
        // Top-left corner.
        if sx < r && sy < r {
            let dx = r as f32 - sx as f32 - 0.5;
            let dy = r as f32 - sy as f32 - 0.5;
            return dx * dx + dy * dy > r as f32 * r as f32;
        }
        // Top-right corner.
        if sx >= w - r && sy < r {
            let dx = sx as f32 - (w - r) as f32 + 0.5;
            let dy = r as f32 - sy as f32 - 0.5;
            return dx * dx + dy * dy > r as f32 * r as f32;
        }
        // Bottom-left corner.
        if sx < r && sy >= h - r {
            let dx = r as f32 - sx as f32 - 0.5;
            let dy = sy as f32 - (h - r) as f32 + 0.5;
            return dx * dx + dy * dy > r as f32 * r as f32;
        }
        // Bottom-right corner.
        if sx >= w - r && sy >= h - r {
            let dx = sx as f32 - (w - r) as f32 + 0.5;
            let dy = sy as f32 - (h - r) as f32 + 0.5;
            return dx * dx + dy * dy > r as f32 * r as f32;
        }
        false
    }
}

impl Layer for RectLayer {
    fn z_order(&self) -> u32 {
        self.z
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn bounds(&self) -> Option<Rect> {
        Some(Rect::new(self.x, self.y, self.width, self.height))
    }

    fn render(&self, target: &mut FrameBuffer, offset: (u32, u32), opacity: f32) {
        let ox = self.x.saturating_add(offset.0);
        let oy = self.y.saturating_add(offset.1);
        let effective = (f32::from(self.color[3]) / 255.0 * opacity).clamp(0.0, 1.0);
        let r = self.border_radius.min(self.width / 2).min(self.height / 2);
        for sy in 0..self.height {
            for sx in 0..self.width {
                // Skip pixels outside the rounded corners.
                if r > 0 && Self::is_outside_radius(sx, sy, self.width, self.height, r) {
                    continue;
                }
                let tx = ox + sx;
                let ty = oy + sy;
                if let Some(px) = target.get_pixel_mut(tx, ty) {
                    crate::framebuffer::blend_over(px, &self.color, effective);
                }
            }
        }
    }
}

// ─── TextLayer ──────────────────────────────────────────────────

/// Text horizontal alignment.
///
/// Controls how multi-line text is positioned relative to the
/// layer's `(x, y)` origin. The origin marks the start of each
/// line for `Left`, the centre of each line for `Center`, and
/// the end of each line for `Right`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum TextAlignment {
    /// Left-aligned (default). Each line starts at `x`.
    #[default]
    Left,
    /// Centred. Each line is horizontally centred on `x`.
    Center,
    /// Right-aligned. Each line ends at `x`.
    Right,
}

/// Source of font data for text rendering.
///
/// Only available when the `font-rasterizer` Cargo feature is
/// enabled. The default is [`FontSource::Bundled`], which uses
/// the compiled-in Fira Mono Regular font (~174KB, SIL OFL
/// licensed).
#[cfg(feature = "font-rasterizer")]
#[derive(Debug, Clone, Default)]
pub enum FontSource {
    /// Use the bundled default monospace font (Fira Mono Regular,
    /// SIL OFL licensed, embedded in the binary at compile time).
    #[default]
    Bundled,
    /// Load a TrueType or OpenType font from the given file path
    /// at first render. **Panics** if the file does not exist or is
    /// not a valid TTF/OTF font.
    Path(std::path::PathBuf),
    /// Use raw font bytes (TTF/OTF format). The bytes must remain
    /// valid for the lifetime of the program. **Panics** if the
    /// bytes are not a valid TTF/OTF font.
    Bytes(&'static [u8]),
}

/// Embedded Fira Mono Regular TrueType font data (~174KB, SIL
/// OFL licensed). Bundled at compile time via `include_bytes!`.
/// Used as the default font when the `font-rasterizer` feature
/// is enabled.
#[cfg(feature = "font-rasterizer")]
const BUNDLED_FONT_DATA: &[u8] = include_bytes!("../assets/FiraMono-Regular.ttf");

/// A text layer that renders UTF-8 text into the framebuffer
/// using glyph rasterization.
///
/// When the `font-rasterizer` Cargo feature is enabled, text is
/// rendered using the bundled Fira Mono font (or a custom font
/// via [`FontSource`]) with per-pixel alpha blending. Without
/// the feature, text renders as a solid-coloured placeholder
/// block (one cell per Unicode scalar value) for layout and
/// z-order verification.
///
/// The text content is always available via
/// [`TextLayer::render_glyph`] regardless of feature flags.
#[derive(Debug)]
pub struct TextLayer {
    /// Left edge, inclusive.
    pub x: u32,
    /// Top edge, inclusive.
    pub y: u32,
    /// The text content (UTF-8).
    pub text: String,
    /// `[R, G, B, A]` in `0..=255` per channel.
    pub color: [u8; 4],
    z: u32,
    name: String,
    /// Horizontal text alignment (default: Left).
    alignment: TextAlignment,
    /// Font size in pixels (only used when `font-rasterizer` is
    /// enabled). Default: 14.0.
    #[cfg(feature = "font-rasterizer")]
    font_size: f32,
    /// Font source (only used when `font-rasterizer` is enabled).
    /// Default: [`FontSource::Bundled`].
    #[cfg(feature = "font-rasterizer")]
    font_source: FontSource,
    /// Lazily initialised font engine; loaded on the first call
    /// to [`TextLayer::text_width`] or [`Layer::render`]. The
    /// bundled font is known-good and will never fail.
    #[cfg(feature = "font-rasterizer")]
    font: std::sync::OnceLock<fontdue::Font>,
}

impl TextLayer {
    /// Creates a new text layer at `(x, y)` with the given
    /// `text` and RGBA `color`. Uses the bundled Fira Mono font
    /// at 14px when the `font-rasterizer` feature is enabled;
    /// falls back to the solid-block placeholder otherwise.
    pub fn new(x: u32, y: u32, text: impl Into<String>, color: [u8; 4]) -> Self {
        let text = text.into();
        Self {
            x,
            y,
            text,
            color,
            z: 0,
            name: "TextLayer".to_owned(),
            alignment: TextAlignment::Left,
            #[cfg(feature = "font-rasterizer")]
            font_size: 14.0,
            #[cfg(feature = "font-rasterizer")]
            font_source: FontSource::Bundled,
            #[cfg(feature = "font-rasterizer")]
            font: std::sync::OnceLock::new(),
        }
    }

    /// Builder: sets the default z-order.
    #[must_use]
    pub fn with_z(mut self, z: u32) -> Self {
        self.z = z;
        self
    }

    /// Builder: sets a human-readable name.
    #[must_use]
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    /// Builder: sets the horizontal text alignment.
    #[must_use]
    pub fn with_alignment(mut self, alignment: TextAlignment) -> Self {
        self.alignment = alignment;
        self
    }

    /// Returns the current text alignment.
    pub fn alignment(&self) -> TextAlignment {
        self.alignment
    }

    /// Computes the starting x-coordinate for a new line based on
    /// the alignment setting. `ox` is the base x-offset.
    fn line_start_x(&self, ox: i32, line_width: i32) -> i32 {
        match self.alignment {
            TextAlignment::Left => ox,
            TextAlignment::Center => ox - line_width / 2,
            TextAlignment::Right => ox - line_width,
        }
    }

    /// Returns the text content.
    pub fn render_glyph(&self) -> &str {
        &self.text
    }

    /// The font size in pixels (only meaningful when the
    /// `font-rasterizer` feature is enabled). Default: 14.0.
    #[cfg(feature = "font-rasterizer")]
    pub fn font_size(&self) -> f32 {
        self.font_size
    }

    /// Builder: sets a custom font source and pixel size.
    /// Only available when the `font-rasterizer` feature is
    /// enabled.
    #[cfg(feature = "font-rasterizer")]
    #[must_use]
    pub fn with_font(mut self, source: FontSource, px_size: f32) -> Self {
        self.font_source = source;
        self.font_size = px_size;
        self
    }

    /// Builder: sets the font pixel size (e.g. 14.0, 18.0, 24.0).
    /// Uses the current font source. Only available when the
    /// `font-rasterizer` feature is enabled.
    #[cfg(feature = "font-rasterizer")]
    #[must_use]
    pub fn with_font_size(mut self, px_size: f32) -> Self {
        self.font_size = px_size;
        self
    }

    /// Total text advance width in pixels.
    ///
    /// When the `font-rasterizer` feature is enabled, this returns
    /// the maximum line width (using measured glyph advance widths).
    /// Without the feature, returns the maximum line length in
    /// Unicode scalar values. Empty text returns 0.
    pub fn text_width(&self) -> u32 {
        #[cfg(feature = "font-rasterizer")]
        {
            let font = self.ensure_font();
            self.text
                .lines()
                .map(|line| {
                    line.chars()
                        .map(|ch| {
                            let glyph_idx = font.lookup_glyph_index(ch);
                            let (metrics, _) = font.rasterize_indexed(glyph_idx, self.font_size);
                            metrics.advance_width as u32
                        })
                        .sum()
                })
                .max()
                .unwrap_or(0)
        }
        #[cfg(not(feature = "font-rasterizer"))]
        {
            self.text
                .lines()
                .map(|line| line.chars().count() as u32)
                .max()
                .unwrap_or(0)
        }
    }

    /// Number of visual lines in the text. A line is delimited by
    /// `\n`. Always at least 1 (empty text is one blank line).
    #[cfg(feature = "font-rasterizer")]
    pub fn num_lines(&self) -> u32 {
        if self.text.is_empty() {
            1
        } else {
            self.text.matches('\n').count() as u32 + 1
        }
    }

    /// Ensures the font is loaded (lazy initialisation). Returns
    /// a reference to the [`fontdue::Font`].
    #[cfg(feature = "font-rasterizer")]
    fn ensure_font(&self) -> &fontdue::Font {
        self.font.get_or_init(|| {
            let bytes: &[u8] = match &self.font_source {
                FontSource::Bundled => BUNDLED_FONT_DATA,
                FontSource::Path(path) => {
                    // Reading on first render; the caller is
                    // responsible for ensuring the font file
                    // is accessible.
                    &std::fs::read(path).expect("font-rasterizer: failed to read font file")
                }
                FontSource::Bytes(b) => b,
            };
            fontdue::Font::from_bytes(
                bytes,
                fontdue::FontSettings {
                    collection_index: 0,
                    scale: self.font_size,
                    load_substitutions: true,
                },
            )
            .expect("font-rasterizer: embedded font data is valid")
        })
    }
}

impl Layer for TextLayer {
    fn z_order(&self) -> u32 {
        self.z
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn bounds(&self) -> Option<Rect> {
        #[cfg(feature = "font-rasterizer")]
        {
            let w = self.text_width();
            let nl = self.num_lines();
            let h = (self.font_size as u32).max(1) * nl;
            let x = match self.alignment {
                TextAlignment::Left => self.x,
                TextAlignment::Center => self.x.saturating_sub(w / 2),
                TextAlignment::Right => self.x.saturating_sub(w),
            };
            Some(Rect::new(x, self.y, w.max(1), h.max(1)))
        }
        #[cfg(not(feature = "font-rasterizer"))]
        {
            // Must stay in sync with render_placeholder, which
            // uses .lines().count() — strip trailing empty lines.
            let nl = self.text.lines().count().max(1) as u32;
            let w = self.text_width();
            let x = match self.alignment {
                TextAlignment::Left => self.x,
                TextAlignment::Center => self.x.saturating_sub(w / 2),
                TextAlignment::Right => self.x.saturating_sub(w),
            };
            Some(Rect::new(x, self.y, w, nl))
        }
    }

    fn render(&self, target: &mut FrameBuffer, offset: (u32, u32), opacity: f32) {
        #[cfg(feature = "font-rasterizer")]
        {
            self.render_with_font(target, offset, opacity);
        }
        #[cfg(not(feature = "font-rasterizer"))]
        {
            self.render_placeholder(target, offset, opacity);
        }
    }
}

#[cfg(feature = "font-rasterizer")]
impl TextLayer {
    /// Actual glyph-rasterizing render path. Called when the
    /// `font-rasterizer` feature is enabled.
    fn render_with_font(&self, target: &mut FrameBuffer, offset: (u32, u32), opacity: f32) {
        let font = self.ensure_font();
        let ox = self.x.saturating_add(offset.0);
        let oy = self.y.saturating_add(offset.1);
        let effective = (f32::from(self.color[3]) / 255.0 * opacity).clamp(0.0, 1.0);
        let line_height = self.font_size as i32;

        // Approximate the first baseline at ~85% of font size
        // below the top of the first line.
        let first_baseline_y = oy as i32 + (self.font_size * 0.85) as i32;

        // Pre-compute line widths so we can apply alignment.
        let lines: Vec<&str> = self.text.lines().collect();
        let line_widths: Vec<i32> = lines
            .iter()
            .map(|line| {
                line.chars()
                    .map(|ch| {
                        let glyph_idx = font.lookup_glyph_index(ch);
                        let (metrics, _) = font.rasterize_indexed(glyph_idx, self.font_size);
                        metrics.advance_width as i32
                    })
                    .sum()
            })
            .collect();

        for (line_idx, line) in lines.iter().enumerate() {
            let line_w = line_widths[line_idx];
            let line_x = self.line_start_x(ox as i32, line_w);
            let mut cursor_x = line_x;
            let cursor_y = first_baseline_y + line_idx as i32 * line_height;

            for ch in line.chars() {
                if ch == ' ' {
                    let glyph_idx = font.lookup_glyph_index(ch);
                    let (metrics, _) = font.rasterize_indexed(glyph_idx, self.font_size);
                    cursor_x += metrics.advance_width as i32;
                    continue;
                }

                let glyph_idx = font.lookup_glyph_index(ch);
                let (metrics, alpha) = font.rasterize_indexed(glyph_idx, self.font_size);

                let glyph_x = cursor_x + metrics.xmin;
                let glyph_y = cursor_y + metrics.ymin;

                for gy in 0..metrics.height {
                    for gx in 0..metrics.width {
                        let px = glyph_x + gx as i32;
                        let py = glyph_y + gy as i32;
                        if px < 0 || py < 0 {
                            continue;
                        }
                        let alpha_val = alpha[gy * metrics.width + gx];
                        if alpha_val == 0 {
                            continue;
                        }
                        let glyph_alpha = f32::from(alpha_val) / 255.0 * effective;
                        if let Some(dst) = target.get_pixel_mut(px as u32, py as u32) {
                            crate::framebuffer::blend_over(dst, &self.color, glyph_alpha);
                        }
                    }
                }

                cursor_x += metrics.advance_width as i32;
            }
        }
    }
}

#[cfg(not(feature = "font-rasterizer"))]
impl TextLayer {
    /// Placeholder render path: draws a solid-colour block one
    /// cell per Unicode scalar value high and 1 pixel tall.
    /// Used when the `font-rasterizer` feature is disabled.
    fn render_placeholder(&self, target: &mut FrameBuffer, offset: (u32, u32), opacity: f32) {
        let ox = self.x.saturating_add(offset.0);
        let oy = self.y.saturating_add(offset.1);
        let effective = (f32::from(self.color[3]) / 255.0 * opacity).clamp(0.0, 1.0);

        for (line_idx, line) in self.text.lines().enumerate() {
            let w = line.chars().count() as u32;
            let ty = oy + line_idx as u32;
            let line_x = self.line_start_x(ox as i32, w as i32);
            for sx in 0..w {
                let tx = line_x as i32 + sx as i32;
                if tx >= 0 {
                    if let Some(px) = target.get_pixel_mut(tx as u32, ty) {
                        crate::framebuffer::blend_over(px, &self.color, effective);
                    }
                }
            }
        }
    }
}

// ─── ImageLayer ─────────────────────────────────────────────────

/// A raster image layer: pixel data decoded by the `image` crate
/// from a PNG or JPEG (the formats enabled by the `image-decoder`
/// Cargo feature). Only available when that feature is enabled.
///
/// `bounds()` reports the image's decoded size at the layer's
/// `(x, y)`; `render` copies pixels into the target with
/// per-pixel alpha blending and the additive `offset` translation.
#[cfg(feature = "image-decoder")]
pub struct ImageLayer {
    /// Left edge, inclusive.
    pub x: u32,
    /// Top edge, inclusive.
    pub y: u32,
    pixels: image::RgbaImage,
    z: u32,
    name: String,
}

#[cfg(feature = "image-decoder")]
impl ImageLayer {
    /// Decodes the image at `path` (PNG or JPEG, depending on
    /// enabled features) and wraps it at position `(x, y)`.
    pub fn from_path<P: AsRef<std::path::Path>>(
        path: P,
        x: u32,
        y: u32,
    ) -> Result<Self, image::ImageError> {
        let img = image::open(path)?;
        Ok(Self::from_dynamic(img, x, y))
    }

    /// Wraps an already-decoded `image::DynamicImage` at position
    /// `(x, y)`.
    pub fn from_dynamic(img: image::DynamicImage, x: u32, y: u32) -> Self {
        let pixels = img.to_rgba8();
        let name = format!("Image({}x{})", pixels.width(), pixels.height());
        Self {
            x,
            y,
            pixels,
            z: 0,
            name,
        }
    }

    /// Decoded image width in pixels.
    pub fn width(&self) -> u32 {
        self.pixels.width()
    }

    /// Decoded image height in pixels.
    pub fn height(&self) -> u32 {
        self.pixels.height()
    }

    /// Builder: sets the default z-order.
    #[must_use]
    pub fn with_z(mut self, z: u32) -> Self {
        self.z = z;
        self
    }

    /// Builder: sets a human-readable name.
    #[must_use]
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }
}

#[cfg(feature = "image-decoder")]
impl std::fmt::Debug for ImageLayer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ImageLayer")
            .field("x", &self.x)
            .field("y", &self.y)
            .field("width", &self.pixels.width())
            .field("height", &self.pixels.height())
            .field("z", &self.z)
            .field("name", &self.name)
            .finish()
    }
}

#[cfg(feature = "image-decoder")]
impl Layer for ImageLayer {
    fn z_order(&self) -> u32 {
        self.z
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn bounds(&self) -> Option<Rect> {
        Some(Rect::new(
            self.x,
            self.y,
            self.pixels.width(),
            self.pixels.height(),
        ))
    }

    fn render(&self, target: &mut FrameBuffer, offset: (u32, u32), opacity: f32) {
        let ox = self.x.saturating_add(offset.0);
        let oy = self.y.saturating_add(offset.1);
        let img_w = self.pixels.width();
        let img_h = self.pixels.height();
        for sy in 0..img_h {
            for sx in 0..img_w {
                let tx = ox + sx;
                let ty = oy + sy;
                let Some(px) = target.get_pixel_mut(tx, ty) else {
                    continue;
                };
                let src = self.pixels.get_pixel(sx, sy).0;
                let src_alpha = f32::from(src[3]) / 255.0 * opacity;
                crate::framebuffer::blend_over(px, &src, src_alpha);
            }
        }
    }
}

// ─── BorderLayer ────────────────────────────────────────────────

/// A rectangular border layer: draws the outline of a rectangle
/// at a specific position and size. Unlike [`RectLayer`] which
/// fills the interior, `BorderLayer` only draws the edges.
///
/// `border_width` controls the thickness of the border in pixels.
/// A `border_width` of `1` draws a 1-pixel-wide outline. The
/// border is drawn inward from the rectangle's edges.
///
/// `bounds()` returns the full rectangle including the border.
/// `render` writes only the border pixels, leaving the interior
/// unchanged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BorderLayer {
    /// Left edge, inclusive.
    pub x: u32,
    /// Top edge, inclusive.
    pub y: u32,
    /// Width in cells/pixels (including border).
    pub width: u32,
    /// Height in cells/pixels (including border).
    pub height: u32,
    /// `[R, G, B, A]` in `0..=255` per channel.
    pub color: [u8; 4],
    /// Border thickness in pixels (drawn inward from edges).
    pub border_width: u32,
    z: u32,
    name: String,
}

impl BorderLayer {
    /// Creates a new border layer at `(x, y)` with the given
    /// `width x height`, RGBA `color`, and `border_width`.
    pub fn new(x: u32, y: u32, width: u32, height: u32, color: [u8; 4], border_width: u32) -> Self {
        Self {
            x,
            y,
            width,
            height,
            color,
            border_width,
            z: 0,
            name: format!("Border({x},{y},{width}x{height},bw={border_width})"),
        }
    }

    /// Builder: sets the default z-order.
    #[must_use]
    pub fn with_z(mut self, z: u32) -> Self {
        self.z = z;
        self
    }

    /// Builder: sets a human-readable name.
    #[must_use]
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }
}

impl Layer for BorderLayer {
    fn z_order(&self) -> u32 {
        self.z
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn bounds(&self) -> Option<Rect> {
        Some(Rect::new(self.x, self.y, self.width, self.height))
    }

    fn render(&self, target: &mut FrameBuffer, offset: (u32, u32), opacity: f32) {
        let ox = self.x.saturating_add(offset.0);
        let oy = self.y.saturating_add(offset.1);
        let effective = (f32::from(self.color[3]) / 255.0 * opacity).clamp(0.0, 1.0);
        if effective == 0.0 {
            return;
        }
        if self.border_width == 0 {
            return;
        }
        let bw = self.border_width.min(self.width).min(self.height);
        if bw == 0 {
            return;
        }
        // Top edge
        for sy in 0..bw {
            for sx in 0..self.width {
                let tx = ox + sx;
                let ty = oy + sy;
                if let Some(px) = target.get_pixel_mut(tx, ty) {
                    crate::framebuffer::blend_over(px, &self.color, effective);
                }
            }
        }
        // Bottom edge
        for sy in (self.height - bw)..self.height {
            for sx in 0..self.width {
                let tx = ox + sx;
                let ty = oy + sy;
                if let Some(px) = target.get_pixel_mut(tx, ty) {
                    crate::framebuffer::blend_over(px, &self.color, effective);
                }
            }
        }
        // Left edge
        for sy in bw..(self.height - bw) {
            for sx in 0..bw {
                let tx = ox + sx;
                let ty = oy + sy;
                if let Some(px) = target.get_pixel_mut(tx, ty) {
                    crate::framebuffer::blend_over(px, &self.color, effective);
                }
            }
        }
        // Right edge
        for sy in bw..(self.height - bw) {
            for sx in (self.width - bw)..self.width {
                let tx = ox + sx;
                let ty = oy + sy;
                if let Some(px) = target.get_pixel_mut(tx, ty) {
                    crate::framebuffer::blend_over(px, &self.color, effective);
                }
            }
        }
    }
}

// ─── CanvasLayer ────────────────────────────────────────────────

/// A freeform drawing canvas layer.
///
/// `CanvasLayer` provides a pixel-level drawing API that users
/// can draw into before the compositor renders it. Unlike other
/// layer types which are created with a fixed shape, `CanvasLayer`
/// starts as a transparent buffer and offers methods to draw
/// individual pixels, lines, and circles.
///
/// After drawing, the layer composites its pixels into the target
/// framebuffer using standard alpha blending.
#[derive(Debug, Clone)]
pub struct CanvasLayer {
    /// Left edge, inclusive.
    pub x: u32,
    /// Top edge, inclusive.
    pub y: u32,
    width: u32,
    height: u32,
    pixels: Vec<[u8; 4]>,
    z: u32,
    name: String,
}

impl CanvasLayer {
    /// Creates a new empty canvas of the given dimensions.
    /// All pixels start as fully transparent.
    pub fn new(width: u32, height: u32) -> Self {
        let count = (width as usize).saturating_mul(height as usize);
        Self {
            x: 0,
            y: 0,
            width,
            height,
            pixels: vec![[0, 0, 0, 0]; count],
            z: 0,
            name: format!("Canvas({width}x{height})"),
        }
    }

    /// Sets the position of this canvas in the framebuffer.
    #[must_use]
    pub fn at(mut self, x: u32, y: u32) -> Self {
        self.x = x;
        self.y = y;
        self
    }

    /// Builder: sets the default z-order.
    #[must_use]
    pub fn with_z(mut self, z: u32) -> Self {
        self.z = z;
        self
    }

    /// Builder: sets a human-readable name.
    #[must_use]
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    /// Canvas width in pixels.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Canvas height in pixels.
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Draws a single pixel at `(px, py)` in canvas-local
    /// coordinates. Coordinates outside the canvas are silently
    /// ignored.
    pub fn draw_pixel(&mut self, px: u32, py: u32, color: [u8; 4]) {
        if px < self.width && py < self.height {
            let idx = (py as usize) * (self.width as usize) + (px as usize);
            self.pixels[idx] = color;
        }
    }

    /// Returns a reference to the pixel at `(px, py)` in
    /// canvas-local coordinates, or `None` if out of bounds.
    pub fn get_pixel(&self, px: u32, py: u32) -> Option<[u8; 4]> {
        if px < self.width && py < self.height {
            let idx = (py as usize) * (self.width as usize) + (px as usize);
            Some(self.pixels[idx])
        } else {
            None
        }
    }

    /// Draws a line from `(x0, y0)` to `(x1, y1)` using
    /// Bresenham's line algorithm.
    pub fn draw_line(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, color: [u8; 4]) {
        let dx = (x1 - x0).abs();
        let dy = -(y1 - y0).abs();
        let sx = if x0 < x1 { 1 } else { -1 };
        let sy = if y0 < y1 { 1 } else { -1 };
        let mut err = dx + dy;
        let mut x = x0;
        let mut y = y0;
        loop {
            if x >= 0 && y >= 0 {
                self.draw_pixel(x as u32, y as u32, color);
            }
            if x == x1 && y == y1 {
                break;
            }
            let e2 = 2 * err;
            if e2 >= dy {
                err += dy;
                x += sx;
            }
            if e2 <= dx {
                err += dx;
                y += sy;
            }
        }
    }

    /// Draws a circle centered at `(cx, cy)` with the given
    /// `radius` using the midpoint circle algorithm.
    pub fn draw_circle(&mut self, cx: i32, cy: i32, radius: i32, color: [u8; 4]) {
        if radius < 0 {
            return;
        }
        let mut x = radius;
        let mut y = 0;
        let mut err = 1 - radius;
        while x >= y {
            self.draw_pixel((cx + x) as u32, (cy + y) as u32, color);
            self.draw_pixel((cx - x) as u32, (cy + y) as u32, color);
            self.draw_pixel((cx + x) as u32, (cy - y) as u32, color);
            self.draw_pixel((cx - x) as u32, (cy - y) as u32, color);
            self.draw_pixel((cx + y) as u32, (cy + x) as u32, color);
            self.draw_pixel((cx - y) as u32, (cy + x) as u32, color);
            self.draw_pixel((cx + y) as u32, (cy - x) as u32, color);
            self.draw_pixel((cx - y) as u32, (cy - x) as u32, color);
            y += 1;
            if err < 0 {
                err += 2 * y + 1;
            } else {
                x -= 1;
                err += 2 * (y - x) + 1;
            }
        }
    }

    /// Fills a rectangle in canvas-local coordinates with the
    /// given colour.
    pub fn fill_rect(&mut self, rx: u32, ry: u32, rw: u32, rh: u32, color: [u8; 4]) {
        for sy in ry..ry.saturating_add(rh) {
            for sx in rx..rx.saturating_add(rw) {
                self.draw_pixel(sx, sy, color);
            }
        }
    }

    /// Clears the canvas to fully transparent.
    pub fn clear(&mut self) {
        for px in &mut self.pixels {
            *px = [0, 0, 0, 0];
        }
    }
}

impl Layer for CanvasLayer {
    fn z_order(&self) -> u32 {
        self.z
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn bounds(&self) -> Option<Rect> {
        Some(Rect::new(self.x, self.y, self.width, self.height))
    }

    fn render(&self, target: &mut FrameBuffer, offset: (u32, u32), opacity: f32) {
        let ox = self.x.saturating_add(offset.0);
        let oy = self.y.saturating_add(offset.1);
        for sy in 0..self.height {
            for sx in 0..self.width {
                let src = self.pixels[(sy as usize) * (self.width as usize) + (sx as usize)];
                if src[3] == 0 {
                    continue;
                }
                let tx = ox + sx;
                let ty = oy + sy;
                let src_alpha = f32::from(src[3]) / 255.0 * opacity;
                if let Some(px) = target.get_pixel_mut(tx, ty) {
                    crate::framebuffer::blend_over(px, &src, src_alpha);
                }
            }
        }
    }
}

// ─── SceneGraph ─────────────────────────────────────────────────

/// A scene graph node that supports parent-child layer relationships
/// and grouped transforms.
pub struct SceneGraph {
    nodes: Vec<SceneNode>,
    root: usize,
    z: u32,
    name: String,
}

struct SceneNode {
    layer: Option<Box<dyn Layer>>,
    children: Vec<usize>,
    #[allow(dead_code)]
    parent: Option<usize>,
    local_offset: (i32, i32),
    local_opacity: f32,
    visible: bool,
}

impl SceneGraph {
    /// Creates a new empty scene graph.
    pub fn new() -> Self {
        let root = 0;
        Self {
            nodes: vec![SceneNode {
                layer: None,
                children: Vec::new(),
                parent: None,
                local_offset: (0, 0),
                local_opacity: 1.0,
                visible: true,
            }],
            root,
            z: 0,
            name: "SceneGraph".to_owned(),
        }
    }

    /// Builder: sets the default z-order.
    #[must_use]
    pub fn with_z(mut self, z: u32) -> Self {
        self.z = z;
        self
    }

    /// Builder: sets a human-readable name.
    #[must_use]
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    /// Adds a group node under the root. Returns the node index.
    pub fn add_group(&mut self, offset: (i32, i32), opacity: f32, visible: bool) -> usize {
        let idx = self.nodes.len();
        self.nodes.push(SceneNode {
            layer: None,
            children: Vec::new(),
            parent: Some(self.root),
            local_offset: offset,
            local_opacity: opacity,
            visible,
        });
        self.nodes[self.root].children.push(idx);
        idx
    }

    /// Adds a group node under a specific parent. Returns the node index.
    pub fn add_group_to(
        &mut self,
        parent: usize,
        offset: (i32, i32),
        opacity: f32,
        visible: bool,
    ) -> usize {
        let idx = self.nodes.len();
        self.nodes.push(SceneNode {
            layer: None,
            children: Vec::new(),
            parent: Some(parent),
            local_offset: offset,
            local_opacity: opacity,
            visible,
        });
        self.nodes[parent].children.push(idx);
        idx
    }

    /// Adds a leaf layer under the root. Returns the node index.
    pub fn add_child(&mut self, layer: impl Layer + 'static) -> usize {
        self.add_child_to(self.root, layer)
    }

    /// Adds a leaf layer under a specific parent. Returns the node index.
    pub fn add_child_to(&mut self, parent: usize, layer: impl Layer + 'static) -> usize {
        let idx = self.nodes.len();
        self.nodes.push(SceneNode {
            layer: Some(Box::new(layer)),
            children: Vec::new(),
            parent: Some(parent),
            local_offset: (0, 0),
            local_opacity: 1.0,
            visible: true,
        });
        self.nodes[parent].children.push(idx);
        idx
    }

    /// Sets the visibility of a node.
    pub fn set_visible(&mut self, idx: usize, visible: bool) {
        if let Some(node) = self.nodes.get_mut(idx) {
            node.visible = visible;
        }
    }

    /// Sets the opacity of a node.
    pub fn set_opacity(&mut self, idx: usize, opacity: f32) {
        if let Some(node) = self.nodes.get_mut(idx) {
            node.local_opacity = opacity;
        }
    }

    /// Sets the offset of a node.
    pub fn set_offset(&mut self, idx: usize, offset: (i32, i32)) {
        if let Some(node) = self.nodes.get_mut(idx) {
            node.local_offset = offset;
        }
    }

    fn render_node(
        &self,
        idx: usize,
        target: &mut FrameBuffer,
        parent_offset: (i32, i32),
        parent_opacity: f32,
        parent_visible: bool,
    ) {
        let node = &self.nodes[idx];
        let visible = parent_visible && node.visible;
        let opacity = parent_opacity * node.local_opacity;
        let offset = (
            parent_offset.0 + node.local_offset.0,
            parent_offset.1 + node.local_offset.1,
        );

        if visible && opacity > 0.0 {
            if let Some(layer) = &node.layer {
                let abs_offset = (offset.0.max(0) as u32, offset.1.max(0) as u32);
                layer.render(target, abs_offset, opacity);
            }
        }

        for &child in &node.children {
            self.render_node(child, target, offset, opacity, visible);
        }
    }
}

impl Default for SceneGraph {
    fn default() -> Self {
        Self::new()
    }
}

impl Layer for SceneGraph {
    fn z_order(&self) -> u32 {
        self.z
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn bounds(&self) -> Option<Rect> {
        None
    }

    fn render(&self, target: &mut FrameBuffer, offset: (u32, u32), opacity: f32) {
        let parent_offset = (offset.0 as i32, offset.1 as i32);
        self.render_node(self.root, target, parent_offset, opacity, true);
    }
}

// ─── DropShadow ─────────────────────────────────────────────────

/// A wrapper layer that adds a drop shadow behind any inner layer.
pub struct DropShadow {
    inner: Box<dyn Layer>,
    /// Shadow offset in pixels (x, y).
    pub offset: (i32, i32),
    /// Box blur radius in pixels.
    pub blur_radius: u32,
    /// Shadow colour `[R, G, B, A]`.
    pub shadow_color: [u8; 4],
    /// Shadow spread in pixels.
    pub spread: i32,
    z: u32,
    name: String,
}

/// Type alias for [`DropShadow`].
pub type ShadowLayer = DropShadow;

impl DropShadow {
    /// Creates a new drop shadow wrapper around the given layer.
    pub fn new(inner: Box<dyn Layer>) -> Self {
        Self {
            inner,
            offset: (2, 2),
            blur_radius: 2,
            shadow_color: [0, 0, 0, 80],
            spread: 0,
            z: 0,
            name: "DropShadow".to_owned(),
        }
    }

    /// Builder: sets the shadow offset in pixels.
    #[must_use]
    pub fn with_offset(mut self, x: i32, y: i32) -> Self {
        self.offset = (x, y);
        self
    }

    /// Builder: sets the box blur radius.
    #[must_use]
    pub fn with_blur(mut self, radius: u32) -> Self {
        self.blur_radius = radius;
        self
    }

    /// Builder: sets the shadow colour.
    #[must_use]
    pub fn with_shadow_color(mut self, color: [u8; 4]) -> Self {
        self.shadow_color = color;
        self
    }

    /// Builder: sets the shadow spread in pixels.
    #[must_use]
    pub fn with_spread(mut self, spread: i32) -> Self {
        self.spread = spread;
        self
    }

    /// Builder: configures a glow effect.
    #[must_use]
    pub fn with_glow(self, color: [u8; 4], blur: u32) -> Self {
        self.with_shadow_color(color)
            .with_offset(0, 0)
            .with_blur(blur)
    }

    /// Builder: sets the default z-order.
    #[must_use]
    pub fn with_z(mut self, z: u32) -> Self {
        self.z = z;
        self
    }

    /// Builder: sets a human-readable name.
    #[must_use]
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    /// Returns a reference to the inner layer.
    pub fn inner(&self) -> &dyn Layer {
        &*self.inner
    }

    fn spread_alpha(pixels: &mut [[u8; 4]], width: u32, height: u32, spread: i32) {
        if spread == 0 || width == 0 || height == 0 {
            return;
        }
        let w = width as usize;
        let h = height as usize;
        let s = spread.unsigned_abs() as usize;
        let mut out = vec![[0u8; 4]; w * h];

        for y in 0..h {
            for x in 0..w {
                let mut max_alpha: u8 = 0;
                let mut min_alpha: u8 = 255;
                for dy in y.saturating_sub(s)..=(y + s).min(h - 1) {
                    for dx in x.saturating_sub(s)..=(x + s).min(w - 1) {
                        let a = pixels[dy * w + dx][3];
                        max_alpha = max_alpha.max(a);
                        min_alpha = min_alpha.min(a);
                    }
                }
                let src = pixels[y * w + x];
                let alpha = if spread > 0 { max_alpha } else { min_alpha };
                out[y * w + x] = [src[0], src[1], src[2], alpha];
            }
        }

        pixels.copy_from_slice(&out);
    }

    fn box_blur(pixels: &mut [[u8; 4]], width: u32, height: u32, radius: u32) {
        if radius == 0 || width == 0 || height == 0 {
            return;
        }
        let w = width as usize;
        let h = height as usize;
        let r = radius as usize;
        let mut tmp = vec![[0u8; 4]; w * h];

        for y in 0..h {
            for x in 0..w {
                let mut sum = [0u32; 4];
                let mut count = 0u32;
                for kx in x.saturating_sub(r)..=(x + r).min(w - 1) {
                    let p = pixels[y * w + kx];
                    for c in 0..4 {
                        sum[c] += p[c] as u32;
                    }
                    count += 1;
                }
                for c in 0..4 {
                    tmp[y * w + x][c] = (sum[c] / count) as u8;
                }
            }
        }

        for y in 0..h {
            for x in 0..w {
                let mut sum = [0u32; 4];
                let mut count = 0u32;
                for ky in y.saturating_sub(r)..=(y + r).min(h - 1) {
                    let p = tmp[ky * w + x];
                    for c in 0..4 {
                        sum[c] += p[c] as u32;
                    }
                    count += 1;
                }
                for c in 0..4 {
                    pixels[y * w + x][c] = (sum[c] / count) as u8;
                }
            }
        }
    }
}

impl Layer for DropShadow {
    fn z_order(&self) -> u32 {
        self.z
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn bounds(&self) -> Option<Rect> {
        self.inner.bounds()
    }

    fn render(&self, target: &mut FrameBuffer, offset: (u32, u32), opacity: f32) {
        let effective = opacity.clamp(0.0, 1.0);
        if effective == 0.0 {
            return;
        }

        let w = target.width();
        let h = target.height();
        if w == 0 || h == 0 {
            return;
        }

        let mut shadow_buf = FrameBuffer::new(w, h);
        self.inner.render(&mut shadow_buf, offset, 1.0);

        if self.spread != 0 {
            Self::spread_alpha(shadow_buf.pixels_mut(), w, h, self.spread);
        }

        Self::box_blur(shadow_buf.pixels_mut(), w, h, self.blur_radius);

        let (sx, sy) = self.offset;
        let sc = self.shadow_color;
        for sy_row in 0..h {
            for sx_col in 0..w {
                let src = shadow_buf.pixels()[sy_row as usize * w as usize + sx_col as usize];
                if src[3] == 0 {
                    continue;
                }
                let shadow_alpha = f32::from(sc[3]) / 255.0 * effective;
                let dst_x = sx_col as i32 + sx;
                let dst_y = sy_row as i32 + sy;
                if dst_x >= 0 && dst_y >= 0 {
                    let dx = dst_x as u32;
                    let dy = dst_y as u32;
                    if let Some(px) = target.get_pixel_mut(dx, dy) {
                        crate::framebuffer::blend_over(px, &sc, shadow_alpha);
                    }
                }
            }
        }

        self.inner.render(target, offset, effective);
    }
}

// ─── GradientLayer ──────────────────────────────────────────────

/// A gradient layer that interpolates between two colours.
#[derive(Debug, Clone)]
pub struct GradientLayer {
    /// Left edge, inclusive.
    pub x: u32,
    /// Top edge, inclusive.
    pub y: u32,
    /// Width of the gradient area.
    pub width: u32,
    /// Height of the gradient area.
    pub height: u32,
    /// `[R, G, B, A]` at the start of the gradient.
    pub start_color: [u8; 4],
    /// `[R, G, B, A]` at the end of the gradient.
    pub end_color: [u8; 4],
    /// The type of gradient.
    pub kind: GradientKind,
    z: u32,
    name: String,
}

/// The type of gradient interpolation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GradientKind {
    /// Linear gradient from `(start_x, start_y)` to `(end_x, end_y)`.
    Linear {
        /// Start X coordinate in pixels.
        start_x: u32,
        /// Start Y coordinate in pixels.
        start_y: u32,
        /// End X coordinate in pixels.
        end_x: u32,
        /// End Y coordinate in pixels.
        end_y: u32,
    },
    /// Radial gradient from `center_x, center_y` outward to `radius`.
    Radial {
        /// Center X coordinate in pixels.
        center_x: u32,
        /// Center Y coordinate in pixels.
        center_y: u32,
        /// Gradient radius in pixels.
        radius: u32,
    },
}

impl GradientLayer {
    /// Creates a new linear gradient layer.
    // TODO(v2.0): refactor to builder pattern to reduce argument count
    #[allow(clippy::too_many_arguments)]
    pub fn linear(
        x: u32,
        y: u32,
        width: u32,
        height: u32,
        start_color: [u8; 4],
        end_color: [u8; 4],
        start_x: u32,
        start_y: u32,
        end_x: u32,
        end_y: u32,
    ) -> Self {
        Self {
            x,
            y,
            width,
            height,
            start_color,
            end_color,
            kind: GradientKind::Linear {
                start_x,
                start_y,
                end_x,
                end_y,
            },
            z: 0,
            name: "GradientLayer".to_owned(),
        }
    }

    /// Creates a new radial gradient layer.
    // TODO(v2.0): refactor to builder pattern to reduce argument count
    #[allow(clippy::too_many_arguments)]
    pub fn radial(
        x: u32,
        y: u32,
        width: u32,
        height: u32,
        start_color: [u8; 4],
        end_color: [u8; 4],
        center_x: u32,
        center_y: u32,
        radius: u32,
    ) -> Self {
        Self {
            x,
            y,
            width,
            height,
            start_color,
            end_color,
            kind: GradientKind::Radial {
                center_x,
                center_y,
                radius,
            },
            z: 0,
            name: "GradientLayer".to_owned(),
        }
    }

    /// Builder: sets the default z-order.
    #[must_use]
    pub fn with_z(mut self, z: u32) -> Self {
        self.z = z;
        self
    }

    /// Builder: sets a human-readable name.
    #[must_use]
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    fn interpolate(t: f32, start: [u8; 4], end: [u8; 4]) -> [u8; 4] {
        let t = t.clamp(0.0, 1.0);
        let inv = 1.0 - t;
        [
            (f32::from(start[0]) * inv + f32::from(end[0]) * t).round() as u8,
            (f32::from(start[1]) * inv + f32::from(end[1]) * t).round() as u8,
            (f32::from(start[2]) * inv + f32::from(end[2]) * t).round() as u8,
            (f32::from(start[3]) * inv + f32::from(end[3]) * t).round() as u8,
        ]
    }
}

impl Layer for GradientLayer {
    fn z_order(&self) -> u32 {
        self.z
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn bounds(&self) -> Option<Rect> {
        Some(Rect::new(self.x, self.y, self.width, self.height))
    }

    fn render(&self, target: &mut FrameBuffer, offset: (u32, u32), opacity: f32) {
        let ox = self.x.saturating_add(offset.0);
        let oy = self.y.saturating_add(offset.1);
        let effective = opacity.clamp(0.0, 1.0);
        if effective == 0.0 {
            return;
        }

        match self.kind {
            GradientKind::Linear {
                start_x,
                start_y,
                end_x,
                end_y,
            } => {
                let dx = end_x as f32 - start_x as f32;
                let dy = end_y as f32 - start_y as f32;
                let len_sq = dx * dx + dy * dy;

                for sy in 0..self.height {
                    for sx in 0..self.width {
                        let tx = ox + sx;
                        let ty = oy + sy;
                        let Some(px) = target.get_pixel_mut(tx, ty) else {
                            continue;
                        };

                        let px_f = sx as f32 - start_x as f32;
                        let py_f = sy as f32 - start_y as f32;
                        let t = if len_sq == 0.0 {
                            0.0
                        } else {
                            (px_f * dx + py_f * dy) / len_sq
                        };

                        let colour = Self::interpolate(t, self.start_color, self.end_color);
                        let src_alpha = f32::from(colour[3]) / 255.0 * effective;
                        crate::framebuffer::blend_over(px, &colour, src_alpha);
                    }
                }
            }
            GradientKind::Radial {
                center_x,
                center_y,
                radius,
            } => {
                let r_f = radius as f32;
                let cx_f = center_x as f32;
                let cy_f = center_y as f32;

                for sy in 0..self.height {
                    for sx in 0..self.width {
                        let tx = ox + sx;
                        let ty = oy + sy;
                        let Some(px) = target.get_pixel_mut(tx, ty) else {
                            continue;
                        };

                        let dx = sx as f32 - cx_f;
                        let dy = sy as f32 - cy_f;
                        let dist = (dx * dx + dy * dy).sqrt();
                        let t = if r_f == 0.0 { 0.0 } else { dist / r_f };

                        let colour = Self::interpolate(t, self.start_color, self.end_color);
                        let src_alpha = f32::from(colour[3]) / 255.0 * effective;
                        crate::framebuffer::blend_over(px, &colour, src_alpha);
                    }
                }
            }
        }
    }
}

// ─── SVGLayer ───────────────────────────────────────────────────

/// An SVG rendering layer. Requires the `svg-decoder` feature.
#[cfg(feature = "svg-renderer")]
pub struct SVGLayer {
    /// Left edge, inclusive.
    pub x: u32,
    /// Top edge, inclusive.
    pub y: u32,
    width: u32,
    height: u32,
    pixels: Vec<[u8; 4]>,
    z: u32,
    name: String,
}

// ─── ClipLayer ──────────────────────────────────────────────────

/// Clipping region for a [`ClipLayer`].
#[derive(Debug, Clone)]
pub enum ClipRegion {
    /// Clip to an explicit rectangle in target-space coordinates.
    Rect(Rect),
    /// Clip to the inner layer's own [`Layer::bounds`].
    LayerBounds,
}

/// A wrapper layer that clips its inner layer's rendering to a
/// rectangular region.
pub struct ClipLayer {
    inner: Box<dyn Layer>,
    region: ClipRegion,
    z: u32,
    name: String,
}

impl ClipLayer {
    /// Creates a new clip layer wrapping the given inner layer.
    pub fn new(inner: Box<dyn Layer>) -> Self {
        Self {
            inner,
            region: ClipRegion::LayerBounds,
            z: 0,
            name: "ClipLayer".to_owned(),
        }
    }

    /// Builder: sets the clipping region.
    #[must_use]
    pub fn with_region(mut self, region: ClipRegion) -> Self {
        self.region = region;
        self
    }

    /// Builder: sets the default z-order.
    #[must_use]
    pub fn with_z(mut self, z: u32) -> Self {
        self.z = z;
        self
    }

    /// Builder: sets a human-readable name.
    #[must_use]
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }
}

impl Layer for ClipLayer {
    fn z_order(&self) -> u32 {
        self.z
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn bounds(&self) -> Option<Rect> {
        match &self.region {
            ClipRegion::Rect(r) => Some(*r),
            ClipRegion::LayerBounds => self.inner.bounds(),
        }
    }

    fn render(&self, target: &mut FrameBuffer, offset: (u32, u32), opacity: f32) {
        let effective = opacity.clamp(0.0, 1.0);
        if effective == 0.0 {
            return;
        }

        let tw = target.width();
        let th = target.height();
        if tw == 0 || th == 0 {
            return;
        }

        // Resolve the clip rectangle in target-space coordinates.
        let clip = match &self.region {
            ClipRegion::Rect(r) => *r,
            ClipRegion::LayerBounds => match self.inner.bounds() {
                Some(b) => Rect::new(
                    b.x.saturating_add(offset.0),
                    b.y.saturating_add(offset.1),
                    b.width,
                    b.height,
                ),
                None => return,
            },
        };

        // Render the inner layer into a temp buffer, then copy only the clip region.
        let mut buf = FrameBuffer::new(tw, th);
        self.inner.render(&mut buf, offset, effective);

        for sy in clip.y..clip.y.saturating_add(clip.height).min(th) {
            for sx in clip.x..clip.x.saturating_add(clip.width).min(tw) {
                if let Some(src) = buf.get_pixel(sx, sy) {
                    if let Some(dst) = target.get_pixel_mut(sx, sy) {
                        crate::framebuffer::blend_over(dst, src, 1.0);
                    }
                }
            }
        }
    }
}

// ─── LayerEntry ─────────────────────────────────────────────────

/// A wrapper around a [`Layer`] that adds per-entry state such as
/// opacity, visibility, z-override, name override, transform, and
/// accessibility metadata.
pub struct LayerEntry {
    id: LayerId,
    layer: Box<dyn Layer>,
    opacity: f32,
    visible: bool,
    z_override: Option<u32>,
    name: Option<String>,
    transform: Option<crate::geometry::Transform>,
    accessibility: Option<AccessibilityMetadata>,
}

impl LayerEntry {
    /// Creates a new entry wrapping `layer` with the given
    /// `id`. The entry starts fully opaque, visible, with no
    /// z-override, no custom name, no transform, and no
    /// accessibility metadata.
    pub fn new(id: LayerId, layer: Box<dyn Layer>) -> Self {
        Self {
            id,
            layer,
            opacity: 1.0,
            visible: true,
            z_override: None,
            name: None,
            transform: None,
            accessibility: None,
        }
    }

    /// Returns the entry's id.
    pub fn id(&self) -> LayerId {
        self.id
    }

    /// Returns a reference to the wrapped layer.
    pub fn layer(&self) -> &dyn Layer {
        &*self.layer
    }

    /// Returns the entry's opacity in `0.0..=1.0`.
    pub fn opacity(&self) -> f32 {
        self.opacity
    }

    /// Sets the entry's opacity, clamping to `0.0..=1.0`.
    pub fn set_opacity(&mut self, opacity: f32) {
        self.opacity = opacity.clamp(0.0, 1.0);
    }

    /// Returns whether the entry is currently visible.
    pub fn is_visible(&self) -> bool {
        self.visible
    }

    /// Sets the entry's visibility.
    pub fn set_visible(&mut self, visible: bool) {
        self.visible = visible;
    }

    /// Returns the effective z-order: the z-override if set,
    /// otherwise the layer's own `z_order()`.
    pub fn effective_z(&self) -> u32 {
        self.z_override.unwrap_or_else(|| self.layer.z_order())
    }

    /// Sets an explicit z-order override.
    pub fn set_z_override(&mut self, z: u32) {
        self.z_override = Some(z);
    }

    /// Clears the z-order override, reverting to the layer's
    /// default.
    pub fn clear_z_override(&mut self) {
        self.z_override = None;
    }

    /// Returns the effective name: the custom name if set,
    /// otherwise the layer's own `name()`.
    pub fn name(&self) -> &str {
        self.name.as_deref().unwrap_or_else(|| self.layer.name())
    }

    /// Sets a custom name for this entry.
    pub fn set_name(&mut self, name: impl Into<String>) {
        self.name = Some(name.into());
    }

    /// Returns a reference to the current transform, if set.
    pub fn transform(&self) -> Option<&crate::geometry::Transform> {
        self.transform.as_ref()
    }

    /// Returns a mutable reference to the current transform, if set.
    pub fn transform_mut(&mut self) -> Option<&mut crate::geometry::Transform> {
        self.transform.as_mut()
    }

    /// Sets or clears the transform.
    pub fn set_transform(&mut self, transform: Option<crate::geometry::Transform>) {
        self.transform = transform;
    }

    /// Builder: sets the transform.
    #[must_use]
    pub fn with_transform(mut self, transform: crate::geometry::Transform) -> Self {
        self.transform = Some(transform);
        self
    }

    /// Consumes the entry and returns the wrapped layer.
    pub fn into_layer_box(self) -> Box<dyn Layer> {
        self.layer
    }

    /// Replaces the wrapped layer, preserving the entry's id
    /// and control state.
    pub fn set_layer(&mut self, layer: Box<dyn Layer>) {
        self.layer = layer;
    }

    // ─── Accessibility metadata ───────────────────────────────

    /// Returns a reference to the accessibility metadata, if set.
    pub fn accessibility(&self) -> Option<&AccessibilityMetadata> {
        self.accessibility.as_ref()
    }

    /// Returns a mutable reference to the accessibility metadata.
    /// If none exists, a default instance is created automatically.
    pub fn accessibility_mut(&mut self) -> &mut AccessibilityMetadata {
        self.accessibility
            .get_or_insert_with(AccessibilityMetadata::new)
    }

    /// Sets the accessibility metadata for this entry.
    pub fn set_accessibility(&mut self, meta: Option<AccessibilityMetadata>) {
        self.accessibility = meta;
    }

    /// Builder: sets the accessibility metadata.
    #[must_use]
    pub fn with_accessibility(mut self, meta: AccessibilityMetadata) -> Self {
        self.accessibility = Some(meta);
        self
    }
}

impl std::fmt::Debug for LayerEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LayerEntry")
            .field("id", &self.id)
            .field("name", &self.name())
            .field("opacity", &self.opacity)
            .field("visible", &self.visible)
            .field("z_override", &self.z_override)
            .field("effective_z", &self.effective_z())
            .field("accessibility", &self.accessibility)
            .finish()
    }
}

// ─── Tests ──────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn solid_color_defaults() {
        let s = SolidColor::new(1, 2, 3, 4).with_z(5).with_name("bg");
        assert_eq!(s.z_order(), 5);
        assert_eq!(s.name(), "bg");
    }

    #[test]
    fn rect_layer_defaults() {
        let r = RectLayer::new(1, 2, 3, 4, [5; 4])
            .with_z(7)
            .with_name("box");
        assert_eq!(r.z_order(), 7);
        assert_eq!(r.name(), "box");
    }

    #[test]
    fn text_layer_defaults() {
        let t = TextLayer::new(1, 2, "hi", [3; 4])
            .with_z(3)
            .with_name("label");
        assert_eq!(t.z_order(), 3);
        assert_eq!(t.name(), "label");
    }

    // ─── Accessibility tests ────────────────────────────────

    #[test]
    fn accessibility_metadata_defaults() {
        let meta = AccessibilityMetadata::new();
        assert!(meta.alt_text().is_none());
        assert_eq!(meta.role(), SemanticRole::None);
    }

    #[test]
    fn accessibility_metadata_builder() {
        let meta = AccessibilityMetadata::new()
            .with_alt_text("Status indicator")
            .with_role(SemanticRole::Status);
        assert_eq!(meta.alt_text(), Some("Status indicator"));
        assert_eq!(meta.role(), SemanticRole::Status);
    }

    #[test]
    fn accessibility_metadata_setters() {
        let mut meta = AccessibilityMetadata::new();
        meta.set_alt_text("Button label");
        assert_eq!(meta.alt_text(), Some("Button label"));
        meta.set_role(SemanticRole::Button);
        assert_eq!(meta.role(), SemanticRole::Button);
        meta.clear_alt_text();
        assert!(meta.alt_text().is_none());
    }

    #[test]
    fn layer_entry_accessibility() {
        let entry = LayerEntry::new(0, Box::new(SolidColor::new(0, 0, 0, 255))).with_accessibility(
            AccessibilityMetadata::new()
                .with_alt_text("Background")
                .with_role(SemanticRole::Container),
        );
        let meta = entry.accessibility().unwrap();
        assert_eq!(meta.alt_text(), Some("Background"));
        assert_eq!(meta.role(), SemanticRole::Container);
    }

    #[test]
    fn layer_entry_accessibility_mut_creates_default() {
        let mut entry = LayerEntry::new(0, Box::new(SolidColor::new(0, 0, 0, 255)));
        assert!(entry.accessibility().is_none());
        entry.accessibility_mut().set_alt_text("Auto-created");
        assert_eq!(
            entry.accessibility().unwrap().alt_text(),
            Some("Auto-created")
        );
    }

    #[test]
    fn semantic_role_custom() {
        let role = SemanticRole::Custom("toggle");
        assert_eq!(role, SemanticRole::Custom("toggle"));
    }

    #[test]
    fn semantic_role_default_is_none() {
        assert_eq!(SemanticRole::default(), SemanticRole::None);
    }

    #[test]
    fn accessibility_metadata_debug() {
        let meta = AccessibilityMetadata::new()
            .with_alt_text("test")
            .with_role(SemanticRole::Button);
        let s = format!("{meta:?}");
        assert!(s.contains("test"));
        assert!(s.contains("Button"));
    }

    #[test]
    fn layer_entry_debug_includes_accessibility() {
        let e = LayerEntry::new(0, Box::new(SolidColor::new(0, 0, 0, 255).with_name("dbg")))
            .with_accessibility(
                AccessibilityMetadata::new()
                    .with_alt_text("hello")
                    .with_role(SemanticRole::Text),
            );
        let s = format!("{e:?}");
        assert!(s.contains("LayerEntry"));
        assert!(s.contains("dbg"));
        assert!(s.contains("hello"));
        assert!(s.contains("Text"));
    }
}

#[test]
fn text_alignment_defaults_to_left() {
    let t = TextLayer::new(0, 0, "hello", [255; 4]);
    assert_eq!(t.alignment(), TextAlignment::Left);
}

#[test]
fn text_alignment_builder() {
    let t = TextLayer::new(0, 0, "hello", [255; 4]).with_alignment(TextAlignment::Center);
    assert_eq!(t.alignment(), TextAlignment::Center);
}

#[test]
fn text_alignment_right() {
    let t = TextLayer::new(0, 0, "hello", [255; 4]).with_alignment(TextAlignment::Right);
    assert_eq!(t.alignment(), TextAlignment::Right);
}

#[test]
fn text_alignment_left_renders_at_x() {
    use super::*;
    let t = TextLayer::new(0, 0, "A", [255; 4]).with_alignment(TextAlignment::Left);
    assert_eq!(t.alignment(), TextAlignment::Left);
    assert_eq!(t.bounds().unwrap().x, 0);
}

#[test]
fn text_alignment_right_renders_before_x() {
    use super::*;
    let mut fb = FrameBuffer::new(20, 3);
    let t = TextLayer::new(15, 0, "AB", [255; 4]).with_alignment(TextAlignment::Right);
    t.render(&mut fb, (0, 0), 1.0);
    // Right-aligned: text should end at x=15, so pixels
    // should be at x < 15.
    let any_pixel_15plus = (15..20).any(|x| fb.get_pixel(x, 0).is_some_and(|p| p[3] > 0));
    assert!(
        !any_pixel_15plus,
        "right-aligned text should not render past x=15"
    );
}

#[test]
fn text_alignment_bounds_adjusts_x() {
    use super::*;
    let t_left = TextLayer::new(10, 5, "AB", [255; 4]).with_alignment(TextAlignment::Left);
    let t_right = TextLayer::new(10, 5, "AB", [255; 4]).with_alignment(TextAlignment::Right);
    let b_left = t_left.bounds().unwrap();
    let b_right = t_right.bounds().unwrap();
    assert_eq!(b_left.x, 10, "left-aligned bounds x should be 10");
    // Right-aligned: x should be <= 10 (shifted left).
    assert!(
        b_right.x <= 10,
        "right-aligned bounds x should be <= 10, got {}",
        b_right.x
    );
}

// ─── DropShadow tests ──────────────────────────────────────

#[test]
fn drop_shadow_new_defaults() {
    let inner = RectLayer::new(0, 0, 5, 5, [255; 4]);
    let ds = DropShadow::new(Box::new(inner));
    assert_eq!(ds.offset, (2, 2));
    assert_eq!(ds.blur_radius, 2);
    assert_eq!(ds.shadow_color, [0, 0, 0, 80]);
    assert_eq!(ds.spread, 0);
}

#[test]
fn drop_shadow_builder() {
    let inner = RectLayer::new(0, 0, 5, 5, [255; 4]);
    let ds = DropShadow::new(Box::new(inner))
        .with_offset(3, 4)
        .with_blur(5)
        .with_shadow_color([255, 0, 0, 128])
        .with_spread(2)
        .with_z(10)
        .with_name("shadow");
    assert_eq!(ds.offset, (3, 4));
    assert_eq!(ds.blur_radius, 5);
    assert_eq!(ds.shadow_color, [255, 0, 0, 128]);
    assert_eq!(ds.spread, 2);
    assert_eq!(ds.z_order(), 10);
    assert_eq!(ds.name(), "shadow");
}

#[test]
fn drop_shadow_glow_builder() {
    let inner = RectLayer::new(0, 0, 5, 5, [255; 4]);
    let ds = DropShadow::new(Box::new(inner)).with_glow([255, 200, 0, 200], 4);
    assert_eq!(ds.shadow_color, [255, 200, 0, 200]);
    assert_eq!(ds.offset, (0, 0));
    assert_eq!(ds.blur_radius, 4);
}

#[test]
fn drop_shadow_inner_reference() {
    let inner = RectLayer::new(0, 0, 5, 5, [255; 4]);
    let ds = DropShadow::new(Box::new(inner));
    assert_eq!(ds.inner().name(), "Rect(0,0,5x5)");
}

#[test]
fn drop_shadow_bounds_delegates_to_inner() {
    let inner = RectLayer::new(5, 10, 20, 15, [255; 4]);
    let ds = DropShadow::new(Box::new(inner));
    assert_eq!(ds.bounds(), Some(Rect::new(5, 10, 20, 15)));
}

#[test]
fn drop_shadow_render_does_not_panic() {
    let inner = RectLayer::new(0, 0, 3, 3, [255; 4]);
    let ds = DropShadow::new(Box::new(inner));
    let mut fb = FrameBuffer::new(20, 20);
    ds.render(&mut fb, (0, 0), 1.0);
    // Should not panic; some pixels should be non-transparent.
    let any_pixel = fb.get_pixel(0, 0).is_some_and(|p| p[3] > 0);
    assert!(any_pixel, "drop shadow should render some pixels");
}

#[test]
fn drop_shadow_zero_opacity_is_noop() {
    let inner = RectLayer::new(0, 0, 5, 5, [255; 4]);
    let ds = DropShadow::new(Box::new(inner));
    let mut fb = FrameBuffer::new(20, 20);
    ds.render(&mut fb, (0, 0), 0.0);
    assert!(fb.is_fully_transparent());
}

#[test]
fn drop_shadow_negative_spread() {
    let inner = RectLayer::new(0, 0, 10, 10, [255; 4]);
    let ds = DropShadow::new(Box::new(inner))
        .with_spread(-2)
        .with_blur(1);
    let mut fb = FrameBuffer::new(30, 30);
    ds.render(&mut fb, (0, 0), 1.0);
    // Negative spread should still render without panic.
    let any_pixel = fb.get_pixel(0, 0).is_some_and(|p| p[3] > 0);
    assert!(any_pixel, "negative spread should still render");
}

// ─── CanvasLayer tests ──────────────────────────────────────

#[test]
fn canvas_layer_defaults() {
    let c = CanvasLayer::new(10, 8);
    assert_eq!(c.width(), 10);
    assert_eq!(c.height(), 8);
    assert_eq!(c.x, 0);
    assert_eq!(c.y, 0);
}

#[test]
fn canvas_layer_at_sets_position() {
    let c = CanvasLayer::new(10, 8).at(5, 3);
    assert_eq!(c.x, 5);
    assert_eq!(c.y, 3);
}

#[test]
fn canvas_layer_draw_pixel_in_bounds() {
    let mut c = CanvasLayer::new(5, 5);
    c.draw_pixel(2, 3, [255, 0, 0, 255]);
    assert_eq!(c.get_pixel(2, 3), Some([255, 0, 0, 255]));
}

#[test]
fn canvas_layer_draw_pixel_out_of_bounds() {
    let mut c = CanvasLayer::new(5, 5);
    c.draw_pixel(10, 10, [255, 0, 0, 255]);
    assert_eq!(c.get_pixel(10, 10), None);
}

#[test]
fn canvas_layer_get_pixel_out_of_bounds() {
    let c = CanvasLayer::new(5, 5);
    assert_eq!(c.get_pixel(5, 0), None);
    assert_eq!(c.get_pixel(0, 5), None);
}

#[test]
fn canvas_layer_draw_circle_zero_radius() {
    let mut c = CanvasLayer::new(10, 10);
    c.draw_circle(5, 5, 0, [255, 0, 0, 255]);
    // Zero radius should draw only the center pixel.
    assert_eq!(c.get_pixel(5, 5), Some([255, 0, 0, 255]));
}

#[test]
fn canvas_layer_draw_circle_negative_radius() {
    let mut c = CanvasLayer::new(10, 10);
    c.draw_circle(5, 5, -1, [255, 0, 0, 255]);
    // Negative radius should be a no-op.
    assert!(c.get_pixel(0, 0).map_or(true, |p| p == [0u8, 0, 0, 0]));
}

#[test]
fn canvas_layer_fill_rect() {
    let mut c = CanvasLayer::new(10, 10);
    c.fill_rect(2, 2, 3, 3, [0, 255, 0, 255]);
    assert_eq!(c.get_pixel(2, 2), Some([0, 255, 0, 255]));
    assert_eq!(c.get_pixel(4, 4), Some([0, 255, 0, 255]));
    assert_eq!(c.get_pixel(5, 5), Some([0, 0, 0, 0]));
}

#[test]
fn canvas_layer_clear() {
    let mut c = CanvasLayer::new(5, 5);
    c.fill_rect(0, 0, 5, 5, [255; 4]);
    c.clear();
    assert!(c.get_pixel(0, 0).map_or(true, |p| p == [0u8, 0, 0, 0]));
}

#[test]
fn canvas_layer_draw_line_horizontal() {
    let mut c = CanvasLayer::new(10, 10);
    c.draw_line(0, 5, 9, 5, [255; 4]);
    for x in 0..10 {
        assert_eq!(c.get_pixel(x, 5), Some([255; 4]));
    }
}

#[test]
fn canvas_layer_render_does_not_panic() {
    let mut c = CanvasLayer::new(5, 5);
    c.draw_pixel(2, 2, [255; 4]);
    let mut fb = FrameBuffer::new(10, 10);
    c.render(&mut fb, (0, 0), 1.0);
    assert_eq!(fb.get_pixel(2, 2), Some(&[255; 4]));
}

#[test]
fn canvas_layer_bounds() {
    let c = CanvasLayer::new(10, 8).at(3, 4);
    assert_eq!(c.bounds(), Some(Rect::new(3, 4, 10, 8)));
}

// ─── GradientLayer tests ────────────────────────────────────

#[test]
fn gradient_linear_new() {
    let g = GradientLayer::linear(
        0,
        0,
        10,
        10,
        [255, 0, 0, 255],
        [0, 0, 255, 255],
        0,
        0,
        10,
        10,
    );
    assert_eq!(g.x, 0);
    assert_eq!(g.width, 10);
    assert_eq!(g.start_color, [255, 0, 0, 255]);
    assert_eq!(g.end_color, [0, 0, 255, 255]);
}

#[test]
fn gradient_radial_new() {
    let g = GradientLayer::radial(0, 0, 10, 10, [255; 4], [0; 4], 5, 5, 5);
    assert_eq!(g.x, 0);
    assert_eq!(g.width, 10);
    assert_eq!(
        g.kind,
        GradientKind::Radial {
            center_x: 5,
            center_y: 5,
            radius: 5
        }
    );
}

#[test]
fn gradient_builder() {
    let g = GradientLayer::linear(0, 0, 10, 10, [255; 4], [0; 4], 0, 0, 10, 10)
        .with_z(5)
        .with_name("grad");
    assert_eq!(g.z_order(), 5);
    assert_eq!(g.name(), "grad");
}

#[test]
fn gradient_bounds() {
    let g = GradientLayer::linear(5, 10, 20, 15, [255; 4], [0; 4], 0, 0, 20, 15);
    assert_eq!(g.bounds(), Some(Rect::new(5, 10, 20, 15)));
}

#[test]
fn gradient_render_does_not_panic() {
    let g = GradientLayer::linear(0, 0, 5, 5, [255, 0, 0, 255], [0, 0, 255, 255], 0, 0, 5, 5);
    let mut fb = FrameBuffer::new(10, 10);
    g.render(&mut fb, (0, 0), 1.0);
    let any_pixel = fb.get_pixel(0, 0).is_some_and(|p| p[3] > 0);
    assert!(any_pixel, "linear gradient should render pixels");
}

#[test]
fn gradient_radial_render_does_not_panic() {
    let g = GradientLayer::radial(0, 0, 10, 10, [255; 4], [0; 4], 5, 5, 5);
    let mut fb = FrameBuffer::new(15, 15);
    g.render(&mut fb, (0, 0), 1.0);
    // Check pixel near the center of the gradient (5,5) where alpha should be non-zero
    let center_pixel = fb.get_pixel(5, 5).is_some_and(|p| p[3] > 0);
    assert!(
        center_pixel,
        "radial gradient center should have non-zero alpha"
    );
}

#[test]
fn gradient_zero_length_line() {
    let g = GradientLayer::linear(0, 0, 5, 5, [255; 4], [0; 4], 2, 2, 2, 2);
    let mut fb = FrameBuffer::new(10, 10);
    g.render(&mut fb, (0, 0), 1.0);
    // Zero-length gradient line should render start_color at the gradient position
    let pixel = fb.get_pixel(2, 2).unwrap();
    assert_eq!(
        pixel,
        &[255, 255, 255, 255],
        "zero-length gradient should render start_color"
    );
}

// ─── SceneGraph tests ──────────────────────────────────────

#[test]
fn scene_graph_new() {
    let s = SceneGraph::new();
    assert_eq!(s.z_order(), 0);
    assert_eq!(s.name(), "SceneGraph");
}

#[test]
fn scene_graph_builder() {
    let s = SceneGraph::new().with_z(5).with_name("scene");
    assert_eq!(s.z_order(), 5);
    assert_eq!(s.name(), "scene");
}

#[test]
fn scene_graph_add_group() {
    let mut s = SceneGraph::new();
    let g = s.add_group((10, 5), 0.8, true);
    assert_eq!(g, 1); // root is 0, first group is 1
}

#[test]
fn scene_graph_add_child() {
    let mut s = SceneGraph::new();
    let c = s.add_child(RectLayer::new(0, 0, 5, 5, [255; 4]));
    assert_eq!(c, 1); // root is 0, first child is 1
}

#[test]
fn scene_graph_render_does_not_panic() {
    let mut s = SceneGraph::new();
    s.add_child(RectLayer::new(0, 0, 5, 5, [255; 4]));
    let mut fb = FrameBuffer::new(10, 10);
    s.render(&mut fb, (0, 0), 1.0);
}

#[test]
fn scene_graph_empty_render() {
    let s = SceneGraph::new();
    let mut fb = FrameBuffer::new(10, 10);
    s.render(&mut fb, (0, 0), 1.0);
    assert!(fb.is_fully_transparent());
}

// ─── ClipLayer tests ────────────────────────────────────────

#[test]
fn clip_layer_rect() {
    let inner = RectLayer::new(0, 0, 20, 20, [255; 4]);
    let clip =
        ClipLayer::new(Box::new(inner)).with_region(ClipRegion::Rect(Rect::new(5, 5, 10, 10)));
    let mut fb = FrameBuffer::new(30, 30);
    clip.render(&mut fb, (0, 0), 1.0);
    // Only the clipped region should have pixels.
    assert_eq!(fb.get_pixel(0, 0), Some(&[0, 0, 0, 0]));
    assert!(fb.get_pixel(6, 6).unwrap()[3] > 0);
}

#[test]
fn clip_layer_layer_bounds() {
    let inner = RectLayer::new(5, 5, 10, 10, [255; 4]);
    let clip = ClipLayer::new(Box::new(inner)).with_region(ClipRegion::LayerBounds);
    let mut fb = FrameBuffer::new(30, 30);
    clip.render(&mut fb, (0, 0), 1.0);
    // Should clip to the inner layer's bounds.
    assert_eq!(fb.get_pixel(4, 4), Some(&[0, 0, 0, 0]));
    assert!(fb.get_pixel(6, 6).unwrap()[3] > 0);
}

#[test]
fn clip_layer_builder() {
    let inner = RectLayer::new(0, 0, 10, 10, [255; 4]);
    let clip = ClipLayer::new(Box::new(inner)).with_z(5).with_name("clip");
    assert_eq!(clip.z_order(), 5);
    assert_eq!(clip.name(), "clip");
}

// ─── RectLayer edge case tests ──────────────────────────────

#[test]
fn rect_layer_border_radius_clamped() {
    let r = RectLayer::new(0, 0, 10, 10, [255; 4]).with_border_radius(100); // larger than min(w,h)/2
    let mut fb = FrameBuffer::new(20, 20);
    r.render(&mut fb, (0, 0), 1.0);
    // Check pixel inside the rect (5,5) - should be non-transparent
    let any_pixel = fb.get_pixel(5, 5).is_some_and(|p| p[3] > 0);
    assert!(any_pixel, "rounded rect should render center pixel");
}

#[test]
fn rect_layer_zero_size() {
    let r = RectLayer::new(5, 5, 0, 0, [255; 4]);
    let mut fb = FrameBuffer::new(10, 10);
    r.render(&mut fb, (0, 0), 1.0);
    assert!(fb.is_fully_transparent());
}

#[test]
fn rect_layer_zero_opacity() {
    let r = RectLayer::new(0, 0, 5, 5, [255; 4]);
    let mut fb = FrameBuffer::new(10, 10);
    r.render(&mut fb, (0, 0), 0.0);
    assert!(fb.is_fully_transparent());
}

// ─── TextLayer edge case tests ──────────────────────────────

#[test]
fn text_layer_empty_text() {
    let t = TextLayer::new(0, 0, "", [255; 4]);
    assert_eq!(t.text_width(), 0);
}

#[test]
fn text_layer_multiline() {
    let t = TextLayer::new(0, 0, "A\nB\nC", [255; 4]);
    #[cfg(feature = "font-rasterizer")]
    assert_eq!(t.num_lines(), 3);
    #[cfg(not(feature = "font-rasterizer"))]
    assert_eq!(t.text_width(), 1); // max line width
}

#[test]
fn text_layer_render_glyph() {
    let t = TextLayer::new(0, 0, "hello", [255; 4]);
    assert_eq!(t.render_glyph(), "hello");
}

#[test]
fn text_layer_debug() {
    let t = TextLayer::new(0, 0, "hi", [255; 4]).with_name("label");
    let s = format!("{t:?}");
    assert!(s.contains("TextLayer"));
    assert!(s.contains("label"));
}

// ─── LayerEntry edge case tests ──────────────────────────────

#[test]
fn layer_entry_into_layer_box() {
    let entry = LayerEntry::new(0, Box::new(SolidColor::new(0, 0, 0, 255)));
    let layer = entry.into_layer_box();
    assert_eq!(layer.name(), "SolidColor(r=0, g=0, b=0, a=255)");
}

#[test]
fn layer_entry_set_layer_preserves_state() {
    let mut entry = LayerEntry::new(5, Box::new(SolidColor::new(0, 0, 0, 255)));
    entry.set_opacity(0.5);
    entry.set_visible(false);
    entry.set_z_override(10);
    entry.set_name("custom");
    entry.set_layer(Box::new(RectLayer::new(0, 0, 1, 1, [255; 4])));
    // All state should be preserved.
    assert_eq!(entry.id(), 5);
    assert_eq!(entry.opacity(), 0.5);
    assert!(!entry.is_visible());
    assert_eq!(entry.effective_z(), 10);
    assert_eq!(entry.name(), "custom");
}

#[test]
fn layer_entry_transform_roundtrip() {
    use crate::geometry::Transform;
    let entry = LayerEntry::new(0, Box::new(SolidColor::new(0, 0, 0, 255)))
        .with_transform(Transform::new().with_rotation(45.0));
    assert!(entry.transform().is_some());
    assert_eq!(entry.transform().unwrap().rotation(), 45.0);
}

// ─── SolidColor edge case tests ──────────────────────────────

#[test]
fn solid_color_render() {
    let s = SolidColor::new(128, 64, 32, 200);
    let mut fb = FrameBuffer::new(5, 5);
    s.render(&mut fb, (0, 0), 1.0);
    assert_eq!(fb.get_pixel(2, 2), Some(&[128, 64, 32, 200]));
}

#[test]
fn solid_color_zero_opacity() {
    let s = SolidColor::new(255, 0, 0, 255);
    let mut fb = FrameBuffer::new(5, 5);
    s.render(&mut fb, (0, 0), 0.0);
    assert!(fb.is_fully_transparent());
}

#[test]
fn solid_color_half_opacity() {
    let s = SolidColor::new(255, 0, 0, 255);
    let mut fb = FrameBuffer::new(1, 1);
    s.render(&mut fb, (0, 0), 0.5);
    let px = fb.get_pixel(0, 0).unwrap();
    assert!(
        px[3] > 0 && px[3] < 255,
        "alpha should be between 0 and 255, got {}",
        px[3]
    );
}
