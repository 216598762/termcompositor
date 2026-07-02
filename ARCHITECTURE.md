# Architecture

`dashcompositor` is a **layer-based graphics compositor for the terminal**. It keeps an in-memory stack of layers, composites them into a single off-screen RGBA framebuffer, and projects the result to a terminal emulator via the Kitty graphics protocol or Sixel.

---

## Full stack

```
┌────────────┐  ┌────────────┐  ┌────────────┐  ┌────────────┐
│  Layer N   │  │  Layer …   │  │  Layer 1   │  │  Layer 0   │
└─────┬──────┘  └─────┬──────┘  └─────┬──────┘  └─────┬──────┘
      └───────────────┴───────────────┴───────────────┘
                              │ composite()
                              ▼
                    ┌─────────────────────┐
                    │   FrameBuffer       │
                    │   (RGBA pixels)     │
                    └─────────┬───────────┘
                              │ encode()
              ┌───────────────┴───────────────┐
              ▼                               ▼
   ┌────────────────────┐          ┌────────────────────┐
   │ Kitty graphics     │          │ Sixel              │
   │ protocol encoder   │          │ encoder            │
   └─────────┬──────────┘          └─────────┬──────────┘
             ▼                               ▼
                       terminal stdout
```

Each stage maps to a public module:

| Stack layer | Module | Central types |
|---|---|---|
| Layer types | [`layer`] | `Layer` trait, `SolidColor`, `RectLayer`, `TextLayer`, `ImageLayer` |
| Layer storage | [`compositor`] | `LayerStack`, `LayerEntry`, `LayerId` |
| Compositing | [`compositor`] | `Compositor` trait, `CpuCompositor` |
| Pixel buffer | [`framebuffer`] | `FrameBuffer`, `blend_over` |
| Encoding | [`encoder`] | `Protocol`, `ProtocolEncoder` trait, `EncoderError`, `dispatch_to_writer` |
| Terminal detection | [`terminal`] | `TerminalSize` |
| Geometric primitives | [`geometry`] | `Rect` |

---

## Public API surface

### Layer stack

The [`LayerStack`] is the core orchestrator. Backend code pushes layers, receives stable [`LayerId`] handles, and can manipulate entries at any time.

```rust
let mut stack = LayerStack::new();

// Push layers; each returns a stable LayerId.
let bg = stack.push(SolidColor::new(0, 0, 64, 255).with_name(\"bg\"));
let rect = stack.push(RectLayer::new(10, 5, 20, 10, [0, 255, 0, 200]));

// Manipulate entries by id.
stack.get_mut(bg).unwrap().set_visible(false);
stack.get_mut(rect).unwrap().set_opacity(0.5);

// Remove and re-add.
stack.remove(bg);
let new_bg = stack.push(SolidColor::new(10, 10, 10, 255));
```

Key methods on `LayerStack`:

| Method | Description |
|---|---|
| `push(layer)` | Add a layer, returns `LayerId` |
| `remove(id)` | Remove by id, returns the entry |
| `get(id)` / `get_mut(id)` | Access entry (read-only / mutable) |
| `render(fb)` | Composite into a `FrameBuffer` using the default `CpuCompositor` |
| `render_with(fb, compositor)` | Composite with a custom `Compositor` |
| `render_to_current_terminal()` | Detect terminal size, create `FrameBuffer`, render, return `(FrameBuffer, TerminalSize)` |
| `len()` / `is_empty()` | Stack size queries |
| `entries()` / `entries_mut()` | Iterate all entries (insertion order) |
| `iter_sorted()` | Iterate sorted by effective z-order |
| `index_of(id)` | Find insertion index for a given id |
| `reorder(from, to)` | Move an entry to a new position |
| `clear()` | Remove all entries |

### Compositor trait

The [`Compositor`] trait controls how layers are combined:

```rust
pub trait Compositor {
    fn compose(&self, stack: &LayerStack, fb: &mut FrameBuffer);
}
```

The default [`CpuCompositor`] sorts visible entries by effective z-order (stable on ties) and calls each layer's `render` with its opacity. Implement your own for custom blending, concurrency, or GPU offload.

### Layer types

All built-in layers implement the [`Layer`] trait.

