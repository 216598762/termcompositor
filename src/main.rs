//! `dashcompositor` CLI -- terminal-fit layer-stack demo.
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
//! 4. Report the terminal size back through the API.

use dashcompositor::{LayerStack, RectLayer, SolidColor, TerminalSize, TextLayer};

fn main() {
    let size = TerminalSize::current();
    eprintln!(
        "dashcompositor v0.4.0 -- multi-layer compositor: \
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
    eprintln!(
        "  bg    : {}",
        stack.get(bg).map_or("<missing>", |e| e.name()),
    );
    eprintln!(
        "  rect  : {} (bounds={:?})",
        stack.get(rect).map_or("<missing>", |e| e.name()),
        stack.get(rect).and_then(|e| e.layer().bounds()),
    );
    eprintln!(
        "  label : {:?}",
        stack.get(label).map(|e| e.layer().bounds()),
    );

    // 5. Control at will: hide the title, remove the background,
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
}
