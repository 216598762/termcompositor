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
| `image-decoder`  |   off   | `image = "0.25"`  | `ImageLayer` (PNG + JPEG)      |

A custom `Compositor` can be plugged in via `LayerStack::render_with`;
the default `CpuCompositor` is a zero-dependency reference
implementation.