| Type | Constructor | Notes |
|---|---|---|
| [`SolidColor`] | `SolidColor::new(r, g, b, a)` | Fills the entire framebuffer with a single RGBA colour. Has no position. |
| [`RectLayer`] | `RectLayer::new(x, y, w, h, [r, g, b, a])` | Positioned rectangle with colour. Clipped to framebuffer bounds. |
| [`TextLayer`] | `TextLayer::new(x, y, text, [r, g, b, a])` | UTF-8 text rendered with the bundled Fira Mono font (when `font-rasterizer` is enabled) or a solid-block placeholder (without). |
| [`ImageLayer`] | `ImageLayer::from_path(path)` (requires `image-decoder` feature) | Decodes PNG and JPEG images. |

Common builders (chainable):

| Builder | Applies to | Effect |
|---|---|---|
| `.with_z(z)` | All | Z-order (higher = on top) |
| `.with_name(name)` | All | Debug label |
| `.with_opacity(opacity)` | All (via entry) | Per-layer opacity multiplier |
| `.with_font(src, size)` | `TextLayer` | Custom font source and pixel size |
| `.with_font_size(size)` | `TextLayer` | Change pixel size only |

### Framebuffer

The [`FrameBuffer`] is a 2D RGBA pixel array — the compositor's output target:

```rust
let mut fb = FrameBuffer::new(80, 24);
fb.clear([0, 0, 0, 255]);          // Fill with a colour
fb.pixels()                        // &[[u8; 4]] — all pixels
fb.pixels_mut()                    // &mut [[u8; 4]] — mutable
fb.get_pixel(x, y)                 // Option<&[u8; 4]> — bounds-checked read
fb.get_pixel_mut(x, y)             // Option<&mut [u8; 4]> — bounds-checked write
fb.width() / fb.height()           // Dimensions in pixels
blend_over(src, dst)               // Straight-alpha over-compositing
```

### Encoder

The encoder module converts a `FrameBuffer` into terminal escape sequences:

- [`Protocol`] — enum with `Kitty`, `Sixel`, and `Auto` variants.
- [`ProtocolEncoder`] trait — `encode(&self, frame: &FrameBuffer) -> Result<Vec<u8>, EncoderError>`.
- [`dispatch_to_writer`] — one-shot streaming encode + write to any `&mut impl Write` sink.
- [`detect`] — pure env-var-based protocol detection (`TERM` / `TERM_PROGRAM` / `COLORTERM`).
- [`detect_with_probe`] — I/O-based Kitty query-response probe (requires `kitty-encoder` feature).
- [`EncoderError`] — `UnsupportedProtocol`, `InvalidDimensions`, `Encode(String)`.

```rust
// Encode and write in one call:
let mut out = Vec::new();
dispatch_to_writer(Protocol::Auto, &fb, &mut out)?;

// Or use the trait for a Vec<u8>:
let bytes = Protocol::Kitty.encode(&fb)?;
```

Streaming entry points (feature-gated):

| Function | Feature | Memory |
|---|---|---|
| `encoder::kitty::encode_to_writer` | `kitty-encoder` | O(1) per chunk (~4KB) |
| `encoder::sixel::encode_to_writer` | `sixel-encoder` | O(N) input + O(1) output |
| `encoder::sixel::encode_to_writer_streaming` | `sixel-encoder` | O(1) total |

### Protocol auto-detection

`detect()` uses the following heuristic priority:

1. **`TERM_PROGRAM`** (most specific): `kitty`, `wezterm`, `ghostty` → Kitty.
2. **`TERM`**: `xterm-kitty`, `foot`, `foot-*` → Kitty; `tmux`, `tmux-*` → Sixel (or Kitty when `DASHPASSTHROUGH` is set).
3. **`COLORTERM`** tiebreaker: `truecolor` / `24bit` → Kitty when TERM is inconclusive.
4. **Default**: Sixel (most universal fallback).

`Protocol::Auto` defers to `detect()` at encode time.

### Tmux passthrough

