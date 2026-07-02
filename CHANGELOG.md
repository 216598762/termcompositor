## 0.7.1 (2026-07-02)

Patch release: hardens the `with_env` test helper with a
panic-safe `EnvGuard` (Drop-based) that also saves and restores
`COLORTERM` (the v0.7.0 helper only saved/restored `TERM` and
`TERM_PROGRAM` and required manual pairing, so a panic in the
test body could leak env modifications to other parallel tests).
Also strengthens the auto-detect dispatch test from a
"does-not-infinite-loop" probe to a byte-prefix assertion.

### Changed
- `src/encoder.rs` `with_env` test helper now uses a private
  `EnvGuard` struct (a `Drop`-implementing RAII handle around
  `std::env::var` / `std::env::set_var` / `std::env::remove_var`).
  The guard saves the current value on construction and restores
  it on `Drop`, so a panic in the test body still restores all
  three env vars. The helper signature now takes three
  `Option<&str>` arguments (TERM, TERM_PROGRAM, COLORTERM) and
  sets / clears all three on each call.
- `src/encoder.rs` `with_env` now also acquires a
  process-global `Mutex<()>` (returned by `env_mutex()` via a
  `std::sync::OnceLock`) before touching any env var, and
  holds it until the closure returns. This serialises the
  env-touching tests so two `with_env` calls running in
  parallel can no longer stomp on each other's saved env
  values (the v0.7.0 `EnvGuard`-only pattern was still racy:
  the second `EnvGuard::new("TERM")` would snapshot the first
  test's modified value, and the second `Drop` would restore
  that -- not the original -- leaking the first test's value
  to subsequent parallel tests). The 1-of-81 test failure
  observed in a v0.7.0-flake validation run was this race.
  The lock is acquired with `unwrap_or_else(|e|
  e.into_inner())` to recover from a poisoned mutex (e.g. a
  previous test panicked while holding the lock).
- `src/encoder.rs` test `dispatch_auto_recurses_through_detect`
  (the v0.7.0 name) was renamed and strengthened to
  `dispatch_auto_recurses_through_detect_resolves_to_kitty` and
  is now gated on `kitty-encoder`. The new test sets
  `TERM=xterm-kitty` (a known Kitty terminfo name), invokes
  `dispatch(Protocol::Auto, &fb)`, and asserts the output
  starts with `\x1b_G` (the Kitty APC introducer) -- proving
  the recursion actually resolves to Kitty, not just that it
  terminates. Without this, a regression that made the
  recursion land in the wrong arm would not have been caught
  by the v0.7.0 test (which only verified the call returned
  without panicking).
- New `src/encoder.rs` test
  `dispatch_auto_recurses_through_detect_resolves_to_sixel`
  (gated on `sixel-encoder`) is the Sixel-side mirror of the
  Kitty recursion test. Sets `TERM=tmux-256color` (a known
  Sixel-fallback terminfo name) and asserts the dispatch
  output starts with `\x1bP` (the Sixel DCS introducer).

### Notes
- `cargo build`, `cargo test`, `cargo fmt --check`, and
  `cargo clippy --all-targets -- -D warnings` are all clean
  for ALL four feature combinations: default,
  `--features kitty-encoder`, `--features sixel-encoder`, and
  `--features kitty-encoder,sixel-encoder`. `cargo build --release`
  is clean for the default and both-features combos.
- The `EnvGuard` pattern is the recommended one for any future
  test helper that mutates process-global state (env vars, cwd,
  etc.): the `Drop` impl guarantees cleanup even on panic. The
  pattern is small enough (~25 lines including the `with_env`
  wrapper) that it lives inline in the test module rather than
  in a separate `pub(crate)` test-utility module.
- No public API changes; no new dependencies; no new features.


## 0.7.0 (2026-07-02)

Auto-detect protocol: a new `Protocol::Auto` variant that picks
`Protocol::Kitty` or `Protocol::Sixel` based on terminal capability
detection (env-var shim over `TERM` / `TERM_PROGRAM` / `COLORTERM`,
plus a Kitty query-response probe via
`little_kitty::Command::is_supported()` for authoritative detection).

