# Contributing to `dashcompositor`

Thank you for your interest in contributing! `dashcompositor` is a
layer-based graphics compositor for the terminal, written in Rust.

This file covers the conventions and workflows for human contributors.

## Quick start

```bash
# Build (default features)
cargo build

# Build with all features
cargo build --all-features

# Run tests (all feature combos)
cargo test
cargo test --features kitty-encoder,sixel-encoder,image-decoder

# Run lints
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings

# Run benchmarks
cargo bench
```

## Feature matrix

`dashcompositor` uses Cargo features to keep the default build
dependency-light. Every PR must be tested against all relevant
feature combinations:

| Feature             | Default | Purpose                              |
| ------------------- | :-----: | ------------------------------------ |
| `font-rasterizer`   | **on**  | Real glyph rendering in `TextLayer`  |
| `kitty-encoder`     |   off   | Kitty graphics protocol encoder      |
| `sixel-encoder`     |   off   | Sixel encoder                        |
| `image-decoder`     |   off   | `ImageLayer` (PNG + JPEG)            |

At minimum, run `cargo test` with:
- Default features (`cargo test`)
- `--features kitty-encoder`
- `--features sixel-encoder`
- `--features kitty-encoder,sixel-encoder`
- `--no-default-features` (verifies the placeholder fallback)
- `--no-default-features --features font-rasterizer,kitty-encoder,sixel-encoder`

## Commit messages

Follow [Conventional Commits](https://www.conventionalcommits.org/):

```
<type>: short description (≤72 chars)

Longer body explaining *what* and *why*. Wrap at 72 characters.
Reference relevant design decisions.
```

Types: `feat`, `fix`, `docs`, `refactor`, `test`, `chore`, `ci`.

All commits must be **signed** (GPG). The project uses a
loopback-pinentry GPG agent; `git commit -S` is the norm.

## Code style

- **rustfmt**: All code must be formatted with `cargo fmt --all`.
  CI enforces this.
- **clippy**: `cargo clippy --all-targets -- -D warnings` must pass.
  No warnings allowed in library or binary code.
- **Documentation**: Every `pub` item must have a doc comment.
  `#[warn(missing_docs)]` is set in `Cargo.toml`.
- **No unwrap/expect in library code**: Tests and examples may use
  them, but library paths must handle errors gracefully.
- **No dead code**: `#[allow(dead_code)]` should be rare and
  justified in a comment.

## MSRV policy

The Minimum Supported Rust Version is **1.73**. This is pinned in
`Cargo.toml` via `rust-version = "1.73"`. CI validates the crate
compiles on this version.

The MSRV is bumped only when a dependency or language feature
requires it. MSRV bumps are documented in `CHANGELOG.md`.

## Pull request process

1. Branch from `main`.
2. Make your changes, following the code style and testing
   requirements above.
3. Run the full validation chain:
   ```bash
   cargo fmt --all -- --check
   cargo build
   cargo test
   cargo clippy --all-targets -- -D warnings
   ```
4. Push and open a PR targeting `main`.
5. Ensure CI passes (all feature combos).
6. Squash-merge with a descriptive commit message.

## Adding dependencies

Before adding a new dependency:
1. Search [`awesome-rust`](https://github.com/rust-unofficial/awesome-rust)
   for existing options.
2. Run `cargo search <crate>` and verify the latest version,
   license, and maintenance status.
3. Add the dependency as **optional** behind a Cargo feature flag
   unless it is required for the core library.
4. Document *why* the crate was chosen over alternatives in the
   `Cargo.toml` comment.

## License

By contributing, you agree that your contributions will be licensed
under the MIT License (see [`LICENSE`](./LICENSE)).
