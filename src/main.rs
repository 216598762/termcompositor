//! `dashcompositor` CLI -- terminal-fit layer-stack + auto-detect
//! protocol encoder demo.
//!
//! Demonstrates that a backend (this binary) can:
//! 1. Detect the host terminal's cell-grid size via
//!    [`dashcompositor::TerminalSize`].
//! 2. Build a [`dashcompositor::LayerStack`], add and remove
//!    layers (full-frame [`dashcompositor::SolidColor`],
//!    positioned [`dashcompositor::RectLayer`], and
//!    [`dashcompositor::TextLayer`] placeholder), and control
//!    their opacity / visibility / z-order override.
//! 3. Render the stack into a framebuffer auto-sized to the
//!    terminal via
//!    [`dashcompositor::LayerStack::render_to_current_terminal`].
//! 4. Encode the framebuffer via
//!    [`dashcompositor::ProtocolEncoder`] (auto-detected by
//!    default: `Protocol::Auto` picks Kitty or Sixel based on
//!    `TERM` / `TERM_PROGRAM`; pass `--protocol <kitty|sixel|auto>`
//!    to override; pass `--probe` to use the I/O-based Kitty
//!    query-response probe) and write the escape sequences to
//!    stdout. Stderr is reserved for human-readable logging.

use std::io::Write;

use dashcompositor::{
    detect, LayerStack, Protocol, ProtocolEncoder, RectLayer, SolidColor, TerminalSize, TextLayer,
};
// `detect_with_probe` is only re-exported from `dashcompositor`
// when the `kitty-encoder` Cargo feature is enabled (because the
// probe depends on `little_kitty`). Gate the import accordingly
// so the default build still compiles.
#[cfg(feature = "kitty-encoder")]
use dashcompositor::detect_with_probe;

/// Parse the `--protocol <kitty|sixel|auto>` CLI flag. Returns
/// `None` if the flag is absent (caller should fall back to
/// `Protocol::Auto`).
fn parse_protocol_flag() -> Option<Protocol> {
    let args: Vec<String> = std::env::args().collect();
    let idx = args.iter().position(|a| a == "--protocol")?;
    let val = match args.get(idx + 1) {
        Some(v) => v.as_str(),
        None => {
            eprintln!("warning: --protocol missing value; using auto");
            return None;
        }
    };
    Some(match val {
        "kitty" => Protocol::Kitty,
        "sixel" => Protocol::Sixel,
        "auto" => Protocol::Auto,
        other => {
            eprintln!("warning: unknown --protocol value `{other}`; falling back to `auto`");
            Protocol::Auto
        }
    })
}

/// Parse the `--probe` CLI flag (boolean switch, no value).
fn parse_probe_flag() -> bool {
    std::env::args().any(|a| a == "--probe")
}

fn main() {
    let size = TerminalSize::current();
    eprintln!(
        "dashcompositor v0.7.1 -- multi-layer + auto-detect encoder: \
host terminal = {cols} cols x {rows} rows",
        cols = size.cols,
        rows = size.rows,
    );

    let mut stack = LayerStack::new();

    // 1. Add a full-frame blue background at z=0.
    let bg = stack.push(SolidColor::new(0, 0, 64, 255).with_name("background-blue"));

    // 2. Add a positioned green rect at z=10.
    let rect = stack.push(
        RectLayer::new(
            (size.cols as u32 / 4).max(1),
            (size.rows as u32 / 4).max(1),
            (size.cols as u32 / 2).max(1),
            (size.rows as u32 / 2).max(1),
            [0, 200, 0, 200],
        )
        .with_z(10)
        .with_name("centered-rect"),
    );

    // 3. Add a text placeholder at z=20, anchored top-left.
    let label = stack.push(
        TextLayer::new(2, 1, "dashcompositor", [255, 255, 255, 255])
            .with_z(20)
            .with_name("title"),
    );
    if let Some(entry) = stack.get_mut(label) {
        entry.set_opacity(0.9);
    }

    // 4. Auto-fit the framebuffer to the host terminal and render.
    let (fb, reported) = stack.render_to_current_terminal();
    assert_eq!(reported.cols as u32, fb.width());
    assert_eq!(reported.rows as u32, fb.height());
    eprintln!(
        "rendered {}x{} framebuffer ({} pixels, {} layer(s))",
        fb.width(),
        fb.height(),
        fb.pixels().len(),
        stack.len(),
    );

    // 5. Pick a protocol: explicit --protocol flag wins,
    //    otherwise default to Auto (env-var shim, with --probe
    //    upgrading to the I/O-based Kitty probe).
    let requested = parse_protocol_flag().unwrap_or(Protocol::Auto);
    let use_probe = parse_probe_flag();

    // The concrete protocol we will encode with. If `requested`
    // is `Auto`, resolve it via the pure shim (or the probe if
    // --probe was passed). The `Auto` arm of `encode` would do
    // the same resolution, but resolving here lets us log the
    // detected protocol before encoding.
    let resolved = match requested {
        Protocol::Auto if use_probe => {
            // Authoritative detection via the Kitty probe.
            // Only available when the kitty-encoder feature is
            // enabled; without it, fall back to the env-var shim.
            #[cfg(feature = "kitty-encoder")]
            {
                match detect_with_probe() {
                    Ok(p) => p,
                    Err(e) => {
                        eprintln!(
                            "warning: kitty probe failed ({e}); falling back to env-var shim"
                        );
                        detect()
                    }
                }
            }
            #[cfg(not(feature = "kitty-encoder"))]
            {
                eprintln!(
                    "warning: --probe requires the kitty-encoder feature; \
using the env-var shim instead"
                );
                detect()
            }
        }
        Protocol::Auto => detect(),
        other => other,
    };

    eprintln!(
        "requested protocol: {}; resolved: {}",
        requested.as_str(),
        resolved.as_str(),
    );

    // 6. Encode the framebuffer to escape sequences and write
    //    them to stdout. Stderr is for human-readable log
    //    lines; the raw escape bytes go to stdout.
    let stdout = std::io::stdout();
    let mut handle = stdout.lock();
    match resolved.encode(&fb) {
        Ok(bytes) => {
            eprintln!(
                "encoded {} bytes via {}; writing to stdout",
                bytes.len(),
                resolved.as_str(),
            );
            handle.write_all(&bytes).expect("write to stdout");
            handle.flush().expect("flush stdout");
        }
        Err(e) => {
            eprintln!(
                "encoder error for protocol `{}`: {e} \
(is the required Cargo feature enabled?)",
                resolved.as_str(),
            );
        }
    }

    // Exercise the control API on the rect before the post-render mutations:
    if let Some(entry) = stack.get_mut(rect) {
        entry.set_opacity(0.75);
    }

    // 7. Control at will: hide the title, remove the background,
    //    re-add a new accent layer with a z-override, re-render.
    if let Some(entry) = stack.get_mut(label) {
        entry.set_visible(false);
    }
    let _ = stack.remove(bg);
    let accent = stack.push(SolidColor::new(255, 0, 0, 255).with_name("accent-red"));
    if let Some(entry) = stack.get_mut(accent) {
        entry.set_z_override(100);
    }
    let (fb2, _) = stack.render_to_current_terminal();
    eprintln!(
        "after control: rendered {}x{} framebuffer ({} pixels, {} layer(s))",
        fb2.width(),
        fb2.height(),
        fb2.pixels().len(),
        stack.len(),
    );
    if let Ok(bytes) = resolved.encode(&fb2) {
        eprintln!("re-encoded {} bytes", bytes.len());
    }
}
