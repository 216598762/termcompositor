//! `dashcompositor` CLI -- terminal-fit layer-stack + Kitty encoder
//! demo.
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
//!    [`dashcompositor::ProtocolEncoder`] (Kitty when the
//!    `kitty-encoder` feature is enabled) and write the escape
//!    sequences to stdout. Stderr is reserved for human-readable
//!    logging.

use std::io::Write;

use dashcompositor::{
    LayerStack, Protocol, ProtocolEncoder, RectLayer, SolidColor, TerminalSize, TextLayer,
};

fn main() {
    let size = TerminalSize::current();
    eprintln!(
        "dashcompositor v0.5.0 -- multi-layer + Kitty encoder: \
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

    // 5. Encode the framebuffer to Kitty escape sequences and
    //    write them to stdout. Stderr is for human-readable log
    //    lines; the raw escape bytes go to stdout.
    let protocol = Protocol::Kitty;
    let stdout = std::io::stdout();
    let mut handle = stdout.lock();
    match protocol.encode(&fb) {
        Ok(bytes) => {
            eprintln!(
                "encoded {} bytes via {}; writing to stdout",
                bytes.len(),
                protocol.as_str(),
            );
            handle.write_all(&bytes).expect("write to stdout");
            handle.flush().expect("flush stdout");
        }
        Err(e) => {
            eprintln!(
                "encoder error for protocol `{}`: {e} (is the required Cargo feature enabled?)",
                protocol.as_str(),
            );
        }
    }

    // Exercise the control API on the rect before the post-render mutations:
    if let Some(entry) = stack.get_mut(rect) {
        entry.set_opacity(0.75);
    }

    // 6. Control at will: hide the title, remove the background,
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
    if let Ok(bytes) = protocol.encode(&fb2) {
        eprintln!("re-encoded {} bytes", bytes.len());
    }
}
