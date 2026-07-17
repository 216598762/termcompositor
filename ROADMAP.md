# termcompositor Roadmap

Feature ideas for future development, organized by priority tier.

## Tier 1 — High Impact

### 1. Animation Loop / Render Loop
The current API is single-shot: build layers → composite → encode → done. Add
a built-in animation loop with frame scheduling, delta-time tracking, and
optional vsync. Users who want dashboards, progress bars, or live data displays
currently must build their own event loop.

**Design sketch:**
```rust
termcompositor::run(60.0, |ctx| {
    ctx.layers_mut().get(progress_bar).set_progress(t);
    ctx.request_redraw();
});
```

### 2. Layer Transforms (rotation, scaling)
Layers only have `(x, y)` positioning. No rotation, scaling, or arbitrary
affine transforms. This limits visual expressiveness for icons, logos, and
decorative elements.

**Approach:** Add an optional `Transform` struct to `LayerEntry` with
rotation (degrees), scale (x, y factors), and anchor point. Apply the
transform during `render()` via bilinear interpolation on the framebuffer.

### 3. Gradient Layers
No `GradientLayer` for linear/radial gradients. Currently users must
approximate gradients with many thin `RectLayer` strips.

**Design sketch:**
```rust
GradientLayer::linear(x, y, w, h, start_color, end_color, angle)
GradientLayer::radial(cx, cy, r, inner_color, outer_color)
```

### 4. Layer Clipping / Masking
No way to clip one layer to the bounds of another (e.g., round image inside
a circle). The only clipping is at framebuffer boundaries.

**Approach:** Add a `clip_to: Option<LayerId>` field on `LayerEntry`. When
set, the compositor renders the clipped layer into a temporary framebuffer
and blits only the overlapping pixels.

## Tier 2 — Medium Impact

### 5. Border / Stroke Support
`RectLayer` fills a rectangle but can't draw just the border. A `border_width`
field or separate `StrokeRect` layer would be useful for UI boxes and panels.

**Approach:** Add `border_width: Option<u32>` and `border_color: Option<[u8; 4]>`
to `RectLayer`. When set, only the border pixels are drawn.

### 6. Rounded Corners
No rounded-rectangle support. Common in terminal UIs for card-like layouts.

**Approach:** Add `border_radius: Option<u32>` to `RectLayer`. During render,
skip pixels whose distance from the nearest corner exceeds the radius.

### 7. Shadow / Glow Effects
No drop shadow or glow primitives. Users must manually create offset copies
with reduced opacity.

**Approach:** Add a `ShadowLayer` wrapper that takes an inner layer, renders
it to a temp buffer, applies a box blur, offsets the result, and composites
with configurable color and opacity.

### 8. SVG Rendering Layer
`ImageLayer` loads raster images only. An `SvgLayer` using `resvg` or `usvg`
would enable vector graphics at any resolution.

## Tier 3 — Quality of Life

### 9. Canvas API for Custom Drawing
No API for users to draw arbitrary pixels/shapes without creating a new
`Layer` implementation. A `CanvasLayer` with `draw_pixel`, `draw_line`,
`draw_circle` methods would lower the barrier for custom rendering.

### 10. Diff-Based Rendering
Each frame re-composites everything from scratch. For animated content,
diffing the previous and current framebuffers could skip unchanged regions,
reducing encode time.

### 11. Scene Graph / Layer Hierarchy
Layers are flat in the `LayerStack`. No parent-child relationships. A scene
graph would enable grouped transforms and visibility cascading.

### 12. Accessibility Metadata
No mechanism to attach alt-text or semantic roles to layers for screen
readers or headless terminals.

## Quick Wins

| Feature | Effort | Impact |
|---------|--------|--------|
| `GradientLayer` (linear) | Low | High |
| `RectLayer` border mode | Low | High |
| `TextLayer` alignment (left/center/right) | Low | Medium |
| `LayerStack::find_by_name()` | Trivial | Medium |
| `FrameBuffer::fill_rect()` helper | Trivial | Medium |

## Version Targets

| Version | Target Features |
|---------|-----------------|
| v0.13.0 | GradientLayer, RectLayer border mode, Canvas API |
| v0.14.0 | Animation loop, Layer transforms |
| v0.15.0 | Layer clipping, Rounded corners, Shadow effects |
| v1.0.0 | SVG layer, Scene graph, Accessibility metadata |
