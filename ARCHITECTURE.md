# Architecture

`termcompositor` is a **layer-based graphics compositor for the terminal**. It keeps an in-memory stack of layers, composites them into a single off-screen RGBA framebuffer, and projects the result to a terminal emulator via the Kitty graphics protocol or Sixel.

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
fb.clear();                        // Reset all pixels to transparent black
fb.pixels()                        // &[[u8; 4]] — all pixels
fb.pixels_mut()                    // &mut [[u8; 4]] — mutable
fb.get_pixel(x, y)                 // Option<&[u8; 4]> — bounds-checked read
fb.get_pixel_mut(x, y)             // Option<&mut [u8; 4]> — bounds-checked write
fb.width() / fb.height()           // Dimensions in pixels
blend_over(dst, src, src_alpha)    // Straight-alpha over-compositing
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

### Streaming encode memory model

The streaming entry points are the default encode path. Even the `Vec<u8>`-returning
`encode` functions internally delegate to them. This section explains the memory
behaviour of each path, from best (O(1) regardless of framebuffer size) to worst
(O(N) in both input and output).

#### Overview

| Function | Feature | Peak working set | Dominant allocation |
|---|---|---|---|
| `encoder::kitty::encode_to_writer` | `kitty-encoder` | O(1) per chunk (~4KB scratch) | Per-chunk pixel slice of ≤ 3072 bytes |
| `encoder::sixel::encode_to_writer_streaming` | `sixel-encoder` | O(1) total (~32KB one-time) | 32KB LUT (built once, shared) |
| `encoder::sixel::encode_to_writer` | `sixel-encoder` | O(N) input + O(1) output | Full RGBA `Vec<u8>` (e.g. 8MB for 1920×1080) |
| `encoder::kitty::encode` / `sixel::encode` | respective feature | O(N) in output | Full output `Vec<u8>` |
| `dispatch(Protocol, frame)` | respective feature | O(N) in output + optional wrap | Output + optional tmux wrap `Vec` |

#### Kitty streaming path (`encode_to_writer`)

**O(1) per chunk.** The Kitty graphics protocol limits each APC command to
4096 bytes of base64-encoded data. `termcompositor` encodes 768 RGBA pixels
(3072 raw bytes) per chunk — the exact amount that produces 4096 base64 chars
with no padding, satisfying the spec's alignment requirement.

Per chunk:
1. A slice of up to 768 pixels is taken from the `FrameBuffer`'s internal
   `Vec<[[u8; 4]]` — **no copy of the pixel data** at this stage.
2. The pixel slice is flattened into a `Vec<u8>` of RGBA bytes (≤ 3072 bytes).
   This is the **only per-chunk allocation**, and it's bounded regardless of
   framebuffer size.
3. The `little-kitty` crate's `KittyCommandWriter` base64-encodes the chunk
   into a temporary APC buffer (~4KB).
4. The APC buffer is written to the `&mut impl Write` sink and dropped.

For a 1920×1080 framebuffer (2,073,600 pixels), the encode produces ~2,701
chunks. Each chunk allocates and frees ~7KB total (3072 RGBA + ~4KB APC
framing), so peak working set never exceeds ~11KB regardless of the
framebuffer being 8MB+ in memory.

**Input memory**: The `FrameBuffer`'s pixel `Vec<[[u8; 4]]` is always in
memory — it was allocated during compositing. The streaming encoder does not
make its own copy or allocate any additional O(N) structure.

```rust
use termcompositor::encoder::kitty;

let fb = FrameBuffer::new(1920, 1080);
let mut out = Vec::new();               // grows with output size
kitty::encode_to_writer(&fb, &mut out)?; // O(1) scratch per chunk
```

#### Sixel adaptive path (`encode_to_writer`)

**O(N) input + O(1) output.** Uses the `icy_sixel` crate's adaptive colour
quantizer for high image quality. `icy_sixel` 0.5 takes owned RGBA bytes via
`SixelImage::from_rgba(Vec<u8>, w, h)` and has no streaming input API, so the
full RGBA byte buffer must be materialised first (8MB+ for a 2MP frame).

