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
    /// When the `font-rasterizer` feature is enabled, this sums
    /// the measured advance widths of each glyph (using the
    /// lazy-loaded font). Without the feature, returns the number
    /// of Unicode scalar values (the placeholder width).
    pub fn text_width(&self) -> u32 {
        #[cfg(feature = "font-rasterizer")]
        {
            let font = self.ensure_font();
            self.text
                .chars()
                .map(|ch| {
                    let glyph_idx = font.lookup_glyph_index(ch);
                    let (metrics, _) = font.rasterize_indexed(glyph_idx, self.font_size);
                    metrics.advance_width as u32
                })
                .sum()
        }
        #[cfg(not(feature = "font-rasterizer"))]
        {
            self.text.chars().count() as u32
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
            let h = self.font_size as u32;
            Some(Rect::new(self.x, self.y, w.max(1), h.max(1)))
        }
        #[cfg(not(feature = "font-rasterizer"))]
        {
            Some(Rect::new(self.x, self.y, self.text_width(), 1))
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

        // Approximate the baseline at ~85% of font size below
        // the top of the line, which is a reasonable heuristic
        // for most Latin monospace fonts.
        let baseline_y = oy.saturating_add((self.font_size * 0.85) as u32) as i32;
        let mut cursor_x = ox as i32;

        for ch in self.text.chars() {
            if ch == '\n' {
                cursor_x = ox as i32;
                continue;
            }
            if ch == ' ' {
                // Space: use the font's actual space advance width,
                // even though the glyph bitmap is empty.
                let glyph_idx = font.lookup_glyph_index(ch);
                let (metrics, _) = font.rasterize_indexed(glyph_idx, self.font_size);
                cursor_x += metrics.advance_width as i32;
                continue;
            }

            let glyph_idx = font.lookup_glyph_index(ch);
            let (metrics, alpha) = font.rasterize_indexed(glyph_idx, self.font_size);

            // Position the glyph bitmap. fontdue's ymin is the
            // y-offset from the baseline to the top of the bitmap
            // in screen coordinates (positive = below baseline,
            // negative = above).
            let glyph_x = cursor_x + metrics.xmin;
            let glyph_y = baseline_y + metrics.ymin;

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
        let w = self.text_width();
        for sx in 0..w {
            let tx = ox + sx;
            let ty = oy;
            if let Some(px) = target.get_pixel_mut(tx, ty) {
                crate::framebuffer::blend_over(px, &self.color, effective);
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
}
