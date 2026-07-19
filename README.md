# termcompositor

A **layer-based graphics compositor for the terminal**, written in Rust.

Keep an in-memory compositing pipeline — push layers (colours, shapes, text,
images), render them into an off-screen RGBA framebuffer, and project the
result to your terminal via the Kitty graphics protocol or Sixel.

It is a **rendering pipeline**, not a terminal emulator: no TTY management,
no shell input. You build a frame, `termcompositor` encodes it.

## Highlights

- **Animation loop** — built-in frame loop with delta-time tracking, terminal resize handling, and opt-in rendering via `animation::run()`. CLI flags: `--animate` and `--fps <N>`.
- **Layer transforms** — per-layer rotation and scaling via `Transform` with anchor points and bilinear interpolation.
- **Rich layer types** — `SolidColor`, `RectLayer`, `TextLayer`, `GradientLayer`, `BorderLayer`, `CanvasLayer`, `DropShadow`, `SceneGraph`.
- **Dual protocol support** — Kitty graphics protocol and Sixel, with auto-detection.

## What's new in v2.0.0

- **GradientLayer Builder** — `GradientLayer::linear()` and `GradientLayer::radial()` replaced by a fluent `GradientLayerBuilder` API with `new_linear()`, `new_radial()`, `at()`, `size()`, `colors()`, etc.
- **FontSource memory leak fix** — `TextLayer::with_font(FontSource::Path)` no longer leaks memory.
- **SceneNode parent-child traversal** — `SceneGraph` now supports `parent()`, `children()`, `ancestors()`, `depth()`, `descendants()`, and `move_to()` with cycle detection.

## Architecture (one line)

```
Layers → compositor → FrameBuffer → protocol encoder → terminal stdout
```

- [`ARCHITECTURE.md`](./ARCHITECTURE.md) — full stack, API reference, memory model, design decisions.
- [`DOCS.md`](./DOCS.md) — installation, library usage, CLI flags, feature flags, troubleshooting.
- [`CONTRIBUTING.md`](./CONTRIBUTING.md) — contributor guide.

## Feature flags

| Feature | Default | Enables |
|---|---|---|
| `font-rasterizer` | **on** | Real glyph rendering in `TextLayer` via `fontdue` |
| `kitty-encoder` | off | Kitty graphics protocol output |
| `sixel-encoder` | off | Sixel output |
| `image-decoder` | off | `ImageLayer` (PNG + JPEG) |

Enable at least one encoder feature to produce terminal output.

## MSRV

**1.73** — pinned in `Cargo.toml` and validated in CI.

## License

MIT — see [`LICENSE`](./LICENSE).