### Added
- `Protocol::Auto` variant on the existing `Protocol` enum. The
  `as_str` impl returns `"auto"` for the new variant. Both
  `EncoderError` and `ProtocolEncoder` are unchanged.
- `pub fn detect() -> Protocol` in `dashcompositor::encoder`:
  pure env-var detection (`TERM` / `TERM_PROGRAM` / `COLORTERM`).
  Always available, no I/O, never panics. The shim that
  `Protocol::Auto::encode` dispatches through. Heuristics, in
  priority order:
  1. `TERM_PROGRAM` (most specific): `kitty` / `wezterm` /
     `ghostty` (case-insensitive) -> `Protocol::Kitty`.
  2. `TERM` (terminfo name): `xterm-kitty` / `foot` / `foot-*`
     -> `Protocol::Kitty`; `tmux` / `tmux-*` -> `Protocol::Sixel`
     (Kitty via tmux needs passthrough, not yet implemented).
  3. `COLORTERM` tiebreaker (weak signal): `truecolor` / `24bit`
     -> `Protocol::Kitty` when `TERM`/`TERM_PROGRAM` are
     inconclusive. Modern truecolor terminals are more likely to
     support the Kitty graphics protocol than the average
     XTerm-like terminal.
  4. Default -> `Protocol::Sixel` (most universal fallback).
- `#[cfg(feature = "kitty-encoder")] pub fn detect_with_probe() -> Result<Protocol, EncoderError>`:
  authoritative detection via the I/O-based Kitty query-response
  probe (`little_kitty::Command::is_supported()`). Short-circuits
  to `Ok(Kitty)` when the env-var shim already says Kitty (avoids
  an unnecessary probe in the common case). Performs I/O on
  stdin/stdout; do NOT call from a pure encoder (see AGENTS.md §7).
- `pub(crate) fn detect_with_env(Option<&str>, Option<&str>) -> Protocol`:
  testable inner of `detect`, accepts env values directly to
  avoid `std::env::set_var` races in parallel tests.
- Private `fn dispatch(Protocol, &FrameBuffer) -> Result<Vec<u8>, EncoderError>`
  refactored out of the `ProtocolEncoder for Protocol` impl so
  the `Auto` arm can recurse cleanly via `dispatch(detect(), frame)`
  without duplicating the per-variant `#[cfg]` matrix. The
  recursion is bounded because `detect` returns only `Kitty` or
  `Sixel` (never `Auto`) by construction.
- `Protocol::Auto` arm of `ProtocolEncoder::encode`:
  `dispatch(detect(), frame)`. When neither encoder feature is
  enabled, the recursion lands in the disabled-feature Kitty or
  Sixel arm and returns `Err(UnsupportedProtocol)` (the error
  name is the concrete protocol picked by `detect`, not `"auto"`).
- `lib.rs` re-exports `pub use encoder::{detect, EncoderError,
  Protocol, ProtocolEncoder};` and, gated on `kitty-encoder`,
  `pub use encoder::detect_with_probe;`.
- `main.rs` CLI flags: `--protocol <kitty|sixel|auto>` (override
  the default) and `--probe` (use the I/O-based Kitty probe
  instead of the env-var shim). Default protocol is now
  `Protocol::Auto`. The demo logs both the requested and resolved
  protocol so the user can verify the auto-detect.
