## 2.0.0 (Unreleased)

The v2.0.0 milestone completes the ROADMAP with API improvements,
technical debt fixes, and developer experience enhancements.

### Added

- **GradientLayer Builder Pattern** (`src/layer.rs`): refactored the
  verbose `GradientLayer::linear()` (10 args) and `GradientLayer::radial()`
  (9 args) constructors into a fluent builder API.
  - `GradientLayerBuilder` struct with methods: `new_linear()`, `new_radial()`,
    `at()`, `size()`, `colors()`, `linear_points()`, `radial_params()`,
    `with_z()`, `with_name()`, `build()`.
  - Deprecated the old `GradientLayer::linear()` and `GradientLayer::radial()`
    constructors with `#[deprecated(since = "2.0.0")]` for backwards
    compatibility.
  - Added `debug_assert!` to `linear_points()` and `radial_params()` to
    catch misuse in debug builds.
  - `Default` impl returns `new_linear()`.
  - `GradientLayerBuilder` re-exported from the crate root.
  - 6 unit tests: `gradient_builder_api`, `gradient_builder_radial`,
    `gradient_builder_default`, `gradient_builder_zero_length_line`,
    `gradient_builder_linear_points_on_radial_panics`,
    `gradient_builder_radial_params_on_linear_panics`.

- **FontSource Memory Leak Fix** (`src/layer.rs`): eliminated the memory
  leak in `FontSource::Path` by replacing `Box::leak()` with a properly
  owned storage.
  - Added `font_data: OnceLock<Vec<u8>>` field to `TextLayer` to store
    font data for `FontSource::Path`.
  - `ensure_font()` now uses `font_data.get_or_init()` to lazily load
    font data instead of leaking it with `Box::leak()`.
  - Font data is now properly dropped when `TextLayer` is dropped.

- **SceneNode Parent Field Activation** (`src/layer.rs`): activated the
  previously dead-code `SceneNode::parent` field with full parent-child
  traversal support.
  - `parent() -> Option<usize>`: returns the parent node index.
  - `children() -> &[usize]` / `children_mut() -> &mut Vec<usize>`:
    access child node indices.
  - `ancestors(idx) -> Vec<usize>`: returns all ancestor indices from
    parent up to and including the root.
  - `depth(idx) -> usize`: returns the depth from root using an
    iterative loop (no allocation).
  - `descendants(idx) -> Vec<usize>`: returns all descendants in
    pre-order traversal.
  - `move_to(idx, new_parent) -> Result<(), usize>`: reparents a node
    with cycle detection to prevent creating cycles.
  - Removed `#[allow(dead_code)]` from the `parent` field.
  - 5 unit tests: `scene_graph_parent_child_traversal`,
    `scene_graph_ancestors_and_depth`, `scene_graph_descendants`,
    `scene_graph_move_to`, `scene_graph_move_to_cycle_detected`.

## 1.0.0 (2026-07-18)

The v1.0.0 milestone release completes the ROADMAP with accessibility
metadata for screen readers and a new FrameBuffer helper.

### Added

- **Accessibility Metadata** (`src/layer.rs`): attach alt-text and
  semantic roles to layers for screen readers and headless terminals.
  - `SemanticRole` enum: `None`, `Text`, `Button`, `Image`,
    `Container`, `Separator`, `Status`, `Navigation`,
    `Custom(&'static str)`.
  - `AccessibilityMetadata` struct: `alt_text: Option<String>`
    and `role: SemanticRole` fields with builder pattern.
  - `LayerEntry` gains `accessibility()`, `accessibility_mut()`,
    `set_accessibility()`, and `with_accessibility()` methods.
  - `AccessibilityMetadata` and `SemanticRole` re-exported from
    the crate root.
  - 9 unit tests covering defaults, builder, setters, and
    LayerEntry integration.

- **`FrameBuffer::fill_rect(rx, ry, rw, rh, color)`**
  (`src/framebuffer.rs`): fills a rectangular region with a
  solid RGBA colour. Silently clips to framebuffer bounds;
  coordinates outside the buffer are ignored. Uses direct
  slice writes for performance. 4 unit tests.

## [Unreleased]

### Added

- **Animation test improvements** (`src/animation.rs`, `tests/animation.rs`): 40 new tests (18 edge-case unit tests, 22 integration tests) covering dirty regions, all layer types, Kitty/Sixel pipelines, and CI flakiness fixes.

- **Diff-Based Rendering** (`src/compositor.rs`): track rectangular dirty regions to skip re-compositing unchanged areas.
  - `DirtyRect` struct: tracks rectangular dirty areas with `x`, `y`, `width`, `height`.
  - `DirtyRegion` tracker: accumulates dirty rects via `mark_rect()`, `mark_full()`, `mark_point()`; resets via `take_regions()`.
  - `LayerStack::render_diff(target, dirty)`: renders to a temporary buffer and copies only dirty regions to the target framebuffer.
  - 8 unit tests covering dirty region tracking and diff rendering.
  - Integrated into the animation loop: `AnimContext` now has a `dirty: DirtyRegion` field, `mark_full()` and `mark_rect()` methods, and `request_redraw()` automatically marks the entire framebuffer as dirty.

- **`LayerStack::find_by_name()` / `find_by_name_mut()`** (`src/compositor.rs`): look up layers by name for convenient runtime modification.
  - `find_by_name(name) -> Option<&LayerEntry>`: returns a reference to the first entry whose name matches.
  - `find_by_name_mut(name) -> Option<&mut LayerEntry>`: mutable variant.
  - 4 unit tests covering lookup, modification, and duplicate-name handling.

## 0.15.0 (2026-07-18)

The third development release adds three visual features to the layer system:
layer clipping/masking, rounded corners on rectangles, and shadow/glow effects.

### Added

- **ClipLayer / ClipRegion**: wrapper layer that clips an inner layer to a
  rectangular region. Supports explicit `ClipRegion::Rect` (user-specified
  rectangle) and `ClipRegion::LayerBounds` (clips to the inner layer's own
  bounds). Rendered via a full-size temporary buffer (same approach as
  `DropShadow`), then only the clipped region is composited.
  `ClipLayer` and `ClipRegion` re-exported from the crate root.
  7 unit tests.

- **Rounded corners for `RectLayer`**: new `border_radius: u32` field and
  `with_border_radius(radius)` builder method. When `radius > 0`, the four
  corners of the rectangle are clipped to circular arcs. The effective radius
  is clamped to `min(width, height) / 2`. 6 unit tests.

- **Shadow / glow enhancements to `DropShadow`**: new `spread: i32` field
  and `with_spread(spread)` builder for dilating (positive) or eroding
  (negative) the shadow shape before blurring. New `with_glow(color, blur)`
  convenience builder that sets a bright shadow colour with zero offset.
  New `ShadowLayer` type alias for discoverability.
  6 new unit tests (spread + glow).

## 0.14.0 (2026-07-17)

Animation loop and layer transforms. This release adds a built-in
frame loop with delta-time tracking and terminal resize handling,
as well as per-layer rotation and scaling via a new `Transform`
struct. Both features are designed for real-time animated dashboards
and visual effects.

### Added

- **Animation Loop** (`src/animation.rs`): built-in frame loop with
  delta-time tracking, frame scheduling, and terminal resize
  handling.
  - `AnimContext`: per-frame context providing access to the layer
    stack, delta time, elapsed time, frame count, and terminal
    dimensions.
  - `AnimConfig`: configuration struct with target FPS, protocol
    selection, and clear-between-frames toggle.
  - `run(fps, callback)`: entry point with empty layer stack.
  - `run_with_stack(stack, fps, callback)`: entry point with
    initial layers.
  - `run_with_config(stack, config, callback)`: full-control entry
    point.
  - `request_redraw()`: opt-in rendering — frames without a redraw
    request skip encoding, saving CPU.
  - `exit()`: graceful shutdown after the current frame.
  - Terminal size is re-detected each frame; the framebuffer is
    resized automatically.
  - Protocol is resolved once at startup (Auto detection cached).
  - 11 unit tests covering context fields, config builders, frame
    timing, exit semantics, and panic on invalid FPS.
  - CLI flags: `--animate` and `--fps <N>` on the `main.rs` demo.

- **Layer Transforms** (`src/geometry.rs`, `src/layer.rs`,
  `src/compositor.rs`): per-layer rotation and scaling via affine
  2D transforms.
  - `Transform` struct with rotation (degrees, clockwise), scale
    (x, y), and anchor point (layer-local coordinates).
  - Builder pattern: `Transform::new().with_rotation(45.0)
    .with_scale(1.5, 1.5).with_anchor(50.0, 50.0)`.
  - `apply(x, y)` — forward mapping (layer-local → target).
  - `apply_inverse(x, y)` — inverse mapping (target → layer-local)
    for bilinear interpolation during rendering.
  - `LayerEntry` gains `transform`, `transform_mut()`,
    `set_transform()`, and `with_transform()` builder.
  - `CpuCompositor::compose` applies transforms via inverse mapping
    with bilinear interpolation when a non-identity transform is
    present.
  - Bounding-box optimization: the compositor transforms the four
    corners of the source region and only iterates over the
    resulting axis-aligned bounding box on the target.
  - `Transform` re-exported at crate root alongside existing `Rect`.
  - 8 unit tests + 6 proptest property tests for `Transform`
    (identity, builder, apply, inverse roundtrip, anchor).
  - 5 compositor-level tests for transform rendering (rotation,
    scale, identity equivalence, opacity correctness, position
    change).

- **GradientLayer**: linear and radial gradient support with sRGB
  interpolation
  - `GradientKind::Linear` with start/end coordinates
  - `GradientKind::Radial` with center and radius
  - Builder pattern with `with_z()` and `with_name()`
- **BorderLayer**: rectangular border (stroke only) with configurable
  border width
  - Draws the outline of a rectangle without filling the interior
  - `border_width` controls thickness, clamped to half the smallest
    dimension
  - Supports opacity and offset translation
