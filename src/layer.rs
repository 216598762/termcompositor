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
                            let (metrics, _) =
                                font.rasterize_indexed(glyph_idx, self.font_size);
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
                    &std::fs::read(path)
                        .expect("font-rasterizer: failed to read font file")
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
            Some(Rect::new(self.x, self.y, w.max(1), h.max(1)))
        }
        #[cfg(not(feature = "font-rasterizer"))]
        {
            // Must stay in sync with render_placeholder, which
            // uses .lines().count() — strip trailing empty lines.
            let nl = self.text.lines().count().max(1) as u32;
            Some(Rect::new(self.x, self.y, self.text_width(), nl))
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
        let mut cursor_x = ox as i32;
        let mut cursor_y = first_baseline_y;

        for ch in self.text.chars() {
            if ch == '\n' {
                cursor_x = ox as i32;
                cursor_y += line_height;
                continue;
            }
            if ch == ' ' {
                // Space: use the font's actual space advance width.
                let glyph_idx = font.lookup_glyph_index(ch);
                let (metrics, _) = font.rasterize_indexed(glyph_idx, self.font_size);
                cursor_x += metrics.advance_width as i32;
                continue;
            }

            let glyph_idx = font.lookup_glyph_index(ch);
            let (metrics, alpha) = font.rasterize_indexed(glyph_idx, self.font_size);

            // Position the glyph bitmap relative to the current
            // line's baseline.
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
            for sx in 0..w {
                let tx = ox + sx;
                if let Some(px) = target.get_pixel_mut(tx, ty) {
                    crate::framebuffer::blend_over(px, &self.color, effective);
                }
            }
        }
    }
}

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
        if self.border_width == 0 { return; }
        let bw = self.border_width.min(self.width).min(self.height);
        if bw == 0 {
            return;
        }
        // Top edge: rows y..y+bw, columns x..x+width
        for sy in 0..bw {
            for sx in 0..self.width {
                let tx = ox + sx;
                let ty = oy + sy;
                if let Some(px) = target.get_pixel_mut(tx, ty) {
                    crate::framebuffer::blend_over(px, &self.color, effective);
                }
            }
        }
        // Bottom edge: rows (y+height-bw)..(y+height), columns x..x+width
        for sy in (self.height - bw)..self.height {
            for sx in 0..self.width {
                let tx = ox + sx;
                let ty = oy + sy;
                if let Some(px) = target.get_pixel_mut(tx, ty) {
                    crate::framebuffer::blend_over(px, &self.color, effective);
                }
            }
        }
        // Left edge: rows (y+bw)..(y+height-bw), columns x..x+bw
        for sy in bw..(self.height - bw) {
            for sx in 0..bw {
                let tx = ox + sx;
                let ty = oy + sy;
                if let Some(px) = target.get_pixel_mut(tx, ty) {
                    crate::framebuffer::blend_over(px, &self.color, effective);
                }
            }
        }
        // Right edge: rows (y+bw)..(y+height-bw), columns (x+width-bw)..(x+width)
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
///
/// # Example
///
/// ```
/// use termcompositor::{CanvasLayer, Layer};
/// use termcompositor::FrameBuffer;
///
/// let mut canvas = CanvasLayer::new(20, 10);
/// canvas.draw_pixel(5, 3, [255, 0, 0, 255]);
/// canvas.draw_line(0, 0, 19, 9, [0, 255, 0, 255]);
/// canvas.draw_circle(10, 5, 4, [0, 0, 255, 255]);
///
/// let mut fb = FrameBuffer::new(20, 10);
/// canvas.render(&mut fb, (0, 0), 1.0);
/// ```
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
    /// Bresenham's line algorithm. Negative coordinates are
    /// silently clipped (via wrapping to `u32`); coordinates
    /// outside the canvas are ignored.
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
    /// `radius` using the midpoint circle algorithm. Negative
    /// coordinates are silently clipped (via wrapping to `u32`);
    /// coordinates outside the canvas are ignored.
    pub fn draw_circle(&mut self, cx: i32, cy: i32, radius: i32, color: [u8; 4]) {
        if radius < 0 {
            return;
        }
        let mut x = radius;
        let mut y = 0;
        let mut err = 1 - radius;
        while x >= y {
            // Draw 8 octants
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
    /// given colour. Coordinates outside the canvas are
    /// silently clipped.
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
                    continue; // Skip transparent pixels
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

/// A scene graph node that supports parent-child layer relationships
/// and grouped transforms.
///
/// `SceneGraph` is a tree of layers where each node can have children.
/// Transforms (offset, visibility, opacity) cascade from parent to child:
///
/// - **Visibility**: If a parent is invisible, all descendants are invisible.
/// - **Opacity**: Parent and child opacities multiply.
/// - **Offset**: Parent offset is added to child offset.
///
/// # Example
///
/// ```
/// use termcompositor::{SceneGraph, RectLayer, SolidColor, Layer, FrameBuffer};
///
/// let mut scene = SceneGraph::new();
/// let group = scene.add_group((10, 5), 0.8, true);
/// scene.add_child_to(group, RectLayer::new(0, 0, 5, 5, [255, 0, 0, 255]));
/// scene.add_child_to(group, RectLayer::new(6, 0, 5, 5, [0, 255, 0, 255]));
///
/// let mut fb = FrameBuffer::new(30, 20);
/// scene.render(&mut fb, (0, 0), 1.0);
/// ```
pub struct SceneGraph {
    nodes: Vec<SceneNode>,
    root: usize,
    z: u32,
    name: String,
}

struct SceneNode {
    layer: Option<Box<dyn Layer>>,
    children: Vec<usize>,
    parent: Option<usize>,
    /// Offset added to all children (accumulated from ancestors).
    local_offset: (i32, i32),
    /// Opacity multiplier (multiplied with parent opacity).
    local_opacity: f32,
    /// Visibility flag (false = all descendants hidden).
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

    /// Adds a group node (no visual layer, just a transform container).
    /// Returns the node index.
    pub fn add_group(
        &mut self,
        offset: (i32, i32),
        opacity: f32,
        visible: bool,
    ) -> usize {
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

    /// Recursively renders the scene graph into the target framebuffer.
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
                let abs_offset = (
                    offset.0.max(0) as u32,
                    offset.1.max(0) as u32,
                );
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
        None // Scene graph bounds depend on children; no fixed bounds.
    }

    fn render(&self, target: &mut FrameBuffer, offset: (u32, u32), opacity: f32) {
        let parent_offset = (offset.0 as i32, offset.1 as i32);
        self.render_node(self.root, target, parent_offset, opacity, true);
    }
}

/// A wrapper layer that adds a drop shadow behind any inner layer.
///
/// `DropShadow` renders the inner layer into a temporary buffer,
/// applies a box blur to create the shadow, composites the blurred
/// shadow at an offset, then composites the original layer on top.
///
/// # Example
///
/// ```
/// use termcompositor::{DropShadow, RectLayer, Layer, LayerStack};
/// use termcompositor::FrameBuffer;
///
/// let inner = RectLayer::new(5, 5, 10, 5, [255, 255, 255, 255]);
/// let shadow = DropShadow::new(Box::new(inner))
///     .with_offset(2, 2)
///     .with_blur(3)
///     .with_shadow_color([0, 0, 0, 128]);
///
/// let mut fb = FrameBuffer::new(30, 20);
/// shadow.render(&mut fb, (0, 0), 1.0);
/// ```
pub struct DropShadow {
    inner: Box<dyn Layer>,
    /// Shadow offset in pixels (x, y).
    pub offset: (i32, i32),
    /// Box blur radius in pixels. Higher values produce softer shadows.
    pub blur_radius: u32,
    /// Shadow colour `[R, G, B, A]`.
    pub shadow_color: [u8; 4],
    /// Shadow spread in pixels. Positive values dilate (expand)
    /// the shadow shape before blurring; negative values shrink it.
    pub spread: i32,
    z: u32,
    name: String,
}

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

    /// Builder: sets the shadow spread in pixels. Positive values
    /// dilate (expand) the shadow shape before blurring; negative
    /// values shrink it.
    #[must_use]
    pub fn with_spread(mut self, spread: i32) -> Self {
        self.spread = spread;
        self
    }

    /// Builder: configures a glow effect. This is a convenience
    /// method equivalent to setting a bright `shadow_color` with
    /// a zero offset and a moderate blur radius.
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

    /// Dilates (positive spread) or erodes (negative spread) the
    /// alpha channel in a pixel buffer. This is used to expand or
    /// shrink the shadow shape before blurring.
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
                // Find the max/min alpha in the neighborhood.
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

    /// Applies a simple box blur to the pixel buffer in-place.
    /// Uses a square kernel of size (2*radius+1) x (2*radius+1).
    fn box_blur(pixels: &mut [[u8; 4]], width: u32, height: u32, radius: u32) {
        if radius == 0 || width == 0 || height == 0 {
            return;
        }
        let w = width as usize;
        let h = height as usize;
        let r = radius as usize;
        let mut tmp = vec![[0u8; 4]; w * h];

        // Horizontal pass
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

        // Vertical pass
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

        // Step 1: Render inner layer to a temporary buffer.
        let mut shadow_buf = FrameBuffer::new(w, h);
        self.inner.render(&mut shadow_buf, offset, 1.0);

        // Step 2: Apply spread (dilate/erode) if nonzero.
        if self.spread != 0 {
            Self::spread_alpha(shadow_buf.pixels_mut(), w, h, self.spread);
        }

        // Step 3: Apply box blur to create the shadow.
        Self::box_blur(shadow_buf.pixels_mut(), w, h, self.blur_radius);

        // Step 3: Composite the blurred shadow at the offset.
        let (sx, sy) = self.offset;
        let sc = self.shadow_color;
        for sy_row in 0..h {
            for sx_col in 0..w {
                let src = shadow_buf.pixels()[sy_row as usize * w as usize + sx_col as usize];
                // Only composite where the inner layer had content.
                if src[3] == 0 {
                    continue;
                }
                // Apply shadow colour with its alpha.
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

        // Step 4: Composite the original layer on top.
        self.inner.render(target, offset, effective);
    }
}

/// A gradient layer that interpolates between two colours.
///
/// Supports two gradient types:
/// - **Linear**: interpolates from `start_color` to `end_color`
///   along a line defined by `(start_x, start_y)` →
///   `(end_x, end_y)`. The gradient extends across the
///   bounding box defined by `x, y, width, height`.
/// - **Radial**: interpolates from `start_color` to `end_color`
///   from `center_x, center_y` outward to `radius` pixels.
///
/// Colour interpolation is performed in sRGB space (per-channel
/// linear interpolation) which is simple and fast. For
/// perceptually uniform gradients, a future version could
/// interpolate in Oklab or Oklch.
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
    /// Linear gradient from `(start_x, start_y)` to `(end_x, end_y)`
    /// within the gradient's bounding box. Coordinates are relative
    /// to the gradient's top-left corner.
    Linear {
        /// Start X coordinate (relative to gradient origin).
        start_x: u32,
        /// Start Y coordinate (relative to gradient origin).
        start_y: u32,
        /// End X coordinate (relative to gradient origin).
        end_x: u32,
        /// End Y coordinate (relative to gradient origin).
        end_y: u32,
    },
    /// Radial gradient from `center_x, center_y` outward to `radius`
    /// pixels. Coordinates are relative to the gradient's top-left
    /// corner.
    Radial {
        /// Center X coordinate (relative to gradient origin).
        center_x: u32,
        /// Center Y coordinate (relative to gradient origin).
        center_y: u32,
        /// Radius in pixels.
        radius: u32,
    },
}

impl GradientLayer {
    /// Creates a new linear gradient layer.
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

    /// Interpolates between `start_color` and `end_color` at
    /// position `t` in `0.0..=1.0`. Returns the blended RGBA
    /// colour.
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
                // Compute the direction vector and its squared length.
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

                        // Project (sx, sy) onto the gradient line.
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
                let radius_f = radius as f32;
                let cx = center_x as f32;
                let cy = center_y as f32;

                for sy in 0..self.height {
                    for sx in 0..self.width {
                        let tx = ox + sx;
                        let ty = oy + sy;
                        let Some(px) = target.get_pixel_mut(tx, ty) else {
                            continue;
                        };

                        // Distance from the pixel to the center.
                        let dx = sx as f32 - cx;
                        let dy = sy as f32 - cy;
                        let dist = (dx * dx + dy * dy).sqrt();

                        let t = if radius_f == 0.0 {
                            0.0
                        } else {
                            dist / radius_f
                        };

                        let colour = Self::interpolate(t, self.start_color, self.end_color);
                        let src_alpha = f32::from(colour[3]) / 255.0 * effective;
                        crate::framebuffer::blend_over(px, &colour, src_alpha);
                    }
                }
            }
        }
    }
}

/// Type alias for [`DropShadow`]. Provides a more descriptive
/// name when the wrapper is used for shadow/glow effects.
pub type ShadowLayer = DropShadow;

/// An SVG rendering layer that rasterises SVG content into the framebuffer.
///
/// `SVGLayer` uses the `resvg` crate to parse and render SVG data (from
/// a string, file path, or bytes) into a pixel buffer, then composites
/// it into the target framebuffer with per-pixel alpha blending.
///
/// Only available when the `svg-renderer` Cargo feature is enabled.
///
/// # Example
///
/// ```ignore
/// use termcompositor::SVGLayer;
///
/// let svg = SVGLayer::from_str(
///     r#"<svg width="100" height="100"><circle cx="50" cy="50" r="40" fill="red"/></svg>"#,
///     0, 0,
/// ).unwrap();
/// ```
#[cfg(feature = "svg-renderer")]
pub struct SVGLayer {
    /// Left edge, inclusive.
    pub x: u32,
    /// Top edge, inclusive.
    pub y: u32,
    pixels: Vec<[u8; 4]>,
    width: u32,
    height: u32,
    z: u32,
    name: String,
}

#[cfg(feature = "svg-renderer")]
impl SVGLayer {
    /// Parses and renders SVG data from a string at position `(x, y)`.
    ///
    /// The SVG is rendered at its intrinsic size (width/height attributes
    /// or viewBox). Returns an error if the SVG cannot be parsed or
    /// rendered.
    pub fn from_str(svg_data: &str, x: u32, y: u32) -> Result<Self, String> {
        let rtree = resvg::usvg::Tree::from_str(svg_data, &resvg::usvg::Options::default())
            .map_err(|e| format!("usvg parse error: {e}"))?;
        Self::from_tree(&rtree, x, y)
    }

    /// Parses and renders SVG data from bytes at position `(x, y)`.
    pub fn from_bytes(svg_data: &[u8], x: u32, y: u32) -> Result<Self, String> {
        let rtree = resvg::usvg::Tree::from_data(svg_data, &resvg::usvg::Options::default())
            .map_err(|e| format!("usvg parse error: {e}"))?;
        Self::from_tree(&rtree, x, y)
    }