Once the RGBA bytes are collected, `icy_sixel`'s `encode()` produces a single
`String` of Sixel DCS bytes. The streaming win over the v0.6.0 path is that
this `String`'s internal buffer is **borrowed** via `as_bytes()` and written
directly to the `&mut impl Write` sink, rather than being **copied** into a
new `Vec<u8>` via `into_bytes()`.

```rust
use termcompositor::encoder::sixel;

let fb = FrameBuffer::new(1920, 1080);
let mut out = Vec::new();
sixel::encode_to_writer(&fb, &mut out)?; // 8MB+ RGBA alloc, then O(1) write
```

#### Sixel streaming path (`encode_to_writer_streaming`)

**Fully O(1) memory.** Written for the use case where even the single full-frame
RGBA allocation of the adaptive path is too expensive (e.g. multi-megapixel
dashboards on memory-constrained hardware). Uses a fixed xterm-256 palette
(16 basic + 6×6×6 cube + 24 grayscale) instead of adaptive quantization.

Allocations (one-time, shared across calls):
- A 5-bit RGB → palette index LUT: 32×32×32 = 32,768 entries × 1 byte =
  **32KB**, built lazily on first call via `OnceLock`. Computation: ~8M
  operations (~100ms). After init, per-pixel quantization is a single table
  lookup.

The palette itself (256 × 3 bytes = 768 bytes) is generated at **compile time**
via a `const fn` — zero runtime cost.

Per band (6 rows):
- No per-band heap allocations. Pixels are read directly from the `FrameBuffer`
  via `frame.pixels()` — a reference borrow with zero copy.
- A fixed-size `[u8; 6]` stack array tracks the six pixels in the current
  column.
- Output bytes are written directly to the `&mut impl Write` sink via
  `write!()` and `write_all()`, with no intermediate buffer.

```rust
use termcompositor::encoder::sixel;

let fb = FrameBuffer::new(1920, 1080);
let mut out = Vec::new();
sixel::encode_to_writer_streaming(&fb, &mut out)?; // zero full-frame allocs
```

#### dispatch_to_writer — end-to-end streaming

`dispatch_to_writer` combines all three paths into a single `Protocol`-based
dispatch that writes to a `&mut impl Write` sink. It is the recommended entry
point for most users:

```rust
use termcompositor::{dispatch_to_writer, detect, FrameBuffer};

let fb = FrameBuffer::new(1920, 1080);
let mut out = Vec::new();
dispatch_to_writer(Protocol::Auto, &fb, &mut out)?;
```

Memory behaviour depends on the resolved protocol:
- `Protocol::Kitty` → delegates to `kitty::encode_passthrough_to_writer`,
  which calls `encode_to_writer` (O(1) per chunk) and, if the tmux passthrough
  opt-in is set, wraps the output through a `PassthroughWriter` adapter
  (also O(1) — no intermediate `Vec`).
- `Protocol::Sixel` → delegates to `sixel::encode_to_writer` (the
  `icy_sixel`-based path: O(N) input).
- `Protocol::Auto` → recurses via
  `dispatch_to_writer(detect(), frame, out)`. The recursion is bounded
  because `detect()` returns only `Kitty` or `Sixel` (never `Auto`).

#### When to use which

| Use case | Recommended path | Rationale |
|---|---|---|
| Terminal-sized framebuffer (≤ 80×24) | `Protocol::Auto` via `dispatch_to_writer` or `ProtocolEncoder::encode` | The framebuffer is tiny (≤ 7680 pixels); even the O(N) paths are trivial. |
| Multi-megapixel framebuffer, want maximum image quality | `encoder::sixel::encode_to_writer` or `Protocol::Sixel` | Adaptive quantization gives best quality for photos. Accept the O(N) input allocation. |
| Multi-megapixel framebuffer, memory-constrained | `encoder::kitty::encode_to_writer` (Kitty terminals) or `encoder::sixel::encode_to_writer_streaming` (Sixel terminals) | Both paths are O(1) regardless of framebuffer size. The Sixel streaming path uses a fixed palette — slightly lower quality for photos, identical for UI. |
| Any framebuffer size, don't want to think about it | `dispatch_to_writer(Protocol::Auto, &fb, &mut out)` | Uses `detect()` to pick the best protocol, then the best available path. |

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
use termcompositor::wrap_for_tmux;
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

