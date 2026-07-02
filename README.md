# dashcompositor

A **layer-based graphics compositor for the terminal**, written in Rust.

`dashcompositor` keeps an in-memory stack of layers — sprites, images, text, and
shapes — composites them into a single off-screen pixel buffer, and then projects
the fully composited image to your terminal emulator via either the
**Kitty graphics protocol** or **Sixel**, depending on what the host terminal
supports.

It is a **rendering pipeline**, not a terminal emulator: `dashcompositor` does
not parse shell input or manage a TTY. It composes a frame and writes it out.

## Features (target)

- Layer model with z-ordering, per-layer opacity, and transforms.
- Pluggable layer types: raster image, text glyphs, vector shape, sprite.
- Single composited framebuffer per output frame.
- Output via the **Kitty graphics protocol** (preferred) or **Sixel**
  (fallback).
- Pure Rust stack; transitively relies on well-trodden crates from
  [awesome-rust](https://github.com/rust-unofficial/awesome-rust).

## Status

Early-stage design. The contributor / agent rulebook is
[`AGENTS.md`](./AGENTS.md) — read it before opening a PR.

## How it works (one-line)

Layers → compositor → framebuffer → protocol encoder → terminal escape
sequences.

```
┌────────────┐  ┌────────────┐  ┌────────────┐  ┌────────────┐
│  Layer N   │  │  Layer …   │  │  Layer 1   │  │  Layer 0   │
└─────┬──────┘  └─────┬──────┘  └─────┬──────┘  └─────┬──────┘
      └───────────────┴───────────────┴───────────────┘
                              │ composite()
                              ▼
                    ┌─────────────────────┐
                    │   Frame buffer      │
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

## Contributing

Read [`AGENTS.md`](./AGENTS.md) first. Key rules:

- Use existing Rust libraries where they exist; start searching from
  [awesome-rust](https://github.com/rust-unofficial/awesome-rust).
- Commit and push frequently with detailed, multi-line commit messages.
- Never open issues on this repository.

## License

Licensed under the **MIT License** — see [`LICENSE`](./LICENSE).
## Usage (library)

The `dashcompositor` library exposes a `LayerStack` that the backend
can drive at will. Layers are added with stable `LayerId` handles,
and each entry's per-layer state -- opacity, visibility, z-override,
name -- can be tweaked at any time. Four built-in `Layer` types
are provided:

| Type        | Has position? | Notes                                  |
| ----------- | :-----------: | -------------------------------------- |
| `SolidColor`| no (fills)    | Single RGBA colour, fills whole target |
| `RectLayer` | yes           | RGBA solid at `(x, y)` of `width x height` |
| `TextLayer` | yes           | Placeholder; renders a coloured block, exposes `render_glyph()` for a future font rasterizer |
| `ImageLayer`| yes (optional) | Decodes PNG / JPEG via the `image` crate (gated on the `image-decoder` feature) |

```rust
use dashcompositor::{
    FrameBuffer, LayerStack, RectLayer, SolidColor, TerminalSize, TextLayer,
};

let mut stack = LayerStack::new();

// Full-frame background.
let bg = stack.push(SolidColor::new(0, 0, 64, 255).with_name("bg"));

// Positioned rectangle.
let rect = stack.push(
    RectLayer::new(20, 6, 40, 12, [0, 200, 0, 200])
        .with_z(10)
        .with_name("centered-rect"),
);

// Text placeholder (will be swapped for a real glyph rasterizer
// later; for now it draws a colored block the size of the text).
let label = stack.push(
    TextLayer::new(2, 1, "dashcompositor", [255, 255, 255, 255])
        .with_z(20)
        .with_name("title"),
);

// Render into a framebuffer auto-sized to the host terminal.
let (fb, size) = stack.render_to_current_terminal();
assert_eq!(size.cols as u32, fb.width());
assert_eq!(size.rows as u32, fb.height());

// Control at will.
stack.get_mut(rect).unwrap().set_opacity(0.5);
stack.get_mut(label).unwrap().set_visible(false);
```

### Optional: raster image layer

`ImageLayer` decodes PNG and JPEG into a layer via the `image`
crate. Enable it with the `image-decoder` feature in your
`Cargo.toml`:

```toml
dashcompositor = { version = "0.4", features = ["image-decoder"] }
```

then:

```rust
use dashcompositor::ImageLayer;
let img = ImageLayer::from_path("logo.png", 4, 2)?;
let id = stack.push(img);
```

### Optional feature flags

| Feature          | Default | Pulls in          | Enables                        |
| ---------------- | :-----: | ----------------- | ------------------------------ |
| `sixel-encoder`   |   off   | `icy_sixel = "0.5"`    | `Protocol::Sixel` produces real Sixel DCS escape sequences  |
| `kitty-encoder`   |   off   | `little-kitty = "0.0.3"` | `Protocol::Kitty` produces real Kitty escape sequences |
| `image-decoder`  |   off   | `image = "0.25"`  | `ImageLayer` (PNG + JPEG)      |

### Auto-detect protocol

`dashcompositor::Protocol::Auto` (re-exported as the
[`Protocol`] enum variant) defers the choice to the
pure env-var shim [`detect`] at encode time, which
picks `Protocol::Kitty` or `Protocol::Sixel` based on
`TERM` / `TERM_PROGRAM` / `COLORTERM`. The CLI demo
defaults to `Protocol::Auto`; pass
`--protocol <kitty|sixel|auto>` to override or
`--probe` to use the I/O-based Kitty query-response
probe ([`detect_with_probe`], requires the
`kitty-encoder` feature). Enable at least one of the
encoder features for `Protocol::Auto` to produce
anything other than an `UnsupportedProtocol` error.

[`detect`]: https://docs.rs/dashcompositor/latest/dashcompositor/encoder/fn.detect.html
[`detect_with_probe`]: https://docs.rs/dashcompositor/latest/dashcompositor/encoder/fn.detect_with_probe.html
[`Protocol`]: https://docs.rs/dashcompositor/latest/dashcompositor/enum.Protocol.html


### Tmux passthrough (v0.8.0)

When the host is running inside tmux, the Kitty graphics
protocol needs its APC payload wrapped in a tmux passthrough
DCS (`ESC P tmux ; ... ESC \`) so the bytes survive the
tmux -> outer-terminal hop. `dashcompositor` does this
automatically when **both** conditions are met:

1. The env var `DASHPASSTHROUGH` is set to a non-empty value
   (typically `DASHPASSTHROUGH=1`), AND
2. The `TMUX` env var is set (the canonical signal that
   we are inside a tmux session).

The opt-in is explicit because tmux requires
`set -g allow-passthrough on` in `~/.tmux.conf` for
tmux 3.2+ (released 2021) to forward APC payloads -- a
user with a stock tmux config would otherwise get
corrupted output. Both conditions are required so a
user with `DASHPASSTHROUGH` set in their shell rc on a
non-tmux host does not get accidental double-wrapping.

Enable with one of:

```bash
# In your shell rc (~/.bashrc, ~/.zshrc, etc.):
export DASHPASSTHROUGH=1

# Or per-invocation:
DASHPASSTHROUGH=1 dashcompositor

# Or via the CLI flag (the demo only):
dashcompositor --tmux-passthrough
```

The CLI demo logs the resolved tmux-passthrough state
to stderr so you can verify the opt-in was picked up:

```
$ dashcompositor --tmux-passthrough
dashcompositor v0.8.0 -- multi-layer + auto-detect encoder: host terminal = 80 cols x 24 rows
...
requested protocol: auto; resolved: kitty
tmux passthrough: enabled (DASHPASSTHROUGH=1)
encoded 10268 bytes via kitty; writing to stdout
```

Without the opt-in, the v0.7.0 default is preserved:
`TERM=tmux*` resolves to Sixel. The heuristic
priority order is unchanged: `TERM_PROGRAM` still wins
over `TERM`, and the COLORTERM tiebreaker still kicks
in for unknown terminals.

The `wrap_for_tmux` helper is also exported as a
public function in case you want to wrap your own
encoder output for tmux passthrough:

```rust
use dashcompositor::wrap_for_tmux;

let raw_kitty = encoder.encode(&fb)?;
let tmux_safe = wrap_for_tmux(raw_kitty);
std::io::stdout().write_all(&tmux_safe)?;
```

The pure byte transform does no I/O and reads no
env vars; the opt-in check is the caller's
responsibility. See the
[`wrap_for_tmux`] docs for the exact wrapping rules
(inner-ESC doubling, prefix, suffix).

[`wrap_for_tmux`]: https://docs.rs/dashcompositor/latest/dashcompositor/fn.wrap_for_tmux.html


### Chunked encoding for large images (v0.8.1)

The Kitty graphics protocol caps each APC payload at
4096 bytes of base64-encoded data. For 32-bit RGBA
(4 bytes/pixel), that means a single Kitty command
can carry at most 768 pixels of payload. The v0.8.1
encoder transparently splits larger framebuffers into
multiple APC commands using the protocol's `m=1` /
`m=0` chunking mechanism:

```
\x1b_Ga=T,f=32,q=2,s=W,v=H,m=1;<chunk1_base64>\x1b\\
\x1b_Gm=1;<chunk2_base64>\x1b\\
\x1b_Gm=0;<last_chunk_base64>\x1b\\
```

The first chunk carries the full control list
(`a`, `f`, `q`, `s`, `v`) plus `m=1`. Intermediate
chunks carry only `m=1` (the terminal remembers the
metadata from the first chunk per the spec). The
last chunk carries `m=0`.

**Backwards compatibility**: framebuffers that fit
in a single 4096-byte base64 chunk (≤768 RGBA pixels,
~12 KB raw payload) continue to use the v0.8.0
single-command wire format with no `m` key. The
chunked path is only used for framebuffers larger
than 768 pixels. This means terminals that pre-date
the chunking extension keep working unchanged for
small images.

**v0.8.0 tmux passthrough is preserved**: the
dispatch wraps the entire multi-chunk output (all
concatenated APC commands) in a single
`wrap_for_tmux` call, so tmux users with
`DASHPASSTHROUGH=1` get the chunked output
passthrough-wrapped as one passthrough DCS.

**No configuration required**: the chunk size (4096
base64 bytes) and pixels-per-chunk (768 RGBA pixels)
are hardcoded constants matching the Kitty spec's
hard limit. There is no CLI flag or env var to tune
them; the spec is unambiguous and a configurable
chunk size would add API surface for no concrete
benefit.

A custom `Compositor` can be plugged in via `LayerStack::render_with`;
the default `CpuCompositor` is a zero-dependency reference
implementation.