    /// Renders an already-parsed `usvg::Tree` at position `(x, y)`.
    fn from_tree(tree: &resvg::usvg::Tree, x: u32, y: u32) -> Result<Self, String> {
        let size = tree.size();
        let w = size.width() as u32;
        let h = size.height() as u32;
        if w == 0 || h == 0 {
            return Err("SVG has zero dimensions".to_owned());
        }

        let mut pixmap = resvg::tiny_skia::Pixmap::new(w, h)
            .ok_or_else(|| "failed to create pixmap".to_owned())?;
        let mut pm = pixmap.as_mut();
        resvg::render(
            tree,
            resvg::tiny_skia::Transform::default(),
            &mut pm,
        );

        // Convert premultiplied RGBA pixels to straight RGBA.
        // tiny-skia returns premultiplied alpha; we must demultiply
        // to get correct colours for semi-transparent pixels.
        let raw = pixmap.data();
        let pixels: Vec<[u8; 4]> = raw
            .chunks_exact(4)
            .map(|c| {
                let a = c[3] as f32 / 255.0;
                if a == 0.0 {
                    [0, 0, 0, 0]
                } else {
                    [
                        (c[0] as f32 / a).min(255.0) as u8,
                        (c[1] as f32 / a).min(255.0) as u8,
                        (c[2] as f32 / a).min(255.0) as u8,
                        c[3],
                    ]
                }
            })
            .collect();

        Ok(Self {
            x,
            y,
            pixels,
            width: w,
            height: h,
            z: 0,
            name: format!("SVG({w}x{h})"),
        })
    }

    /// SVG width in pixels.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// SVG height in pixels.
    pub fn height(&self) -> u32 {
        self.height
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

#[cfg(feature = "svg-renderer")]
impl std::fmt::Debug for SVGLayer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SVGLayer")
            .field("x", &self.x)
            .field("y", &self.y)
            .field("width", &self.width)
            .field("height", &self.height)
            .field("z", &self.z)
            .field("name", &self.name)
            .finish()
    }
}

#[cfg(feature = "svg-renderer")]
impl Layer for SVGLayer {
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

/// Describes the clipping region for a [`ClipLayer`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClipRegion {
    /// Clip to an explicit rectangle in target-space coordinates.
    Rect(crate::geometry::Rect),
    /// Clip to the inner layer's own [`Layer::bounds`].
    /// Falls back to the full target if the inner layer reports
    /// `None` bounds.
    LayerBounds,
}

/// A wrapper layer that clips an inner layer to a rectangular region.
///
/// `ClipLayer` renders the inner layer into a temporary buffer,
/// then copies only the pixels that fall within the clip region
/// to the target. This is useful for confining text, images, or
/// other layers to a specific area without modifying the inner
/// layer itself.
///
/// The clip region can be an explicit rectangle or derived from
/// the inner layer's own [`Layer::bounds`].
///
/// # Example
///
/// ```
/// use termcompositor::{ClipLayer, ClipRegion, RectLayer, Layer};
/// use termcompositor::FrameBuffer;
/// use termcompositor::geometry::Rect;
///
/// let inner = RectLayer::new(0, 0, 40, 20, [255, 0, 0, 255]);
/// let clip = ClipLayer::new(Box::new(inner))
///     .with_region(ClipRegion::Rect(Rect::new(5, 5, 10, 10)));
///
/// let mut fb = FrameBuffer::new(40, 20);
/// clip.render(&mut fb, (0, 0), 1.0);
/// ```
pub struct ClipLayer {
    inner: Box<dyn Layer>,
    region: ClipRegion,
    z: u32,
    name: String,
}

impl ClipLayer {
    /// Creates a new clip wrapper around the given layer.
    /// Defaults to clipping to the inner layer's bounds.
    pub fn new(inner: Box<dyn Layer>) -> Self {
        Self {
            inner,
            region: ClipRegion::LayerBounds,
            z: 0,
            name: "ClipLayer".to_owned(),
        }
    }

    /// Builder: sets the clip region.
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

    /// Returns a reference to the inner layer.
    pub fn inner(&self) -> &dyn Layer {
        &*self.inner
    }

    /// Returns a reference to the clip region.
    pub fn region(&self) -> &ClipRegion {
        &self.region
    }

    /// Resolves the effective clip rectangle. For `LayerBounds` mode,
    /// returns the inner layer's bounds (in layer-local coordinates).
    /// For `Rect` mode, returns the explicit rectangle.
    fn resolve_clip(&self) -> Option<crate::geometry::Rect> {
        match &self.region {
            ClipRegion::Rect(r) => Some(*r),
            ClipRegion::LayerBounds => self.inner.bounds(),
        }
    }
}