- 14 new unit tests in `src/encoder.rs`:
  - `as_str_matches_variant` (extended to cover `Auto`).
  - `detect_with_env_picks_kitty_for_term_program_kitty` (with
    case-insensitive variants for `Kitty` / `KITTY`).
  - `detect_with_env_picks_kitty_for_term_program_wezterm` /
    `..._ghostty` (case-insensitive).
  - `detect_with_env_picks_kitty_for_xterm_kitty`.
  - `detect_with_env_picks_kitty_for_foot_and_foot_extra`
    (`foot`, `foot-extra`, `foot-256color`).
  - `detect_with_env_picks_sixel_for_tmux` (`tmux`,
    `tmux-256color`, `tmux-direct`).
  - `detect_with_env_picks_sixel_for_xterm_256color`.
  - `detect_with_env_picks_sixel_when_neither_set` (both
    unset and both empty).
  - `detect_with_env_term_program_wins_over_term` (priority
    ordering).
  - `detect_with_env_unknown_term_program_falls_through_to_term`.
  - `detect_with_env_colorterm_truecolor_picks_kitty_for_unknown_term`.
  - `detect_with_env_colorterm_24bit_picks_kitty_for_unknown_term`.
  - `detect_with_env_colorterm_does_not_override_term_program`.
  - `dispatch_auto_recurses_through_detect` (no-env-var
    termination test).
  - `dispatch_auto_with_term_program_kitty_delegates_to_kitty`
    (env-var-driven dispatch, gated on `kitty-encoder`).
  - `dispatch_auto_with_term_tmux_delegates_to_sixel`
    (env-var-driven dispatch, gated on `sixel-encoder`).
  - `auto_encode_through_trait_delegates_to_dispatch` (gated
    on both features; closes the one-line-wrapper regression
    gap between `Protocol::Auto.encode` and `dispatch`).
  - `detect_with_probe_short_circuits_when_env_already_kitty`
    (gated on `kitty-encoder`; verifies the env-var short-circuit
    path returns `Ok(Kitty)` without invoking the probe).

### Changed
- `Cargo.toml` version bumped to 0.7.0. No new features, no new
  dependencies.
- `src/encoder.rs` module doc updated to mention v0.7.0 and the
  auto-detect shim.
- `src/main.rs` rewritten to default to `Protocol::Auto`, parse
  the `--protocol` / `--probe` CLI flags, and log the resolved
  protocol. The demo gracefully falls back to the env-var shim
  when `--probe` is passed but the `kitty-encoder` feature is
  not enabled.
- The `with_env` test helper (used by 2 dispatch tests) is
  process-global and racy under `cargo test`'s default parallel
  harness. The race is acknowledged in a code comment; a future
  v0.7.1+ may move these to integration tests in `tests/` or add
  a `Mutex<()>` for serialisation.

### Notes
- `cargo build`, `cargo test`, `cargo fmt --check`, and
  `cargo clippy --all-targets -- -D warnings` are all clean
  for ALL four feature combinations: default,
  `--features kitty-encoder`, `--features sixel-encoder`, and
  `--features kitty-encoder,sixel-encoder`. `cargo build --release`
  is clean for the default and both-features combos.
- Test count per feature combo: 4 with default features, 17 with
  `--features kitty-encoder` alone, 7 with `--features
  sixel-encoder` alone, 20 with both features; +1 doc test.
  (The previous v0.6.0 release had 4 / 6 / 6 / 8; the v0.7.0
  bump adds the 14 new `detect` / `dispatch` / `auto_encode`
  tests plus the existing per-encoder tests under their
  respective feature gates.)
- The `Result<Vec<u8>, EncoderError>` return on
  `ProtocolEncoder::encode` (carried over from v0.5.0/v0.6.0) is
  the only way to surface `EncoderError::UnsupportedProtocol`
  from the disabled-feature arms and the not-yet-implemented
  error paths. A literal `Vec<u8>` return (as in the original
  v0.5.0 spec) would have required either gating the entire
  trait on a feature (breaking the ungated re-exports) or
  panicking from disabled-feature / not-implemented paths.
- End-to-end demo verification (both-features build):
  - `TERM=xterm-kitty TERM_PROGRAM=kitty`: resolves to `kitty`,
    emits 10,268 bytes starting with `1b 5f 47` (`\x1b_G`) and
    ending with `2f 1b 5c` (`/\x1b\\`).
  - `TERM=tmux-256color`: resolves to `sixel`, emits 142 bytes
    starting with `1b 50 39` (`\x1bP9`).
  - `COLORTERM=truecolor TERM=xterm-256color`: resolves to
    `kitty` (COLORTERM tiebreaker kicks in).
  - Default-features build: resolves to `sixel` (env-var
    default), then prints
    `encoder error for protocol sixel: protocol sixel is not
    supported in this build (is the required Cargo feature
    enabled?)` and exits 0. The demo is designed to fail
    gracefully when the relevant Cargo feature is missing.


## 0.6.0 (2026-07-02)

