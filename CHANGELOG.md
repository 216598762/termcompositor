## 0.4.0 (2026-07-02)

Multi-layer compositor: three new `Layer` types, an optional
image-decoder feature, a `Rect` geometry primitive, and a
breaking extension to the `Layer` trait (additive render
offset + `bounds()`).

### Added
- `src/geometry.rs` -- new `Rect { x, y, width, height }`
  geometry primitive with `is_empty`, `contains`, and
  `intersects` helpers. Re-exported as `dashcompositor::Rect`.
- `RectLayer` (always available): RGBA solid at `(x, y)`
  with `width x height`. `bounds()` reports the rect; render
  writes are clipped to the framebuffer.
- `TextLayer` (always available): UTF-8 text + position +
  colour placeholder. Exposes `render_glyph() -> &str` (a
  placeholder for a future font-backed glyph rasterizer)
  and `text_width()` (one cell per Unicode scalar value).
  Renders as a solid block the size of the text's bounding
  box so layout and z-order are visually verifiable.
- `ImageLayer` (gated on the new `image-decoder` Cargo
  feature): decodes PNG and JPEG via the `image` crate
  (version 0.25, MIT, `default-features = false`, only
  `png` + `jpeg` decoders enabled per AGENTS.md section 3).
  Constructors: `ImageLayer::from_path` and
  `ImageLayer::from_dynamic`.
- `FrameBuffer::get_pixel` and `FrameBuffer::get_pixel_mut`:
  bounds-checked per-pixel accessors that return `Option`,
  giving layers a single, consistent way to clip writes.

### Changed
- `Layer` trait extended (breaking for downstream implementors):
  - New `bounds() -> Option<Rect>` with a default impl
    returning `None` (full-frame layers like `SolidColor`).
  - `render` signature gains an additive `offset: (u32, u32)`
    translation parameter; layers that have no position
    (e.g. `SolidColor`) ignore it.
- `Cargo.toml`: added the `image` crate as an optional
  dependency and the `image-decoder` Cargo feature
  (`default = []`; `image-decoder = ["dep:image"]`). The
  default build remains dependency-light (only
  `terminal_size`).
- `main.rs` demo now drives a `SolidColor` background, a
  positioned `RectLayer`, and a `TextLayer` placeholder,
  reports each layer's bounds, and exercises the full
  add / control / remove / re-add / re-render flow.

### Notes
- `cargo build`, `cargo test` (58 unit tests with default features, 64 with `--features image-decoder`; +1 doc test),
  `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`,
  and `cargo build --release` all remain clean -- both with
  default features and with `--features image-decoder`.
- The `image` crate evaluation per AGENTS.md section 3: BSD
  3-Clause / Apache-2.0 / MIT, ~70M downloads, the de-facto
  Rust image-decoding library. Adopted as an optional dep
  with `default-features = false` + `png` + `jpeg` only.

# Changelog