impl Layer for ClipLayer {
    fn z_order(&self) -> u32 {
        self.z
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn bounds(&self) -> Option<crate::geometry::Rect> {
        // The clipped bounds are the intersection of the inner
        // layer's bounds and the clip region.
        match &self.region {
            ClipRegion::Rect(r) => self.inner.bounds().map(|ib| {
                let ix = ib.x.max(r.x);
                let iy = ib.y.max(r.y);
                let ir = ib.x.saturating_add(ib.width).min(r.x.saturating_add(r.width));
                let ib2 = ib.y.saturating_add(ib.height).min(r.y.saturating_add(r.height));
                let iw = ir.saturating_sub(ix);
                let ih = ib2.saturating_sub(iy);
                crate::geometry::Rect::new(ix, iy, iw, ih)
            }).or(Some(*r)),
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
        let clip = match self.resolve_clip() {
            Some(r) => r,
            None => {
                // No clip region — render inner layer directly.
                self.inner.render(target, offset, effective);
                return;
            }
        };

        // Apply the offset to the clip region to get target-space coords.
        let cx = clip.x.saturating_add(offset.0);
        let cy = clip.y.saturating_add(offset.1);
        let cw = clip.width;
        let ch = clip.height;

        if cw == 0 || ch == 0 {
            return;
        }

        // Render the inner layer into a full-size temporary buffer.
        // O(W×H) memory — same approach as DropShadow. A smaller
        // clip-sized buffer would require i32 offsets which the
        // Layer trait doesn't support.
        let mut tmp = FrameBuffer::new(tw, th);
        self.inner.render(&mut tmp, offset, 1.0);

        // Copy only the clipped region from tmp to target.
        for sy in 0..ch {
            for sx in 0..cw {
                let tx = cx + sx;
                let ty = cy + sy;
                if let Some(src) = tmp.get_pixel(tx, ty) {
                    if src[3] == 0 {
                        continue;
                    }
                    let src_alpha = f32::from(src[3]) / 255.0 * effective;
                    if let Some(px) = target.get_pixel_mut(tx, ty) {
                        crate::framebuffer::blend_over(px, src, src_alpha);
                    }
                }
            }
        }
    }
}

/// A [`Layer`] plus the per-entry control state managed by
/// [`crate::LayerStack`]: opacity, visibility, optional z-order
/// override, and an optional custom name.
pub struct LayerEntry {
    id: LayerId,
    layer: Box<dyn Layer>,
    opacity: f32,
    visible: bool,
    z_override: Option<u32>,
    name: Option<String>,
    transform: Option<crate::geometry::Transform>,
}

impl LayerEntry {
    /// Creates a new entry wrapping `layer` with the given
    /// `id`. The entry starts fully opaque, visible, with no
    /// z-override, and no custom name.
    pub fn new(id: LayerId, layer: Box<dyn Layer>) -> Self {
        Self {
            id,
            layer,
            opacity: 1.0,
            visible: true,
            z_override: None,
            name: None,
            transform: None,
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

    /// Toggles the entry's visibility. Invisible entries are
    /// skipped by the compositor.
    pub fn set_visible(&mut self, visible: bool) {
        self.visible = visible;
    }

    /// Returns the effective z-order used by the compositor:
    /// the override if set, otherwise the layer's default.
    pub fn effective_z(&self) -> u32 {
        self.z_override.unwrap_or_else(|| self.layer.z_order())
    }

    /// Sets an explicit z-order override, replacing any
    /// previous override. Pass to
    /// [`LayerEntry::clear_z_override`] to fall back to the
    /// layer's default.
    pub fn set_z_override(&mut self, z: u32) {
        self.z_override = Some(z);
    }

    /// Clears any z-order override; [`LayerEntry::effective_z`]
    /// falls back to the layer's default.
    pub fn clear_z_override(&mut self) {
        self.z_override = None;
    }

    /// Returns the entry's name: the override if set,
    /// otherwise the layer's [`Layer::name`].
    pub fn name(&self) -> &str {
        self.name.as_deref().unwrap_or_else(|| self.layer.name())
    }

    /// Sets a custom name for this entry.
    pub fn set_name(&mut self, name: impl Into<String>) {
        self.name = Some(name.into());
    }

    /// Returns a reference to the transform, if set.
    pub fn transform(&self) -> Option<&crate::geometry::Transform> {
        self.transform.as_ref()
    }

    /// Returns a mutable reference to the transform, if set.
    pub fn transform_mut(&mut self) -> Option<&mut crate::geometry::Transform> {
        self.transform.as_mut()
    }

    /// Sets the transform for this entry. Pass `None` to clear.
    pub fn set_transform(&mut self, transform: Option<crate::geometry::Transform>) {
        self.transform = transform;
    }

    /// Builder: sets the transform for this entry.
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
    /// and control state. Useful for hot-swapping a layer's
    /// contents without invalidating external [`LayerId`]
    /// handles.
    pub fn set_layer(&mut self, layer: Box<dyn Layer>) {
        self.layer = layer;
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
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::{BorderLayer, CanvasLayer, DropShadow, GradientKind, GradientLayer, Layer, LayerEntry, RectLayer, SceneGraph, SolidColor, TextLayer};
    use crate::framebuffer::FrameBuffer;
    use proptest::prelude::*;
    use crate::geometry::Rect;

    #[test]
    fn solid_color_default_z_is_zero() {
        let s = SolidColor::new(1, 2, 3, 4);
        assert_eq!(s.z_order(), 0);
        assert_eq!(s.color, [1, 2, 3, 4]);
    }

    #[test]
    fn solid_color_builders() {
        let s = SolidColor::new(1, 2, 3, 4).with_z(5).with_name("bg");
        assert_eq!(s.z_order(), 5);
        assert_eq!(s.name(), "bg");
    }

    #[test]
    fn solid_color_bounds_is_none() {
        let s = SolidColor::new(0, 0, 0, 255);
        assert_eq!(s.bounds(), None);
    }

    #[test]
    fn solid_color_render_fills_with_color() {
        let s = SolidColor::new(10, 20, 30, 255);
        let mut fb = FrameBuffer::new(2, 2);
        s.render(&mut fb, (0, 0), 1.0);
        for px in fb.pixels() {
            assert_eq!(*px, [10, 20, 30, 255]);
        }
    }

    #[test]
    fn solid_color_render_zero_opacity_noop() {
        let s = SolidColor::new(10, 20, 30, 255);
        let mut fb = FrameBuffer::new(1, 1);
        s.render(&mut fb, (0, 0), 0.0);
        assert_eq!(fb.pixels()[0], [0, 0, 0, 0]);
    }

    #[test]
    fn solid_color_render_ignores_offset() {
        // SolidColor fills the whole target regardless of offset.
        let s = SolidColor::new(1, 2, 3, 255);
        let mut fb = FrameBuffer::new(2, 2);
        s.render(&mut fb, (50, 50), 1.0);
        for px in fb.pixels() {
            assert_eq!(*px, [1, 2, 3, 255]);
        }
    }

    #[test]
    fn rect_layer_new_defaults() {
        let r = RectLayer::new(2, 3, 4, 5, [10, 20, 30, 40]);
        assert_eq!(r.x, 2);
        assert_eq!(r.y, 3);
        assert_eq!(r.width, 4);
        assert_eq!(r.height, 5);
        assert_eq!(r.color, [10, 20, 30, 40]);
        assert_eq!(r.z_order(), 0);
    }

    #[test]
    fn rect_layer_builders() {
        let r = RectLayer::new(0, 0, 1, 1, [0, 0, 0, 255])
            .with_z(7)
            .with_name("box");
        assert_eq!(r.z_order(), 7);
        assert_eq!(r.name(), "box");
    }

    #[test]
    fn rect_layer_bounds() {
        let r = RectLayer::new(3, 4, 5, 6, [0, 0, 0, 255]);
        assert_eq!(r.bounds(), Some(Rect::new(3, 4, 5, 6)));
    }

    #[test]
    fn rect_layer_render_writes_only_inside_rect() {
        let r = RectLayer::new(1, 1, 2, 2, [255, 0, 0, 255]);
        let mut fb = FrameBuffer::new(4, 4);
        r.render(&mut fb, (0, 0), 1.0);
        // Inside the rect.
        assert_eq!(fb.get_pixel(1, 1), Some(&[255, 0, 0, 255]));
        assert_eq!(fb.get_pixel(2, 2), Some(&[255, 0, 0, 255]));
        // Outside the rect (still transparent).
        assert_eq!(fb.get_pixel(0, 0), Some(&[0, 0, 0, 0]));
        assert_eq!(fb.get_pixel(3, 3), Some(&[0, 0, 0, 0]));
    }

    #[test]
    fn rect_layer_render_offset_translates() {
        let r = RectLayer::new(0, 0, 2, 2, [0, 255, 0, 255]);
        let mut fb = FrameBuffer::new(4, 4);
        r.render(&mut fb, (1, 1), 1.0);
        assert_eq!(fb.get_pixel(0, 0), Some(&[0, 0, 0, 0]));
        assert_eq!(fb.get_pixel(1, 1), Some(&[0, 255, 0, 255]));
        assert_eq!(fb.get_pixel(2, 2), Some(&[0, 255, 0, 255]));
        assert_eq!(fb.get_pixel(3, 3), Some(&[0, 0, 0, 0]));
    }

    #[test]
    fn rect_layer_render_clips_outside_target() {
        // Rect partially off-screen; should silently clip.
        let r = RectLayer::new(2, 2, 5, 5, [10, 20, 30, 255]);
        let mut fb = FrameBuffer::new(3, 3);
        r.render(&mut fb, (0, 0), 1.0);
        assert_eq!(fb.get_pixel(2, 2), Some(&[10, 20, 30, 255]));
        // Out-of-bounds: get_pixel returns None for reads; writes were clipped.
        assert_eq!(fb.get_pixel(4, 4), None);
    }

    #[test]
    fn text_layer_builders() {
        let t = TextLayer::new(0, 0, "x", [0, 0, 0, 255])
            .with_z(3)
            .with_name("label");
        assert_eq!(t.z_order(), 3);
        assert_eq!(t.name(), "label");
    }

    #[test]
    fn text_layer_render_glyph_returns_text() {
        let t = TextLayer::new(0, 0, "placeholder", [0, 0, 0, 255]);
        assert_eq!(t.render_glyph(), "placeholder");
    }

    // -- Placeholder-path tests (font-rasterizer feature OFF) -----
    //
    // Without the font-rasterizer feature, TextLayer uses the
    // solid-block placeholder: text_width returns char count,
    // bounds is (x, y, char_count, 1), and render draws a solid
    // block.

    #[cfg(not(feature = "font-rasterizer"))]
    #[test]
    fn text_layer_new_defaults() {
        let t = TextLayer::new(1, 2, "hi", [10, 20, 30, 255]);
        assert_eq!(t.x, 1);
        assert_eq!(t.y, 2);
        assert_eq!(t.text, "hi");
        assert_eq!(t.color, [10, 20, 30, 255]);
        assert_eq!(t.z_order(), 0);
        assert_eq!(t.text_width(), 2);
    }

    #[cfg(not(feature = "font-rasterizer"))]
    #[test]
    fn text_layer_bounds_one_cell_per_char() {
        let t = TextLayer::new(2, 3, "hello", [0, 0, 0, 255]);
        assert_eq!(t.bounds(), Some(Rect::new(2, 3, 5, 1)));
    }

    #[cfg(not(feature = "font-rasterizer"))]
    #[test]
    fn text_layer_render_draws_colored_block() {
        let t = TextLayer::new(1, 1, "abc", [0, 0, 255, 255]);
        let mut fb = FrameBuffer::new(5, 3);
        t.render(&mut fb, (0, 0), 1.0);
        for x in 1..4 {
            assert_eq!(fb.get_pixel(x, 1), Some(&[0, 0, 255, 255]));
        }
        // Outside the text bounds: still transparent.
        assert_eq!(fb.get_pixel(0, 0), Some(&[0, 0, 0, 0]));
        assert_eq!(fb.get_pixel(4, 1), Some(&[0, 0, 0, 0]));
    }

    #[cfg(not(feature = "font-rasterizer"))]
    #[test]
    fn text_layer_text_width_handles_unicode() {
        // 'x' is 1 char; '日本語' is 3 chars.
        let a = TextLayer::new(0, 0, "x", [0, 0, 0, 255]);
        let b = TextLayer::new(0, 0, "\u{65e5}\u{672c}\u{8a9e}", [0, 0, 0, 255]);
        assert_eq!(a.text_width(), 1);
        assert_eq!(b.text_width(), 3);
    }

    // -- Font-rasterizer-path tests (font-rasterizer feature ON) --
    //
    // With the feature enabled, text_width returns measured
    // advance widths, bounds reflects the pixel-accurate size,
    // and render draws real glyph bitmaps.

    #[cfg(feature = "font-rasterizer")]
    #[test]
    fn text_layer_new_defaults_with_font() {
        let t = TextLayer::new(1, 2, "hi", [10, 20, 30, 255]);
        assert_eq!(t.x, 1);
        assert_eq!(t.y, 2);
        assert_eq!(t.text, "hi");
        assert_eq!(t.color, [10, 20, 30, 255]);
        assert_eq!(t.z_order(), 0);
        assert_eq!(t.font_size(), 14.0);
        // text_width should return measured advance width (not 2).
        assert!(t.text_width() > 0);
    }

    #[cfg(feature = "font-rasterizer")]
    #[test]
    fn text_layer_bounds_with_font_uses_font_size() {
        let t = TextLayer::new(0, 0, "ab", [0, 0, 0, 255]).with_font_size(14.0);
        let b = t.bounds().unwrap();
        // Width should be at least some positive advance sum.
        assert!(b.width >= 2);
        // Height should be approx font_size.
        assert_eq!(b.height, 14);
    }

    #[cfg(feature = "font-rasterizer")]
    #[test]
    fn text_layer_font_source_defaults_to_bundled() {
        let t = TextLayer::new(0, 0, "test", [255; 4]);
        // Lazy load happens on text_width or render; calling
        // text_width verifies the bundled font loads correctly.
        assert!(t.text_width() > 0, "bundled font must load and produce positive width");
    }

    #[cfg(feature = "font-rasterizer")]
    #[test]
    fn text_layer_render_produces_non_empty_bitmap() {
        let t = TextLayer::new(0, 0, "A", [200, 100, 50, 255]).with_font_size(14.0);
        let mut fb = FrameBuffer::new(20, 20);
        t.render(&mut fb, (0, 0), 1.0);
        // The letter 'A' at (0,0) with 14px Fira Mono should
        // produce at least some non-transparent pixels in the
        // expected area.
        let has_glyph_pixels = fb.pixels().iter().any(|p| p[3] > 0);
        assert!(has_glyph_pixels, "font rasterizer should render non-transparent pixels");
    }

    #[cfg(feature = "font-rasterizer")]
    #[test]
    fn text_layer_with_font_size_changes_width() {
        let small = TextLayer::new(0, 0, "hello", [255; 4]).with_font_size(10.0);
        let large = TextLayer::new(0, 0, "hello", [255; 4]).with_font_size(20.0);
        assert!(
            large.text_width() >= small.text_width(),
            "larger font size should produce >= advance width"
        );
    }

    // -- Multi-line text tests (both feature configs) -------------

    #[cfg(feature = "font-rasterizer")]
    #[test]
    fn text_layer_multi_line_text_width_is_max_line() {
        let t = TextLayer::new(0, 0, "short\na longer line\ntiny", [255; 4]);
        // text_width should be the longest line, not the sum.
        // "a longer line" > "short" and "tiny", so that line's
        // advance width is text_width.
        assert!(t.text_width() > 0);
        // The width of "a longer line" alone.
        let single = TextLayer::new(0, 0, "a longer line", [255; 4]).with_font_size(14.0);
        assert_eq!(t.text_width(), single.text_width());
    }

    #[cfg(feature = "font-rasterizer")]
    #[test]
    fn text_layer_multi_line_bounds_height_includes_all_lines() {
        let t = TextLayer::new(0, 0, "line1\nline2\nline3", [255; 4]).with_font_size(14.0);
        let b = t.bounds().unwrap();
        // 3 lines, each 14px tall = 42px.
        assert_eq!(b.height, 42);
        // Width is the widest line.
        let single = TextLayer::new(0, 0, "line1", [255; 4]).with_font_size(14.0);
        assert_eq!(b.width, single.text_width().max(1));
    }

    #[cfg(feature = "font-rasterizer")]
    #[test]
    fn text_layer_multi_line_render_renders_all_lines() {
        let t = TextLayer::new(0, 0, "A\nB\nC", [255, 255, 255, 255]).with_font_size(14.0);
        let mut fb = FrameBuffer::new(20, 50);
        t.render(&mut fb, (0, 0), 1.0);
        // All three lines should produce non-transparent pixels
        // scattered across the full height.
        let has_pixels = fb.pixels().iter().any(|p| p[3] > 0);
        assert!(has_pixels, "multi-line render should produce glyph pixels");
        // Pixels in the bottom third indicate the third line rendered.
        let h = fb.height();
        let bottom_third_pixels = fb.pixels()[(((2 * h / 3) as usize) * fb.width() as usize)..]
            .iter()
            .any(|p| p[3] > 0);
        assert!(
            bottom_third_pixels,
            "third line (bottom third of framebuffer) should have rendered pixels"
        );
    }

    #[cfg(feature = "font-rasterizer")]
    #[test]
    fn text_layer_multi_line_num_lines() {
        let t = TextLayer::new(0, 0, "a\nb\nc", [255; 4]);
        assert_eq!(t.num_lines(), 3);
        let single = TextLayer::new(0, 0, "hello", [255; 4]);
        assert_eq!(single.num_lines(), 1);
        let empty = TextLayer::new(0, 0, "", [255; 4]);
        assert_eq!(empty.num_lines(), 1);
        let trailing = TextLayer::new(0, 0, "a\nb\n", [255; 4]);
        assert_eq!(trailing.num_lines(), 3);
    }

    // -- Placeholder multi-line tests (font-rasterizer OFF) -------

    #[cfg(not(feature = "font-rasterizer"))]
    #[test]
    fn text_layer_multi_line_placeholder_bounds() {
        let t = TextLayer::new(0, 0, "abc\nde\nf", [255; 4]);
        let b = t.bounds().unwrap();
        // Width should be the widest line ("abc" = 3).
        assert_eq!(b.width, 3);
        // Height should be 3 lines.
        assert_eq!(b.height, 3);
    }

    // -- FontSource::Bytes tests (font-rasterizer ON) ------------
    //
    // These exercise the FontSource::Bytes variant of
    // ensure_font, which takes a &'static [u8] TTF/OTF blob
    // and rasterises glyphs from it.

    #[cfg(feature = "font-rasterizer")]
    #[test]
    fn font_source_bytes_loads_and_produces_width() {
        use super::FontSource;
        // Use the bundled Fira Mono font as a FontSource::Bytes blob.
        let t = TextLayer::new(0, 0, "abc", [255; 4])
            .with_font(FontSource::Bytes(super::BUNDLED_FONT_DATA), 16.0);
        // text_width triggers ensure_font, which must parse the
        // TTF bytes and return a valid fontdue::Font.
        let w = t.text_width();
        assert!(w > 0, "FontSource::Bytes must produce positive width, got {w}");
    }

    #[cfg(feature = "font-rasterizer")]
    #[test]
    fn font_source_bytes_render_produces_glyph_pixels() {
        use super::FontSource;
        let t = TextLayer::new(0, 0, "B", [200, 50, 50, 255])
            .with_font(FontSource::Bytes(super::BUNDLED_FONT_DATA), 18.0);
        let mut fb = FrameBuffer::new(30, 30);
        t.render(&mut fb, (0, 0), 1.0);
        let has_pixels = fb.pixels().iter().any(|p| p[3] > 0);
        assert!(has_pixels, "FontSource::Bytes render must produce non-transparent pixels");
    }

    #[cfg(feature = "font-rasterizer")]
    #[test]
    fn font_source_bytes_matches_bundled_width() {
        use super::FontSource;
        // FontSource::Bytes and FontSource::Bundled should produce
        // identical widths when using the same underlying font data.
        let via_bytes = TextLayer::new(0, 0, "hello world", [255; 4])
            .with_font(FontSource::Bytes(super::BUNDLED_FONT_DATA), 14.0);
        let via_bundled = TextLayer::new(0, 0, "hello world", [255; 4])
            .with_font(FontSource::Bundled, 14.0);
        assert_eq!(via_bytes.text_width(), via_bundled.text_width());
    }

    #[cfg(feature = "font-rasterizer")]
    #[test]
    fn font_source_bytes_bounds_reflects_font_size() {
        use super::FontSource;
        let t = TextLayer::new(5, 10, "X", [255; 4])
            .with_font(FontSource::Bytes(super::BUNDLED_FONT_DATA), 20.0);
        let b = t.bounds().unwrap();
        assert_eq!(b.x, 5);
        assert_eq!(b.y, 10);
        assert!(b.width > 0, "bounds width must be positive");
        assert_eq!(b.height, 20, "bounds height must equal font_size");
    }

    #[test]
    fn layer_entry_opacity_clamps() {
        let e = LayerEntry::new(0, Box::new(SolidColor::new(0, 0, 0, 255)));
        let mut e = e;
        e.set_opacity(2.0);
        assert_eq!(e.opacity(), 1.0);
        e.set_opacity(-1.0);
        assert_eq!(e.opacity(), 0.0);
        e.set_opacity(0.5);
        assert_eq!(e.opacity(), 0.5);
    }

    #[test]
    fn layer_entry_visibility_toggle() {
        let mut e = LayerEntry::new(0, Box::new(SolidColor::new(0, 0, 0, 255)));
        assert!(e.is_visible());
        e.set_visible(false);
        assert!(!e.is_visible());
    }

    #[test]
    fn layer_entry_z_override_beats_layer_default() {
        let mut e = LayerEntry::new(0, Box::new(SolidColor::new(0, 0, 0, 255).with_z(2)));
        assert_eq!(e.effective_z(), 2);
        e.set_z_override(99);
        assert_eq!(e.effective_z(), 99);
        e.clear_z_override();
        assert_eq!(e.effective_z(), 2);
    }

    #[test]
    fn layer_entry_set_layer_keeps_id() {
        let mut e = LayerEntry::new(7, Box::new(SolidColor::new(1, 2, 3, 255)));
        let original_id = e.id();
        e.set_layer(Box::new(SolidColor::new(4, 5, 6, 255)));
        assert_eq!(e.id(), original_id);
        assert_eq!(e.layer().z_order(), 0);
    }

    #[test]
    fn layer_entry_set_layer_swap_to_rect() {
        let mut e = LayerEntry::new(1, Box::new(SolidColor::new(0, 0, 0, 255)));
        e.set_layer(Box::new(RectLayer::new(0, 0, 1, 1, [9, 9, 9, 255])));
        assert_eq!(e.layer().bounds(), Some(Rect::new(0, 0, 1, 1)));
    }

    #[test]
    fn layer_entry_debug_does_not_panic() {
        let e = LayerEntry::new(0, Box::new(SolidColor::new(0, 0, 0, 255).with_name("dbg")));
        let s = format!("{e:?}");
        assert!(s.contains("LayerEntry"));
        assert!(s.contains("dbg"));
    }

    #[test]
    fn layer_entry_name_override() {
        let mut e = LayerEntry::new(0, Box::new(SolidColor::new(0, 0, 0, 255).with_name("a")));
        assert_eq!(e.name(), "a");
        e.set_name("b");
        assert_eq!(e.name(), "b");
    }

    // ── CanvasLayer tests ──────────────────────────────────────────────────'

    #[test]
    fn canvas_layer_new_defaults() {
        let c = CanvasLayer::new(10, 8);
        assert_eq!(c.width(), 10);
        assert_eq!(c.height(), 8);
        assert_eq!(c.z_order(), 0);
        assert!(c.bounds().is_some());
        // All pixels start transparent.
        for y in 0..8 {
            for x in 0..10 {
                assert_eq!(c.get_pixel(x, y), Some([0, 0, 0, 0]));
            }
        }
    }

    #[test]
    fn canvas_layer_builders() {
        let c = CanvasLayer::new(5, 5)
            .at(3, 4)
            .with_z(7)
            .with_name("my-canvas");
        assert_eq!(c.x, 3);
        assert_eq!(c.y, 4);
        assert_eq!(c.z_order(), 7);
        assert_eq!(c.name(), "my-canvas");
    }

    #[test]
    fn canvas_layer_bounds() {
        let c = CanvasLayer::new(10, 20).at(5, 6);
        assert_eq!(c.bounds(), Some(Rect::new(5, 6, 10, 20)));
    }

    #[test]
    fn canvas_layer_draw_pixel() {
        let mut c = CanvasLayer::new(5, 5);
        c.draw_pixel(2, 3, [255, 100, 50, 200]);
        assert_eq!(c.get_pixel(2, 3), Some([255, 100, 50, 200]));
        // Other pixels remain transparent.
        assert_eq!(c.get_pixel(0, 0), Some([0, 0, 0, 0]));
    }

    #[test]
    fn canvas_layer_draw_pixel_out_of_bounds_ignored() {
        let mut c = CanvasLayer::new(3, 3);
        c.draw_pixel(10, 10, [255, 0, 0, 255]);
        assert_eq!(c.get_pixel(10, 10), None);
        assert_eq!(c.get_pixel(0, 0), Some([0, 0, 0, 0]));
    }

    #[test]
    fn canvas_layer_draw_line_horizontal() {
        let mut c = CanvasLayer::new(10, 1);
        c.draw_line(0, 0, 9, 0, [0, 255, 0, 255]);
        for x in 0..10 {
            assert_eq!(c.get_pixel(x, 0), Some([0, 255, 0, 255]));
        }
    }

    #[test]
    fn canvas_layer_draw_line_vertical() {
        let mut c = CanvasLayer::new(1, 10);
        c.draw_line(0, 0, 0, 9, [255, 0, 0, 255]);
        for y in 0..10 {
            assert_eq!(c.get_pixel(0, y), Some([255, 0, 0, 255]));
        }
    }

    #[test]
    fn canvas_layer_draw_line_diagonal() {
        let mut c = CanvasLayer::new(10, 10);
        c.draw_line(0, 0, 9, 9, [0, 0, 255, 255]);
        for i in 0..10 {
            assert_eq!(c.get_pixel(i, i), Some([0, 0, 255, 255]));
        }
    }

    #[test]
    fn canvas_layer_draw_circle_basic() {
        let mut c = CanvasLayer::new(21, 21);
        c.draw_circle(10, 10, 5, [255, 255, 0, 255]);
        // The 4 cardinal points should be drawn.
        assert_eq!(c.get_pixel(10, 5), Some([255, 255, 0, 255]));  // top
        assert_eq!(c.get_pixel(10, 15), Some([255, 255, 0, 255])); // bottom
        assert_eq!(c.get_pixel(5, 10), Some([255, 255, 0, 255]));  // left
        assert_eq!(c.get_pixel(15, 10), Some([255, 255, 0, 255])); // right
        // Center should be empty (no fill).
        assert_eq!(c.get_pixel(10, 10), Some([0, 0, 0, 0]));
    }

    #[test]
    fn canvas_layer_draw_circle_zero_radius() {
        let mut c = CanvasLayer::new(5, 5);
        c.draw_circle(2, 2, 0, [255, 0, 0, 255]);
        // radius=0 draws only the center.
        assert_eq!(c.get_pixel(2, 2), Some([255, 0, 0, 255]));
    }

    #[test]
    fn canvas_layer_draw_circle_negative_radius_noop() {
        let mut c = CanvasLayer::new(5, 5);
        c.draw_circle(2, 2, -1, [255, 0, 0, 255]);
        assert_eq!(c.get_pixel(2, 2), Some([0, 0, 0, 0]));
    }

    #[test]
    fn canvas_layer_fill_rect() {
        let mut c = CanvasLayer::new(10, 10);
        c.fill_rect(2, 2, 3, 3, [100, 100, 100, 255]);
        // Inside the filled rect.
        assert_eq!(c.get_pixel(2, 2), Some([100, 100, 100, 255]));
        assert_eq!(c.get_pixel(4, 4), Some([100, 100, 100, 255]));
        // Outside.
        assert_eq!(c.get_pixel(1, 1), Some([0, 0, 0, 0]));
        assert_eq!(c.get_pixel(5, 5), Some([0, 0, 0, 0]));
    }

    #[test]
    fn canvas_layer_clear() {
        let mut c = CanvasLayer::new(5, 5);
        c.draw_pixel(2, 2, [255, 0, 0, 255]);
        c.fill_rect(0, 0, 5, 5, [0, 255, 0, 128]);
        c.clear();
        for y in 0..5 {
            for x in 0..5 {
                assert_eq!(c.get_pixel(x, y), Some([0, 0, 0, 0]));
            }
        }
    }

    #[test]
    fn canvas_layer_render_composites_to_framebuffer() {
        let mut c = CanvasLayer::new(5, 5).at(1, 1);
        c.draw_pixel(0, 0, [255, 0, 0, 255]); // canvas-local (0,0) → fb (1,1)
        c.draw_pixel(4, 4, [0, 255, 0, 255]); // canvas-local (4,4) → fb (5,5)

        let mut fb = FrameBuffer::new(10, 10);
        c.render(&mut fb, (0, 0), 1.0);

        assert_eq!(fb.get_pixel(1, 1), Some(&[255, 0, 0, 255]));
        assert_eq!(fb.get_pixel(5, 5), Some(&[0, 255, 0, 255]));
        assert_eq!(fb.get_pixel(0, 0), Some(&[0, 0, 0, 0]));
    }

    #[test]
    fn canvas_layer_render_offset_translates() {
        let mut c = CanvasLayer::new(3, 3);
        c.draw_pixel(1, 1, [100, 200, 50, 255]);

        let mut fb = FrameBuffer::new(10, 10);
        c.render(&mut fb, (2, 3), 1.0);

        assert_eq!(fb.get_pixel(3, 4), Some(&[100, 200, 50, 255]));
        assert_eq!(fb.get_pixel(1, 1), Some(&[0, 0, 0, 0]));
    }

    #[test]
    fn canvas_layer_render_opacity_blends() {
        let mut c = CanvasLayer::new(1, 1);
        c.draw_pixel(0, 0, [255, 0, 0, 255]);

        let mut fb = FrameBuffer::new(1, 1);
        c.render(&mut fb, (0, 0), 0.5);

        let px = fb.get_pixel(0, 0).unwrap();
        assert_eq!(px[0], 255);
        assert!(px[3] < 255, "alpha should be < 255 at 50% opacity, got {}", px[3]);
    }

    #[test]
    fn canvas_layer_render_skips_transparent_pixels() {
        let mut c = CanvasLayer::new(3, 3);
        // Only draw one pixel; others stay transparent.
        c.draw_pixel(1, 1, [0, 0, 255, 255]);

        let mut fb = FrameBuffer::new(3, 3);
        // Fill fb with red first.
        for px in fb.pixels_mut() {
            *px = [255, 0, 0, 255];
        }
        c.render(&mut fb, (0, 0), 1.0);

        // The drawn pixel should be blue.
        assert_eq!(fb.get_pixel(1, 1), Some(&[0, 0, 255, 255]));
        // Other pixels should remain red (transparent canvas pixels skipped).
        assert_eq!(fb.get_pixel(0, 0), Some(&[255, 0, 0, 255]));
        assert_eq!(fb.get_pixel(2, 2), Some(&[255, 0, 0, 255]));
    }

    #[test]
    fn canvas_layer_render_clips_outside_framebuffer() {
        let mut c = CanvasLayer::new(5, 5).at(8, 8);
        c.draw_pixel(0, 0, [255, 0, 0, 255]);

        let mut fb = FrameBuffer::new(5, 5);
        c.render(&mut fb, (0, 0), 1.0);

        // All pixels should remain transparent (canvas is outside).
        for px in fb.pixels() {
            assert_eq!(*px, [0, 0, 0, 0]);
        }
    }

    // ── CanvasLayer proptests ────────────────────────────────────────

    proptest! {
        #[test]
        fn prop_draw_pixel_never_panics(
            w in 1u32..50,
            h in 1u32..50,
            px in 0u32..100,
            py in 0u32..100,
            r in 0u8..=255,
            g in 0u8..=255,
            b in 0u8..=255,
            a in 0u8..=255,
        ) {
            let mut c = CanvasLayer::new(w, h);
            c.draw_pixel(px, py, [r, g, b, a]);
            // Out-of-bounds draws are silently ignored.
            if px < w && py < h {
                prop_assert_eq!(c.get_pixel(px, py), Some([r, g, b, a]));
            } else {
                prop_assert_eq!(c.get_pixel(px, py), None);
            }
        }

        #[test]
        fn prop_draw_line_never_panics(
            w in 1u32..50,
            h in 1u32..50,
            x0 in -5i32..50,
            y0 in -5i32..50,
            x1 in -5i32..50,
            y1 in -5i32..50,
        ) {
            let mut c = CanvasLayer::new(w, h);
            c.draw_line(x0, y0, x1, y1, [255, 0, 0, 255]);
            // All pixels on the line path that are in-bounds should be set.
            // The endpoint must always be drawn (if in bounds).
            if x1 >= 0 && y1 >= 0 && (x1 as u32) < w && (y1 as u32) < h {
                prop_assert_eq!(
                    c.get_pixel(x1 as u32, y1 as u32),
                    Some([255, 0, 0, 255]),
                    "endpoint ({}, {}) must be drawn", x1, y1
                );
            }
            if x0 >= 0 && y0 >= 0 && (x0 as u32) < w && (y0 as u32) < h {
                prop_assert_eq!(
                    c.get_pixel(x0 as u32, y0 as u32),
                    Some([255, 0, 0, 255]),
                    "startpoint ({}, {}) must be drawn", x0, y0
                );
            }
        }

        #[test]
        fn prop_draw_line_deterministic(
            w in 5u32..50,
            h in 5u32..50,
            x0 in 0i32..25,
            y0 in 0i32..25,
            x1 in 0i32..25,
            y1 in 0i32..25,
        ) {
            // Drawing the same line twice produces identical results.
            let mut a = CanvasLayer::new(w, h);
            a.draw_line(x0, y0, x1, y1, [0, 255, 0, 255]);
            let mut b = CanvasLayer::new(w, h);
            b.draw_line(x0, y0, x1, y1, [0, 255, 0, 255]);
            for y in 0..h {
                for x in 0..w {
                    prop_assert_eq!(
                        a.get_pixel(x, y), b.get_pixel(x, y),
                        "pixel ({}, {}) differs between identical draws", x, y
                    );
                }
            }
        }

        #[test]
        fn prop_draw_circle_never_panics(
            w in 1u32..50,
            h in 1u32..50,
            cx in -10i32..50,
            cy in -10i32..50,
            radius in 0i32..25,
        ) {
            let mut c = CanvasLayer::new(w, h);
            c.draw_circle(cx, cy, radius, [0, 0, 255, 255]);
            // radius=0 should draw exactly the center pixel.
            if radius == 0 && cx >= 0 && cy >= 0 && (cx as u32) < w && (cy as u32) < h {
                prop_assert_eq!(
                    c.get_pixel(cx as u32, cy as u32),
                    Some([0, 0, 255, 255]),
                    "center pixel must be drawn for radius=0"
                );
            }
        }

        #[test]
        fn prop_draw_circle_center_is_drawn(
            w in 10u32..50,
            h in 10u32..50,
            radius in 1i32..10,
        ) {
            let cx = (w / 2) as i32;
            let cy = (h / 2) as i32;
            let mut c = CanvasLayer::new(w, h);
            c.draw_circle(cx, cy, radius, [0, 255, 0, 255]);
            // The 4 cardinal points should be drawn.
            // Top: (cx, cy - radius)
            let top_y = cy - radius;
            if top_y >= 0 {
                prop_assert_eq!(
                    c.get_pixel(cx as u32, top_y as u32),
                    Some([0, 255, 0, 255]),
                    "top cardinal must be drawn"
                );
            }
        }

        #[test]
        fn prop_draw_circle_negative_radius_is_noop(
            w in 1u32..50,
            h in 1u32..50,
            cx in 0i32..50,
            cy in 0i32..50,
        ) {
            let mut c = CanvasLayer::new(w, h);
            c.draw_circle(cx, cy, -1, [255, 0, 0, 255]);
            // Nothing should be drawn.
            for y in 0..h {
                for x in 0..w {
                    prop_assert_eq!(
                        c.get_pixel(x, y), Some([0, 0, 0, 0]),
                        "negative radius must not draw anything"
                    );
                }
            }
        }
    }

    // ── SceneGraph tests ──────────────────────────────────────────────────'

    #[test]
    fn scene_graph_new_defaults() {
        let scene = SceneGraph::new();
        assert_eq!(scene.z_order(), 0);
        assert!(scene.bounds().is_none());
    }

    #[test]
    fn scene_graph_builders() {
        let scene = SceneGraph::new().with_z(5).with_name("my-scene");
        assert_eq!(scene.z_order(), 5);
        assert_eq!(scene.name(), "my-scene");
    }

    #[test]
    fn scene_graph_default_trait() {
        let scene = SceneGraph::default();
        assert_eq!(scene.z_order(), 0);
    }

    #[test]
    fn scene_graph_single_child_renders() {
        let mut scene = SceneGraph::new();
        scene.add_child(SolidColor::new(255, 0, 0, 255));
        let mut fb = FrameBuffer::new(5, 5);
        scene.render(&mut fb, (0, 0), 1.0);
        // SolidColor fills the whole buffer.
        for px in fb.pixels() {
            assert_eq!(*px, [255, 0, 0, 255]);
        }
    }

    #[test]
    fn scene_graph_group_offset_translates_children() {
        let mut scene = SceneGraph::new();
        let group = scene.add_group((3, 2), 1.0, true);
        scene.add_child_to(group, RectLayer::new(0, 0, 2, 2, [0, 255, 0, 255]));
        let mut fb = FrameBuffer::new(10, 10);
        scene.render(&mut fb, (0, 0), 1.0);
        // Rect should be at offset (3, 2).
        assert_eq!(fb.get_pixel(3, 2), Some(&[0, 255, 0, 255]));
        assert_eq!(fb.get_pixel(4, 3), Some(&[0, 255, 0, 255]));
        // Original position should be empty.
        assert_eq!(fb.get_pixel(0, 0), Some(&[0, 0, 0, 0]));
    }

    #[test]
    fn scene_graph_group_opacity_multiplies() {
        let mut scene = SceneGraph::new();
        let group = scene.add_group((0, 0), 0.5, true);
        scene.add_child_to(group, RectLayer::new(0, 0, 1, 1, [255, 0, 0, 255]));
        let mut fb = FrameBuffer::new(5, 5);
        scene.render(&mut fb, (0, 0), 1.0);
        let px = fb.get_pixel(0, 0).unwrap();
        assert_eq!(px[0], 255);
        // Alpha should be ~128 (50% of 255).
        assert!((px[3] as i32 - 128).abs() <= 1, "alpha = {}", px[3]);
    }

    #[test]
    fn scene_graph_hidden_group_hides_children() {
        let mut scene = SceneGraph::new();
        let group = scene.add_group((0, 0), 1.0, false);
        scene.add_child_to(group, RectLayer::new(0, 0, 2, 2, [255, 0, 0, 255]));
        let mut fb = FrameBuffer::new(5, 5);
        scene.render(&mut fb, (0, 0), 1.0);
        // All pixels should be transparent.
        for px in fb.pixels() {
            assert_eq!(*px, [0, 0, 0, 0]);
        }
    }

    #[test]
    fn scene_graph_nested_groups_cascade() {
        let mut scene = SceneGraph::new();
        let outer = scene.add_group((5, 5), 0.5, true);
        let inner = scene.add_group_to(outer, (2, 2), 0.5, true);
        scene.add_child_to(inner, RectLayer::new(0, 0, 1, 1, [255, 0, 0, 255]));
        let mut fb = FrameBuffer::new(20, 20);
        scene.render(&mut fb, (0, 0), 1.0);
        // Offset should be (5+2, 5+2) = (7, 7).
        let px = fb.get_pixel(7, 7).unwrap();
        // RGB should be red.
        assert_eq!(px[0], 255, "R should be 255");
        assert_eq!(px[1], 0, "G should be 0");
        assert_eq!(px[2], 0, "B should be 0");
        // Opacity should be 0.5 * 0.5 = 0.25 -> alpha ~64.
        assert!((px[3] as i32 - 64).abs() <= 1, "alpha = {}", px[3]);
    }

    #[test]
    fn scene_graph_set_visible_toggles() {
        let mut scene = SceneGraph::new();
        let idx = scene.add_child(RectLayer::new(0, 0, 2, 2, [255, 0, 0, 255]));
        let mut fb = FrameBuffer::new(5, 5);
        scene.render(&mut fb, (0, 0), 1.0);
        assert_eq!(fb.get_pixel(0, 0), Some(&[255, 0, 0, 255]));

        scene.set_visible(idx, false);
        let mut fb2 = FrameBuffer::new(5, 5);
        scene.render(&mut fb2, (0, 0), 1.0);
        assert_eq!(fb2.get_pixel(0, 0), Some(&[0, 0, 0, 0]));
    }

    #[test]
    fn scene_graph_set_opacity_changes() {
        let mut scene = SceneGraph::new();
        let idx = scene.add_child(RectLayer::new(0, 0, 1, 1, [255, 0, 0, 255]));
        scene.set_opacity(idx, 0.5);
        let mut fb = FrameBuffer::new(5, 5);
        scene.render(&mut fb, (0, 0), 1.0);
        let px = fb.get_pixel(0, 0).unwrap();
        assert!((px[3] as i32 - 128).abs() <= 1, "alpha = {}", px[3]);
    }

    #[test]
    fn scene_graph_set_offset_changes() {
        let mut scene = SceneGraph::new();
        let idx = scene.add_child(RectLayer::new(0, 0, 2, 2, [255, 0, 0, 255]));
        scene.set_offset(idx, (3, 3));
        let mut fb = FrameBuffer::new(10, 10);
        scene.render(&mut fb, (0, 0), 1.0);
        assert_eq!(fb.get_pixel(3, 3), Some(&[255, 0, 0, 255]));
        assert_eq!(fb.get_pixel(0, 0), Some(&[0, 0, 0, 0]));
    }

    #[test]
    fn scene_graph_zero_opacity_is_noop() {
        let mut scene = SceneGraph::new();
        scene.add_child(SolidColor::new(255, 0, 0, 255));
        let mut fb = FrameBuffer::new(5, 5);
        scene.render(&mut fb, (0, 0), 0.0);
        for px in fb.pixels() {
            assert_eq!(*px, [0, 0, 0, 0]);
        }
    }

    #[test]
    fn scene_graph_offset_translates_render() {
        let mut scene = SceneGraph::new();
        scene.add_child(SolidColor::new(255, 0, 0, 255));
        let mut fb = FrameBuffer::new(10, 10);
        scene.render(&mut fb, (3, 3), 1.0);
        // SolidColor fills everything regardless of offset.
        for px in fb.pixels() {
            assert_eq!(*px, [255, 0, 0, 255]);
        }
    }

    #[test]
    fn scene_graph_multiple_children_at_different_positions() {
        let mut scene = SceneGraph::new();
        scene.add_child_to(scene.root, RectLayer::new(0, 0, 2, 2, [255, 0, 0, 255]));
        scene.add_child_to(scene.root, RectLayer::new(5, 5, 2, 2, [0, 0, 255, 255]));
        let mut fb = FrameBuffer::new(10, 10);
        scene.render(&mut fb, (0, 0), 1.0);
        assert_eq!(fb.get_pixel(0, 0), Some(&[255, 0, 0, 255]));
        assert_eq!(fb.get_pixel(5, 5), Some(&[0, 0, 255, 255]));
        assert_eq!(fb.get_pixel(3, 3), Some(&[0, 0, 0, 0]));
    }

    #[test]
    fn scene_graph_hidden_parent_hides_all_descendants() {
        let mut scene = SceneGraph::new();
        let group = scene.add_group((0, 0), 1.0, false);
        let inner = scene.add_group_to(group, (0, 0), 1.0, true);
        scene.add_child_to(inner, RectLayer::new(0, 0, 2, 2, [255, 0, 0, 255]));
        let mut fb = FrameBuffer::new(5, 5);
        scene.render(&mut fb, (0, 0), 1.0);
        for px in fb.pixels() {
            assert_eq!(*px, [0, 0, 0, 0]);
        }
    }

    // ── DropShadow tests ──────────────────────────────────────────────────'

    #[test]
    fn drop_shadow_new_defaults() {
        let inner = RectLayer::new(0, 0, 5, 5, [255, 255, 255, 255]);
        let ds = DropShadow::new(Box::new(inner));
        assert_eq!(ds.z_order(), 0);
        assert_eq!(ds.offset, (2, 2));
        assert_eq!(ds.blur_radius, 2);
        assert_eq!(ds.shadow_color, [0, 0, 0, 80]);
    }

    #[test]
    fn drop_shadow_builders() {
        let inner = RectLayer::new(0, 0, 5, 5, [255, 255, 255, 255]);
        let ds = DropShadow::new(Box::new(inner))
            .with_offset(3, 4)
            .with_blur(5)
            .with_shadow_color([255, 0, 0, 128])
            .with_z(10)
            .with_name("my-shadow");
        assert_eq!(ds.offset, (3, 4));
        assert_eq!(ds.blur_radius, 5);
        assert_eq!(ds.shadow_color, [255, 0, 0, 128]);
        assert_eq!(ds.z_order(), 10);
        assert_eq!(ds.name(), "my-shadow");
    }

    #[test]
    fn drop_shadow_inner_reference() {
        let inner = RectLayer::new(0, 0, 5, 5, [255, 255, 255, 255]);
        let ds = DropShadow::new(Box::new(inner));
        assert!(ds.inner().bounds().is_some());
    }

    #[test]
    fn drop_shadow_render_produces_content() {
        let inner = RectLayer::new(5, 5, 5, 5, [255, 255, 255, 255]);
        let ds = DropShadow::new(Box::new(inner))
            .with_offset(2, 2)
            .with_blur(1)
            .with_shadow_color([0, 0, 0, 200]);
        let mut fb = FrameBuffer::new(20, 20);
        ds.render(&mut fb, (0, 0), 1.0);
        // Should have some non-transparent pixels.
        let has_content = fb.pixels().iter().any(|p| p[3] > 0);
        assert!(has_content, "render must produce content");
    }

    #[test]
    fn drop_shadow_renders_original_on_top() {
        // The original rect at (5,5) should be white.
        let inner = RectLayer::new(5, 5, 3, 3, [255, 255, 255, 255]);
        let ds = DropShadow::new(Box::new(inner))
            .with_offset(2, 2)
            .with_blur(0)
            .with_shadow_color([0, 0, 0, 255]);
        let mut fb = FrameBuffer::new(20, 20);
        ds.render(&mut fb, (0, 0), 1.0);
        // Original rect should be white (on top).
        let px = fb.get_pixel(5, 5).unwrap();
        assert_eq!(px[0], 255, "R should be 255 at original position");
        assert_eq!(px[1], 255, "G should be 255 at original position");
        assert_eq!(px[2], 255, "B should be 255 at original position");
    }

    #[test]
    fn drop_shadow_zero_opacity_is_noop() {
        let inner = RectLayer::new(5, 5, 5, 5, [255, 255, 255, 255]);
        let ds = DropShadow::new(Box::new(inner));
        let mut fb = FrameBuffer::new(20, 20);
        ds.render(&mut fb, (0, 0), 0.0);
        for px in fb.pixels() {
            assert_eq!(*px, [0, 0, 0, 0]);
        }
    }

    #[test]
    fn drop_shadow_offset_translates_shadow() {
        // With offset (2, 2), shadow should appear at (7,7) when rect is at (5,5).
        let inner = RectLayer::new(5, 5, 2, 2, [255, 255, 255, 255]);
        let ds = DropShadow::new(Box::new(inner))
            .with_offset(2, 2)
            .with_blur(0)
            .with_shadow_color([0, 0, 0, 255]);
        let mut fb = FrameBuffer::new(20, 20);
        ds.render(&mut fb, (0, 0), 1.0);
        // Shadow at offset position (7,7) should have black.
        let shadow_px = fb.get_pixel(7, 7).unwrap();
        assert_eq!(shadow_px[0], 0, "shadow R should be 0");
        assert_eq!(shadow_px[1], 0, "shadow G should be 0");
        assert_eq!(shadow_px[2], 0, "shadow B should be 0");
        assert!(shadow_px[3] > 0, "shadow alpha should be > 0");
    }

    #[test]
    fn drop_shadow_blur_softens_edges() {
        // With blur, the shadow area should be larger than without blur.
        let inner = RectLayer::new(10, 10, 2, 2, [255, 255, 255, 255]);
        let mut ds_no_blur = DropShadow::new(Box::new(inner.clone()))
            .with_offset(3, 3)
            .with_blur(0)
            .with_shadow_color([0, 0, 0, 255]);
        let mut fb1 = FrameBuffer::new(20, 20);
        ds_no_blur.render(&mut fb1, (0, 0), 1.0);
        let count_no_blur: usize = fb1.pixels().iter().filter(|p| p[3] > 0).count();

        let ds_blur = DropShadow::new(Box::new(inner))
            .with_offset(3, 3)
            .with_blur(2)
            .with_shadow_color([0, 0, 0, 255]);
        let mut fb2 = FrameBuffer::new(20, 20);
        ds_blur.render(&mut fb2, (0, 0), 1.0);
        let count_blur: usize = fb2.pixels().iter().filter(|p| p[3] > 0).count();

        assert!(count_blur > count_no_blur, "blur should expand shadow area: {} <= {}", count_blur, count_no_blur);
    }

    #[test]
    fn drop_shadow_box_blur_zero_radius_noop() {
        let mut pixels = vec![[0u16 as u8; 4]; 4];
        pixels[0] = [100, 100, 100, 255];
        let before = pixels.clone();
        DropShadow::box_blur(&mut pixels, 2, 2, 0);
        assert_eq!(pixels, before, "zero radius blur should not change pixels");
    }

    #[test]
    fn drop_shadow_box_blur_averages_neighbours() {
        // 3x3 buffer with center pixel set.
        let mut pixels = vec![[0u8; 4]; 9];
        pixels[4] = [255, 255, 255, 255]; // center
        DropShadow::box_blur(&mut pixels, 3, 3, 1);
        // After blur with radius=1, all pixels should have some value.
        for px in &pixels {
            assert!(px[0] > 0, "all pixels should have value after blur, got {:?}", px);
        }
    }

    #[test]
    fn drop_shadow_with_solid_color_layer() {
        let inner = SolidColor::new(200, 200, 200, 255);
        let ds = DropShadow::new(Box::new(inner))
            .with_offset(1, 1)
            .with_blur(1)
            .with_shadow_color([0, 0, 0, 128]);
        let mut fb = FrameBuffer::new(10, 10);
        ds.render(&mut fb, (0, 0), 1.0);
        // Both shadow and original should produce content.
        let has_content = fb.pixels().iter().any(|p| p[3] > 0);
        assert!(has_content, "should produce content");
    }

    // ─── Spread / Glow tests ────────────────────────────────────

    #[test]
    fn drop_shadow_positive_spread_expands_shadow() {
        let inner = RectLayer::new(5, 5, 2, 2, [255, 255, 255, 255]);
        let ds = DropShadow::new(Box::new(inner))
            .with_offset(0, 0)
            .with_blur(0)
            .with_spread(2)
            .with_shadow_color([0, 0, 0, 255]);

        let mut fb = FrameBuffer::new(20, 20);
        ds.render(&mut fb, (0, 0), 1.0);

        // The original 2x2 rect at (5,5) should be white.
        assert_eq!(fb.get_pixel(5, 5), Some(&[255, 255, 255, 255]));

        // With spread=2, shadow should extend beyond the rect.
        assert_eq!(fb.get_pixel(3, 3), Some(&[0, 0, 0, 255]));
        assert_eq!(fb.get_pixel(8, 8), Some(&[0, 0, 0, 255]));
    }

    #[test]
    fn drop_shadow_negative_spread_shrinks_shadow() {
        // With offset (5,5) the shadow peeks out at the bottom-right
        // of the inner layer. Negative spread erodes the shadow
        // inward, so fewer shadow pixels are visible.
        let inner = RectLayer::new(5, 5, 10, 10, [255, 255, 255, 255]);
        let ds = DropShadow::new(Box::new(inner))
            .with_offset(5, 5)
            .with_blur(0)
            .with_spread(-3)
            .with_shadow_color([0, 0, 0, 255]);

        let mut fb = FrameBuffer::new(30, 30);
        ds.render(&mut fb, (0, 0), 1.0);

        // Count shadow-only pixels (bottom-right of inner layer).
        // Shadow at (10,10) to (19,19), inner at (5,5) to (14,14).
        // Shadow-only region: x=15..19, y=15..19.
        let mut shadow_pixels_with_spread = 0u32;
        for y in 15..20 {
            for x in 15..20 {
                if let Some(p) = fb.get_pixel(x, y) {
                    if p[3] > 0 { shadow_pixels_with_spread += 1; }
                }
            }
        }

        // Compare against zero-spread.
        let inner2 = RectLayer::new(5, 5, 10, 10, [255, 255, 255, 255]);
        let ds2 = DropShadow::new(Box::new(inner2))
            .with_offset(5, 5)
            .with_blur(0)
            .with_spread(0)
            .with_shadow_color([0, 0, 0, 255]);
        let mut fb2 = FrameBuffer::new(30, 30);
        ds2.render(&mut fb2, (0, 0), 1.0);
        let mut shadow_pixels_no_spread = 0u32;
        for y in 15..20 {
            for x in 15..20 {
                if let Some(p) = fb2.get_pixel(x, y) {
                    if p[3] > 0 { shadow_pixels_no_spread += 1; }
                }
            }
        }

        assert!(shadow_pixels_no_spread > 0,
            "zero spread should have shadow pixels in the peek region");
        assert!(shadow_pixels_with_spread < shadow_pixels_no_spread,
            "negative spread should shrink shadow: {shadow_pixels_with_spread} < {shadow_pixels_no_spread}");
    }

    #[test]
    fn drop_shadow_zero_spread_is_noop() {
        let inner = RectLayer::new(5, 5, 2, 2, [255, 255, 255, 255]);
        let ds = DropShadow::new(Box::new(inner))
            .with_offset(0, 0)
            .with_blur(0)
            .with_spread(0)
            .with_shadow_color([0, 0, 0, 255]);

        let mut fb = FrameBuffer::new(20, 20);
        ds.render(&mut fb, (0, 0), 1.0);

        assert_eq!(fb.get_pixel(5, 5), Some(&[255, 255, 255, 255]));
        assert_eq!(fb.get_pixel(6, 6), Some(&[255, 255, 255, 255]));
        assert_eq!(fb.get_pixel(4, 4), Some(&[0, 0, 0, 0]));
    }

    #[test]
    fn drop_shadow_glow_sets_zero_offset() {
        let inner = RectLayer::new(3, 3, 4, 4, [255, 255, 255, 255]);
        let glow = DropShadow::new(Box::new(inner))
            .with_glow([255, 200, 0, 200], 3);

        assert_eq!(glow.offset, (0, 0));
        assert_eq!(glow.blur_radius, 3);
        assert_eq!(glow.shadow_color, [255, 200, 0, 200]);
    }

    #[test]
    fn drop_shadow_glow_render_produces_content() {
        let inner = RectLayer::new(5, 5, 2, 2, [255, 255, 255, 255]);
        let glow = DropShadow::new(Box::new(inner))
            .with_glow([255, 200, 0, 200], 3);

        let mut fb = FrameBuffer::new(20, 20);
        glow.render(&mut fb, (0, 0), 1.0);

        assert_eq!(fb.get_pixel(5, 5), Some(&[255, 255, 255, 255]));
        let glow_pixel = fb.get_pixel(4, 4).unwrap();
        assert!(glow_pixel[3] > 0, "glow should produce non-transparent pixels");
    }

    #[test]
    fn shadow_layer_type_alias_works() {
        let inner = RectLayer::new(0, 0, 5, 5, [255, 0, 0, 255]);
        let shadow: super::ShadowLayer = DropShadow::new(Box::new(inner));
        let mut fb = FrameBuffer::new(10, 10);
        shadow.render(&mut fb, (0, 0), 1.0);
        let any_nonzero = fb.pixels().iter().any(|p| p[3] > 0);
        assert!(any_nonzero, "ShadowLayer should render content");
    }


    #[test]
    fn border_layer_corners_not_double_drawn() {
        // Verify that corners are drawn exactly once, not double-blended.
        // A border with bw=3 on a 10x10 rect should have each corner pixel
        // with the same alpha as edge pixels (not double the alpha).
        let border = BorderLayer::new(0, 0, 10, 10, [255, 0, 0, 200], 3);
        let mut fb = FrameBuffer::new(10, 10);
        border.render(&mut fb, (0, 0), 1.0);

        // All border pixels should have the same alpha.
        // Corner pixel (0, 0)
        let corner = fb.get_pixel(0, 0).unwrap();
        // Edge pixel (5, 0) - top edge, not a corner
        let edge_top = fb.get_pixel(5, 0).unwrap();
        // Edge pixel (0, 5) - left edge, not a corner
        let edge_left = fb.get_pixel(0, 5).unwrap();
        // Interior pixel (5, 5) - should be untouched
        let interior = fb.get_pixel(5, 5).unwrap();

        // All border pixels should have the same alpha (not double-blended)
        assert_eq!(corner[3], edge_top[3], "corner and top edge alpha should match");
        assert_eq!(corner[3], edge_left[3], "corner and left edge alpha should match");
        // Interior should be untouched (alpha = 0)
        assert_eq!(interior[3], 0, "interior pixel should be untouched");
    }

    #[test]
    fn border_layer_thick_border_rendering() {
        // Test with border_width > half the smallest dimension.
        // bw=6 on a 10x10 rect means borders overlap completely (no interior).
        let border = BorderLayer::new(0, 0, 10, 10, [0, 255, 0, 255], 6);
        let mut fb = FrameBuffer::new(10, 10);
        border.render(&mut fb, (0, 0), 1.0);

        // Every pixel should be the border color.
        for y in 0..10 {
            for x in 0..10 {
                let px = fb.get_pixel(x, y).unwrap();
                assert_eq!(px[1], 255, "pixel ({x},{y}) should be green");
            }
        }
    }

    // ── BorderLayer tests ───────────────────────────────────────────────'

    #[test]
    fn border_layer_new_defaults() {
        let b = BorderLayer::new(2, 3, 10, 8, [255, 0, 0, 255], 1);
        assert_eq!(b.x, 2);
        assert_eq!(b.y, 3);
        assert_eq!(b.width, 10);
        assert_eq!(b.height, 8);
        assert_eq!(b.color, [255, 0, 0, 255]);
        assert_eq!(b.border_width, 1);
        assert_eq!(b.z_order(), 0);
    }

    #[test]
    fn border_layer_builders() {
        let b = BorderLayer::new(0, 0, 5, 5, [0, 255, 0, 255], 2)
            .with_z(7)
            .with_name("box-border");
        assert_eq!(b.z_order(), 7);
        assert_eq!(b.name(), "box-border");
    }

    #[test]
    fn border_layer_bounds() {
        let b = BorderLayer::new(3, 4, 5, 6, [0, 0, 0, 255], 1);
        assert_eq!(b.bounds(), Some(Rect::new(3, 4, 5, 6)));
    }

    #[test]
    fn border_layer_render_1px_draws_outline_only() {
        let b = BorderLayer::new(1, 1, 3, 3, [255, 0, 0, 255], 1);
        let mut fb = FrameBuffer::new(5, 5);
        b.render(&mut fb, (0, 0), 1.0);
        // Top edge
        assert_eq!(fb.get_pixel(1, 1), Some(&[255, 0, 0, 255]));
        assert_eq!(fb.get_pixel(2, 1), Some(&[255, 0, 0, 255]));
        assert_eq!(fb.get_pixel(3, 1), Some(&[255, 0, 0, 255]));
        // Bottom edge
        assert_eq!(fb.get_pixel(1, 3), Some(&[255, 0, 0, 255]));
        assert_eq!(fb.get_pixel(2, 3), Some(&[255, 0, 0, 255]));
        assert_eq!(fb.get_pixel(3, 3), Some(&[255, 0, 0, 255]));
        // Left edge
        assert_eq!(fb.get_pixel(1, 2), Some(&[255, 0, 0, 255]));
        // Right edge
        assert_eq!(fb.get_pixel(3, 2), Some(&[255, 0, 0, 255]));
        // Interior should be transparent
        assert_eq!(fb.get_pixel(2, 2), Some(&[0, 0, 0, 0]));
    }

    #[test]
    fn border_layer_render_2px_border() {
        let b = BorderLayer::new(0, 0, 6, 6, [0, 255, 0, 255], 2);
        let mut fb = FrameBuffer::new(6, 6);
        b.render(&mut fb, (0, 0), 1.0);
        // Top 2 rows should be green
        for x in 0..6 {
            assert_eq!(fb.get_pixel(x, 0), Some(&[0, 255, 0, 255]));
            assert_eq!(fb.get_pixel(x, 1), Some(&[0, 255, 0, 255]));
        }
        // Bottom 2 rows should be green
        for x in 0..6 {
            assert_eq!(fb.get_pixel(x, 4), Some(&[0, 255, 0, 255]));
            assert_eq!(fb.get_pixel(x, 5), Some(&[0, 255, 0, 255]));
        }
        // Left 2 columns (middle rows) should be green
        assert_eq!(fb.get_pixel(0, 2), Some(&[0, 255, 0, 255]));
        assert_eq!(fb.get_pixel(1, 2), Some(&[0, 255, 0, 255]));
        // Right 2 columns (middle rows) should be green
        assert_eq!(fb.get_pixel(4, 2), Some(&[0, 255, 0, 255]));
        assert_eq!(fb.get_pixel(5, 2), Some(&[0, 255, 0, 255]));
        // Interior (2x2 center) should be transparent
        assert_eq!(fb.get_pixel(2, 2), Some(&[0, 0, 0, 0]));
        assert_eq!(fb.get_pixel(3, 3), Some(&[0, 0, 0, 0]));
    }

    #[test]
    fn border_layer_render_zero_opacity_noop() {
        let b = BorderLayer::new(0, 0, 5, 5, [255, 0, 0, 255], 1);
        let mut fb = FrameBuffer::new(5, 5);
        b.render(&mut fb, (0, 0), 0.0);
        for px in fb.pixels() {
            assert_eq!(*px, [0, 0, 0, 0]);
        }
    }

    #[test]
    fn border_layer_render_offset_translates() {
        let b = BorderLayer::new(0, 0, 3, 3, [0, 0, 255, 255], 1);
        let mut fb = FrameBuffer::new(6, 6);
        b.render(&mut fb, (2, 2), 1.0);
        // Border should appear at offset position
        assert_eq!(fb.get_pixel(2, 2), Some(&[0, 0, 255, 255]));
        assert_eq!(fb.get_pixel(4, 4), Some(&[0, 0, 255, 255]));
        // Interior at offset should be transparent
        assert_eq!(fb.get_pixel(3, 3), Some(&[0, 0, 0, 0]));
        // Original position should be transparent
        assert_eq!(fb.get_pixel(0, 0), Some(&[0, 0, 0, 0]));
    }

    #[test]
    fn border_layer_render_clips_outside_target() {
        let b = BorderLayer::new(3, 3, 5, 5, [255, 0, 0, 255], 1);
        let mut fb = FrameBuffer::new(4, 4);
        b.render(&mut fb, (0, 0), 1.0);
        // Only the top-left corner of the border should be visible
        assert_eq!(fb.get_pixel(3, 3), Some(&[255, 0, 0, 255]));
    }

    #[test]
    fn border_layer_border_width_clamped_to_half() {
        // border_width > width/2 should be clamped
        let b = BorderLayer::new(0, 0, 4, 4, [255, 0, 0, 255], 3);
        let mut fb = FrameBuffer::new(4, 4);
        b.render(&mut fb, (0, 0), 1.0);
        // All pixels should be filled (border clamped to 2)
        for px in fb.pixels() {
            assert_eq!(*px, [255, 0, 0, 255]);
        }
    }

    #[test]
    fn border_layer_zero_border_width_is_noop() {
        let b = BorderLayer::new(0, 0, 5, 5, [255, 0, 0, 255], 0);
        let mut fb = FrameBuffer::new(5, 5);
        b.render(&mut fb, (0, 0), 1.0);
        for px in fb.pixels() {
            assert_eq!(*px, [0, 0, 0, 0]);
        }
    }

    #[test]
    fn border_layer_1x1_pixel() {
        let b = BorderLayer::new(2, 2, 1, 1, [100, 200, 50, 255], 1);
        let mut fb = FrameBuffer::new(5, 5);
        b.render(&mut fb, (0, 0), 1.0);
        assert_eq!(fb.get_pixel(2, 2), Some(&[100, 200, 50, 255]));
    }

    #[test]
    fn border_layer_full_width_border() {
        // border_width == width/2 fills the entire rectangle
        let b = BorderLayer::new(0, 0, 4, 4, [0, 128, 255, 255], 2);
        let mut fb = FrameBuffer::new(4, 4);
        b.render(&mut fb, (0, 0), 1.0);
        for px in fb.pixels() {
            assert_eq!(*px, [0, 128, 255, 255]);
        }
    }

    // ── GradientLayer tests ───────────────────────────────────

    #[test]
    fn gradient_layer_linear_defaults() {
        let g = GradientLayer::linear(
            0, 0, 10, 10,
            [255, 0, 0, 255], [0, 0, 255, 255],
            0, 0, 10, 10,
        );
        assert_eq!(g.x, 0);
        assert_eq!(g.y, 0);
        assert_eq!(g.width, 10);
        assert_eq!(g.height, 10);
        assert_eq!(g.z_order(), 0);
        assert_eq!(g.name(), "GradientLayer");
    }

    #[test]
    fn gradient_layer_radial_defaults() {
        let g = GradientLayer::radial(
            0, 0, 20, 20,
            [0, 255, 0, 255], [0, 0, 0, 255],
            10, 10, 10,
        );
        assert_eq!(g.width, 20);
        assert_eq!(g.height, 20);
        assert!(matches!(g.kind, GradientKind::Radial { .. }));
    }

    #[test]
    fn gradient_layer_builders() {
        let g = GradientLayer::linear(
            0, 0, 10, 10,
            [255, 0, 0, 255], [0, 0, 255, 255],
            0, 0, 10, 10,
        ).with_z(5).with_name("bg-grad");
        assert_eq!(g.z_order(), 5);
        assert_eq!(g.name(), "bg-grad");
    }

    #[test]
    fn gradient_layer_bounds() {
        let g = GradientLayer::linear(
            3, 4, 5, 6,
            [0, 0, 0, 255], [255, 255, 255, 255],
            0, 0, 5, 6,
        );
        assert_eq!(g.bounds(), Some(Rect::new(3, 4, 5, 6)));
    }

    #[test]
    fn gradient_layer_linear_horizontal() {
        // Horizontal gradient from red (left) to blue (right).
        let g = GradientLayer::linear(
            0, 0, 10, 1,
            [255, 0, 0, 255], [0, 0, 255, 255],
            0, 0, 9, 0,
        );
        let mut fb = FrameBuffer::new(10, 1);
        g.render(&mut fb, (0, 0), 1.0);

        // Leftmost pixel should be red.
        let left = fb.get_pixel(0, 0).unwrap();
        assert_eq!(left[0], 255, "left pixel R should be 255");
        assert_eq!(left[2], 0, "left pixel B should be 0");

        // Rightmost pixel should be blue.
        let right = fb.get_pixel(9, 0).unwrap();
        assert!(right[2] > 200, "right pixel B should be > 200, got {}", right[2]);

        // Middle pixel should be a mix.
        let mid = fb.get_pixel(4, 0).unwrap();
        assert!(mid[0] > 50 && mid[0] < 200, "mid R = {}", mid[0]);
        assert!(mid[2] > 50 && mid[2] < 200, "mid B = {}", mid[2]);
    }

    #[test]
    fn gradient_layer_linear_vertical() {
        // Vertical gradient from green (top) to transparent (bottom).
        let g = GradientLayer::linear(
            0, 0, 1, 10,
            [0, 255, 0, 255], [0, 255, 0, 0],
            0, 0, 0, 9,
        );
        let mut fb = FrameBuffer::new(1, 10);
        g.render(&mut fb, (0, 0), 1.0);

        let top = fb.get_pixel(0, 0).unwrap();
        assert_eq!(top[1], 255, "top pixel G should be 255");
        assert_eq!(top[3], 255, "top pixel A should be 255");

        let bottom = fb.get_pixel(0, 9).unwrap();
        assert!(bottom[3] < 10, "bottom pixel A should be near 0, got {}", bottom[3]);
    }

    #[test]
    fn gradient_layer_linear_zero_length_line() {
        // start == end → t == 0 for all pixels → start colour everywhere.
        let g = GradientLayer::linear(
            0, 0, 5, 5,
            [100, 200, 50, 255], [0, 0, 0, 255],
            2, 2, 2, 2,
        );
        let mut fb = FrameBuffer::new(5, 5);
        g.render(&mut fb, (0, 0), 1.0);
        for y in 0..5 {
            for x in 0..5 {
                let px = fb.get_pixel(x, y).unwrap();
                assert_eq!(px[0], 100, "R at ({x},{y}) should be 100");
                assert_eq!(px[1], 200, "G at ({x},{y}) should be 200");
            }
        }
    }

    #[test]
    fn gradient_layer_linear_zero_opacity_is_noop() {
        let g = GradientLayer::linear(
            0, 0, 5, 5,
            [255, 0, 0, 255], [0, 0, 255, 255],
            0, 0, 5, 5,
        );
        let mut fb = FrameBuffer::new(5, 5);
        g.render(&mut fb, (0, 0), 0.0);
        for px in fb.pixels() {
            assert_eq!(*px, [0, 0, 0, 0], "zero opacity must not change framebuffer");
        }
    }

    #[test]
    fn gradient_layer_linear_offset_translates() {
        let g = GradientLayer::linear(
            0, 0, 3, 1,
            [255, 0, 0, 255], [0, 0, 255, 255],
            0, 0, 2, 0,
        );
        let mut fb = FrameBuffer::new(6, 1);
        g.render(&mut fb, (3, 0), 1.0);
        // Pixels 0..3 should still be transparent.
        assert_eq!(fb.get_pixel(0, 0), Some(&[0, 0, 0, 0]));
        // Pixel at offset should be rendered.
        assert!(fb.get_pixel(3, 0).unwrap()[0] > 0, "offset pixel should have red");
    }

    #[test]
    fn gradient_layer_linear_clips_outside_framebuffer() {
        let g = GradientLayer::linear(
            5, 5, 10, 10,
            [255, 0, 0, 255], [0, 0, 255, 255],
            0, 0, 10, 10,
        );
        let mut fb = FrameBuffer::new(3, 3);
        g.render(&mut fb, (0, 0), 1.0);
        // All pixels should remain transparent (gradient is outside).
        for px in fb.pixels() {
            assert_eq!(*px, [0, 0, 0, 0]);
        }
    }

    #[test]
    fn gradient_layer_radial_center_is_start_colour() {
        // Radial gradient from white center to black edge.
        let g = GradientLayer::radial(
            0, 0, 21, 21,
            [255, 255, 255, 255], [0, 0, 0, 255],
            10, 10, 10,
        );
        let mut fb = FrameBuffer::new(21, 21);
        g.render(&mut fb, (0, 0), 1.0);

        // Center pixel should be white.
        let center = fb.get_pixel(10, 10).unwrap();
        assert_eq!(center[0], 255);
        assert_eq!(center[1], 255);
        assert_eq!(center[2], 255);
    }

    #[test]
    fn gradient_layer_radial_edge_is_end_colour() {
        // Radial gradient: at radius distance, colour should be end colour.
        let g = GradientLayer::radial(
            0, 0, 21, 1,
            [255, 0, 0, 255], [0, 0, 255, 255],
            10, 0, 10,
        );
        let mut fb = FrameBuffer::new(21, 1);
        g.render(&mut fb, (0, 0), 1.0);

        // Pixel at (0, 0) is 10px from center → at radius → end colour.
        let edge = fb.get_pixel(0, 0).unwrap();
        assert!(edge[2] > 200, "edge pixel B should be > 200, got {}", edge[2]);
    }

    #[test]
    fn gradient_layer_radial_beyond_radius_is_end_colour() {
        // Pixels beyond the radius should clamp to t=1 → end colour.
        let g = GradientLayer::radial(
            0, 0, 30, 30,
            [255, 0, 0, 255], [0, 0, 255, 255],
            15, 15, 5,
        );
        let mut fb = FrameBuffer::new(30, 30);
        g.render(&mut fb, (0, 0), 1.0);

        // Corner pixel (0,0) is ~21px from center, beyond radius=5.
        let corner = fb.get_pixel(0, 0).unwrap();
        assert!(corner[2] > 200, "corner B should be > 200, got {}", corner[2]);
    }

    #[test]
    fn gradient_layer_radial_zero_radius() {
        // radius=0 → all t=0 → start colour everywhere.
        let g = GradientLayer::radial(
            0, 0, 5, 5,
            [100, 200, 50, 255], [0, 0, 0, 255],
            2, 2, 0,
        );
        let mut fb = FrameBuffer::new(5, 5);
        g.render(&mut fb, (0, 0), 1.0);
        for y in 0..5 {
            for x in 0..5 {
                let px = fb.get_pixel(x, y).unwrap();
                assert_eq!(px[0], 100, "R at ({x},{y}) should be 100");
                assert_eq!(px[1], 200, "G at ({x},{y}) should be 200");
            }
        }
    }

    #[test]
    fn gradient_layer_radial_zero_opacity_is_noop() {
        let g = GradientLayer::radial(
            0, 0, 5, 5,
            [255, 0, 0, 255], [0, 0, 255, 255],
            2, 2, 5,
        );
        let mut fb = FrameBuffer::new(5, 5);
        g.render(&mut fb, (0, 0), 0.0);
        for px in fb.pixels() {
            assert_eq!(*px, [0, 0, 0, 0]);
        }
    }

    #[test]
    fn gradient_layer_interpolate_at_endpoints() {
        use super::GradientLayer;
        // t=0 → start colour, t=1 → end colour.
        let start = [100, 50, 20, 200];
        let end = [200, 150, 80, 100];
        assert_eq!(GradientLayer::interpolate(0.0, start, end), start);
        assert_eq!(GradientLayer::interpolate(1.0, start, end), end);
        assert_eq!(GradientLayer::interpolate(0.5, start, end), [150, 100, 50, 150]);
    }

    #[test]
    fn gradient_kind_debug() {
        let kind = GradientKind::Linear { start_x: 0, start_y: 0, end_x: 10, end_y: 10 };
        let s = format!("{kind:?}");
        assert!(s.contains("Linear"));

        let kind = GradientKind::Radial { center_x: 5, center_y: 5, radius: 10 };
        let s = format!("{kind:?}");
        assert!(s.contains("Radial"));
    }

    #[test]
    fn gradient_kind_eq() {
        let a = GradientKind::Linear { start_x: 0, start_y: 0, end_x: 10, end_y: 10 };
        let b = GradientKind::Linear { start_x: 0, start_y: 0, end_x: 10, end_y: 10 };
        assert_eq!(a, b);

        let c = GradientKind::Radial { center_x: 5, center_y: 5, radius: 10 };
        assert_ne!(a, c);
    }
}

#[cfg(all(test, feature = "image-decoder"))]
mod image_layer_tests {
    use super::{ImageLayer, Layer};
    use crate::framebuffer::FrameBuffer;
    use crate::geometry::Rect;
    use image::{ImageBuffer, Rgba};

    /// 1x1 red PNG, base64-decoded on the fly. Kept tiny so the
    /// test is hermetic (no temp files, no I/O).
    fn red_pixel_image() -> image::DynamicImage {
        let img: ImageBuffer<Rgba<u8>, Vec<u8>> =
            ImageBuffer::from_pixel(1, 1, Rgba([255, 0, 0, 255]));
        image::DynamicImage::ImageRgba8(img)
    }

    #[test]
    fn image_layer_from_dynamic_has_correct_dimensions() {
        let l = ImageLayer::from_dynamic(red_pixel_image(), 0, 0);
        assert_eq!(l.width(), 1);
        assert_eq!(l.height(), 1);
    }

    #[test]
    fn image_layer_bounds_reports_image_size_at_position() {
        let l = ImageLayer::from_dynamic(red_pixel_image(), 3, 4);
        assert_eq!(l.bounds(), Some(Rect::new(3, 4, 1, 1)));
    }

    #[test]
    fn image_layer_render_writes_pixel_at_offset_position() {
        let l = ImageLayer::from_dynamic(red_pixel_image(), 1, 2);
        let mut fb = FrameBuffer::new(3, 4);
        l.render(&mut fb, (0, 0), 1.0);
        assert_eq!(fb.get_pixel(1, 2), Some(&[255, 0, 0, 255]));
        // Other pixels stay transparent.
        assert_eq!(fb.get_pixel(0, 0), Some(&[0, 0, 0, 0]));
        assert_eq!(fb.get_pixel(2, 3), Some(&[0, 0, 0, 0]));
    }

    #[test]
    fn image_layer_render_offset_translates() {
        let l = ImageLayer::from_dynamic(red_pixel_image(), 0, 0);
        let mut fb = FrameBuffer::new(3, 3);
        l.render(&mut fb, (2, 1), 1.0);
        assert_eq!(fb.get_pixel(2, 1), Some(&[255, 0, 0, 255]));
        // Without the offset, (0, 0) is still transparent.
        assert_eq!(fb.get_pixel(0, 0), Some(&[0, 0, 0, 0]));
    }

    #[test]
    fn image_layer_render_opacity_blends_pixel() {
        // Fully opaque red at 50% opacity onto transparent: result
        // should be a translucent red.
        let l = ImageLayer::from_dynamic(red_pixel_image(), 0, 0);
        let mut fb = FrameBuffer::new(1, 1);
        l.render(&mut fb, (0, 0), 0.5);
        let px = fb.get_pixel(0, 0).unwrap();
        assert_eq!(px[0], 255);
        assert_eq!(px[1], 0);
        assert_eq!(px[2], 0);
        // Alpha should be 128 (50% of 255).
        assert!((px[3] as i32 - 128).abs() <= 1, "alpha = {}", px[3]);
    }

    #[test]
    fn image_layer_render_clips_outside_framebuffer() {
        // 2x2 image at (5, 5) is fully off the 3x3 framebuffer.
        let img: ImageBuffer<Rgba<u8>, Vec<u8>> =
            ImageBuffer::from_pixel(2, 2, Rgba([1, 2, 3, 255]));
        let l = ImageLayer::from_dynamic(image::DynamicImage::ImageRgba8(img), 5, 5);
        let mut fb = FrameBuffer::new(3, 3);
        l.render(&mut fb, (0, 0), 1.0);
        for px in fb.pixels() {
            assert_eq!(*px, [0, 0, 0, 0]);
        }
    }

    // -- from_path tests (require filesystem I/O) ----------------
    //
    // These tests exercise ImageLayer::from_path, which reads a
    // PNG/JPEG file from disk. We create a tiny temp file, write
    // a 1x1 red PNG to it, then call from_path.

    /// Helper: write a small RGBA image to a temp PNG file and
    /// return the path. The file is deleted when the TempDir
    /// guard is dropped.
    fn write_temp_png(
        img: &image::DynamicImage,
    ) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().expect("failed to create temp dir");
        let path = dir.path().join("test_image.png");
        img.write_to(
            &mut std::fs::File::create(&path).expect("failed to create temp file"),
            image::ImageFormat::Png,
        )
        .expect("failed to write PNG");
        (dir, path)
    }

    #[test]
    fn image_layer_from_path_loads_png() {
        let img = red_pixel_image();
        let (_dir, path) = write_temp_png(&img);

        let result = ImageLayer::from_path(&path, 5, 10);
        assert!(result.is_ok(), "from_path must succeed: {:?}", result.err());
        let layer = result.unwrap();
        assert_eq!(layer.width(), 1);
        assert_eq!(layer.height(), 1);
        assert_eq!(layer.x, 5);
        assert_eq!(layer.y, 10);
    }

    #[test]
    fn image_layer_from_path_render_writes_pixel() {
        let img = red_pixel_image();
        let (_dir, path) = write_temp_png(&img);

        let layer = ImageLayer::from_path(&path, 2, 3).unwrap();
        let mut fb = FrameBuffer::new(5, 5);
        layer.render(&mut fb, (0, 0), 1.0);
        assert_eq!(fb.get_pixel(2, 3), Some(&[255, 0, 0, 255]));
        assert_eq!(fb.get_pixel(0, 0), Some(&[0, 0, 0, 0]));
    }

    #[test]
    fn image_layer_from_path_with_offset() {
        let img = red_pixel_image();
        let (_dir, path) = write_temp_png(&img);

        let layer = ImageLayer::from_path(&path, 0, 0).unwrap();
        let mut fb = FrameBuffer::new(5, 5);
        layer.render(&mut fb, (3, 4), 1.0);
        assert_eq!(fb.get_pixel(3, 4), Some(&[255, 0, 0, 255]));
    }

    #[test]
    fn image_layer_from_path_nonexistent_returns_error() {
        let result = ImageLayer::from_path("/nonexistent/path/test.png", 0, 0);
        assert!(result.is_err(), "from_path with nonexistent file must fail");
        let err = result.unwrap_err();
        // The error should be a NotFound or similar IO error.
        let err_str = err.to_string();
        assert!(
            err_str.contains("not found") || err_str.contains("No such file") || err_str.contains("os error"),
            "error should mention file not found: {err_str}"
        );
    }

    #[test]
    fn image_layer_from_path_invalid_format_returns_error() {
        // Write random bytes to a file — not a valid PNG.
        let dir = tempfile::tempdir().expect("failed to create temp dir");
        let path = dir.path().join("invalid.png");
        std::fs::write(&path, b"not a real png file").expect("failed to write");

        let result = ImageLayer::from_path(&path, 0, 0);
        assert!(result.is_err(), "from_path with invalid file must fail");
    }

    #[test]
    fn image_layer_from_path_bounds_reports_position() {
        let img = red_pixel_image();
        let (_dir, path) = write_temp_png(&img);

        let layer = ImageLayer::from_path(&path, 10, 20).unwrap();
        assert_eq!(layer.bounds(), Some(Rect::new(10, 20, 1, 1)));
    }

    #[test]
    fn image_layer_from_path_z_order_and_name() {
        let img = red_pixel_image();
        let (_dir, path) = write_temp_png(&img);

        let layer = ImageLayer::from_path(&path, 0, 0)
            .unwrap()
            .with_z(5)
            .with_name("test-img");
        assert_eq!(layer.z_order(), 5);
        assert_eq!(layer.name(), "test-img");
    }
}

// ── Additional font-rasterizer coverage tests ───────────────
// Targeted at the 21 uncovered lines in render_with_font,
// ensure_font (FontSource::Path), and boundary conditions.

#[cfg(all(test, feature = "font-rasterizer"))]
mod font_rasterizer_extra {
    use super::{FontSource, Layer, TextLayer};
    use crate::framebuffer::FrameBuffer;

    #[test]
    fn font_source_path_loads_from_file() {
        // Copy the bundled font to a temp file so ensure_font
        // exercises the FontSource::Path branch.
        let dir = tempfile::tempdir().expect("failed to create temp dir");
        let path = dir.path().join("test_font.ttf");
        std::fs::write(&path, crate::layer::BUNDLED_FONT_DATA)
            .expect("failed to write temp font file");

        let t = TextLayer::new(0, 0, "AB", [255; 4])
            .with_font(FontSource::Path(path), 14.0);
        // text_width triggers ensure_font, which reads from disk.
        let w = t.text_width();
        assert!(w > 0, "FontSource::Path must produce positive width, got {w}");
    }

    #[test]
    fn render_with_font_handles_spaces() {
        // 'A B' contains a space; exercise the space-advance path.
        let t = TextLayer::new(0, 0, "A B", [200, 100, 50, 255])
            .with_font_size(14.0);
        let mut fb = FrameBuffer::new(40, 20);
        t.render(&mut fb, (0, 0), 1.0);
        // The 'A' and 'B' should produce non-transparent pixels;
        // the space between them should remain transparent.
        let has_pixels = fb.pixels().iter().any(|p| p[3] > 0);
        assert!(has_pixels, "render with spaces should produce glyph pixels");
    }

    #[test]
    fn render_with_font_clips_outside_framebuffer() {
        // Position the text far to the right so most glyphs fall
        // outside the framebuffer; get_pixel_mut returns None
        // and the glyph is silently clipped.
        let t = TextLayer::new(50, 50, "X", [255, 255, 255, 255])
            .with_font_size(14.0);
        let mut fb = FrameBuffer::new(10, 10);
        t.render(&mut fb, (0, 0), 1.0);
        // All pixels should remain transparent (clipped).
        let any_nonzero = fb.pixels().iter().any(|p| p[3] > 0);
        assert!(!any_nonzero, "text fully outside framebuffer should produce no pixels");
    }

    #[test]
    fn render_with_font_zero_opacity_is_noop() {
        let t = TextLayer::new(0, 0, "A", [200, 100, 50, 255])
            .with_font_size(14.0);
        let mut fb = FrameBuffer::new(20, 20);
        t.render(&mut fb, (0, 0), 0.0);
        // effective = 0.0, so the framebuffer must stay untouched.
        let any_nonzero = fb.pixels().iter().any(|p| p[3] > 0);
        assert!(!any_nonzero, "render with 0 opacity should not change framebuffer");
    }

    #[test]
    fn render_with_font_negative_offset_clips_glyph() {
        // Use a tiny 1x1 framebuffer and position text at (0,0).
        // The glyph will extend beyond the single pixel, so
        // get_pixel_mut returns None for out-of-bounds pixels,
        // exercising the clipping path inside render_with_font.
        // We also pass an offset that pushes the cursor negative.
        let t = TextLayer::new(0, 0, "A", [200, 100, 50, 255])
            .with_font_size(14.0);
        let mut fb = FrameBuffer::new(1, 1);
        // offset = u32::MAX wraps when added to x=0 via saturating_add,
        // then ox as i32 = -1, making glyph_x negative for most pixels.
        t.render(&mut fb, (u32::MAX, u32::MAX), 1.0);
        // The single pixel may or may not be hit depending on
        // glyph metrics; what matters is the code path runs without panic.
    }

    #[test]
    fn render_with_font_partial_clip_left_edge() {
        // Position text so the glyph is partially outside the left
        // edge of the framebuffer, exercising the px < 0 path
        // for some glyph pixels while others render normally.
        let t = TextLayer::new(0, 0, "A", [200, 100, 50, 255])
            .with_font_size(14.0);
        let mut fb = FrameBuffer::new(3, 20);
        t.render(&mut fb, (0, 0), 1.0);
        // Some pixels should render (the rightmost glyph pixels
        // that fall within bounds).
        let has_pixels = fb.pixels().iter().any(|p| p[3] > 0);
        assert!(has_pixels, "partial clip should still render some glyph pixels");
    }

    #[test]
    fn render_with_font_multi_line_with_newlines_and_spaces() {
        // Exercise newline reset + space advance in a single render.
        let t = TextLayer::new(0, 0, "A B\nCD", [200, 100, 50, 255])
            .with_font_size(14.0);
        let mut fb = FrameBuffer::new(40, 40);
        t.render(&mut fb, (0, 0), 1.0);
        let has_pixels = fb.pixels().iter().any(|p| p[3] > 0);
        assert!(has_pixels, "multi-line render with spaces should produce pixels");
        // Verify two distinct rows of glyph pixels exist.
        let h = fb.height() as usize;
        let w = fb.width() as usize;
        let row1 = fb.pixels()[..(w * 20)].iter().any(|p| p[3] > 0);
        let row2 = fb.pixels()[(w * 20)..].iter().any(|p| p[3] > 0);
        assert!(row1 && row2, "both lines should produce glyph pixels");
    }

    #[test]
    fn render_with_font_empty_text_is_noop() {
        let t = TextLayer::new(0, 0, "", [200, 100, 50, 255]);
        let mut fb = FrameBuffer::new(10, 10);
        t.render(&mut fb, (0, 0), 1.0);
        let any_nonzero = fb.pixels().iter().any(|p| p[3] > 0);
        assert!(!any_nonzero, "empty text render should not change framebuffer");
    }

    // ─── SVGLayer tests ──────────────────────────────────────

    #[cfg(feature = "svg-renderer")]
    mod svg_tests {
        use super::*;
        use crate::SVGLayer;

        const SVG_RECT_10: &str =
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"><rect width="10" height="10" fill="red"/></svg>"#;
        const SVG_CIRCLE_5: &str =
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="5" height="5"><circle cx="2.5" cy="2.5" r="2" fill="blue"/></svg>"#;
        const SVG_RECT_5: &str =
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="5" height="5"><rect width="5" height="5" fill="green"/></svg>"#;
        const SVG_RECT_20X15: &str =
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="15"><rect width="20" height="15" fill="red"/></svg>"#;

        #[test]
        fn svg_layer_from_str_renders_rect() {
            let svg = SVGLayer::from_str(SVG_RECT_10, 0, 0)
                .expect("should parse SVG");

            assert_eq!(svg.width(), 10);
            assert_eq!(svg.height(), 10);

            let mut fb = FrameBuffer::new(10, 10);
            svg.render(&mut fb, (0, 0), 1.0);

            let center = fb.get_pixel(5, 5).unwrap();
            assert!(center[0] > 200, "red channel should be high, got {}", center[0]);
            assert!(center[3] > 0, "alpha should be non-zero");
        }

        #[test]
        fn svg_layer_from_bytes_works() {
            let svg = SVGLayer::from_bytes(SVG_CIRCLE_5.as_bytes(), 0, 0)
                .expect("should parse SVG from bytes");
            assert_eq!(svg.width(), 5);
            assert_eq!(svg.height(), 5);
        }

        #[test]
        fn svg_layer_invalid_svg_returns_error() {
            let result = SVGLayer::from_str("not valid svg", 0, 0);
            assert!(result.is_err(), "invalid SVG should return error");
        }

        #[test]
        fn svg_layer_with_position() {
            let svg = SVGLayer::from_str(SVG_RECT_5, 10, 10)
                .expect("should parse SVG");

            let mut fb = FrameBuffer::new(20, 20);
            svg.render(&mut fb, (0, 0), 1.0);

            // Pixel at (10,10) should have non-zero alpha.
            let px = fb.get_pixel(10, 10).unwrap();
            assert!(px[3] > 0, "alpha should be non-zero at (10,10)");
            // Pixel at (0,0) should be transparent.
            assert_eq!(fb.get_pixel(0, 0), Some(&[0, 0, 0, 0]));
        }

        #[test]
        fn svg_layer_bounds() {
            let svg = SVGLayer::from_str(SVG_RECT_20X15, 5, 5)
                .expect("should parse SVG");

            let bounds = svg.bounds().unwrap();
            assert_eq!(bounds.x, 5);
            assert_eq!(bounds.y, 5);
            assert_eq!(bounds.width, 20);
            assert_eq!(bounds.height, 15);
        }
    }

    // ─── ClipLayer tests ───────────────────────────────────────

    use crate::geometry::Rect;
    use super::{ClipLayer, ClipRegion, RectLayer};

    // ─── Rounded corners tests ──────────────────────────────────

    #[test]
    fn rect_layer_zero_radius_is_sharp() {
        let r = RectLayer::new(0, 0, 10, 10, [255, 0, 0, 255]);
        let mut fb = FrameBuffer::new(10, 10);
        r.render(&mut fb, (0, 0), 1.0);
        // All pixels should be red.
        for px in fb.pixels() {
            assert_eq!(*px, [255, 0, 0, 255]);
        }
    }

    #[test]
    fn rect_layer_radius_clips_corners() {
        let r = RectLayer::new(0, 0, 10, 10, [255, 0, 0, 255])
            .with_border_radius(3);
        let mut fb = FrameBuffer::new(10, 10);
        r.render(&mut fb, (0, 0), 1.0);

        // Center pixel should be red.
        assert_eq!(fb.get_pixel(5, 5), Some(&[255, 0, 0, 255]));

        // Corners should be transparent.
        assert_eq!(fb.get_pixel(0, 0), Some(&[0, 0, 0, 0]));
        assert_eq!(fb.get_pixel(9, 0), Some(&[0, 0, 0, 0]));
        assert_eq!(fb.get_pixel(0, 9), Some(&[0, 0, 0, 0]));
        assert_eq!(fb.get_pixel(9, 9), Some(&[0, 0, 0, 0]));

        // Pixels just inside the arc should be red.
        assert_eq!(fb.get_pixel(1, 1), Some(&[255, 0, 0, 255]));
    }

    #[test]
    fn rect_layer_large_radius_clips_more() {
        let r = RectLayer::new(0, 0, 20, 20, [0, 255, 0, 255])
            .with_border_radius(8);
        let mut fb = FrameBuffer::new(20, 20);
        r.render(&mut fb, (0, 0), 1.0);

        // Center should be green.
        assert_eq!(fb.get_pixel(10, 10), Some(&[0, 255, 0, 255]));

        // Extreme corners should be transparent.
        assert_eq!(fb.get_pixel(0, 0), Some(&[0, 0, 0, 0]));
        assert_eq!(fb.get_pixel(19, 19), Some(&[0, 0, 0, 0]));

        // Near the corner arc boundary.
        assert_eq!(fb.get_pixel(1, 1), Some(&[0, 0, 0, 0]));
        assert_eq!(fb.get_pixel(2, 2), Some(&[0, 255, 0, 255]));
    }

    #[test]
    fn rect_layer_radius_clamped_to_half() {
        // radius > min(w,h)/2 should be clamped.
        let r = RectLayer::new(0, 0, 6, 4, [255, 0, 0, 255])
            .with_border_radius(100);
        let mut fb = FrameBuffer::new(6, 4);
        r.render(&mut fb, (0, 0), 1.0);

        // Center pixels should be filled.
        assert_eq!(fb.get_pixel(3, 2), Some(&[255, 0, 0, 255]));
        // Corners clipped.
        assert_eq!(fb.get_pixel(0, 0), Some(&[0, 0, 0, 0]));
    }

    #[test]
    fn rect_layer_with_position_and_radius() {
        let r = RectLayer::new(5, 5, 8, 8, [0, 0, 255, 255])
            .with_border_radius(2);
        let mut fb = FrameBuffer::new(20, 20);
        r.render(&mut fb, (0, 0), 1.0);

        // Inside the rect, center should be blue.
        assert_eq!(fb.get_pixel(9, 9), Some(&[0, 0, 255, 255]));

        // Corner of the rect (5,5) should be transparent.
        assert_eq!(fb.get_pixel(5, 5), Some(&[0, 0, 0, 0]));

        // Outside the rect entirely.
        assert_eq!(fb.get_pixel(0, 0), Some(&[0, 0, 0, 0]));
    }

    #[test]
    fn rect_layer_zero_radius_preserves_all_pixels() {
        // Ensure border_radius=0 (default) doesn't skip any pixels.
        let r = RectLayer::new(2, 2, 6, 6, [255, 255, 0, 255]);
        let mut fb = FrameBuffer::new(10, 10);
        r.render(&mut fb, (0, 0), 1.0);

        for sy in 2..8 {
            for sx in 2..8 {
                assert_eq!(fb.get_pixel(sx, sy), Some(&[255, 255, 0, 255]));
            }
        }
    }

    // ─── ClipLayer tests ───────────────────────────────────────

    #[test]
    fn clip_layer_rect_clips_content() {
        // A 20x10 rect clipped to a 5x5 region at (2,2).
        let inner = RectLayer::new(0, 0, 20, 10, [255, 0, 0, 255]);
        let clip = ClipLayer::new(Box::new(inner))
            .with_region(ClipRegion::Rect(Rect::new(2, 2, 5, 5)));

        let mut fb = FrameBuffer::new(20, 10);
        clip.render(&mut fb, (0, 0), 1.0);

        // Pixels inside the clip rect should be red.
        assert_eq!(fb.get_pixel(2, 2), Some(&[255, 0, 0, 255]));
        assert_eq!(fb.get_pixel(6, 6), Some(&[255, 0, 0, 255]));

        // Pixels outside the clip rect should be transparent.
        assert_eq!(fb.get_pixel(0, 0), Some(&[0, 0, 0, 0]));
        assert_eq!(fb.get_pixel(8, 8), Some(&[0, 0, 0, 0]));
    }

    #[test]
    fn clip_layer_layer_bounds_clips_to_inner() {
        // LayerBounds mode should clip to the inner layer's bounds.
        let inner = RectLayer::new(5, 5, 10, 10, [0, 255, 0, 255]);
        let clip = ClipLayer::new(Box::new(inner))
            .with_region(ClipRegion::LayerBounds);

        let mut fb = FrameBuffer::new(30, 30);
        clip.render(&mut fb, (0, 0), 1.0);

        // Inside inner layer bounds → green.
        assert_eq!(fb.get_pixel(5, 5), Some(&[0, 255, 0, 255]));
        assert_eq!(fb.get_pixel(14, 14), Some(&[0, 255, 0, 255]));

        // Outside inner layer bounds → transparent.
        assert_eq!(fb.get_pixel(0, 0), Some(&[0, 0, 0, 0]));
        assert_eq!(fb.get_pixel(20, 20), Some(&[0, 0, 0, 0]));
    }

    #[test]
    fn clip_layer_zero_size_region_is_noop() {
        let inner = RectLayer::new(0, 0, 10, 10, [255, 0, 0, 255]);
        let clip = ClipLayer::new(Box::new(inner))
            .with_region(ClipRegion::Rect(Rect::new(0, 0, 0, 0)));

        let mut fb = FrameBuffer::new(10, 10);
        clip.render(&mut fb, (0, 0), 1.0);

        // Nothing should be rendered.
        for px in fb.pixels() {
            assert_eq!(*px, [0, 0, 0, 0]);
        }
    }

    #[test]
    fn clip_layer_bounds_returns_intersection() {
        let inner = RectLayer::new(0, 0, 20, 20, [255, 0, 0, 255]);
        let clip = ClipLayer::new(Box::new(inner))
            .with_region(ClipRegion::Rect(Rect::new(5, 5, 10, 10)));

        let bounds = clip.bounds().unwrap();
        assert_eq!(bounds.x, 5);
        assert_eq!(bounds.y, 5);
        assert_eq!(bounds.width, 10);
        assert_eq!(bounds.height, 10);
    }

    #[test]
    fn clip_layer_zero_opacity_is_noop() {
        let inner = RectLayer::new(0, 0, 10, 10, [255, 0, 0, 255]);
        let clip = ClipLayer::new(Box::new(inner))
            .with_region(ClipRegion::Rect(Rect::new(0, 0, 10, 10)));

        let mut fb = FrameBuffer::new(10, 10);
        clip.render(&mut fb, (0, 0), 0.0);

        for px in fb.pixels() {
            assert_eq!(*px, [0, 0, 0, 0]);
        }
    }

    #[test]
    fn clip_layer_with_offset_shifts_clip_region() {
        let inner = RectLayer::new(0, 0, 20, 20, [255, 0, 0, 255]);
        let clip = ClipLayer::new(Box::new(inner))
            .with_region(ClipRegion::Rect(Rect::new(0, 0, 5, 5)));

        let mut fb = FrameBuffer::new(30, 30);
        // Offset (3, 3) should shift the clip region to (3,3)..(8,8).
        clip.render(&mut fb, (3, 3), 1.0);

        assert_eq!(fb.get_pixel(3, 3), Some(&[255, 0, 0, 255]));
        assert_eq!(fb.get_pixel(7, 7), Some(&[255, 0, 0, 255]));
        // Outside the shifted clip → transparent.
        assert_eq!(fb.get_pixel(0, 0), Some(&[0, 0, 0, 0]));
        assert_eq!(fb.get_pixel(10, 10), Some(&[0, 0, 0, 0]));
    }

    #[test]
    fn clip_layer_partial_overlap() {
        // Inner layer is 10x10 at (0,0), clip rect is 8x8 at (5,5).
        // Only the overlapping region (5,5)..(8,8) should be visible.
        let inner = RectLayer::new(0, 0, 10, 10, [0, 0, 255, 255]);
        let clip = ClipLayer::new(Box::new(inner))
            .with_region(ClipRegion::Rect(Rect::new(5, 5, 8, 8)));

        let mut fb = FrameBuffer::new(20, 20);
        clip.render(&mut fb, (0, 0), 1.0);

        // Inside both inner and clip → blue.
        assert_eq!(fb.get_pixel(5, 5), Some(&[0, 0, 255, 255]));
        assert_eq!(fb.get_pixel(7, 7), Some(&[0, 0, 255, 255]));

        // Inside clip but outside inner → transparent.
        assert_eq!(fb.get_pixel(10, 10), Some(&[0, 0, 0, 0]));

        // Inside inner but outside clip → transparent.
        assert_eq!(fb.get_pixel(0, 0), Some(&[0, 0, 0, 0]));
    }
}
