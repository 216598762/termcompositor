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
