# Usage Guide

This guide covers how to use `termcompositor` as a library and as a CLI tool.

---

## Table of contents

- [Installation](#installation)
- [Library usage](#library-usage)
- [Animation loop](#animation-loop)
- [Layer transforms](#layer-transforms)
- [Diff-based rendering](#diff-based-rendering)
- [Layer lookup by name](#layer-lookup-by-name)
- [Layer clipping](#layer-clipping)
- [Rounded corners](#rounded-corners)
- [Shadow and glow effects](#shadow-and-glow-effects)
- [Accessibility metadata](#accessibility-metadata)
- [CLI usage](#cli-usage)
- [Feature flags](#feature-flags)
- [Protocol auto-detection](#protocol-auto-detection)
- [Encoder features](#encoder-features)
- [Tmux passthrough](#tmux-passthrough)
- [Benchmarks](#benchmarks)
- [Minimum supported Rust version](#minimum-supported-rust-version)

---

## Installation

### As a library

```toml
[dependencies]
termcompositor = "0.12"
```

Enable encoder features to produce terminal output:

```toml
[dependencies]
termcompositor = { version = "0.12", features = ["kitty-encoder", "sixel-encoder"] }
```

### As a CLI tool

```bash
cargo install termcompositor
```

Or build from source:

```bash
git clone https://github.com/216598762/termcompositor
cd termcompositor
cargo build --release --features kitty-encoder,sixel-encoder
./target/release/termcompositor
```

---

## Library usage

### Quick start

```rust
use termcompositor::{
    dispatch_to_writer, detect, FrameBuffer, LayerStack, SolidColor, TextLayer,
};

let mut stack = LayerStack::new();

// Full-frame background.
stack.push(SolidColor::new(0, 0, 64, 255).with_name("bg"));

// Positioned text.
stack.push(TextLayer::new(0, 0, "hello", [255; 4]).with_z(10));

let mut fb = FrameBuffer::new(80, 24);
stack.render(&mut fb);

// Encode to terminal protocol and write to stdout.
let mut out = Vec::new();
dispatch_to_writer(detect(), &fb, &mut out).unwrap();
std::io::stdout().write_all(&out).unwrap();
```

### Working with layers

```rust
use termcompositor::{LayerStack, RectLayer, SolidColor, TextLayer};

let mut stack = LayerStack::new();

let bg = stack.push(SolidColor::new(0, 0, 64, 255).with_name("bg"));
let rect = stack.push(
    RectLayer::new(20, 6, 40, 12, [0, 200, 0, 200])
        .with_z(10)
        .with_name("centered-rect"),
);
let label = stack.push(
    TextLayer::new(2, 1, "termcompositor", [255, 255, 255, 255])
        .with_z(20)
        .with_name("title"),
);

// Render into an 80x24 framebuffer.
let mut fb = FrameBuffer::new(80, 24);
stack.render(&mut fb);

// Mutate layers at runtime.
stack.get_mut(rect).unwrap().set_opacity(0.5);
stack.get_mut(label).unwrap().set_visible(false);
```

### Terminal-sized render

Auto-detect the terminal size and render to fit:

```rust
use termcompositor::{LayerStack, SolidColor, TextLayer};

let mut stack = LayerStack::new();
stack.push(SolidColor::new(0, 0, 64, 255).with_name("bg"));
stack.push(TextLayer::new(0, 0, "hello", [255; 4]).with_z(10));

let (fb, size) = stack.render_to_current_terminal();
assert_eq!(size.cols as u32, fb.width());
assert_eq!(size.rows as u32, fb.height());
```

### End-to-end: encode and write

```rust
use termcompositor::{
    dispatch_to_writer, detect, FrameBuffer, LayerStack, SolidColor,
};
use std::io::{BufWriter, Write};

let mut stack = LayerStack::new();
stack.push(SolidColor::new(0, 0, 64, 255));

let mut fb = FrameBuffer::new(80, 24);
stack.render(&mut fb);

let stdout = std::io::stdout();
let mut writer = BufWriter::new(stdout.lock());
dispatch_to_writer(detect(), &fb, &mut writer).unwrap();
writer.flush().unwrap();
```

### Custom compositor

Implement the [`Compositor`](https://docs.rs/termcompositor/latest/termcompositor/trait.Compositor.html) trait for custom rendering logic:

```rust
use termcompositor::{Compositor, CpuCompositor, FrameBuffer, LayerStack};

struct DoubleCompositor;

impl Compositor for DoubleCompositor {
    fn compose(&self, stack: &LayerStack, fb: &mut FrameBuffer) {
        // Render normally first.
        CpuCompositor.compose(stack, fb);
        // Then apply a double-brightness effect.
        for pixel in fb.pixels_mut() {
            for channel in 0..3 {
                pixel[channel] = pixel[channel].saturating_mul(2);
            }
        }
    }
}

let mut stack = LayerStack::new();
// ... add layers ...
let mut fb = FrameBuffer::new(80, 24);
stack.render_with(&mut fb, &DoubleCompositor);
```

### Raster image layer

Requires the `image-decoder` feature:

```toml
[dependencies]
termcompositor = { version = "0.12", features = ["image-decoder"] }
```

```rust
use termcompositor::{ImageLayer, LayerStack};

let mut stack = LayerStack::new();
let img = ImageLayer::from_path("logo.png", 4, 2).unwrap();
let id = stack.push(img.with_z(10));
```

### Streaming encode (zero-copy output)

For multi-megapixel framebuffers, use the streaming entry points to avoid materialising the full output in memory:

```rust,ignore
use termcompositor::encoder::kitty;
use termcompositor::framebuffer::FrameBuffer;
use std::io::BufWriter;

let fb = FrameBuffer::new(1920, 1080);
let stdout = std::io::stdout();
let mut writer = BufWriter::new(stdout.lock());
kitty::encode_to_writer(&fb, &mut writer).unwrap();
```

For Sixel (requires the `sixel-encoder` feature):

```rust,ignore
use termcompositor::encoder::sixel;

let fb = FrameBuffer::new(1920, 1080);
let mut writer = BufWriter::new(std::io::stdout().lock());
sixel::encode_to_writer(&fb, &mut writer).unwrap();
```

For the O(1)-memory Sixel path (fixed xterm-256 palette, no `icy_sixel`):

```rust,ignore
use termcompositor::encoder::sixel;

let fb = FrameBuffer::new(1920, 1080);
let mut writer = BufWriter::new(std::io::stdout().lock());
sixel::encode_to_writer_streaming(&fb, &mut writer).unwrap();
```

---

## Animation loop

The `animation` module provides a built-in frame loop with delta-time
tracking, terminal resize handling, and protocol encoding. This is
the easiest way to build animated terminal dashboards.

### Quick start

```rust,no_run
use termcompositor::animation::{self, AnimContext};
use termcompositor::{LayerStack, RectLayer, SolidColor};

fn main() {
    let mut stack = LayerStack::new();
    let bg = stack.push(SolidColor::new(0, 0, 64, 255).with_z(0));
    let bar = stack.push(
        RectLayer::new(2, 10, 20, 5, [0, 200, 0, 255]).with_z(10)
    );

    animation::run_with_stack(stack, 30.0, move |ctx| {
        let t = ctx.elapsed().as_secs_f32();
        // Animate the bar opacity using a sine wave.
        let opacity = (t * 2.0).sin() * 0.5 + 0.5;
        if let Some(entry) = ctx.layers_mut().get_mut(bar) {
            entry.set_opacity(opacity);
        }
        ctx.request_redraw();
    });
}
```

### Entry points

| Function | Description |
|---|---|
| `animation::run(fps, callback)` | Start with an empty layer stack. |
| `animation::run_with_stack(stack, fps, callback)` | Start with initial layers. |
| `animation::run_with_config(stack, config, callback)` | Full control over protocol, FPS, and clear-between-frames. |

### AnimContext

The callback receives a `&mut AnimContext` each frame with:

| Method | Description |
|---|---|
| `layers()` / `layers_mut()` | Access the layer stack. |
| `delta_time()` | Time since the last frame (frame-rate-independent animation). |
| `elapsed()` | Total time since the loop started. |
| `frame_count()` | Current frame number (0-indexed). |
| `terminal_size()` | Current terminal dimensions (auto-detected each frame). |
| `request_redraw()` | Opt-in rendering — skip encoding without this call. |
| `exit()` | Graceful shutdown after the current frame. |

### AnimConfig

```rust
use termcompositor::animation::AnimConfig;
use termcompositor::Protocol;

let config = AnimConfig::new(60.0)           // target FPS
    .with_protocol(Protocol::Kitty)           // force a protocol
    .with_clear_between_frames(false);        // skip ANSI clear
```

### CLI flags

```bash
termcompositor --animate              # default 30 FPS
termcompositor --animate --fps 60     # 60 FPS
termcompositor --animate --fps 10     # slow-motion
```

### How it works

1. Protocol is resolved once at startup (Auto detection is cached).
2. Each frame: callback runs → if `request_redraw()` was called, render
   and encode → sleep for remaining frame time.
3. Terminal size is re-detected each frame; the framebuffer is resized
   automatically.
4. The loop exits when `ctx.exit()` is called.

---

## Layer transforms

Per-layer rotation and scaling via an affine 2D `Transform`. Transforms
are applied at render time using inverse mapping with bilinear
interpolation, so rotated and scaled layers look smooth.

### Creating a transform

```rust
use termcompositor::geometry::Transform;

// Rotate 45° around the center of a 100×100 layer.
let t = Transform::new()
    .with_rotation(45.0)          // degrees, clockwise
    .with_scale(1.5, 1.5)         // horizontal, vertical
    .with_anchor(50.0, 50.0);     // rotation/scale center
```

| Builder method | Description |
|---|---|
| `Transform::new()` | Identity transform (no rotation, scale = 1.0, anchor at origin). |
| `.with_rotation(degrees)` | Clockwise rotation in degrees. |
| `.with_scale(sx, sy)` | Horizontal and vertical scale factors. |
| `.with_anchor(x, y)` | Anchor point in layer-local coordinates. |

### Applying a transform to a layer

```rust
use termcompositor::{LayerEntry, RectLayer, Transform};

let rect = RectLayer::new(10, 10, 50, 50, [255, 0, 0, 255]);
let entry = LayerEntry::new(0, Box::new(rect))
    .with_transform(
        Transform::new()
            .with_rotation(30.0)
            .with_scale(2.0, 2.0)
            .with_anchor(25.0, 25.0)
    );
```

Or set it after pushing:

```rust
let id = stack.push(rect);
stack.get_mut(id).unwrap().set_transform(Some(
    Transform::new().with_rotation(90.0)
));
```

### Accessor methods

| Method | Description |
|---|---|
| `entry.transform()` | `Option<&Transform>` — reference to the current transform. |
| `entry.transform_mut()` | `Option<&mut Transform>` — mutable reference. |
| `entry.set_transform(Some(t))` | Set or clear the transform. |

### How transforms are rendered

The compositor uses **inverse mapping**: for each pixel in the target
framebuffer, it computes the corresponding source coordinate via the
inverse transform, then samples the source with bilinear interpolation.

This means:
- Rotated layers look smooth (no aliasing artifacts at edges).
- Scaled layers use bilinear filtering (no nearest-neighbour blockiness).
- The bounding box is computed from the transformed corners, so only
  the affected region is iterated.

### Identity optimization

If the transform is the identity (no rotation, scale = 1.0), the
compositor skips the transform path entirely and renders the layer
directly — zero overhead.

### Forward and inverse mapping

For advanced use cases, the `Transform` struct exposes both mapping
directions:

```rust
use termcompositor::geometry::Transform;

let t = Transform::new().with_rotation(90.0).with_anchor(5.0, 5.0);

// Forward: layer-local → target space.
let (tx, ty) = t.apply(10.0, 0.0);

// Inverse: target → layer-local (used internally by the compositor).
let (lx, ly) = t.apply_inverse(tx, ty);
assert!((lx - 10.0).abs() < 1e-5);
assert!((ly - 0.0).abs() < 1e-5);
```

---

## Diff-based rendering

`LayerStack` supports diff-based rendering via the `render_diff()` method
and the `DirtyRegion` tracker. This allows you to mark specific regions
as dirty and only re-copy those regions to the framebuffer, reducing
memory bandwidth and improving performance in animation loops.

### Quick start

```rust
use termcompositor::{DirtyRegion, DirtyRect, FrameBuffer, LayerStack, RectLayer};

let mut stack = LayerStack::new();
stack.push(RectLayer::new(0, 0, 50, 50, [255, 0, 0, 255]));

let mut fb = FrameBuffer::new(100, 100);
let mut dirty = DirtyRegion::new();

// First render: mark everything as dirty.
dirty.mark_full();
stack.render_diff(&mut fb, &mut dirty);

// Later: mark only a specific region as dirty.
dirty.mark_rect(DirtyRect::new(10, 10, 20, 20));
stack.render_diff(&mut fb, &mut dirty);
```

### DirtyRegion

The `DirtyRegion` struct tracks rectangular dirty areas across frames:

| Method | Description |
|---|---|
| `DirtyRegion::new()` | Create an empty tracker (clean state). |
| `mark_full()` | Mark the entire framebuffer as dirty (full re-render). |
| `mark_rect(rect)` | Mark a specific rectangular region as dirty. |
| `mark_point(x, y)` | Mark a single pixel as dirty. |
| `is_clean()` | Returns `true` if no regions are marked dirty. |
| `take_regions()` | Consume and return the list of dirty regions (drains the tracker). |
| `region_count()` | Number of tracked dirty regions. |

### DirtyRect

A simple rectangle for specifying dirty regions:

```rust
use termcompositor::DirtyRect;

let rect = DirtyRect::new(x, y, width, height);
```

### Integration with animation loop

The `AnimContext` in the animation loop has built-in dirty region tracking:

| Method | Description |
|---|---|
| `mark_full()` | Mark entire framebuffer dirty (triggers full re-render). |
| `mark_rect(rect)` | Mark a specific region dirty. |
| `request_redraw()` | Set redraw flag AND mark full dirty. |

```rust,no_run
use termcompositor::animation::{self, AnimContext};
use termcompositor::{DirtyRect, LayerStack, RectLayer};

fn main() {
    let mut stack = LayerStack::new();
    let bar = stack.push(
        RectLayer::new(0, 0, 20, 5, [0, 200, 0, 255]).with_z(10)
    );

    animation::run_with_stack(stack, 30.0, move |ctx| {
        // Move the bar and mark only its old/new position as dirty.
        if let Some(entry) = ctx.layers_mut().get_mut(bar) {
            let old_x = entry.layer().bounds().map(|b| b.x).unwrap_or(0);
            // Update position...
            let new_x = old_x + 1;
            ctx.mark_rect(DirtyRect::new(old_x, 0, 20, 5));
            ctx.mark_rect(DirtyRect::new(new_x, 0, 20, 5));
        }
    });
}
```

### Using outside the animation loop

`DirtyRegion` is not tied to the animation module — you can use it in any
rendering context, such as a manual render loop, a game loop, or a
custom compositor:

```rust
use termcompositor::{DirtyRegion, DirtyRect, FrameBuffer, LayerStack, RectLayer};

let mut stack = LayerStack::new();
stack.push(RectLayer::new(0, 0, 50, 50, [255, 0, 0, 255]));

let mut fb = FrameBuffer::new(100, 100);
let mut dirty = DirtyRegion::new();

// First render: mark everything dirty.
dirty.mark_full();
stack.render_diff(&mut fb, &mut dirty);

// Simulate a layer moving from (10,10) to (20,20).
// Mark both the old and new positions as dirty.
dirty.mark_rect(DirtyRect::new(10, 10, 50, 50));
stack.render_diff(&mut fb, &mut dirty);

// Simulate a small update: only a 10x10 region changed.
dirty.mark_rect(DirtyRect::new(30, 30, 10, 10));
stack.render_diff(&mut fb, &mut dirty);

// Force a full re-render.
dirty.mark_full();
stack.render_diff(&mut fb, &mut dirty);
```

This pattern is useful when:
- You have your own frame loop (e.g., a game engine, GUI toolkit).
- You want to optimize static frames where only some layers change.
- You are doing one-shot rendering with incremental updates.

### How it works

1. `render_diff()` checks if `dirty` is clean — if so, does nothing.
2. If dirty, composites all layers into a temporary buffer.
3. Copies only the dirty regions from the temp buffer to `target`.
4. Consumes (drains) the dirty regions after copying.

Note: The full composition still runs every frame — the optimization
is at the copy stage, not the composition stage. This reduces memory
bandwidth when only small regions change.

### Performance characteristics

- **Memory**: O(W×H) for the temporary buffer (same as `render()`).
- **Copy**: O(D) where D = area of dirty regions (typically << W×H).
- **Composition**: O(W×H×N) where N = number of layers (same as `render()`).

The best-case improvement comes when `D << W×H` — e.g., animating a
small rectangle on a large background.

---

## Layer lookup by name

`LayerStack` provides named lookup methods to find layers by their name
without knowing the `LayerId`:

```rust
use termcompositor::{LayerStack, RectLayer, SolidColor};

let mut stack = LayerStack::new();
stack.push(SolidColor::new(0, 0, 64, 255).with_name("bg"));
stack.push(RectLayer::new(10, 5, 20, 10, [255, 0, 0, 255]).with_name("rect"));

// Find by name (returns first match).
let entry = stack.find_by_name("rect").expect("should find rect");
assert_eq!(entry.layer().bounds().unwrap().x, 10);

// Find and modify.
if let Some(entry) = stack.find_by_name_mut("bg") {
    entry.set_opacity(0.5);
}
```

### API reference

| Method | Description |
|---|---|
| `find_by_name(name)` | Returns `Option<&LayerEntry>` for the first entry whose name matches. |
| `find_by_name_mut(name)` | Returns `Option<&mut LayerEntry>` for the first matching entry. |

### Notes

- Names are set via `.with_name("name")` on any layer before pushing.
- Only the **first** matching entry is returned (if duplicates exist).
- Returns `None` if no entry has the given name.
- Lookup is O(n) linear scan — suitable for small stacks; use `LayerId`
  for O(1) access in performance-critical code.

---

## Layer clipping

`ClipLayer` wraps any inner layer and clips its rendering to a
rectangular region. Two clipping modes are available:

- **`ClipRegion::Rect(rect)`** — clip to an explicit rectangle in
  target-space coordinates.
- **`ClipRegion::LayerBounds`** — clip to the inner layer's own
  [`Layer::bounds`] (falls back to no clipping if the inner layer
  reports `None` bounds).

### Quick start

```rust
use termcompositor::{ClipLayer, ClipRegion, RectLayer};
use termcompositor::geometry::Rect;

let inner = RectLayer::new(0, 0, 40, 20, [255, 0, 0, 255]);
let clip = ClipLayer::new(Box::new(inner))
    .with_region(ClipRegion::Rect(Rect::new(5, 5, 10, 10)));
```

Or clip to the inner layer's own bounds:

```rust
let clip = ClipLayer::new(Box::new(inner))
    .with_region(ClipRegion::LayerBounds);
```

### How clipping works

The compositor renders the inner layer into a full-size temporary
buffer, then copies only the pixels within the clip region to the
target. This is the same approach used by `DropShadow`.

---

## Rounded corners

`RectLayer` supports rounded corners via the `border_radius` field
and `with_border_radius(radius)` builder method.

```rust
use termcompositor::RectLayer;

let rect = RectLayer::new(5, 5, 40, 20, [255, 0, 0, 255])
    .with_border_radius(8);
```

When `radius > 0`, the four corners are clipped to circular arcs.
The effective radius is clamped to `min(width, height) / 2`.

| Method | Description |
|---|---|
| `RectLayer::new(…)` | Creates a rectangle with `border_radius = 0` (sharp corners). |
| `.with_border_radius(r)` | Sets the corner radius in pixels. |

---

## Shadow and glow effects

`DropShadow` (aliased as `ShadowLayer`) wraps any inner layer and
adds a blurred shadow behind it. New in v0.15.0: **spread** for
shadow dilation/erosion, and **glow** for centered light effects.

### Drop shadow

```rust
use termcompositor::{DropShadow, RectLayer};

let inner = RectLayer::new(5, 5, 10, 5, [255, 255, 255, 255]);
let shadow = DropShadow::new(Box::new(inner))
    .with_offset(2, 2)
    .with_blur(3)
    .with_shadow_color([0, 0, 0, 128]);
```

### Spread

Positive spread dilates (expands) the shadow shape before blurring;
negative spread erodes (shrinks) it.

```rust
let shadow = DropShadow::new(Box::new(inner))
    .with_spread(3)   // expand shadow by 3px
    .with_blur(2)
    .with_shadow_color([0, 0, 0, 100]);
```

### Glow

A glow is a centered shadow with zero offset. Use the `with_glow`
convenience builder:

```rust
let glow = DropShadow::new(Box::new(inner))
    .with_glow([255, 200, 0, 200], 4);  // color, blur radius
```

This is equivalent to:

```rust
let glow = DropShadow::new(Box::new(inner))
    .with_shadow_color([255, 200, 0, 200])
    .with_offset(0, 0)
    .with_blur(4);
```

### API reference

| Method | Description |
|---|---|
| `DropShadow::new(inner)` | Default shadow: offset (2,2), blur 2, black 80% alpha. |
| `.with_offset(x, y)` | Shadow displacement in pixels. |
| `.with_blur(radius)` | Box blur radius. |
| `.with_shadow_color(rgba)` | Shadow/glow colour. |
| `.with_spread(pixels)` | Dilate (+) or erode (−) the shadow shape before blur. |
| `.with_glow(color, blur)` | Convenience: bright colour + zero offset + blur. |

`ShadowLayer` is a type alias for `DropShadow`.

---


---

## Accessibility metadata

Layers can carry accessibility metadata (alt-text and a semantic role)
for screen readers, headless terminals, and other assistive
technologies.

### Quick start

```rust
use termcompositor::{AccessibilityMetadata, SemanticRole, LayerEntry, RectLayer};

let rect = RectLayer::new(0, 0, 10, 5, [255, 0, 0, 255]);
let entry = LayerEntry::new(0, Box::new(rect))
    .with_accessibility(
        AccessibilityMetadata::new()
            .with_alt_text("Status indicator")
            .with_role(SemanticRole::Status)
    );

assert_eq!(entry.accessibility().unwrap().alt_text(), Some("Status indicator"));
assert_eq!(entry.accessibility().unwrap().role(), SemanticRole::Status);
```

### SemanticRole

| Role | Description |
|---|---|
| `None` | No specific role; the layer is decorative (default). |
| `Text` | A text label or heading. |
| `Button` | A button or interactive element. |
| `Image` | An image or icon. |
| `Container` | A container grouping child layers. |
| `Separator` | A separator or divider. |
| `Status` | A progress indicator or status display. |
| `Navigation` | A navigation element. |
| `Custom(&str)` | A custom role with a static string label. |

### API reference

| Method | Description |
|---|---|
| `AccessibilityMetadata::new()` | Create empty metadata (no alt-text, `SemanticRole::None`). |
| `.with_alt_text(text)` | Builder: set the alt-text. |
| `.with_role(role)` | Builder: set the semantic role. |
| `entry.accessibility()` | Returns `Option<&AccessibilityMetadata>`. |
| `entry.accessibility_mut()` | Returns `&mut AccessibilityMetadata` (creates default if none). |
| `entry.set_accessibility(Some(meta))` | Set or clear the metadata. |
| `entry.with_accessibility(meta)` | Builder on `LayerEntry`. |

### Notes

- `accessibility()` returns `None` when no metadata is set.
- `accessibility_mut()` always returns a mutable reference — if
  none exists, a default (`alt_text: None`, `role: None`) is
  created automatically.
- `SemanticRole::Custom` uses `&'static str` to avoid runtime
  allocation; for dynamic roles, store the string in a
  `Box::leak` or `OnceLock`.

---

## CLI usage

```bash
# Default: auto-detect terminal size and protocol
termcompositor

# Override protocol
termcompositor --protocol kitty
termcompositor --protocol sixel
termcompositor --protocol auto

# Use I/O-based Kitty probe (requires kitty-encoder feature)
termcompositor --probe

# Enable tmux passthrough
termcompositor --tmux-passthrough
```

The CLI:
1. Detects the terminal size.
2. Builds a multi-layer stack (background + rectangle + text).
3. Renders into a framebuffer.
4. Encodes via the chosen protocol (auto-detected by default).
5. Writes the escape sequences to stdout.

Human-readable diagnostics are written to stderr:

```
$ termcompositor
termcompositor v0.12.0 -- multi-layer + auto-detect encoder: host terminal = 80 cols x 24 rows
background: SolidColor([0, 0, 64, 255])
rect:      RectLayer at (20,6) 40x12 [0,200,0,200] z=10
label:     TextLayer at (2,1) "termcompositor" z=20
requested protocol: auto; resolved: sixel
encoded 142 bytes via sixel; writing to stdout
```

---

## Feature flags

| Feature | Default | Description |
|---|---|---|
| `font-rasterizer` | **on** | Real glyph rasterization in `TextLayer` via `fontdue`. Bundles Fira Mono Regular (~174KB). |
| `kitty-encoder` | off | Kitty graphics protocol encoder via `little-kitty`. |
| `sixel-encoder` | off | Sixel encoder via `icy_sixel`. |
| `image-decoder` | off | `ImageLayer` support (PNG + JPEG) via the `image` crate. |

Enable at least one encoder feature to produce terminal output. With neither, `dispatch_to_writer` returns `UnsupportedProtocol`.

---

## Protocol auto-detection

`termcompositor::detect()` reads `TERM`, `TERM_PROGRAM`, and `COLORTERM` from the process environment and returns a `Protocol` suggestion.

Priority order:

1. **`TERM_PROGRAM`** (set by the terminal app): `kitty`, `wezterm`, `ghostty` → `Protocol::Kitty`.
2. **`TERM`**: `xterm-kitty`, `foot`, `foot-*` → `Protocol::Kitty`; `tmux`, `tmux-*` → `Protocol::Sixel` (or `Protocol::Kitty` when `TMUXPASSTHROUGH` is set).
3. **`COLORTERM`**: `truecolor` / `24bit` → `Protocol::Kitty` when TERM is inconclusive.
4. **Default**: `Protocol::Sixel`.

`Protocol::Auto` defers to `detect()` at encode time.

For authoritative detection (I/O-based Kitty query-response probe), use `detect_with_probe()` (requires `kitty-encoder` feature):

```rust
use termcompositor::detect_with_probe;
let protocol = detect_with_probe().unwrap_or_else(|_| detect());
```

---

## Encoder features

### Kitty encoder (`kitty-encoder`)

The Kitty graphics protocol encoder supports:

- **Single-chunk encode** (no `m` key) for framebuffers ≤ 768 RGBA pixels — byte-for-byte compatible with the v0.8.0 wire format.
- **Chunked encode** (`m=1`/`m=0`) for larger framebuffers, transparently splitting into 4096-byte base64 chunks as required by the Kitty spec.
- **Streaming encode** via `encoder::kitty::encode_to_writer` — O(1) memory per chunk (~4KB scratch), regardless of framebuffer size.
- **Tmux passthrough** via `wrap_for_tmux`, `wrap_for_tmux_to_writer`, and `PassthroughWriter`.

### Sixel encoder (`sixel-encoder`)

The Sixel encoder provides two paths:

- **`encode_to_writer`** — uses `icy_sixel` for adaptive color quantization. Higher image quality for photos; materialises the input RGBA `Vec<u8>`.
- **`encode_to_writer_streaming`** — uses a fixed xterm-256 palette with a 5-bit LUT. Fully O(1) memory, no full-framebuffer allocation. Slightly lower quality for photos, but identical for UI/dashboards.

---

## Choosing the right encode path

`termcompositor` has three encode paths with different memory and quality
profiles. Here's how to pick the right one for your use case.

### Quick decision guide

| You are encoding… | Recommended path | Reason |
|---|---|---|
| A terminal-sized framebuffer (≤ 80×24 cells) | `dispatch_to_writer(Protocol::Auto, …)` | The framebuffer is tiny; any path works. `Auto` picks the best protocol for your terminal. |
| A large image or full-screen UI, quality matters | `dispatch_to_writer(Protocol::Sixel, …)` | Uses `icy_sixel`'s adaptive colour quantiser. Higher quality for photos. Materialises ~8MB of input RGBA for a 1920×1080 frame — fine on a desktop. |
| A large image or full-screen UI, memory is tight | `dispatch_to_writer(Protocol::Kitty, …)` on Kitty terminals, or `sixel::encode_to_writer_streaming` on Sixel terminals | Both use O(1) memory regardless of framebuffer size (~7KB scratch for Kitty, ~32KB one-time LUT for Sixel streaming). |
| You have no idea what terminal your users have | `dispatch_to_writer(Protocol::Auto, …)` | Calls `detect()` to pick the best protocol, then the best available path. Enable **both** encoder features so every user gets real output. |

### What the numbers mean

**O(1) memory** means the encoder uses a fixed amount of scratch space
regardless of how many pixels the framebuffer has. A 1 MP image and a
100 MP image cost the same in temporary allocations. This is important
for embedded devices, long-running servers, or applications that encode
many frames per second.

**O(N) memory** means the encoder allocates space proportional to the
framebuffer size. A 1920×1080 frame needs ~8MB of RGBA pixels before
encoding can start. This is still fine on modern machines — the
allocation is freed as soon as `encode_to_writer` returns — but can
cause memory spikes on large framebuffers.

### Path walkthrough

#### Path 1: `dispatch_to_writer` (the easy button)

```rust
use termcompositor::{dispatch_to_writer, detect, FrameBuffer};

let fb = FrameBuffer::new(1920, 1080);
let mut stdout = std::io::stdout();
dispatch_to_writer(Protocol::Auto, &fb, &mut stdout)?;
```

This calls `detect()` to figure out the terminal protocol, then delegates
to the correct encoder. If the terminal supports Kitty, you get the O(1)
Kitty path. If it only supports Sixel, you get the adaptive `icy_sixel`
path (O(N) input). One function call, best-effort behaviour.

#### Path 2: Kitty direct (O(1) memory, requires `kitty-encoder`)

```rust
use termcompositor::encoder::kitty;

kitty::encode_to_writer(&fb, &mut stdout)?;
```

Works only on terminals that support the Kitty graphics protocol. The
encoder processes the framebuffer in 768-pixel chunks, keeping peak
memory at ~7KB regardless of framebuffer size. Ideal for:
- Continuous rendering loops (game, dashboard, animation)
- Very large framebuffers (multi-megapixel)
- Memory-constrained environments

#### Path 3: Sixel via `icy_sixel` (O(N) input, requires `sixel-encoder`)

```rust
use termcompositor::encoder::sixel;

sixel::encode_to_writer(&fb, &mut stdout)?;
```

Uses `icy_sixel`'s adaptive colour quantizer for the best Sixel image
quality. The encoder collects all RGBA pixels into a `Vec<u8>` before
encoding, so a 1920×1080 frame allocates ~8MB of temporary RGBA data.
Best for:
- One-shot renders where quality matters (screenshots, static images)
- Desktop applications with ample memory

#### Path 4: Sixel streaming (O(1) memory, requires `sixel-encoder`)

```rust
use termcompositor::encoder::sixel;

sixel::encode_to_writer_streaming(&fb, &mut stdout)?;
```

Uses a fixed xterm-256 palette instead of adaptive quantization. Reads
pixels directly from the `FrameBuffer` without copying them into a
temporary buffer. The trade-off: slightly lower colour accuracy for
photographic images, identical quality for solid-colour UI.

**One-time cost**: a 32KB lookup table is built on the first call (takes
~100ms). After that, per-pixel quantisation is a single table lookup.

### How to test which path is best for you

```bash
# Kitchen-sink build with everything enabled
cargo build --release --features kitty-encoder,sixel-encoder

# Run the benchmarks to see real allocation numbers
cargo bench --all-features
```

Then try each encoder path in your application with realistic
framebuffer sizes and measure memory with a profiler.

---

## Tmux passthrough

When the host is running inside tmux, the Kitty graphics protocol's APC payload must be wrapped in a tmux passthrough DCS to survive the tmux → outer-terminal hop.

Enable with:

```bash
export TMUXPASSTHROUGH=1
termcompositor
```

Or per-invocation:

```bash
TMUXPASSTHROUGH=1 termcompositor
```

Or via the CLI flag:

```bash
termcompositor --tmux-passthrough
```

**Requirements:**
- tmux 3.2+ (released 2021).
- `set -g allow-passthrough on` in `~/.tmux.conf`.

Both the `TMUXPASSTHROUGH` env var and the `TMUX` env var must be set for the wrapping to apply. This prevents double-wrapping on non-tmux hosts.

Programmatic usage:

```rust
use termcompositor::wrap_for_tmux;

let raw_kitty = Protocol::Kitty.encode(&fb)?;
let tmux_safe = wrap_for_tmux(raw_kitty);
std::io::stdout().write_all(&tmux_safe)?;
```

Or use the end-to-end streaming entry point for O(1) memory:

```rust,ignore
use termcompositor::encoder::kitty;

kitty::encode_passthrough_to_writer(&fb, &mut stdout)?;
```

---

## Troubleshooting

### "Protocol not supported" error

**Error message**: `protocol kitty is not supported in this build` or
`protocol sixel is not supported in this build`

**Cause**: You called `dispatch_to_writer` or `Protocol::Kitty.encode()` without
enabling the corresponding Cargo feature in your `Cargo.toml`.

**Fix**: Add the missing feature:

```toml
[dependencies]
termcompositor = { version = "0.12", features = ["kitty-encoder", "sixel-encoder"] }
```

Enable at least one encoder feature. If you want auto-detection to work for all
users, enable **both**.

### Nothing appears on screen

**Symptoms**: Your code runs without errors, but the terminal shows nothing.

**Checklist**:
1. Are you writing the encoded bytes to stdout? `dispatch_to_writer` writes to
   a `&mut impl Write` sink — pass `&mut std::io::stdout()` (or a `BufWriter`
   wrapping it) to see output.
2. Is at least one encoder feature (`kitty-encoder` or `sixel-encoder`)
   enabled? Without one, `dispatch_to_writer` returns `UnsupportedProtocol`.
3. Does your terminal support the protocol? Run `echo $TERM` and
   `echo $TERM_PROGRAM`. If you're on a plain xterm, it likely only supports
   Sixel. Build with `sixel-encoder` and use `Protocol::Sixel` or
   `Protocol::Auto`.
4. If you're inside tmux without `TMUXPASSTHROUGH`, the default protocol is
   Sixel. Build with `sixel-encoder` or set `TMUXPASSTHROUGH=1` (see
   [Tmux passthrough](#tmux-passthrough)).

### "Invalid dimensions" error

**Error message**: `framebuffer has invalid dimensions: 0x5`

**Cause**: You created a `FrameBuffer` with width or height equal to zero.

**Fix**: Ensure both dimensions are ≥ 1:

```rust
let fb = FrameBuffer::new(80, 24);  // both > 0
let fb = FrameBuffer::new(1, 1);     // minimum viable size
let fb = FrameBuffer::new(0, 0);     // would error
```

If you're using `TerminalSize::detect()`, handle the `None` case or use
`TerminalSize::current()` (which falls back to 80×24 when detection fails).

### Tmux passthrough not working

**Symptoms**: Kitty output is garbled inside tmux. Sixel falls back to the
Kitty APC wrapping, or nothing appears.

**Checklist**:
1. **Is `TMUXPASSTHROUGH` set?** The env var is the opt-in. Run
   `echo $TMUXPASSTHROUGH`. If empty, set it:
   ```bash
   export TMUXPASSTHROUGH=1
   ```
2. **Are you inside tmux?** The `TMUX` env var must be set (tmux sets it
   automatically). Run `echo $TMUX`. If empty, you are not in a tmux session.
3. **Is tmux 3.2+?** tmux < 3.2 cannot forward APC payloads. Run
   `tmux -V`. Upgrade if needed.
4. **Is `allow-passthrough on` set?** Add this to `~/.tmux.conf`:
   ```
   set -g allow-passthrough on
   ```
   Then restart tmux or run `tmux kill-server` and reconnect.
5. **Is the `kitty-encoder` feature enabled?** The tmux passthrough wrapping
   only applies to Kitty output. Both `TMUXPASSTHROUGH` and `kitty-encoder`
   must be present.

### Text renders as solid coloured blocks

**Symptoms**: `TextLayer` shows coloured rectangles where text should be.

**Cause**: The `font-rasterizer` Cargo feature is disabled. Without it,
`TextLayer` renders a solid-block placeholder (one cell per Unicode scalar
value) for layout verification.

**Fix**: Enable the feature:

```toml
[dependencies]
termcompositor = { version = "0.12", default-features = false, features = ["font-rasterizer"] }
```

`font-rasterizer` is enabled by default, so plain `termcompositor = "0.12"`
already includes it. If you're using `default-features = false`, add it back
manually.

### ImageLayer compile error

**Error**: `ImageLayer` is not found or the `from_path` method is missing.

**Cause**: The `image-decoder` Cargo feature is not enabled.

**Fix**:

```toml
[dependencies]
termcompositor = { version = "0.12", features = ["image-decoder"] }
```

### Auto-detection picks the wrong protocol

**Symptoms**: `detect()` returns `Protocol::Sixel` when your terminal supports
Kitty, or vice versa.

**Check the env vars**:

```bash
# What does detect() see?
echo "TERM_PROGRAM=$TERM_PROGRAM"
echo "TERM=$TERM"
echo "COLORTERM=$COLORTERM"
```

`detect()` uses a fixed priority:
1. `TERM_PROGRAM` wins: `kitty`, `wezterm`, `ghostty` → `Protocol::Kitty`
2. `TERM`: `xterm-kitty`, `foot`, `foot-*` → `Kitty`; `tmux`, `tmux-*` →
   `Sixel` (or `Kitty` when `TMUXPASSTHROUGH` is set)
3. `COLORTERM` tiebreaker: `truecolor` / `24bit` → `Kitty` when the above are
   inconclusive
4. Default: `Sixel`

If your terminal sets an unusual `TERM_PROGRAM` that `detect()` doesn't
recognise, override with the specific protocol:

```rust
// Skip auto-detection; use the protocol you know works.
let protocol = Protocol::Kitty;
```

### Memory spikes on large framebuffers

**Symptoms**: The encode call allocates many megabytes of RAM for a
1920×1080 framebuffer, causing memory spikes.

**Cause**: Using the `icy_sixel`-based `sixel::encode_to_writer` path, which
materialises the full RGBA pixel buffer before encoding.

**Fix**: Switch to a streaming (O(1) memory) path:

| Your terminal supports… | Use this path |
|---|---|
| Kitty graphics protocol | `encoder::kitty::encode_to_writer` |
| Sixel only | `encoder::sixel::encode_to_writer_streaming` |
| Either (auto-detect) | `dispatch_to_writer(Protocol::Auto, …)` — on Kitty terminals you get the O(1) Kitty path |

See [Choosing the right encode path](#choosing-the-right-encode-path) for
the full decision guide.

### "Encode failed" error

**Error message**: `encoder failed: <underlying error>`

**Cause**: The underlying encoder crate (`little-kitty` or `icy_sixel`)
returned an error. This is rare and usually indicates an internal issue
in those libraries rather than a misuse of `termcompositor`.

**What to do**:
1. Check the inner error message for clues.
2. Verify the framebuffer contains valid pixel data.
3. If the error is reproducible, open an issue on the upstream crate
   (`little-kitty` or `icy_sixel`).

### CLI error handling

The CLI binary is designed to never crash. If encoding fails (e.g. no
encoder feature is enabled), it prints a descriptive error to stderr and
exits with code 0:

```
$ ./target/release/termcompositor  # default build, no encoder features
termcompositor v0.12.0 -- multi-layer + auto-detect encoder: host terminal = 80 cols x 24 rows
...
encoder error for protocol sixel: protocol sixel is not supported in this build (is the required Cargo feature enabled?)
```

Build with `--features kitty-encoder,sixel-encoder` to get real output.

---

## Benchmarks

Run the benchmark suite with:

```bash
cargo bench
```

For the full suite (including feature-gated encoder benchmarks):

```bash
cargo bench --all-features
```

The 19 Criterion benchmarks cover:
- Framebuffer allocation, clear, blend, and pixel access.
- Solid colour, rect, and text layer rendering.
- Multi-layer stack compositing.
- Kitty and Sixel encoder paths (when the corresponding features are enabled).

---

## Minimum supported Rust version

**MSRV: 1.73**. This is pinned in `Cargo.toml` via `rust-version = "1.73"` and validated in CI. The MSRV is driven by `div_ceil` (stable since 1.73) used in the chunked Kitty encoder and streaming Sixel band loop.