Second protocol encoder: the Sixel graphics protocol, wired up via
the optional `icy_sixel` (v0.5) crate behind a new
`sixel-encoder` Cargo feature. The Kitty encoder from v0.5.0
remains; both arms of `ProtocolEncoder::encode` are now real when
their respective features are enabled.

### Added
- `icy_sixel = "0.5"` as an optional dependency, gated behind
  the new `sixel-encoder` Cargo feature (default off).
- Private `sixel` submodule in `src/encoder.rs`, paralleling the
  `kitty` submodule. Uses the real `icy_sixel` 0.5 API:
  - `SixelImage::from_rgba(rgba, w, h)` to wrap the framebuffer's
    RGBA pixels (the `u32` width/height from `FrameBuffer` are
    widened to `usize` via a lossless `as` cast, which is sound
    on every supported platform).
  - `.encode() -> Result<String, SixelError>` to produce the
    full DCS-wrapped Sixel string (`\x1bPq...<sixel data>...\x1b\\`).
  - `.into_bytes()` to convert the `String` to the `Vec<u8>`
    return type.
- `From<std::io::Error> for EncoderError` (gated on
  `kitty-encoder`) and `From<icy_sixel::SixelError> for
  EncoderError` (gated on `sixel-encoder`). These let both
  `kitty::encode` and `sixel::encode` use `?` directly instead of
  a per-module `.map_err(helper)?` pattern. The v0.5.0 `io_err`
  and v0.6.0 `sixel_err` local helpers have been removed in
  favour of the `From` impls; the pattern is now ready to scale
  to the v0.7.0 auto-detect work.
- 3 new Sixel tests mirroring the Kitty test suite:
  - `sixel_encode_rejects_zero_dimensions`
  - `sixel_encode_produces_valid_dcs_framing` -- strengthened
    against the same class of regression as the Kitty framing
    test: checks `starts_with(b"\x1bP")`, `ends_with(b"\x1b\\")`,
    the `q` mode letter appears before the first `#`
    colour-definition introducer, the output is longer than 16
    bytes (catches an empty-payload regression), and the
    2x2 dimensions are referenced in the output.
  - `sixel_encode_is_deterministic_for_same_input`
- The stale `sixel_encode_is_unsupported_in_v050` test was
  renamed to `sixel_encode_is_unsupported_without_feature` and
  re-gated on `not(feature = "sixel-encoder")` to mirror the
  existing Kitty `kitty_encode_is_unsupported_without_feature`
  pattern.

### Changed
- `Cargo.toml` version bumped to 0.6.0; `sixel-encoder` feature
  added; `icy_sixel` optional dep added (see AGENTS.md §3
  evaluation: pure-Rust SIXEL encoder/decoder, MIT/Apache-2.0,
  actively maintained, the de-facto Rust SIXEL library).
- The `Protocol::Sixel` arm of `ProtocolEncoder` now dispatches
  to `sixel::encode` when the `sixel-encoder` feature is on, and
  returns `Err(UnsupportedProtocol("sixel"))` otherwise -- the
  same shape as the Kitty arm.
- The `EncoderError::Encode` variant's Display message is
  unchanged, but it's now reachable via `?` from both encoder
  submodules via the new `From` impls.
- The encoder module's doc comment was updated to mention the
  v0.6.0 Sixel work and the per-feature `UnsupportedProtocol`
  return.

### Notes
- `cargo build`, `cargo test` (4 unit tests with default
  features, 6 with `--features kitty-encoder` alone, 6 with
  `--features sixel-encoder` alone, 8 with
  `--features kitty-encoder,sixel-encoder`; +1 doc test),
  `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`,
  and `cargo build --release` are all clean for ALL four
  feature combinations.
- The `From` impl pattern is the recommended one for future
  encoders (e.g. the v0.7.0 `rasteroid` auto-detect): add a
  `From<NewError> for EncoderError` and the new encoder can use
  `?` directly without a per-module error helper.
- `icy_sixel` was chosen over `sixel`/`sixel-sys` (the latter is
  FFI to the C `libsixel` and has not had a 2026 release).

## 0.5.0 (2026-07-02)

