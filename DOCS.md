# Usage Guide

This guide covers how to use `termcompositor` as a library and as a CLI tool.

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
2. **`TERM`**: `xterm-kitty`, `foot`, `foot-*` → `Protocol::Kitty`; `tmux`, `tmux-*` → `Protocol::Sixel` (or `Protocol::Kitty` when `DASHPASSTHROUGH` is set).
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
export DASHPASSTHROUGH=1
termcompositor
```

Or per-invocation:

```bash
DASHPASSTHROUGH=1 termcompositor
```

Or via the CLI flag:

```bash
termcompositor --tmux-passthrough
```

**Requirements:**
- tmux 3.2+ (released 2021).
- `set -g allow-passthrough on` in `~/.tmux.conf`.

Both the `DASHPASSTHROUGH` env var and the `TMUX` env var must be set for the wrapping to apply. This prevents double-wrapping on non-tmux hosts.

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
4. If you're inside tmux without `DASHPASSTHROUGH`, the default protocol is
   Sixel. Build with `sixel-encoder` or set `DASHPASSTHROUGH=1` (see
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
1. **Is `DASHPASSTHROUGH` set?** The env var is the opt-in. Run
   `echo $DASHPASSTHROUGH`. If empty, set it:
   ```bash
   export DASHPASSTHROUGH=1
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
   only applies to Kitty output. Both `DASHPASSTHROUGH` and `kitty-encoder`
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
   `Sixel` (or `Kitty` when `DASHPASSTHROUGH` is set)
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