## Error handling

The library uses a mix of `Result<T, EncoderError>` for fallible operations
and `Option<T>` for optional access patterns. Every public API entry point
that can fail documents what it returns and why.

### EncoderError

The single error type [`EncoderError`] covers all encode-path failures:

```rust
pub enum EncoderError {
    /// The requested protocol was not compiled into this build.
    /// E.g. calling `Protocol::Kitty.encode()` without the
    /// `kitty-encoder` Cargo feature.
    UnsupportedProtocol(&'static str),

    /// The framebuffer has zero width or height.
    InvalidDimensions { width: u32, height: u32 },

    /// The underlying encoder crate (`little-kitty` or
    /// `icy_sixel`) returned an error. Display output is
    /// forwarded verbatim.
    Encode(String),
}
```

| Variant | Display format | Trigger |
|---|---|---|
| `UnsupportedProtocol` | `"protocol {p} is not supported in this build"` | Calling `Kitty` or `Sixel` encode without the corresponding feature enabled |
| `InvalidDimensions` | `"framebuffer has invalid dimensions: {width}x{height}"` | Passing a zero-width or zero-height `FrameBuffer` to an encoder |
| `Encode` | `"encoder failed: {msg}"` | Underlying crate failure (e.g. `icy_sixel` colour quantisation failure) |

`EncoderError` implements `std::error::Error` and is the only error type
in the public API.

**Conversion impls** (feature-gated):

| `From` impl | Feature gate | Purpose |
|---|---|---|
| `From<std::io::Error>` | `kitty-encoder` or `sixel-encoder` | `?` on `Write` calls inside streaming encoders |
| `From<icy_sixel::SixelError>` | `sixel-encoder` | `?` on `icy_sixel` calls |

### Option-based fallible APIs

Several public APIs return `Option` instead of `Result` because the failure
is a lookup miss, not an invariant violation:

| API | Returns | Meaning of `None` |
|---|---|---|
| `LayerStack::get(id)` / `get_mut(id)` | `Option<&LayerEntry>` / `Option<&mut LayerEntry>` | The `LayerId` was removed or never existed |
| `LayerStack::remove(id)` | `Option<LayerEntry>` | The `LayerId` was not found |
| `LayerStack::index_of(id)` | `Option<usize>` | The `LayerId` is not in the stack |
| `FrameBuffer::get_pixel(x, y)` | `Option<&[u8; 4]>` | `(x, y)` is outside the framebuffer bounds |
| `FrameBuffer::get_pixel_mut(x, y)` | `Option<&mut [u8; 4]>` | `(x, y)` is outside the framebuffer bounds |
| `TerminalSize::detect()` | `Option<TerminalSize>` | Terminal size could not be determined (non-TTY stdout, unsupported platform) |

### Defaults and fallbacks

Where a failure would block the caller's pipeline, defaults are chosen
instead of propagating an error:

| API | Fallback | Justification |
|---|---|---|
| `TerminalSize::current()` | 80×24 (VT100 default) | Every compositor needs a size; 80×24 is universally understood |
| `FrameBuffer::new(w, h)` | Saturating multiplication on dimensions | Absurdly large inputs produce an empty buffer rather than overflow (zero-width/height buffers still produce an error at encode time) |

### CLI error handling

The CLI binary never panics. All error paths produce a descriptive message
on stderr and exit with code 0 (non-zero exit codes are reserved for
shell-level issues like broken pipes):

```
$ termcompositor  # default build, no encoder features
...
encoder error for protocol sixel: protocol sixel is not supported in this build \
(is the required Cargo feature enabled?)
```

Invalid CLI flags are also handled without panicking:

```
$ termcompositor --protocol unknown
warning: unknown --protocol value `unknown`; falling back to `auto`
```

### Non-panicking guarantees

No public API function panics under normal use. Specifically:

- **`FrameBuffer::new`** uses saturating arithmetic on dimensions, so no
  overflow panic for absurdly large inputs.
- **`encode` / `dispatch_to_writer`** never panics — all failure modes
  return `EncoderError`.