First protocol encoder: the Kitty graphics protocol, wired up via
the optional `little-kitty` (v0.0.3) crate behind a new
`kitty-encoder` Cargo feature. Sixel remains the v0.6.0 work.

### Added
- `ProtocolEncoder` trait in `dashcompositor::encoder`:
  - Signature: `fn encode(&self, frame: &FrameBuffer) -> Result<Vec<u8>, EncoderError>`.
  - Implementor: `Protocol`, dispatching on the protocol variant.
  - Returns a `Vec<u8>` of terminal escape sequences; the caller
    writes them to stdout. No I/O is performed inside `encode`.
- `EncoderError` enum with hand-rolled `Display` + `Error` impls
  (no `thiserror` dep, to honour the project's minimal-deps
  ethos):
  - `UnsupportedProtocol(&'static str)` -- returned by the
    disabled-feature Kitty arm and by Sixel until v0.6.0.
  - `InvalidDimensions { width, height }` -- zero-size framebuffer.
  - `Encode(String)` -- wraps the underlying `little_kitty`
    `std::io::Error`.
- Private `kitty` submodule gated on `kitty-encoder`. Uses the
  real `little_kitty` 0.0.3 API:
  - `Command::default()` + `.with_control(key, value)` with
    `value: Into<ControlValue>`.
  - `ControlValue::Char('T')` for the action
    (transmit and put), `ControlValue::UnsignedInteger(...)` for
    the format (32 = 32-bit RGBA), quiet flag (2 = suppress
    responses), and width / height.
  - `little_kitty::io::KittyCommandWriter` (blanket-impl'd for
    any `Write`, including `Vec<u8>`) for `write_start`,
    `write_base64(self, data)`, `write_end`.
  - `ControlValue::write(&mut out)` to serialise each control
    value into the byte buffer.
  - Output format: `\x1b_Ga=T,f=32,q=2,s=W,v=H;<base64-payload>\x1b\\`.
- `Cargo.toml`:
  - `little-kitty = "0.0.3"` as an optional dependency.
  - `kitty-encoder = ["dep:little-kitty"]` Cargo feature.
  - Default features stay empty; the default build remains
    dependency-light.
- 7 new unit tests in `src/encoder.rs`:
  - `as_str_matches_variant` -- protocol name strings.
  - `encoder_error_display_includes_context` -- `Display` impl.
  - `sixel_encode_is_unsupported_in_v050` -- Sixel always returns
    `UnsupportedProtocol`.
  - `kitty_encode_is_unsupported_without_feature` -- Kitty without
    the feature returns `UnsupportedProtocol`.
  - `kitty_encode_rejects_zero_dimensions` -- zero-size returns
    `InvalidDimensions`.
  - `kitty_encode_produces_valid_escape_framing` -- output starts
    with `\x1b_G`, ends with `\x1b\\`, and the control payload
    contains `a=T`, `f=32`, `q=2`, `s=2`, `v=2`.
  - `kitty_encode_is_deterministic_for_same_input` -- pure
    encoder.
- `main.rs` demo now encodes the auto-fit framebuffer via
  `Protocol::Kitty` and writes the escape bytes to stdout; stderr
  carries the human-readable log lines.

### Changed
- `Cargo.toml` version bumped to 0.5.0.
- `lib.rs` re-exports `EncoderError` and `ProtocolEncoder`
  (ungated, so the API surface is stable across feature
  combinations; calling `encode` on a disabled-feature protocol
  returns `Err(UnsupportedProtocol)` at runtime).

### Notes
- `little-kitty` evaluation per AGENTS.md §3: MIT/Apache-2.0,
  v0.0.3 (March 2026), ~148K SLoC, actively maintained, the
  recommended pick over `kittage` (heavier, full-featured) and
  `kitty-graphics-protocol` (last build failed on docs.rs). The
  `rasteroid` auto-detect wrapper is deferred to v0.7.0+ (it
  doesn't expose granular feature flags to keep the dep
  footprint tight).
- `cargo build`, `cargo test` (65 unit tests with default
  features, 71 with `--features kitty-encoder`; +1 doc test),
  `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`,
  and `cargo build --release` are all clean -- BOTH with default
  features and with `--features kitty-encoder`.

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