When the host is inside tmux, set `DASHPASSTHROUGH=1` to wrap Kitty APC output in a tmux passthrough DCS (`ESC P tmux ; ... ESC \`). Requires `set -g allow-passthrough on` in `~/.tmux.conf` (tmux 3.2+).

```rust
use dashcompositor::wrap_for_tmux;
let tmux_safe = wrap_for_tmux(kitty_bytes);
```

---

## Feature flags

| Feature | Default | Pulls in | Enables |
|---|---|---|---|
| `font-rasterizer` | **on** | `fontdue` | Real glyph rendering in `TextLayer` |
| `kitty-encoder` | off | `little-kitty` | Kitty graphics protocol output |
| `sixel-encoder` | off | `icy_sixel` | Sixel output |
| `image-decoder` | off | `image` | `ImageLayer` (PNG + JPEG) |

Enable at least one encoder feature (`kitty-encoder` or `sixel-encoder`) to produce terminal output. Without either, `dispatch_to_writer` returns `UnsupportedProtocol`.

---

## Design decisions

- **No I/O in encoders**: The `ProtocolEncoder::encode` trait method never performs I/O. It returns a `Vec<u8>` of escape sequences; the caller writes them to stdout. The streaming entry points (`encode_to_writer`, `dispatch_to_writer`) accept a `&mut impl Write` sink for zero-copy output.
- **Per-layer opacity**: Opacity is stored on the `LayerEntry` wrapper, not on the layer itself. This lets the composite-level opacity (layer stack entry) and the layer-level alpha (`[r, g, b, a]`) compose: the effective alpha is `entry.opacity * (a / 255.0)`.
- **Stable LayerId**: `push` returns a monotonically increasing `usize` that is never reused. Removing a layer and pushing a new one gives a new, unique id. External references to a removed layer remain safe (access returns `None`).
- **`CpuCompositor` is a reference implementation**: It's zero-dependency, single-threaded, and correct. Swap it out for a custom compositor via `LayerStack::render_with` without changing your layer setup.
- **Streaming encode is the default path**: Even the `Vec<u8>`-returning `encode` functions delegate internally to the streaming entry points. Memory efficiency is built in.

[`Layer`]: https://docs.rs/dashcompositor/latest/dashcompositor/trait.Layer.html
[`LayerStack`]: https://docs.rs/dashcompositor/latest/dashcompositor/struct.LayerStack.html
[`LayerEntry`]: https://docs.rs/dashcompositor/latest/dashcompositor/struct.LayerEntry.html
[`LayerId`]: https://docs.rs/dashcompositor/latest/dashcompositor/type.LayerId.html
[`Compositor`]: https://docs.rs/dashcompositor/latest/dashcompositor/trait.Compositor.html
[`CpuCompositor`]: https://docs.rs/dashcompositor/latest/dashcompositor/struct.CpuCompositor.html
[`SolidColor`]: https://docs.rs/dashcompositor/latest/dashcompositor/struct.SolidColor.html
[`RectLayer`]: https://docs.rs/dashcompositor/latest/dashcompositor/struct.RectLayer.html
[`TextLayer`]: https://docs.rs/dashcompositor/latest/dashcompositor/struct.TextLayer.html
[`ImageLayer`]: https://docs.rs/dashcompositor/latest/dashcompositor/struct.ImageLayer.html
[`FrameBuffer`]: https://docs.rs/dashcompositor/latest/dashcompositor/struct.FrameBuffer.html
[`Protocol`]: https://docs.rs/dashcompositor/latest/dashcompositor/enum.Protocol.html
[`ProtocolEncoder`]: https://docs.rs/dashcompositor/latest/dashcompositor/trait.ProtocolEncoder.html
[`EncoderError`]: https://docs.rs/dashcompositor/latest/dashcompositor/enum.EncoderError.html
[`dispatch_to_writer`]: https://docs.rs/dashcompositor/latest/dashcompositor/fn.dispatch_to_writer.html
[`detect`]: https://docs.rs/dashcompositor/latest/dashcompositor/encoder/fn.detect.html
[`detect_with_probe`]: https://docs.rs/dashcompositor/latest/dashcompositor/encoder/fn.detect_with_probe.html
[`wrap_for_tmux`]: https://docs.rs/dashcompositor/latest/dashcompositor/fn.wrap_for_tmux.html
[`layer`]: https://docs.rs/dashcompositor/latest/dashcompositor/layer/index.html
[`compositor`]: https://docs.rs/dashcompositor/latest/dashcompositor/compositor/index.html
[`framebuffer`]: https://docs.rs/dashcompositor/latest/dashcompositor/framebuffer/index.html
[`encoder`]: https://docs.rs/dashcompositor/latest/dashcompositor/encoder/index.html
[`terminal`]: https://docs.rs/dashcompositor/latest/dashcompositor/terminal/index.html
[`geometry`]: https://docs.rs/dashcompositor/latest/dashcompositor/geometry/index.html