All notable changes to `dashcompositor` are recorded here. The format
follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and
the project adheres to [Semantic Versioning](https://semver.org/).

## 0.3.0 (2026-07-02)

Terminal-aware compositor: the framebuffer is auto-sized to the
host terminal, and the detected size is reported back through the
API.

### Added
- `terminal_size = "0.3"` as the first runtime dependency (AGENTS.md
  section 3 evaluation: tiny, MIT-licensed, zero transitive deps,
  used by cargo, ripgrep, fd, and many others).
- New `src/terminal.rs` module with `TerminalSize { rows, cols }` and
  the entry points:
  - `TerminalSize::detect()` -- queries the host via
    `ioctl`/console mode; returns `Option<Self>`.
  - `TerminalSize::current()` -- detects or falls back to 80x24.
    Never panics.
  - `TerminalSize::fallback()` -- the static 80x24 default.
  - `TerminalSize::as_framebuffer_size()` -- converts to a
    `(u32, u32)` tuple for `FrameBuffer::new`.
- `LayerStack::render_to_terminal(size)` renders into a
  `FrameBuffer` sized to the given terminal.
- `LayerStack::render_to_current_terminal()` detects the terminal
  size and renders, returning the `(FrameBuffer, TerminalSize)`
  tuple so the backend can report the size back through the API.
- `main.rs` rewritten to detect the terminal size on startup,
  print it, use `render_to_current_terminal` to fit the framebuffer
  to the host, and verify the rendered size matches the reported
  size.
- Tests for `TerminalSize` (fallback, conversion, equality,
  panic-free `current`) and for the new `LayerStack` methods.

the project adheres to [Semantic Versioning](https://semver.org/).

## 0.2.0 (2026-07-02)

First concrete subsystem: a layer stack that the backend (any binary
or library user) can drive at will, addressing the original
"add/remove/control layers from the backend" requirement.

### Added
- `Layer` trait extended with `name()` (default impl) and
  `render(&self, &mut FrameBuffer, opacity)`.
- `LayerEntry` wrapper struct with stable `LayerId`, opacity,
  visibility, optional z-override, and optional custom name.
  - Manual `Debug` impl on `LayerEntry` (the inner `Box<dyn Layer>`
    blocks the derive).
  - `set_layer(Box<dyn Layer>)` for in-place hot-swap without
    invalidating external id handles.
  - `set_z_override(u32)` and `clear_z_override()` (split from the
    prior `set_z_override(Option<u32>)` for ergonomics).
- `LayerStack` with backend-manipulable API: `push` / `remove` /
  `get` / `get_mut` / `index_of` / `reorder` / `len` / `is_empty` /
  `entries` / `entries_mut` / `iter_sorted` / `clear` / `render` /
  `render_with`. Ids are monotonic and not reused for the lifetime of
  the stack.
- `Compositor` trait and `CpuCompositor` default implementation.
  `CpuCompositor` is a zero-dependency reference: it sorts visible
  entries by effective z-order (stable on ties) and calls each
  layer's `render` with its opacity.
- `SolidColor` concrete layer with `with_z` and `with_name` builders.
- `FrameBuffer::clear()` and a free function `blend_over` for
  straight-alpha over-compositing in 8-bit RGBA.
- README "Usage (library)" section showing the push/control/render
  flow.
- 28 unit tests + 1 doc-test covering blend math, layer controls,
  layer-stack add/remove/reorder/render, custom compositor, and the
  iter_sorted z-order + stable-tiebreak contracts.

### Notes
- Zero runtime dependencies. Candidate crates (tiny-skia, wgpu,
  image, kittage, icy_sixel) remain commented-out optional features
  per AGENTS.md section 3.
- `cargo build`, `cargo test`, `cargo fmt --check`,
  `cargo clippy --all-targets -- -D warnings`, and
  `cargo build --release` all clean.
- GPG commit signing via `8CAF4D685F95A842` continues to be wired
  up via loopback pinentry + `allow-preset-passphrase` on the
  gpg-agent; the unsigned scaffold commit `788200e` is grandfathered
  per AGENTS.md section 5 (no rewriting main history).

## 0.1.0 (2026-07-02)

Initial scaffold of `dashcompositor`, a layer-based graphics compositor
for the terminal that projects a fully composited RGBA framebuffer to the
host via the Kitty graphics protocol or Sixel.

### Added
- MIT `LICENSE` (2026).
- `AGENTS.md` -- operating rules for AI agents and human contributors.
- `README.md` -- project overview, target features, architecture diagram.
- `Cargo.toml` -- package metadata, lib + bin targets, `[lints.rust]
  missing_docs = "warn"`. Candidate feature flags (CPU/GPU compositor,
  image decoder, kitty/sixel encoders) are stubbed but commented out
  per AGENTS.md section 3 until each crate is vetted on crates.io.
- `src/lib.rs` plus four module stubs mirroring the AGENTS.md section 7
  architecture: `compositor`, `layer`, `framebuffer`, `encoder`.
- `src/main.rs` -- no-op binary entry point pending a real
  protocol-detector implementation.
- `.gitignore` extended for Rust build output (`target/`, `*.rs.bk`,
  `.cargo/`).
- CI-ready: `cargo build`, `cargo test`, `cargo fmt --check`,
  `cargo clippy --all-targets -- -D warnings`, and `cargo build --release`
  all pass on the scaffold.
- Environment: GPG signing is wired up (loopback pinentry +
  `allow-preset-passphrase` on the gpg-agent, `user.signingkey` pinned
  to the primary key `8CAF4D685F95A842`) so non-interactive commits in
  this host produce verifiable signatures.
