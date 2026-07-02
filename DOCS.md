# Usage Guide

This guide covers how to use `dashcompositor` as a library and as a CLI tool.

---

## Table of contents

- [Installation](#installation)
- [Library usage](#library-usage)
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
dashcompositor = "0.11"
```

Enable encoder features to produce terminal output:

```toml
[dependencies]
dashcompositor = { version = "0.11", features = ["kitty-encoder", "sixel-encoder"] }
```

### As a CLI tool

```bash
cargo install dashcompositor
```

Or build from source:

```bash
git clone https://github.com/216598762/dashcompositor
cd dashcompositor
cargo build --release --features kitty-encoder,sixel-encoder
./target/release/dashcompositor
```

---

## Library usage

### Quick start

```rust
use dashcompositor::{
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
use dashcompositor::{LayerStack, RectLayer, SolidColor, TextLayer};

let mut stack = LayerStack::new();

let bg = stack.push(SolidColor::new(0, 0, 64, 255).with_name("bg"));
let rect = stack.push(
    RectLayer::new(20, 6, 40, 12, [0, 200, 0, 200])
        .with_z(10)
        .with_name("centered-rect"),
);
let label = stack.push(
    TextLayer::new(2, 1, "dashcompositor", [255, 255, 255, 255])
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
use dashcompositor::{LayerStack, SolidColor, TextLayer};

let mut stack = LayerStack::new();
stack.push(SolidColor::new(0, 0, 64, 255).with_name("bg"));
stack.push(TextLayer::new(0, 0, "hello", [255; 4]).with_z(10));

let (fb, size) = stack.render_to_current_terminal();
assert_eq!(size.cols as u32, fb.width());
assert_eq!(size.rows as u32, fb.height());
```

### End-to-end: encode and write

```rust
use dashcompositor::{
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

Implement the [`Compositor`](https://docs.rs/dashcompositor/latest/dashcompositor/trait.Compositor.html) trait for custom rendering logic:

```rust
use dashcompositor::{Compositor, CpuCompositor, FrameBuffer, LayerStack};

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
dashcompositor = { version = "0.11", features = ["image-decoder"] }
```

```rust
use dashcompositor::{ImageLayer, LayerStack};

let mut stack = LayerStack::new();
let img = ImageLayer::from_path("logo.png", 4, 2).unwrap();
let id = stack.push(img.with_z(10));
```

### Streaming encode (zero-copy output)

For multi-megapixel framebuffers, use the streaming entry points to avoid materialising the full output in memory:

```rust,ignore
use dashcompositor::encoder::kitty;
use dashcompositor::framebuffer::FrameBuffer;
use std::io::BufWriter;

let fb = FrameBuffer::new(1920, 1080);
let stdout = std::io::stdout();
let mut writer = BufWriter::new(stdout.lock());
kitty::encode_to_writer(&fb, &mut writer).unwrap();
```

For Sixel (requires the `sixel-encoder` feature):

```rust,ignore
use dashcompositor::encoder::sixel;

let fb = FrameBuffer::new(1920, 1080);
let mut writer = BufWriter::new(std::io::stdout().lock());
sixel::encode_to_writer(&fb, &mut writer).unwrap();
```

For the O(1)-memory Sixel path (fixed xterm-256 palette, no `icy_sixel`):

```rust,ignore
use dashcompositor::encoder::sixel;

let fb = FrameBuffer::new(1920, 1080);
let mut writer = BufWriter::new(std::io::stdout().lock());
sixel::encode_to_writer_streaming(&fb, &mut writer).unwrap();
```

---

## CLI usage

```bash
# Default: auto-detect terminal size and protocol
dashcompositor

# Override protocol
dashcompositor --protocol kitty
dashcompositor --protocol sixel
dashcompositor --protocol auto

# Use I/O-based Kitty probe (requires kitty-encoder feature)
dashcompositor --probe

# Enable tmux passthrough
dashcompositor --tmux-passthrough
```

The CLI:
1. Detects the terminal size.
2. Builds a multi-layer stack (background + rectangle + text).
3. Renders into a framebuffer.
4. Encodes via the chosen protocol (auto-detected by default).
5. Writes the escape sequences to stdout.

Human-readable diagnostics are written to stderr:

```
$ dashcompositor
dashcompositor v0.11.0 -- multi-layer + auto-detect encoder: host terminal = 80 cols x 24 rows
background: SolidColor([0, 0, 64, 255])
rect:      RectLayer at (20,6) 40x12 [0,200,0,200] z=10
label:     TextLayer at (2,1) "dashcompositor" z=20
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

`dashcompositor::detect()` reads `TERM`, `TERM_PROGRAM`, and `COLORTERM` from the process environment and returns a `Protocol` suggestion.

Priority order:

1. **`TERM_PROGRAM`** (set by the terminal app): `kitty`, `wezterm`, `ghostty` → `Protocol::Kitty`.
2. **`TERM`**: `xterm-kitty`, `foot`, `foot-*` → `Protocol::Kitty`; `tmux`, `tmux-*` → `Protocol::Sixel` (or `Protocol::Kitty` when `DASHPASSTHROUGH` is set).
3. **`COLORTERM`**: `truecolor` / `24bit` → `Protocol::Kitty` when TERM is inconclusive.
4. **Default**: `Protocol::Sixel`.

`Protocol::Auto` defers to `detect()` at encode time.

For authoritative detection (I/O-based Kitty query-response probe), use `detect_with_probe()` (requires `kitty-encoder` feature):

```rust
use dashcompositor::detect_with_probe;
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

## Tmux passthrough

When the host is running inside tmux, the Kitty graphics protocol's APC payload must be wrapped in a tmux passthrough DCS to survive the tmux → outer-terminal hop.

Enable with:

```bash
export DASHPASSTHROUGH=1
dashcompositor
```

Or per-invocation:

```bash
DASHPASSTHROUGH=1 dashcompositor
```

Or via the CLI flag:

```bash
dashcompositor --tmux-passthrough
```

**Requirements:**
- tmux 3.2+ (released 2021).
- `set -g allow-passthrough on` in `~/.tmux.conf`.

Both the `DASHPASSTHROUGH` env var and the `TMUX` env var must be set for the wrapping to apply. This prevents double-wrapping on non-tmux hosts.

Programmatic usage:

```rust
use dashcompositor::wrap_for_tmux;

let raw_kitty = Protocol::Kitty.encode(&fb)?;
let tmux_safe = wrap_for_tmux(raw_kitty);
std::io::stdout().write_all(&tmux_safe)?;
```

Or use the end-to-end streaming entry point for O(1) memory:

```rust,ignore
use dashcompositor::encoder::kitty;

kitty::encode_passthrough_to_writer(&fb, &mut stdout)?;
```

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