- **`LayerStack::push`** never panics — the internal `Vec` grows as needed.
- **`CpuCompositor::compose`** never panics — out-of-bounds pixel access is
  silently ignored (layers are clipped to framebuffer bounds).
- **`TerminalSize::current`** never panics — `detect()` returning `None`
  falls through to the 80×24 default.
- **`wrap_for_tmux`** never panics — writing to a `Vec<u8>` is infallible.

---

## Design decisions

- **No I/O in encoders**: The `ProtocolEncoder::encode` trait method never performs I/O. It returns a `Vec<u8>` of escape sequences; the caller writes them to stdout. The streaming entry points (`encode_to_writer`, `dispatch_to_writer`) accept a `&mut impl Write` sink for zero-copy output.
- **Per-layer opacity**: Opacity is stored on the `LayerEntry` wrapper, not on the layer itself. This lets the composite-level opacity (layer stack entry) and the layer-level alpha (`[r, g, b, a]`) compose: the effective alpha is `entry.opacity * (a / 255.0)`.
- **Stable LayerId**: `push` returns a monotonically increasing `usize` that is never reused. Removing a layer and pushing a new one gives a new, unique id. External references to a removed layer remain safe (access returns `None`).
- **`CpuCompositor` is a reference implementation**: It's zero-dependency, single-threaded, and correct. Swap it out for a custom compositor via `LayerStack::render_with` without changing your layer setup.
- **Streaming encode is the default path**: Even the `Vec<u8>`-returning `encode` functions delegate internally to the streaming entry points. Memory efficiency is built in.

[`Layer`]: https://docs.rs/termcompositor/latest/termcompositor/trait.Layer.html
[`LayerStack`]: https://docs.rs/termcompositor/latest/termcompositor/struct.LayerStack.html
[`LayerEntry`]: https://docs.rs/termcompositor/latest/termcompositor/struct.LayerEntry.html
[`LayerId`]: https://docs.rs/termcompositor/latest/termcompositor/type.LayerId.html
[`Compositor`]: https://docs.rs/termcompositor/latest/termcompositor/trait.Compositor.html
[`CpuCompositor`]: https://docs.rs/termcompositor/latest/termcompositor/struct.CpuCompositor.html
[`SolidColor`]: https://docs.rs/termcompositor/latest/termcompositor/struct.SolidColor.html
[`RectLayer`]: https://docs.rs/termcompositor/latest/termcompositor/struct.RectLayer.html
[`TextLayer`]: https://docs.rs/termcompositor/latest/termcompositor/struct.TextLayer.html
[`ImageLayer`]: https://docs.rs/termcompositor/latest/termcompositor/struct.ImageLayer.html
[`FrameBuffer`]: https://docs.rs/termcompositor/latest/termcompositor/struct.FrameBuffer.html
[`Protocol`]: https://docs.rs/termcompositor/latest/termcompositor/enum.Protocol.html
[`ProtocolEncoder`]: https://docs.rs/termcompositor/latest/termcompositor/trait.ProtocolEncoder.html
[`EncoderError`]: https://docs.rs/termcompositor/latest/termcompositor/enum.EncoderError.html
[`dispatch_to_writer`]: https://docs.rs/termcompositor/latest/termcompositor/fn.dispatch_to_writer.html
[`detect`]: https://docs.rs/termcompositor/latest/termcompositor/encoder/fn.detect.html
[`detect_with_probe`]: https://docs.rs/termcompositor/latest/termcompositor/encoder/fn.detect_with_probe.html
[`wrap_for_tmux`]: https://docs.rs/termcompositor/latest/termcompositor/fn.wrap_for_tmux.html
[`layer`]: https://docs.rs/termcompositor/latest/termcompositor/layer/index.html
[`compositor`]: https://docs.rs/termcompositor/latest/termcompositor/compositor/index.html
[`framebuffer`]: https://docs.rs/termcompositor/latest/termcompositor/framebuffer/index.html
[`encoder`]: https://docs.rs/termcompositor/latest/termcompositor/encoder/index.html
[`terminal`]: https://docs.rs/termcompositor/latest/termcompositor/terminal/index.html
[`geometry`]: https://docs.rs/termcompositor/latest/termcompositor/geometry/index.html
