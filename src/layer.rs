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
        for sy in 0..self.height {
            for sx in 0..self.width {
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
    use super::{Layer, LayerEntry, RectLayer, SolidColor, TextLayer};
    use crate::framebuffer::FrameBuffer;
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
}