- **CanvasLayer**: freeform drawing canvas with pixel-level control
  - `draw_pixel()`, `draw_line()` (Bresenham), `draw_circle()`
    (midpoint)
  - `fill_rect()` and `clear()` helpers
  - Skips transparent pixels during render for efficiency
- **DropShadow**: wrapper layer that adds a blurred shadow behind any
  inner layer
  - Configurable offset, blur radius, and shadow colour
  - Uses two-pass box blur (horizontal + vertical)
  - Renders shadow first, then original layer on top
- **SceneGraph**: parent-child tree with grouped transforms
  - Cascading visibility (parent hidden = all children hidden)
  - Cascading opacity (parent opacity × child opacity)
  - Cascading offset (parent offset + child offset)
  - `add_group()`, `add_group_to()`, `add_child()`,
    `add_child_to()`
  - `set_visible()`, `set_opacity()`, `set_offset()` runtime
    control
- **6 proptest property-based tests** for CanvasLayer drawing
  primitives
- **35 integration tests** in tests/pipeline.rs covering all layer
  types with both Kitty and Sixel protocols
- **13 unit tests** for SceneGraph cascading transforms
- **10 unit tests** for DropShadow rendering and box blur
- **46 unit tests** for GradientLayer, BorderLayer, CanvasLayer

### Fixed

- Consolidated 6 flaky `TmuxPassthroughGuard` env var tests into a
  single sequential test (`guard_all_env_var_scenarios`) to eliminate
  parallel execution race conditions
- Fixed `#[cfg(feature = "kitty-encoder")]` gate on two encoder
  tests that referenced `super::kitty::encode_to_writer` without
  the feature flag, which caused compilation failures when running
  the full test suite

## 0.12.0 (2026-07-17)

Project rename and breaking env var change. The crate has been
renamed from `dashcompositor` to `termcompositor` and the tmux
passthrough env var has been renamed from `DASHPASSTHROUGH` to
`TMUXPASSTHROUGH`.

### Changed (breaking)
- **Crate rename**: `dashcompositor` → `termcompositor`. All
  `use dashcompositor::*` imports must be updated to
  `use termcompositor::*`. The `[[bin]]` name changed from
  `dashcompositor` to `termcompositor`.
- **Env var rename**: `DASHPASSTHROUGH` → `TMUXPASSTHROUGH`.
  Users who set `DASHPASSTHROUGH=1` in their shell rc must
  update to `TMUXPASSTHROUGH=1`. The `--tmux-passthrough` CLI
  flag is unchanged.

### Added
- 231 unit and integration tests (coverage 89% → 97%).
- Property-based tests (`proptest`) for `FrameBuffer`,
  `blend_over`, and `Rect`.
- `TerminalSize::detect_with_size` closure-based API for
  testable terminal detection.
- `FontSource::Bytes` variant for loading fonts from static
  byte slices.
- `ImageLayer::from_path` tests using `tempfile`.
- Sixel streaming multiband iteration test (4×10 frame).

### Fixed
- Compiler warnings removed (unused imports, useless
  comparisons, unused `mut`).
- `cov.txt` and `proptest-regressions/` added to `.gitignore`.

### Documentation
- `ARCHITECTURE.md`: streaming encode memory model, per-protocol
  walkthrough, error handling section.
- `DOCS.md`: encode path selection guide, troubleshooting section.
- `README.md`: slimmed from ~400 to ~40 lines.

## 0.11.0 (2026-07-02)

Pre-1.0 infrastructure: MSRV policy, benchmarks, CONTRIBUTING.md.
No API changes; all additions are developer-facing.

### Added
- `rust-version = "1.73"` in `Cargo.toml`: the MSRV is now
  pinned and checked in CI. The minimum is driven by `div_ceil`
  (stabilised in 1.73) used in the chunked Kitty encoder and
  the streaming Sixel band loop.
- `benches/compositor.rs`: 19 Criterion benchmarks covering the
  compositor core (framebuffer allocation/clear/blend/get_pixel,
  solid colour / rect / text layer rendering, multi-layer stacks,
  and feature-gated Kitty/Sixel encoder paths). Run with
  `cargo bench` (or `cargo bench --all-features` for the full
  suite).
- `CONTRIBUTING.md`: project contributing guide covering the
  build/test workflow, feature matrix, commit message format,
  code style, PR process, MSRV policy, and dependency addition
  guidelines.
- CI job `msrv`: verifies `cargo check` succeeds on Rust 1.73
  (the pinned MSRV). Runs in parallel with the existing `fmt`
  and `validate` jobs.
- `criterion = "0.5"` dev-dependency for the benchmark harness.

### Changed
- `Cargo.toml` version bumped from 0.10.0 to 0.11.0.
- `README.md` "Contributing" section updated to point to
  `CONTRIBUTING.md`; MSRV badge added.
- `src/main.rs` version banner updated to v0.11.0.

### Notes
- No API changes; no new runtime dependencies.
- All 7 feature combos clean: fmt, build, test, clippy -D
  warnings, MSRV check.
- This is the third-last pre-1.0 minor. The next release
  (v0.12.0) will address remaining 1.0 checklist items;
  the release after that is v1.0.0.

## 0.10.0 (2026-07-02)

API stabilization release: the public API surface is frozen for
1.0. All pub items have documentation; no breaking changes are
anticipated before 1.0. Publishing is enabled (`publish = true`).

### Changed
- `publish = true`: the crate is now ready for crates.io
  publication. The `publish = false` guard from the v0.1.0
  scaffold has been removed.
- Crate root `//!` doc rewritten with a quick-start example,
  feature-flag table, and module index.

### Notes
- No API changes, no new features, no new dependencies.
- All 6 feature combos clean: fmt, build, test, clippy -D
  warnings.
- This is the last pre-1.0 minor. The next release will be
  v1.0.0.

## 0.9.0 (2026-07-02)

Real font rasterization for `TextLayer`: replaces the solid-block
placeholder with actual glyph rendering via the `fontdue` crate
(optional `font-rasterizer` Cargo feature). When enabled, text
is rendered using a bundled Fira Mono Regular font (~174KB, SIL
OFL licensed) with per-pixel alpha blending, measured advance
widths, and pixel-accurate bounding boxes. The feature is
optional (default off) and the solid-block placeholder is
preserved when disabled, maintaining backwards compatibility.

This is the first v0.9.x release and the most impactful step
toward 1.0 — it turns `TextLayer` from a coloured-block layout
probe into a real text renderer.

### Added
- `fontdue = "0.9"` as an optional dependency (MIT/Apache-2.0/Zlib
  licensed), gated behind the new `font-rasterizer` Cargo feature.
- `termcompositor::FontSource` enum (gated on `font-rasterizer`):
  `Bundled` (default), `Path(PathBuf)`, or `Bytes(&'static [u8])`.
  The bundled font is Fira Mono Regular, embedded at compile time
  via `include_bytes!`.
- `TextLayer::with_font(FontSource, f32)` builder:
  sets a custom font source and pixel size (e.g.
  `.with_font(FontSource::Bundled, 18.0)`).
- `TextLayer::with_font_size(f32)` builder:
  sets only the pixel size, keeping the current font source.
- `TextLayer::font_size() -> f32` accessor (returns the current
  font size, default 14.0).
- Bundled font asset: `assets/FiraMono-Regular.ttf` (~174KB).

### Changed
- `TextLayer` no longer derives `Clone`, `PartialEq`, or `Eq`.
  Only `Debug` is derived (consistent with `ImageLayer`).
- `TextLayer::text_width()` when `font-rasterizer` is enabled
  now returns the sum of measured glyph advance widths in pixels
  (instead of the Unicode scalar value count). The char-count
  fallback is preserved when the feature is disabled.
- `TextLayer::bounds()` when `font-rasterizer` is enabled now
  returns `(x, y, text_width, font_size)` for pixel-accurate
  bounding. Without the feature, returns `(x, y, char_count, 1)`
  as before.
- `TextLayer::render()` when `font-rasterizer` is enabled now
  composites real glyph bitmaps from fontdue into the framebuffer
  with per-pixel alpha blending. The solid-block placeholder is
  preserved as the fallback render path.

### Backwards compatibility
- `TextLayer::new(x, y, text, color)` is unchanged; uses the
  bundled font at 14px when the feature is enabled, or the
  placeholder when disabled.
- `render_glyph() -> &str` is unchanged.
- `with_z` and `with_name` builders are unchanged.
- Default features remain empty (`font-rasterizer` is opt-in).

### Tests
- 5 new font-rasterizer tests (gated on `font-rasterizer`):
  `text_layer_new_defaults_with_font` (font_size == 14.0,
  text_width > 0); `text_layer_bounds_with_font_uses_font_size`
  (bounds height == font_size);
  `text_layer_font_source_defaults_to_bundled` (bundled font
  loads and produces positive width);
  `text_layer_render_produces_non_empty_bitmap` (letter 'A'
  renders at least some non-transparent pixels);
  `text_layer_with_font_size_changes_width` (larger font size
  produces >= advance width).
- 4 existing placeholder tests preserved (gated on
  `not(feature = "font-rasterizer")`).
- `end_to_end_rect_and_text_layers` in lib.rs updated to work
  with both feature configurations.

### Notes
- All feature combinations clean: cargo fmt, cargo build
  (default + each feature + both + font-rasterizer alone +
  font-rasterizer + both encoders), cargo test, cargo clippy
  --all-targets -- -D warnings.
- Binary size impact: ~174KB increase from the bundled font
  (only when `font-rasterizer` is enabled).
- The `FontSource::Path` variant reads the font file on first
  render (not on construction), so a missing file panics at
  render time.
