# Changelog

All notable changes to `dashcompositor` are recorded here. The format
follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and
the project adheres to [Semantic Versioning](https://semver.org/).

## 0.1.0 (2026-07-02)

Initial scaffold of `dashcompositor`, a layer-based graphics compositor
for the terminal that projects a fully composited RGBA framebuffer to
the host via the Kitty graphics protocol or Sixel.

### Added
- MIT `LICENSE` (2026).
- `AGENTS.md` -- operating rules for AI agents and human contributors.
- `README.md` -- project overview, target features, architecture diagram.
- `Cargo.toml` -- package metadata, lib + bin targets, `[lints.rust]
  missing_docs = "warn"`. Candidate feature flags are stubbed but
  commented out per AGENTS.md section 3 until each crate is vetted.
- `src/lib.rs` plus four module stubs mirroring AGENTS.md section 7:
  `compositor`, `layer`, `framebuffer`, `encoder`.
- `src/main.rs` -- no-op binary entry point pending a real
  protocol-detector implementation.
- `.gitignore` extended for Rust build output.
- CI-ready: `cargo build`, `cargo test`, `cargo fmt --check`,
  `cargo clippy --all-targets -- -D warnings`, and
  `cargo build --release` all pass on the scaffold.
- Environment: GPG signing is wired up (loopback pinentry +
  `allow-preset-passphrase`, `user.signingkey` pinned to
  `8CAF4D685F95A842`) so non-interactive commits produce
  verifiable signatures.