- Fira Mono is SIL OFL licensed (see `assets/OFL.txt` or
  https://github.com/mozilla/Fira for the full license text).The font-rasterizer work follows the evaluation: `fontdue` was chosen over `ab_glyph` (requires PBF font format conversion) and `cosmic-text` (heavier shaping pipeline).

## 0.8.6 (2026-07-02)

End-to-end O(1) streaming dispatch: adds a new
public `dispatch_to_writer<W: Write>(protocol, frame,
&mut W) -> Result<(), EncoderError>` entry point that
mirrors the private `dispatch()` function but writes
to a `&mut impl Write` sink instead of returning a
`Vec<u8>`. This combines the v0.8.2 Kitty streaming,
v0.8.3 tmux passthrough wrap, v0.8.4 Sixel streaming,
and v0.8.5 fixed-palette Sixel streaming work into a
single end-to-end streaming dispatch.

### Added
- `pub fn dispatch_to_writer<W: Write>(protocol: Protocol,
  frame: &FrameBuffer, out: &mut W) -> Result<()>` in
  `dashcompositor::encoder`. Re-exported at the crate
  root as `dashcompositor::dispatch_to_writer`.
- For `Protocol::Kitty`: delegates to
  `kitty::encode_passthrough_to_writer` (which handles
  the optional tmux passthrough wrap when the
  `TMUXPASSTHROUGH` opt-in is set).
- For `Protocol::Sixel`: delegates to
  `sixel::encode_to_writer` (the v0.8.4 streaming
  entry point).
- For `Protocol::Auto`: recurses via
  `dispatch_to_writer(detect(), frame, out)`.

### Tests
- 6 new unit tests:
  `dispatch_to_writer_kitty_matches_dispatch` (Kitty
  arm matches the Vec<u8> dispatch byte-for-byte);
  `dispatch_to_writer_sixel_matches_dispatch` (Sixel
  arm matches the Vec<u8> dispatch byte-for-byte);
  `dispatch_to_writer_auto_resolves_via_detect` (Auto
  arm recurses correctly);
  `dispatch_to_writer_2mp_frame_smoke_test` (2MP
  framebuffer through the Kitty arm);
  `dispatch_to_writer_kitty_unsupported_without_feature`
  (disabled-feature arm returns UnsupportedProtocol);
  `dispatch_to_writer_sixel_unsupported_without_feature`
  (disabled-feature arm returns UnsupportedProtocol).

### Notes
- All 4 feature combinations clean: cargo fmt, cargo
  build (default + each feature + both), cargo build
  --release (default + both), cargo test (127 tests
  with both features, 0 failed), cargo clippy
  --all-targets -- -D warnings (0 errors across all 4
  combos).
- No public API removals; no new runtime dependencies;
  no new Cargo features.
## 0.8.5 (2026-07-02)

Fully memory-bounded streaming Sixel encode: the v0.8.4
streaming Sixel path still materialised the full
framebuffer in a `Vec<u8>` (8MB+ for a 2MP image)
because `icy_sixel::SixelImage::from_rgba(Vec<u8>, w, h)`
takes owned bytes and has no streaming input API.
v0.8.5 adds a new public streaming entry point
`dashcompositor::encoder::sixel::encode_to_writer_streaming<W: Write>(
frame: &FrameBuffer, out: &mut W) -> Result<(), EncoderError>`
that uses a fixed xterm-256 palette (16 basic + 6x6x6
RGB cube + 24 grayscale) and emits band-by-band in a
single DCS sequence. Peak working set is O(1) per
write call (~4KB scratch for the LUT and per-chunk
state), independent of framebuffer size. This brings
the Sixel arm to full O(1) memory parity with the
Kitty arm's v0.8.2 streaming entry point.

### Added
- `pub fn sixel::encode_to_writer_streaming<W: Write>(
  frame, &mut W) -> Result<(), EncoderError>` in
  `dashcompositor::encoder`, gated on the
  `sixel-encoder` Cargo feature. Writes the Sixel DCS
  bytes to any `std::io::Write` impl (e.g. `Vec<u8>`,
  `std::fs::File`, `std::net::TcpStream`).
  Re-exported at the encoder module level as
  `dashcompositor::encoder::encode_to_writer_streaming`
  (NOT at the crate root, to avoid the ambiguity of
  a single `encode_to_writer_streaming` name resolving
  to different encoders depending on the feature set).

### Implementation details
- **Fixed xterm-256 palette**: 16 basic colors + 6x6x6
  RGB cube (values [0, 95, 135, 175, 215, 255]) + 24
  grayscale ramp (values [8, 18, 28, ..., 238]).
  Generated at compile time via a `const fn` with
  `while` loops (stable since Rust 1.61).
- **5-bit LUT**: 32x32x32 = 32K entry lookup table
  maps `(r5, g5, b5)` to the nearest palette index
  in O(1). Built lazily on first call via
  `std::sync::OnceLock` (stable since Rust 1.70).
  One-time cost: ~8M operations at startup (~100ms
  on modern hardware).
- **Band-by-band emission**: each band is 6 rows tall
  (one sixel character per column). Bands separated
  by `-` (carriage return + newline). All 256 palette
  colors defined once in the DCS header
  (`#Pc;2;R;G;B` with 0-100 RGB scale).
- **Per-column color tracking**: for each 6-row
  column, the most common palette index is selected
  (ties broken by lower index for determinism).
  Sixel bits mark only the pixels that match.
- **RLE**: `! <n> <ch>` repeat introducer for runs
  >= 4. Sixel character mapping: value 0-63 ->
  `?` (63) to `~` (126) (printable ASCII range).

### Trade-off vs. `encode_to_writer`
The fixed xterm-256 palette means lower image quality
for photos (no adaptive quantization per image)
compared to `icy_sixel`'s adaptive quantiser. For
UI/dashboards (the dashcompositor primary use case)
the quality loss is minimal. The `icy_sixel`-based
`encode_to_writer` is preserved as the high-quality,
O(N)-memory path for small framebuffers;
`encode_to_writer_streaming` is the O(1)-memory path
for multi-megapixel framebuffers.

### Tests
- 5 new unit tests:
  `sixel_encode_to_writer_streaming_basic_structure`
  (DCS header, palette, terminator);
  `sixel_encode_to_writer_streaming_uses_rle_for_solid_color`
  (RLE repeat introducer for uniform colors);
  `sixel_encode_to_writer_streaming_uses_band_separators`
  (`-` between bands for height > 6);
  `sixel_encode_to_writer_streaming_rejects_zero_dimensions`
  (zero-dimensions error path);
  `sixel_encode_to_writer_streaming_2mp_frame_smoke_test`
  (1920x1080 framebuffer, 179 band separators).

### Notes
- All 4 feature combinations clean: cargo fmt, cargo
  build (default + each feature + both), cargo build
  --release (default + both), cargo test (121 tests
  with both features, 0 failed), cargo clippy
  --all-targets -- -D warnings (0 errors across all 4
  combos).
- No public API removals; no new runtime dependencies;
  no new Cargo features.
- Coexists with the v0.8.4 `icy_sixel`-based
  `sixel::encode_to_writer` (which is preserved as
  the high-quality, O(N)-memory path). Callers that
  need O(1) memory use `encode_to_writer_streaming`;
  callers that need the best image quality use
  `encode_to_writer`.
## 0.8.4 (2026-07-02)

Streaming Sixel encode: the v0.6.0 Sixel encoder
materialised the Sixel output in a `Vec<u8>` via
`sixel_string.into_bytes()`, which added one full-frame
allocation on top of the unavoidable RGBA input
allocation. v0.8.4 adds a new public streaming entry
point that writes the Sixel DCS bytes directly to a
caller-supplied `&mut impl Write` sink, eliminating
the intermediate Sixel-output `Vec<u8>` allocation.

### Added
- `pub fn sixel::encode_to_writer<W: Write>(frame, &mut W)
  -> Result<(), EncoderError>` in
  `dashcompositor::encoder`, gated on the
  `sixel-encoder` Cargo feature. Writes the Sixel DCS
  bytes to any `std::io::Write` impl (e.g. `Vec<u8>`,
  `std::fs::File`, `std::net::TcpStream`).
  Re-exported at the encoder module level as
  `dashcompositor::encoder::encode_to_writer`
  (NOT at the crate root, to avoid the ambiguity of a
  single `encode_to_writer` name resolving to different
  encoders depending on the feature set; this mirrors
  the kitty `encode_to_writer` access pattern at
  `dashcompositor::encoder::kitty::encode_to_writer`).

### Changed
- `sixel::encode(frame) -> Result<Vec<u8>, EncoderError>`
  is now a thin wrapper that allocates a fresh `Vec<u8>`
  and delegates to `encode_to_writer`. The wire format
  is unchanged (byte-for-byte equivalent to v0.6.0/
  v0.8.0/v0.8.3 for the same input). The intermediate
  Sixel-output `Vec<u8>` that the v0.8.0 path allocated
  via `sixel_string.into_bytes()` is eliminated in the
  streaming path (the `String`'s internal buffer is
  borrowed, not copied into a new `Vec`).

### Tests
- 4 new unit tests:
  `sixel_encode_to_writer_small_frame_matches_encode`
  (streaming output matches `encode` byte-for-byte for
  a 1x1 frame);
  `sixel_encode_to_writer_writes_to_pre_allocated_vec`
  (the `<W: Write>` generic surface accepts a
  pre-allocated `Vec`);
  `sixel_encode_to_writer_rejects_zero_dimensions`
  (zero-dimensions error path is identical to the
  `encode` path);
  `sixel_encode_to_writer_2mp_frame_smoke_test` (1920x1080
  framebuffer encodes correctly through the streaming
  path).

### Notes
- **Memory profile caveat**: the input RGBA `Vec<u8>`
  (`pixels.iter().flatten().copied().collect()`, 8MB+ for
  a 2MP frame) is still materialised by the streaming
  path, because `icy_sixel` 0.5 takes owned RGBA bytes
  via `SixelImage::from_rgba(Vec<u8>, w, h)` and has no
  streaming input API (verified by reading the local
  crate source: the only public encode methods are
  `encode(&self) -> Result<String>` and
  `encode_with(&self, opts) -> Result<String>`, neither
  of which takes a `Write` sink). The v0.8.4 streaming
  entry point therefore saves one full-frame allocation
  (the Sixel output) but not the input RGBA allocation.
  The Kitty arm's `kitty::encode_to_writer` (v0.8.2)
  avoids both allocations because `little_kitty`'s
  `KittyCommandWriter` is per-chunk-incremental. A
  future v0.9.x could address the Sixel input
  allocation by either (a) waiting for `icy_sixel` to
  add a streaming input API, (b) swapping to a different
  Sixel crate that supports streaming, or (c) writing
  our own streaming Sixel quantiser/serialiser.
- All 4 feature combinations clean: cargo fmt, cargo
  build (default + each feature + both), cargo build
  --release (default + both), cargo test (117 tests
  with both features, 0 failed), cargo clippy
  --all-targets -- -D warnings (0 errors across all 4
  combos).
- No public API removals; no new runtime dependencies;
  no new Cargo features.
## 0.8.3 (2026-07-02)

End-to-end O(1) streaming for the tmux passthrough wrap.
v0.8.0's `wrap_for_tmux(Vec<u8>) -> Vec<u8>` materialised
the full wrapped output in a `Vec<u8>`, which (combined
with `dispatch(Protocol::Kitty, frame)`) made the entire
encode + wrap + emit pipeline O(N) in framebuffer size
(8MB+ for a 2MP encode, plus ~11MB for the wrap). v0.8.3
adds three new public APIs that make the entire pipeline
O(1) per write call:

### Added
- `pub fn kitty::wrap_for_tmux_to_writer<W: Write>(
  inner: &[u8], out: &mut W) -> io::Result<()>` in
  `dashcompositor::encoder`. The streaming version of
  `wrap_for_tmux`: takes the raw Kitty APC bytes as a
  slice and writes the wrapped DCS bytes directly to a
  `&mut impl Write` sink. Memory bounded: O(1) (no
  intermediate `Vec` allocation).
- `pub struct kitty::PassthroughWriter<W: Write>` in
  `dashcompositor::encoder`. A `Write` adapter that
  wraps the inner output in a tmux passthrough DCS:
  writes the DCS prefix on the first byte, doubles
  every `ESC` byte in subsequent body writes, and
  writes the DCS terminator on `finish()`. The
  v0.8.3 building block for end-to-end O(1) streaming.
- `pub fn kitty::encode_passthrough_to_writer<W: Write>(
  frame: &FrameBuffer, out: &mut W) -> Result<()>` in
  `dashcompositor::encoder`. The end-to-end O(1) entry
  point: encodes the frame and (if the tmux passthrough
  opt-in is set) wraps the output in a tmux passthrough
  DCS, all in a single pass with O(1) memory. When the
  opt-in is not set, this is equivalent to
  `encode_to_writer`.

### Changed
- `kitty::wrap_for_tmux(Vec<u8>) -> Vec<u8>` is now a
  thin convenience wrapper that delegates to
  `wrap_for_tmux_to_writer` writing to a fresh
  `Vec<u8>`. The wire format is unchanged (byte-for-byte
  equivalent to v0.8.0/v0.8.2 for the same input).

### Tests
- 7 new unit tests: `wrap_for_tmux_to_writer` matches
  `wrap_for_tmux` for various inputs; ESC doubling in
  the body; empty input edge case;
  `PassthroughWriter` prefix/doubling/suffix semantics;
  `encode_passthrough_to_writer` with and without the
  tmux passthrough opt-in.

All 4 feature combinations clean: cargo fmt, cargo build
(default + each feature + both), cargo build --release
(default + both), cargo test (113 tests with both
features, 0 failed), cargo clippy --all-targets
-- -D warnings (0 errors across all 4 combos).
## 0.8.2 (2026-07-02)

Memory-bounded streaming Kitty encode: the v0.8.1 chunked
encoder previously materialised the entire framebuffer in
a `Vec<u8>` (8MB+ for a 2MP image) before chunking.
v0.8.2 adds a new public streaming entry point
`dashcompositor::encoder::kitty::encode_to_writer<W: Write>(
frame: &FrameBuffer, out: &mut W) -> Result<(), EncoderError>`
that writes the encoded APC bytes directly to a
caller-supplied `&mut impl Write` sink. Peak working set
is now O(1) per chunk (~4KB scratch), independent of
framebuffer size. The existing `encode -> Vec<u8>` entry
point is preserved for backwards compat and now also runs
through the streaming path internally (it just passes
`&mut Vec::new()` to `encode_to_writer`).

### Added
- `pub fn kitty::encode_to_writer<W: Write>(frame, &mut W)
  -> Result<(), EncoderError>` in `dashcompositor::encoder`,
  gated on the `kitty-encoder` Cargo feature. Writes the
  encoded APC bytes to any `std::io::Write` impl (e.g.
  `Vec<u8>`, `std::fs::File`, `std::net::TcpStream`).

### Changed
- `kitty::encode(frame) -> Result<Vec<u8>, EncoderError>`
  is now a thin wrapper that allocates a fresh `Vec<u8>`
  and delegates to `encode_to_writer`. The wire format
  is unchanged (byte-for-byte equivalent to v0.8.1 for
  the same input). The memory profile is improved:
  the v0.8.1 full-framebuffer RGBA `Vec<u8>` is gone;
  the only per-call allocations are one scratch `Vec<u8>`
  per chunk (≤ 3072 raw bytes) plus the per-chunk APC
  scratch (≈ 4KB), both bounded independent of
  framebuffer size.

### Tests
- 5 new unit tests: streaming output matches `encode`
  byte-for-byte; single-chunk fast path produces no
  `m` key; multi-chunk path produces the correct
  `m=1`/`m=0` distribution; pre-allocated `Vec<u8>`
  writer grows as needed; 2MP (1920×1080) framebuffer
  smoke test produces the expected 2,701 chunks.

All 4 feature combinations clean: cargo fmt, cargo build
(default + each feature + both), cargo build --release
(default + both), cargo test (99 tests with both
features, 0 failed), cargo clippy --all-targets
-- -D warnings (0 errors across all 4 combos).
## 0.8.1 (2026-07-02)

Chunked Kitty encoding: for framebuffers whose base64-encoded
payload exceeds the Kitty graphics protocol's 4096-byte
per-chunk limit, the Kitty encoder now splits the payload
into multiple APC commands using the protocol's `m=1` /
`m=0` chunking mechanism. This lifts the previous implicit
size limit (a single Kitty command) and supports
multi-megapixel framebuffers without hitting terminal
buffer limits. The implementation is fully backwards
compatible: small framebuffers (those that fit in a single
4096-byte base64 chunk = 768 RGBA pixels) continue to use
the v0.8.0 single-command wire format byte-for-byte.

### Added
- v0.8.1 chunked encoding logic in `kitty::encode` (gated
  on `kitty-encoder`). The encoder now:
  1. Computes `total_pixels` from the framebuffer's
     RGBA byte count.
  2. If `total_pixels <= 768` (the v0.8.1
     `PIXELS_PER_CHUNK` constant), emits the v0.8.0
     single-command format `\x1b_Ga=T,f=32,q=2,s=W,v=H;<base64>\x1b\\`
     (no `m` key) via a new private
     `encode_single_chunk` helper. This preserves
     byte-for-byte compatibility with v0.8.0 for
     small images.
  3. If `total_pixels > 768`, enters the multi-chunk
     path: splits the payload into `768`-pixel
     chunks and emits one APC per chunk. The first
     chunk carries the full control list
     (`a`, `f`, `q`, `s`, `v`) plus `m=1`; intermediate
     chunks carry ONLY `m=1` (the terminal remembers
     the metadata from the first chunk per the spec);
     the last chunk carries `m=0`.
- `const CHUNK_PAYLOAD_BYTES: usize = 4096` and
  `const PIXELS_PER_CHUNK: usize = 768` on the
  `kitty` module. The 4096 limit is the Kitty
  graphics protocol spec's hard per-chunk limit
  (<https://sw.kovidgoyal.net/kitty/graphics-protocol/>).
  The 768-pixel value is derived: 4096 base64 chars
  decode to 3072 raw bytes = exactly 768 4-byte
  RGBA pixels, so the base64 encoding of a full
  intermediate chunk is exactly 4096 chars with no
  padding (guaranteeing the spec's requirement that
  non-last chunks have a payload length that is a
  multiple of 4 base64 chars).
- Private `build_apc_command(controls, payload)`
  helper in the `kitty` module. Extracted from the
  v0.8.0 single-command code path so both the
  single-chunk fast path and the multi-chunk path
  share the same per-chunk APC construction
  (`write_start` + controls + `;` + `write_base64` +
  `write_end`). This keeps the chunking code DRY
  and makes the per-chunk format easy to verify in
  tests.
- 6 new unit tests in `src/encoder.rs` (gated on
  `kitty-encoder`), all using `with_env(None, None,
  None, None, ...)` for env-mutex serialization:
  - `kitty_encode_single_chunk_produces_no_m_key`:
     small frame (2x2 = 4 pixels) must use the
     v0.8.0 wire format (no `m` key, all v0.8.0
     controls present). Locks in the v0.8.0
     backwards-compat contract.
  - `kitty_encode_exactly_768_pixels_is_single_chunk`:
     boundary case -- 768 pixels exactly still
     uses the single-chunk fast path (the condition
     is `<=`, not `<`).
  - `kitty_encode_two_chunks_has_m1_m0`:
     769 pixels -> exactly 2 chunks. First chunk
     has full controls + `m=1`; second chunk has
     ONLY `m=0` (no full control list).
  - `kitty_encode_three_chunks_boundary`:
     1537 pixels -> exactly 3 chunks. Verifies
     that the middle chunk carries ONLY `m=1`
     (not the full control list) and the last
     chunk carries `m=0`.
  - `kitty_encode_intermediate_chunks_base64_aligned`:
     2304 pixels (3 * 768, all chunks of equal
     size) -> every chunk's base64 payload must
     be a multiple of 4 chars (the spec's hard
     requirement for non-last chunks). This
     would catch a regression in the
     `PIXELS_PER_CHUNK` constant.
  - `kitty_encode_chunked_is_deterministic`:
     encoding the same frame twice produces
     byte-identical output (the encode path is
     pure, no hidden state).

### Changed
- `Cargo.toml` version bumped from 0.8.0 to 0.8.1.
- The v0.5.0 TODO comment in `kitty::encode`
  ("chunk large images (m=0 more-chunks / m=1
  last chunk)") has been removed -- the chunking
  is now implemented in v0.8.1.
- `kitty::encode` is now slightly more complex
  (multi-chunk loop) but the per-chunk APC
  construction is shared via `build_apc_command`,
  so the net code addition is modest (~50 lines
  including the chunking loop and the 6 new tests).

### Notes
- The v0.8.0 single-chunk wire format is preserved
  byte-for-byte for framebuffers that fit in one
  chunk. Terminals that pre-date the chunking
  extension (or that have it disabled) keep
  working unchanged for small images. The
  chunked path is only used for framebuffers
  larger than 768 RGBA pixels (~12 KB raw
  payload, ~16 KB base64).
- The v0.8.1 chunked encoder is fully compatible
  with the v0.8.0 tmux passthrough: the dispatch
  wraps the entire multi-chunk output (all
  concatenated APC commands) in a single
  `wrap_for_tmux` call, so tmux users with
  `TMUXPASSTHROUGH=1` get the chunked output
  passthrough-wrapped as one passthrough DCS.
- `cargo build`, `cargo test`, `cargo fmt --check`,
  `cargo clippy --all-targets -- -D warnings`, and
  `cargo build --release` are all clean for ALL
  four feature combinations.
- Test count per feature combo: default = 26,
  kitty-encoder alone = 45 (+6 new chunking tests),
  sixel-encoder alone = 28, both = 47 (+1 doc test).
  The bump from v0.8.0 (39 / 41) reflects the 6 new
  v0.8.1 chunking tests.
- No public API changes; no new runtime
  dependencies; no new Cargo features.
- Known limitation: the chunking is hardcoded at
  4096 base64 bytes (the spec's hard limit). A
  future v0.8.x could expose this as a
  configurable encoder option if a particular
  terminal needs a smaller chunk size for some
  reason, but no such terminal is known at the
  time of writing.

## 0.8.0 (2026-07-02)

tmux passthrough: the Kitty encoder now wraps its APC output in
a tmux passthrough DCS (`ESC P tmux ; ... ESC \`) so the bytes
survive the tmux -> outer-terminal hop when the host is running
inside tmux. Opt-in via the new `TMUXPASSTHROUGH` env var (any
non-empty value, typically `TMUXPASSTHROUGH=1`) or the new
`--tmux-passthrough` CLI flag on the `main.rs` demo. The
v0.7.0 default (`TERM=tmux*` -> Sixel) is preserved: without
the opt-in, tmux users still get Sixel. The wrapping is
opt-in because tmux requires `set -g allow-passthrough on`
in `tmux.conf` for tmux 3.2+ to forward APC payloads -- a
user with a stock tmux config would otherwise get corrupted
output.

### Added
- `pub fn wrap_for_tmux(inner: Vec<u8>) -> Vec<u8>` in
  `dashcompositor::encoder` (gated on the `kitty-encoder`
  Cargo feature; re-exported as `dashcompositor::wrap_for_tmux`).
  Pure byte transform: prepends `ESC P tmux ;` to the input,
  doubles every inner `ESC` byte (so tmux 3.2+ passes them
  through as a single literal `ESC` to the outer terminal),
  and appends `ESC \`. No I/O, no env-var reads, fully
  testable without tmux. Useful for downstream encoders
  that want to wrap their own output for tmux passthrough.
- `tmux_passthrough_enabled()` private helper in
  `src/encoder.rs` (gated on `kitty-encoder`): returns
  `true` when `TMUXPASSTHROUGH` is set to a non-empty
  value AND `TMUX` is set (the canonical signal that we
  are inside a tmux session). Both conditions are required
  so a user with `TMUXPASSTHROUGH` set in their shell rc
  on a non-tmux host does not get accidental double-wrapping.
- `v0.8.0` entry in the `detect_with_env` heuristic: when
  `TERM=tmux*` AND `TMUXPASSTHROUGH` is set to a non-empty
  value, pick `Protocol::Kitty` (the dispatch will then
  auto-wrap). Without the opt-in, the v0.7.0 Sixel fallback
  is preserved. `TERM_PROGRAM` still wins (a user with
  `TERM_PROGRAM=wezterm` running inside tmux is using
  wezterm, not native tmux-attached kitty passthrough).
- `TMUXPASSTHROUGH` as a 4th argument to
  `pub(crate) fn detect_with_env(Option<&str>, Option<&str>,
  Option<&str>, Option<&str>) -> Protocol`. `pub fn detect()`
  reads it from the process environment. The empty-string
  case is treated as "not opted in" (consistent with the
  `is_some_and(|v| !v.is_empty())` check in
  `tmux_passthrough_enabled`).
- Auto-wrap in `dispatch(Protocol::Kitty, &frame)`: after
  `kitty::encode` produces the raw APC bytes, the dispatch
  checks `tmux_passthrough_enabled()` and, if true, calls
  `kitty::wrap_for_tmux` to wrap the output. The check is
  in `dispatch` (not in `detect`) so a user who passes
  `--protocol kitty` directly still gets the passthrough
  wrapping when opted in -- the heuristic would have
  picked Sixel, but the explicit Kitty choice overrides.
- `with_env` test helper in `src/encoder.rs` extended
  with a 4th `tmux_passthrough: Option<&str>` argument
  and a 4th `EnvGuard` (RAII save/restore for
  `TMUXPASSTHROUGH`). The 5 existing call sites are
  updated to pass `None` for the new arg. The
  process-global env mutex + RAII guard pattern from
  v0.7.1 is preserved, so the new tests are race-free
  with the existing env-touching tests.
- `main.rs` CLI: `--tmux-passthrough` flag (boolean
  switch, no value). Sets `TMUXPASSTHROUGH=1` for the
  duration of `main` (restored on exit via the new
  `TmuxPassthroughGuard` RAII helper that saves the
  current value on construction and restores it on
  `Drop`). The demo also logs the resolved tmux-passthrough
  state (enabled/disabled) to stderr, so the user can
  verify the opt-in was picked up.
- 9 new unit tests in `src/encoder.rs` (all gated on
  `kitty-encoder` for the encoder-touching ones, and on
  the `EnvGuard` pattern for the env-touching ones):
  - `detect_with_env_tmux_picks_kitty_with_tmux_passthrough`
    (heuristic: opt-in + `TERM=tmux*` -> Kitty; also
    verifies `TERM_PROGRAM` still wins).
  - `detect_with_env_tmux_picks_sixel_with_empty_or_missing_tmux_passthrough`
    (heuristic: no opt-in -> Sixel, preserving the v0.7.0
    fallback for `TERM=tmux*`).
  - `wrap_for_tmux_wraps_inner_apc_in_dcs_passthrough`
    (pure byte transform: prefix, suffix, and length
    arithmetic on a typical Kitty APC).
  - `wrap_for_tmux_doubles_inner_esc_bytes` (pure byte
    transform: a middle `ESC TEST` becomes `ESC ESC TEST`).
  - `wrap_for_tmux_handles_empty_inner` (edge case: empty
    input -> `ESC P tmux ; ESC \`, exactly 9 bytes).
  - `wrap_for_tmux_leaves_non_esc_bytes_untouched`
    (pure byte transform: no-ESC payloads pass through
    verbatim).
  - `dispatch_kitty_with_tmux_passthrough_wraps_output`
    (env-driven auto-wrap: `TMUXPASSTHROUGH=1` + `TMUX`
    set -> output starts with `ESC P tmux ;` and ends
    with `ESC \`).
  - `dispatch_kitty_without_tmux_passthrough_does_not_wrap`
    (env-driven auto-wrap disabled: v0.7.0 raw APC
    output, even with `TERM=tmux-256color` and `TMUX` set).
  - `dispatch_kitty_explicit_protocol_still_wraps_when_opted_in`
    (explicit `Protocol::Kitty` with the opt-in + `TMUX`
    set -> wraps, even when the heuristic would have
    picked Sixel for `TERM=xterm-256color`).
- `Cargo.toml`:
  - Version bumped to `0.8.0`.
  - The `little-kitty` comment was updated to remove
    the now-incorrect "auto-detects tmux passthrough"
    claim (verified during planning: `little_kitty` 0.0.3
    does NOT auto-wrap; the wrapping is the caller's
    responsibility, hence this v0.8.0 work). The new
    comment says: "emits raw APC escape sequences
    (passthrough wrapping is the v0.8.0 caller's
    responsibility)".

### Changed
- `lib.rs` re-exports `dashcompositor::wrap_for_tmux`
  (gated on `kitty-encoder`).
- `with_env` test helper signature changed from
  `(term, term_program, colorterm, f)` to
  `(term, term_program, colorterm, tmux_passthrough, f)`.
  All 5 existing call sites updated to pass `None` for
  the new arg. This is a test-only change; the public
  API of `dashcompositor` is unchanged except for the
  new `wrap_for_tmux` re-export.
- `main.rs` version banner updated from `v0.7.1` to
  `v0.8.0`. New tmux-passthrough log line added after
  the "requested/resolved" log line.

### Notes
- `cargo build`, `cargo test`, `cargo fmt --check`,
  `cargo clippy --all-targets -- -D warnings`, and
  `cargo build --release` are all clean for ALL four
  feature combinations: default, `--features
  kitty-encoder`, `--features sixel-encoder`, and
  `--features kitty-encoder,sixel-encoder`.
- Test count per feature combo: default = 26, kitty-encoder
  alone = 39, sixel-encoder alone = 28, both = 41 (+1
  doc test). The bump from v0.7.1 (4/17/7/20) reflects
  the 9 new v0.8.0 tests (2 heuristic + 4 wrap_for_tmux
  + 3 dispatch) and the pre-existing per-encoder tests
  that now exercise the 4-arg `detect_with_env` +
  `with_env` signatures.
- The opt-in is intentionally explicit. A user with
  `set -g allow-passthrough on` in their `tmux.conf` is
  the target audience; the explicit opt-in means a user
  with a stock tmux config is unaffected (v0.7.0 Sixel
  fallback preserved) and a user who actively wants
  Kitty passthrough must consciously enable it (reducing
  the blast radius of a misconfigured `tmux.conf`).
- Nested tmux (tmux-in-tmux) is not yet supported: the
  wrapping does not double-wrap. This is left for a
  future v0.8.x; the workaround is to not nest tmux
  sessions when using passthrough.
- tmux < 3.2 has no `ESC ESC` escape mechanism and would
  treat the inner `ESC \` as the outer passthrough
  terminator (corrupting the sequence). The opt-in
  documentation in README.md and CHANGELOG recommends
  tmux 3.2+ (released 2021).
- No public API removals; no new runtime dependencies;
  no new Cargo features.

## 0.7.1 (2026-07-02)

Patch release: hardens the `with_env` test helper with a
panic-safe `EnvGuard` (Drop-based) that also saves and restores
`COLORTERM` (the v0.7.0 helper only saved/restored `TERM` and
`TERM_PROGRAM` and required manual pairing, so a panic in the
test body could leak env modifications to other parallel tests).
Also strengthens the auto-detect dispatch test from a
"does-not-infinite-loop" probe to a byte-prefix assertion.

### Changed
- `src/encoder.rs` `with_env` test helper now uses a private
  `EnvGuard` struct (a `Drop`-implementing RAII handle around
  `std::env::var` / `std::env::set_var` / `std::env::remove_var`).
  The guard saves the current value on construction and restores
  it on `Drop`, so a panic in the test body still restores all
  three env vars. The helper signature now takes three
  `Option<&str>` arguments (TERM, TERM_PROGRAM, COLORTERM) and
  sets / clears all three on each call.
- `src/encoder.rs` `with_env` now also acquires a
  process-global `Mutex<()>` (returned by `env_mutex()` via a
  `std::sync::OnceLock`) before touching any env var, and
  holds it until the closure returns. This serialises the
  env-touching tests so two `with_env` calls running in
  parallel can no longer stomp on each other's saved env
  values (the v0.7.0 `EnvGuard`-only pattern was still racy:
  the second `EnvGuard::new("TERM")` would snapshot the first
  test's modified value, and the second `Drop` would restore
  that -- not the original -- leaking the first test's value
  to subsequent parallel tests). The 1-of-81 test failure
  observed in a v0.7.0-flake validation run was this race.
  The lock is acquired with `unwrap_or_else(|e|
  e.into_inner())` to recover from a poisoned mutex (e.g. a
  previous test panicked while holding the lock).
- `src/encoder.rs` test `dispatch_auto_recurses_through_detect`
  (the v0.7.0 name) was renamed and strengthened to
  `dispatch_auto_recurses_through_detect_resolves_to_kitty` and
  is now gated on `kitty-encoder`. The new test sets
  `TERM=xterm-kitty` (a known Kitty terminfo name), invokes
  `dispatch(Protocol::Auto, &fb)`, and asserts the output
  starts with `\x1b_G` (the Kitty APC introducer) -- proving
  the recursion actually resolves to Kitty, not just that it
  terminates. Without this, a regression that made the
  recursion land in the wrong arm would not have been caught
  by the v0.7.0 test (which only verified the call returned
  without panicking).
- New `src/encoder.rs` test
  `dispatch_auto_recurses_through_detect_resolves_to_sixel`
  (gated on `sixel-encoder`) is the Sixel-side mirror of the
  Kitty recursion test. Sets `TERM=tmux-256color` (a known
  Sixel-fallback terminfo name) and asserts the dispatch
  output starts with `\x1bP` (the Sixel DCS introducer).

### Notes
- `cargo build`, `cargo test`, `cargo fmt --check`, and
  `cargo clippy --all-targets -- -D warnings` are all clean
  for ALL four feature combinations: default,
  `--features kitty-encoder`, `--features sixel-encoder`, and
  `--features kitty-encoder,sixel-encoder`. `cargo build --release`
  is clean for the default and both-features combos.
- The `EnvGuard` pattern is the recommended one for any future
  test helper that mutates process-global state (env vars, cwd,
  etc.): the `Drop` impl guarantees cleanup even on panic. The
  pattern is small enough (~25 lines including the `with_env`
  wrapper) that it lives inline in the test module rather than
  in a separate `pub(crate)` test-utility module.
- No public API changes; no new dependencies; no new features.


## 0.7.0 (2026-07-02)

Auto-detect protocol: a new `Protocol::Auto` variant that picks
`Protocol::Kitty` or `Protocol::Sixel` based on terminal capability
detection (env-var shim over `TERM` / `TERM_PROGRAM` / `COLORTERM`,
plus a Kitty query-response probe via
`little_kitty::Command::is_supported()` for authoritative detection).

### Added
- `Protocol::Auto` variant on the existing `Protocol` enum. The
  `as_str` impl returns `"auto"` for the new variant. Both
  `EncoderError` and `ProtocolEncoder` are unchanged.
- `pub fn detect() -> Protocol` in `dashcompositor::encoder`:
  pure env-var detection (`TERM` / `TERM_PROGRAM` / `COLORTERM`).
  Always available, no I/O, never panics. The shim that
  `Protocol::Auto::encode` dispatches through. Heuristics, in
  priority order:
  1. `TERM_PROGRAM` (most specific): `kitty` / `wezterm` /
     `ghostty` (case-insensitive) -> `Protocol::Kitty`.
  2. `TERM` (terminfo name): `xterm-kitty` / `foot` / `foot-*`
     -> `Protocol::Kitty`; `tmux` / `tmux-*` -> `Protocol::Sixel`
     (Kitty via tmux needs passthrough, not yet implemented).
  3. `COLORTERM` tiebreaker (weak signal): `truecolor` / `24bit`
     -> `Protocol::Kitty` when `TERM`/`TERM_PROGRAM` are
     inconclusive. Modern truecolor terminals are more likely to
     support the Kitty graphics protocol than the average
     XTerm-like terminal.
  4. Default -> `Protocol::Sixel` (most universal fallback).
- `#[cfg(feature = "kitty-encoder")] pub fn detect_with_probe() -> Result<Protocol, EncoderError>`:
  authoritative detection via the I/O-based Kitty query-response
  probe (`little_kitty::Command::is_supported()`). Short-circuits
  to `Ok(Kitty)` when the env-var shim already says Kitty (avoids
  an unnecessary probe in the common case). Performs I/O on
  stdin/stdout; do NOT call from a pure encoder.
- `pub(crate) fn detect_with_env(Option<&str>, Option<&str>) -> Protocol`:
  testable inner of `detect`, accepts env values directly to
  avoid `std::env::set_var` races in parallel tests.
- Private `fn dispatch(Protocol, &FrameBuffer) -> Result<Vec<u8>, EncoderError>`
  refactored out of the `ProtocolEncoder for Protocol` impl so
  the `Auto` arm can recurse cleanly via `dispatch(detect(), frame)`
  without duplicating the per-variant `#[cfg]` matrix. The
  recursion is bounded because `detect` returns only `Kitty` or
  `Sixel` (never `Auto`) by construction.
- `Protocol::Auto` arm of `ProtocolEncoder::encode`:
  `dispatch(detect(), frame)`. When neither encoder feature is
  enabled, the recursion lands in the disabled-feature Kitty or
  Sixel arm and returns `Err(UnsupportedProtocol)` (the error
  name is the concrete protocol picked by `detect`, not `"auto"`).
- `lib.rs` re-exports `pub use encoder::{detect, EncoderError,
  Protocol, ProtocolEncoder};` and, gated on `kitty-encoder`,
  `pub use encoder::detect_with_probe;`.
- `main.rs` CLI flags: `--protocol <kitty|sixel|auto>` (override
  the default) and `--probe` (use the I/O-based Kitty probe
  instead of the env-var shim). Default protocol is now
  `Protocol::Auto`. The demo logs both the requested and resolved
  protocol so the user can verify the auto-detect.
- 14 new unit tests in `src/encoder.rs`:
  - `as_str_matches_variant` (extended to cover `Auto`).
  - `detect_with_env_picks_kitty_for_term_program_kitty` (with
    case-insensitive variants for `Kitty` / `KITTY`).
  - `detect_with_env_picks_kitty_for_term_program_wezterm` /
    `..._ghostty` (case-insensitive).
  - `detect_with_env_picks_kitty_for_xterm_kitty`.
  - `detect_with_env_picks_kitty_for_foot_and_foot_extra`
    (`foot`, `foot-extra`, `foot-256color`).
  - `detect_with_env_picks_sixel_for_tmux` (`tmux`,
    `tmux-256color`, `tmux-direct`).
  - `detect_with_env_picks_sixel_for_xterm_256color`.
  - `detect_with_env_picks_sixel_when_neither_set` (both
    unset and both empty).
  - `detect_with_env_term_program_wins_over_term` (priority
    ordering).
  - `detect_with_env_unknown_term_program_falls_through_to_term`.
  - `detect_with_env_colorterm_truecolor_picks_kitty_for_unknown_term`.
  - `detect_with_env_colorterm_24bit_picks_kitty_for_unknown_term`.
  - `detect_with_env_colorterm_does_not_override_term_program`.
  - `dispatch_auto_recurses_through_detect` (no-env-var
    termination test).
  - `dispatch_auto_with_term_program_kitty_delegates_to_kitty`
    (env-var-driven dispatch, gated on `kitty-encoder`).
  - `dispatch_auto_with_term_tmux_delegates_to_sixel`
    (env-var-driven dispatch, gated on `sixel-encoder`).
  - `auto_encode_through_trait_delegates_to_dispatch` (gated
    on both features; closes the one-line-wrapper regression
    gap between `Protocol::Auto.encode` and `dispatch`).
  - `detect_with_probe_short_circuits_when_env_already_kitty`
    (gated on `kitty-encoder`; verifies the env-var short-circuit
    path returns `Ok(Kitty)` without invoking the probe).

### Changed
- `Cargo.toml` version bumped to 0.7.0. No new features, no new
  dependencies.
- `src/encoder.rs` module doc updated to mention v0.7.0 and the
  auto-detect shim.
- `src/main.rs` rewritten to default to `Protocol::Auto`, parse
  the `--protocol` / `--probe` CLI flags, and log the resolved
  protocol. The demo gracefully falls back to the env-var shim
  when `--probe` is passed but the `kitty-encoder` feature is
  not enabled.
- The `with_env` test helper (used by 2 dispatch tests) is
  process-global and racy under `cargo test`'s default parallel
  harness. The race is acknowledged in a code comment; a future
  v0.7.1+ may move these to integration tests in `tests/` or add
  a `Mutex<()>` for serialisation.

### Notes
- `cargo build`, `cargo test`, `cargo fmt --check`, and
  `cargo clippy --all-targets -- -D warnings` are all clean
  for ALL four feature combinations: default,
  `--features kitty-encoder`, `--features sixel-encoder`, and
  `--features kitty-encoder,sixel-encoder`. `cargo build --release`
  is clean for the default and both-features combos.
- Test count per feature combo: 4 with default features, 17 with
  `--features kitty-encoder` alone, 7 with `--features
  sixel-encoder` alone, 20 with both features; +1 doc test.
  (The previous v0.6.0 release had 4 / 6 / 6 / 8; the v0.7.0
  bump adds the 14 new `detect` / `dispatch` / `auto_encode`
  tests plus the existing per-encoder tests under their
  respective feature gates.)
- The `Result<Vec<u8>, EncoderError>` return on
  `ProtocolEncoder::encode` (carried over from v0.5.0/v0.6.0) is
  the only way to surface `EncoderError::UnsupportedProtocol`
  from the disabled-feature arms and the not-yet-implemented
  error paths. A literal `Vec<u8>` return (as in the original
  v0.5.0 spec) would have required either gating the entire
  trait on a feature (breaking the ungated re-exports) or
  panicking from disabled-feature / not-implemented paths.
- End-to-end demo verification (both-features build):
  - `TERM=xterm-kitty TERM_PROGRAM=kitty`: resolves to `kitty`,
    emits 10,268 bytes starting with `1b 5f 47` (`\x1b_G`) and
    ending with `2f 1b 5c` (`/\x1b\\`).
  - `TERM=tmux-256color`: resolves to `sixel`, emits 142 bytes
    starting with `1b 50 39` (`\x1bP9`).
  - `COLORTERM=truecolor TERM=xterm-256color`: resolves to
    `kitty` (COLORTERM tiebreaker kicks in).
  - Default-features build: resolves to `sixel` (env-var
    default), then prints
    `encoder error for protocol sixel: protocol sixel is not
    supported in this build (is the required Cargo feature
    enabled?)` and exits 0. The demo is designed to fail
    gracefully when the relevant Cargo feature is missing.


## 0.6.0 (2026-07-02)

Second protocol encoder: the Sixel graphics protocol, wired up via
the optional `icy_sixel` (v0.5) crate behind a new
`sixel-encoder` Cargo feature. The Kitty encoder from v0.5.0
remains; both arms of `ProtocolEncoder::encode` are now real when
their respective features are enabled.

### Added
- `icy_sixel = "0.5"` as an optional dependency, gated behind
  the new `sixel-encoder` Cargo feature (default off).
- Private `sixel` submodule in `src/encoder.rs`, paralleling the
  `kitty` submodule. Uses the real `icy_sixel` 0.5 API:
  - `SixelImage::from_rgba(rgba, w, h)` to wrap the framebuffer's
    RGBA pixels (the `u32` width/height from `FrameBuffer` are
    widened to `usize` via a lossless `as` cast, which is sound
    on every supported platform).
  - `.encode() -> Result<String, SixelError>` to produce the
    full DCS-wrapped Sixel string (`\x1bPq...<sixel data>...\x1b\\`).
  - `.into_bytes()` to convert the `String` to the `Vec<u8>`
    return type.
- `From<std::io::Error> for EncoderError` (gated on
  `kitty-encoder`) and `From<icy_sixel::SixelError> for
  EncoderError` (gated on `sixel-encoder`). These let both
  `kitty::encode` and `sixel::encode` use `?` directly instead of
  a per-module `.map_err(helper)?` pattern. The v0.5.0 `io_err`
  and v0.6.0 `sixel_err` local helpers have been removed in
  favour of the `From` impls; the pattern is now ready to scale
  to the v0.7.0 auto-detect work.
- 3 new Sixel tests mirroring the Kitty test suite:
  - `sixel_encode_rejects_zero_dimensions`
  - `sixel_encode_produces_valid_dcs_framing` -- strengthened
    against the same class of regression as the Kitty framing
    test: checks `starts_with(b"\x1bP")`, `ends_with(b"\x1b\\")`,
    the `q` mode letter appears before the first `#`
    colour-definition introducer, the output is longer than 16
    bytes (catches an empty-payload regression), and the
    2x2 dimensions are referenced in the output.
  - `sixel_encode_is_deterministic_for_same_input`
- The stale `sixel_encode_is_unsupported_in_v050` test was
  renamed to `sixel_encode_is_unsupported_without_feature` and
  re-gated on `not(feature = "sixel-encoder")` to mirror the
  existing Kitty `kitty_encode_is_unsupported_without_feature`
  pattern.

### Changed
- `Cargo.toml` version bumped to 0.6.0; `sixel-encoder` feature
  added; `icy_sixel` optional dep added.
  evaluation: pure-Rust SIXEL encoder/decoder, MIT/Apache-2.0,
  actively maintained, the de-facto Rust SIXEL library).
- The `Protocol::Sixel` arm of `ProtocolEncoder` now dispatches
  to `sixel::encode` when the `sixel-encoder` feature is on, and
  returns `Err(UnsupportedProtocol("sixel"))` otherwise -- the
  same shape as the Kitty arm.
- The `EncoderError::Encode` variant's Display message is
  unchanged, but it's now reachable via `?` from both encoder
  submodules via the new `From` impls.
- The encoder module's doc comment was updated to mention the
  v0.6.0 Sixel work and the per-feature `UnsupportedProtocol`
  return.

### Notes
- `cargo build`, `cargo test` (4 unit tests with default
  features, 6 with `--features kitty-encoder` alone, 6 with
  `--features sixel-encoder` alone, 8 with
  `--features kitty-encoder,sixel-encoder`; +1 doc test),
  `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`,
  and `cargo build --release` are all clean for ALL four
  feature combinations.
- The `From` impl pattern is the recommended one for future
  encoders (e.g. the v0.7.0 `rasteroid` auto-detect): add a
  `From<NewError> for EncoderError` and the new encoder can use
  `?` directly without a per-module error helper.
- `icy_sixel` was chosen over `sixel`/`sixel-sys` (the latter is
  FFI to the C `libsixel` and has not had a 2026 release).

## 0.5.0 (2026-07-02)

First protocol encoder: the Kitty graphics protocol, wired up via
the optional `little-kitty` (v0.0.3) crate behind a new
`kitty-encoder` Cargo feature. Sixel remains the v0.6.0 work.

### Added
- `ProtocolEncoder` trait in `dashcompositor::encoder`:
  - Signature: `fn encode(&self, frame: &FrameBuffer) -> Result<Vec<u8>, EncoderError>`.
  - Implementor: `Protocol`, dispatching on the protocol variant.
  - Returns a `Vec<u8>` of terminal escape sequences; the caller
    writes them to stdout. No I/O is performed inside `encode`.
- `EncoderError` enum with hand-rolled `Display` + `Error` impls
  (no `thiserror` dep, to honour the project's minimal-deps
  ethos):
  - `UnsupportedProtocol(&'static str)` -- returned by the
    disabled-feature Kitty arm and by Sixel until v0.6.0.
  - `InvalidDimensions { width, height }` -- zero-size framebuffer.
  - `Encode(String)` -- wraps the underlying `little_kitty`
    `std::io::Error`.
- Private `kitty` submodule gated on `kitty-encoder`. Uses the
  real `little_kitty` 0.0.3 API:
  - `Command::default()` + `.with_control(key, value)` with
    `value: Into<ControlValue>`.
  - `ControlValue::Char('T')` for the action
    (transmit and put), `ControlValue::UnsignedInteger(...)` for
    the format (32 = 32-bit RGBA), quiet flag (2 = suppress
    responses), and width / height.
  - `little_kitty::io::KittyCommandWriter` (blanket-impl'd for
    any `Write`, including `Vec<u8>`) for `write_start`,
    `write_base64(self, data)`, `write_end`.
  - `ControlValue::write(&mut out)` to serialise each control
    value into the byte buffer.
  - Output format: `\x1b_Ga=T,f=32,q=2,s=W,v=H;<base64-payload>\x1b\\`.
- `Cargo.toml`:
  - `little-kitty = "0.0.3"` as an optional dependency.
  - `kitty-encoder = ["dep:little-kitty"]` Cargo feature.
  - Default features stay empty; the default build remains
    dependency-light.
- 7 new unit tests in `src/encoder.rs`:
  - `as_str_matches_variant` -- protocol name strings.
  - `encoder_error_display_includes_context` -- `Display` impl.
  - `sixel_encode_is_unsupported_in_v050` -- Sixel always returns
    `UnsupportedProtocol`.
  - `kitty_encode_is_unsupported_without_feature` -- Kitty without
    the feature returns `UnsupportedProtocol`.
  - `kitty_encode_rejects_zero_dimensions` -- zero-size returns
    `InvalidDimensions`.
  - `kitty_encode_produces_valid_escape_framing` -- output starts
    with `\x1b_G`, ends with `\x1b\\`, and the control payload
    contains `a=T`, `f=32`, `q=2`, `s=2`, `v=2`.
  - `kitty_encode_is_deterministic_for_same_input` -- pure
    encoder.
- `main.rs` demo now encodes the auto-fit framebuffer via
  `Protocol::Kitty` and writes the escape bytes to stdout; stderr
  carries the human-readable log lines.

### Changed
- `Cargo.toml` version bumped to 0.5.0.
- `lib.rs` re-exports `EncoderError` and `ProtocolEncoder`
  (ungated, so the API surface is stable across feature
  combinations; calling `encode` on a disabled-feature protocol
  returns `Err(UnsupportedProtocol)` at runtime).

### Notes
- `little-kitty` evaluation: MIT/Apache-2.0,
  v0.0.3 (March 2026), ~148K SLoC, actively maintained, the
  recommended pick over `kittage` (heavier, full-featured) and
  `kitty-graphics-protocol` (last build failed on docs.rs). The
  `rasteroid` auto-detect wrapper is deferred to v0.7.0+ (it
  doesn't expose granular feature flags to keep the dep
  footprint tight).
- `cargo build`, `cargo test` (65 unit tests with default
  features, 71 with `--features kitty-encoder`; +1 doc test),
  `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`,
  and `cargo build --release` are all clean -- BOTH with default
  features and with `--features kitty-encoder`.

## 0.4.0 (2026-07-02)

Multi-layer compositor: three new `Layer` types, an optional
image-decoder feature, a `Rect` geometry primitive, and a
breaking extension to the `Layer` trait (additive render
offset + `bounds()`).

### Added
- `src/geometry.rs` -- new `Rect { x, y, width, height }`
  geometry primitive with `is_empty`, `contains`, and
  `intersects` helpers. Re-exported as `dashcompositor::Rect`.
- `RectLayer` (always available): RGBA solid at `(x, y)`
  with `width x height`. `bounds()` reports the rect; render
  writes are clipped to the framebuffer.
- `TextLayer` (always available): UTF-8 text + position +
  colour placeholder. Exposes `render_glyph() -> &str` (a
  placeholder for a future font-backed glyph rasterizer)
  and `text_width()` (one cell per Unicode scalar value).
  Renders as a solid block the size of the text's bounding
  box so layout and z-order are visually verifiable.
- `ImageLayer` (gated on the new `image-decoder` Cargo
  feature): decodes PNG and JPEG via the `image` crate
  (version 0.25, MIT, `default-features = false`, only
  `png` + `jpeg` decoders enabled).
  Constructors: `ImageLayer::from_path` and
  `ImageLayer::from_dynamic`.
- `FrameBuffer::get_pixel` and `FrameBuffer::get_pixel_mut`:
  bounds-checked per-pixel accessors that return `Option`,
  giving layers a single, consistent way to clip writes.

### Changed
- `Layer` trait extended (breaking for downstream implementors):
  - New `bounds() -> Option<Rect>` with a default impl
    returning `None` (full-frame layers like `SolidColor`).
  - `render` signature gains an additive `offset: (u32, u32)`
    translation parameter; layers that have no position
    (e.g. `SolidColor`) ignore it.
- `Cargo.toml`: added the `image` crate as an optional
  dependency and the `image-decoder` Cargo feature
  (`default = []`; `image-decoder = ["dep:image"]`). The
  default build remains dependency-light (only
  `terminal_size`).
- `main.rs` demo now drives a `SolidColor` background, a
  positioned `RectLayer`, and a `TextLayer` placeholder,
  reports each layer's bounds, and exercises the full
  add / control / remove / re-add / re-render flow.

### Notes
- `cargo build`, `cargo test` (58 unit tests with default features, 64 with `--features image-decoder`; +1 doc test),
  `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`,
  and `cargo build --release` all remain clean -- both with
  default features and with `--features image-decoder`.
- The `image` crate evaluation: BSD
  3-Clause / Apache-2.0 / MIT, ~70M downloads, the de-facto
  Rust image-decoding library. Adopted as an optional dep
  with `default-features = false` + `png` + `jpeg` only.

# Changelog

All notable changes to `termcompositor` are recorded here. The format
follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and
the project adheres to [Semantic Versioning](https://semver.org/).

## 0.3.0 (2026-07-02)

Terminal-aware compositor: the framebuffer is auto-sized to the
host terminal, and the detected size is reported back through the
API.

### Added- `terminal_size = "0.3"` as the first runtime dependency.
- New `src/terminal.rs` module with `TerminalSize { rows, cols }` and
  the entry points:
  - `TerminalSize::detect()` -- queries the host via
    `ioctl`/console mode; returns `Option<Self>`.
  - `TerminalSize::current()` -- detects or falls back to 80x24.
    Never panics.
  - `TerminalSize::fallback()` -- the static 80x24 default.
  - `TerminalSize::as_framebuffer_size()` -- converts to a
    `(u32, u32)` tuple for `FrameBuffer::new`.
- `LayerStack::render_to_terminal(size)` renders into a
  `FrameBuffer` sized to the given terminal.
- `LayerStack::render_to_current_terminal()` detects the terminal
  size and renders, returning the `(FrameBuffer, TerminalSize)`
  tuple so the backend can report the size back through the API.
- `main.rs` rewritten to detect the terminal size on startup,
  print it, use `render_to_current_terminal` to fit the framebuffer
  to the host, and verify the rendered size matches the reported
  size.
- Tests for `TerminalSize` (fallback, conversion, equality,
  panic-free `current`) and for the new `LayerStack` methods.

the project adheres to [Semantic Versioning](https://semver.org/).

## 0.2.0 (2026-07-02)

First concrete subsystem: a layer stack that the backend (any binary
or library user) can drive at will, addressing the original
"add/remove/control layers from the backend" requirement.

### Added
- `Layer` trait extended with `name()` (default impl) and
  `render(&self, &mut FrameBuffer, opacity)`.
- `LayerEntry` wrapper struct with stable `LayerId`, opacity,
  visibility, optional z-override, and optional custom name.
  - Manual `Debug` impl on `LayerEntry` (the inner `Box<dyn Layer>`
    blocks the derive).
  - `set_layer(Box<dyn Layer>)` for in-place hot-swap without
    invalidating external id handles.
  - `set_z_override(u32)` and `clear_z_override()` (split from the
    prior `set_z_override(Option<u32>)` for ergonomics).
- `LayerStack` with backend-manipulable API: `push` / `remove` /
  `get` / `get_mut` / `index_of` / `reorder` / `len` / `is_empty` /
  `entries` / `entries_mut` / `iter_sorted` / `clear` / `render` /
  `render_with`. Ids are monotonic and not reused for the lifetime of
  the stack.
- `Compositor` trait and `CpuCompositor` default implementation.
  `CpuCompositor` is a zero-dependency reference: it sorts visible
  entries by effective z-order (stable on ties) and calls each
  layer's `render` with its opacity.
- `SolidColor` concrete layer with `with_z` and `with_name` builders.
- `FrameBuffer::clear()` and a free function `blend_over` for
  straight-alpha over-compositing in 8-bit RGBA.
- README "Usage (library)" section showing the push/control/render
  flow.
- 28 unit tests + 1 doc-test covering blend math, layer controls,
  layer-stack add/remove/reorder/render, custom compositor, and the
  iter_sorted z-order + stable-tiebreak contracts.

### Notes
- Zero runtime dependencies.  Candidate crates (tiny-skia, wgpu,
  image, kittage, icy_sixel) remain commented-out optional features.
- `cargo build`, `cargo test`, `cargo fmt --check`,
  `cargo clippy --all-targets -- -D warnings`, and
  `cargo build --release` all clean.
- GPG commit signing continues via loopback pinentry + `allow-preset-passphrase` on the gpg-agent.

## 0.1.0 (2026-07-02)

Initial scaffold of `dashcompositor`, a layer-based graphics compositor
for the terminal that projects a fully composited RGBA framebuffer to the
host via the Kitty graphics protocol or Sixel.

### Added
- MIT `LICENSE` (2026).
- `README.md` -- project overview, target features, architecture diagram.
- `Cargo.toml` -- package metadata, lib + bin targets, `[lints.rust]
  missing_docs = "warn"`. Candidate feature flags (CPU/GPU compositor,
  image decoder, kitty/sixel encoders) are stubbed but commented out
  until each crate is vetted on crates.io.
- `src/lib.rs` plus four module stubs: `compositor`, `layer`, `framebuffer`, `encoder`.
- `src/main.rs` -- no-op binary entry point pending a real
  protocol-detector implementation.
- `.gitignore` extended for Rust build output (`target/`, `*.rs.bk`,
  `.cargo/`).
- CI-ready: `cargo build`, `cargo test`, `cargo fmt --check`,
  `cargo clippy --all-targets -- -D warnings`, and `cargo build --release`
  all pass on the scaffold.
- Environment: GPG signing is wired up (loopback pinentry +
  `allow-preset-passphrase` on the gpg-agent, `user.signingkey` pinned
  to the primary key `8CAF4D685F95A842`) so non-interactive commits in
  this host produce verifiable signatures.
