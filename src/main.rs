//! `termcompositor` CLI -- terminal-fit layer-stack + auto-detect
//! protocol encoder demo.
//!
//! Demonstrates that a backend (this binary) can:
//! 1. Detect the host terminal's cell-grid size via
//!    [`termcompositor::TerminalSize`].
//! 2. Build a [`termcompositor::LayerStack`], add and remove
//!    layers (full-frame [`termcompositor::SolidColor`],
//!    positioned [`termcompositor::RectLayer`], and
//!    [`termcompositor::TextLayer`] placeholder), and control
//!    their opacity / visibility / z-order override.
//! 3. Render the stack into a framebuffer auto-sized to the
//!    terminal via
//!    [`termcompositor::LayerStack::render_to_current_terminal`].
//! 4. Encode the framebuffer via
//!    [`termcompositor::ProtocolEncoder`] (auto-detected by
//!    default: `Protocol::Auto` picks Kitty or Sixel based on
//!    `TERM` / `TERM_PROGRAM`; pass `--protocol <kitty|sixel|auto>`
//!    to override; pass `--probe` to use the I/O-based Kitty
//!    query-response probe) and write the escape sequences to
//!    stdout. Stderr is reserved for human-readable logging.
//!
//! v0.8.0 adds `--tmux-passthrough`: when the host is running
//! inside tmux, the Kitty arm wraps its APC output in a tmux
//! passthrough DCS (ESC P tmux ; ... ESC \\) so the bytes
//! survive the tmux -> outer-terminal hop. The opt-in env
//! var is `TMUXPASSTHROUGH` (any non-empty value, typically
//! `TMUXPASSTHROUGH=1`). The flag sets that env var
//! before calling `detect` / `dispatch`, so the resulting
//! protocol + wrapping decision matches the one a user would
//! get by exporting the var themselves.
//!
//! v0.14.0 adds `--animate` and `--fps <N>`: enters the
//! animation loop with a live demo of the layer control API.

use std::io::Write;

use termcompositor::animation::{self, AnimConfig};
use termcompositor::{
    detect, LayerStack, Protocol, ProtocolEncoder, RectLayer, SolidColor, TerminalSize, TextLayer,
};
// `detect_with_probe` is only re-exported from `termcompositor`
// when the `kitty-encoder` Cargo feature is enabled (because the
// probe depends on `little_kitty`). Gate the import accordingly
// so the default build still compiles.
#[cfg(feature = "kitty-encoder")]
use termcompositor::detect_with_probe;

/// Parse the `--protocol <kitty|sixel|auto>` CLI flag from the
/// given argument list. Returns `None` if the flag is absent
/// (caller should fall back to `Protocol::Auto`).
fn parse_protocol_flag_from(args: &[String]) -> Option<Protocol> {
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

/// Parse the `--probe` CLI flag (boolean switch, no value)
/// from the given argument list.
fn parse_probe_flag_from(args: &[String]) -> bool {
    args.iter().any(|a| a == "--probe")
}

/// Parse the `--tmux-passthrough` CLI flag (v0.8.0; boolean
/// switch, no value) from the given argument list.
fn parse_tmux_passthrough_flag_from(args: &[String]) -> bool {
    args.iter().any(|a| a == "--tmux-passthrough")
}

/// Parse the `--animate` CLI flag (v0.14.0; boolean switch, no
/// value) from the given argument list.
fn parse_animate_flag_from(args: &[String]) -> bool {
    args.iter().any(|a| a == "--animate")
}

/// Parse the `--fps <N>` CLI flag from the given argument list.
/// Returns `None` if the flag is absent.
fn parse_fps_flag_from(args: &[String]) -> Option<f64> {
    let idx = args.iter().position(|a| a == "--fps")?;
    match args.get(idx + 1) {
        Some(v) => v.parse::<f64>().ok().filter(|f| *f > 0.0),
        None => {
            eprintln!("warning: --fps missing value; using default 30");
            Some(30.0)
        }
    }
}

/// RAII guard for the `TMUXPASSTHROUGH` env var set by
/// `--tmux-passthrough`. Saves the current value on
/// construction and restores it on `Drop`. Uses
/// `std::env::set_var` / `std::env::remove_var` (the v0.7.1
/// `with_env` test-helper pattern is parallel but lives
/// inside the test module -- this is a single-env-var
/// ad-hoc version for `main`).
struct TmuxPassthroughGuard {
    saved: Option<String>,
}

impl TmuxPassthroughGuard {
    fn set(value: Option<&str>) -> Self {
        let saved = std::env::var("TMUXPASSTHROUGH").ok();
        match value {
            Some(v) => std::env::set_var("TMUXPASSTHROUGH", v),
            None => std::env::remove_var("TMUXPASSTHROUGH"),
        }
        Self { saved }
    }
}

impl Drop for TmuxPassthroughGuard {
    fn drop(&mut self) {
        match self.saved.as_ref() {
            Some(v) => std::env::set_var("TMUXPASSTHROUGH", v),
            None => std::env::remove_var("TMUXPASSTHROUGH"),
        }
    }
}

/// Build the demo layer stack with background, centered rect,
/// and text label. Returns the stack and the IDs of the
/// background and rect layers (for post-render mutations).
fn build_demo_stack(
    size: TerminalSize,
) -> (LayerStack, termcompositor::LayerId, termcompositor::LayerId) {
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
        TextLayer::new(2, 1, "termcompositor", [255, 255, 255, 255])
            .with_z(20)
            .with_name("title"),
    );
    if let Some(entry) = stack.get_mut(label) {
        entry.set_opacity(0.9);
    }

    let _ = label;
    (stack, bg, rect)
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let size = TerminalSize::current();
    eprintln!(
        "termcompositor v0.14.0 -- multi-layer + auto-detect encoder + animation: \
host terminal = {cols} cols x {rows} rows",
        cols = size.cols,
        rows = size.rows,
    );

    // v0.8.0 tmux passthrough: parse the flag FIRST so the
    // `TMUXPASSTHROUGH` env var is set for the rest of
    // `main` (including the `detect` / `detect_with_probe`
    // calls below). The guard restores the previous value
    // on `Drop` (i.e. on exit), so a shell that has
    // `TMUXPASSTHROUGH=1` exported in its rc is unaffected.
    //
    // IMPORTANT: only create the guard when the flag IS
    // set. If we always create the guard, the
    // `TmuxPassthroughGuard::set(None)` call would REMOVE
    // the user's `TMUXPASSTHROUGH` env var (replacing it
    // with nothing), which would silently disable the
    // passthrough for a user who exported the var in
    // their shell rc but didn't pass `--tmux-passthrough`.
    // The fix: when the flag is absent, do nothing --
    // the user's existing env is respected as-is.
    let tmux_passthrough = parse_tmux_passthrough_flag_from(&args);
    let _passthrough_guard = if tmux_passthrough {
        Some(TmuxPassthroughGuard::set(Some("1")))
    } else {
        None
    };

    // Resolve protocol for both single-shot and animation modes.
    let requested = parse_protocol_flag_from(&args).unwrap_or(Protocol::Auto);
    let use_probe = parse_probe_flag_from(&args);

    let resolved = match requested {
        Protocol::Auto if use_probe => {
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
    eprintln!(
        "tmux passthrough: {}",
        if tmux_passthrough {
            "enabled (TMUXPASSTHROUGH=1)"
        } else {
            "disabled (set --tmux-passthrough or TMUXPASSTHROUGH=1 to opt in)"
        },
    );

    // Animation mode: if --animate was passed, enter the
    // animation loop with a live demo. Ctrl+C to exit.
    if parse_animate_flag_from(&args) {
        let fps = parse_fps_flag_from(&args).unwrap_or(30.0);
        eprintln!("entering animation mode at {fps:.0} FPS (Ctrl+C to exit)");

        let mut stack = LayerStack::new();
        let _bg = stack.push(
            SolidColor::new(0, 0, 64, 255)
                .with_z(0)
                .with_name("anim-bg"),
        );
        let bar = stack.push(
            RectLayer::new(2, 10, 20, 5, [0, 200, 0, 255])
                .with_z(10)
                .with_name("anim-bar"),
        );
        let _label = stack.push(
            TextLayer::new(2, 1, "termcompositor v0.14.0 — animation demo", [255; 4])
                .with_z(20)
                .with_name("anim-title"),
        );

        let config = AnimConfig::new(fps)
            .with_protocol(resolved)
            .with_clear_between_frames(true);

        animation::run_with_config(stack, config, move |ctx| {
            let t = ctx.elapsed().as_secs_f32();

            // Animate the bar opacity using a sine wave.
            let opacity = (t * 2.0).sin() * 0.5 + 0.5;
            if let Some(entry) = ctx.layers_mut().get_mut(bar) {
                entry.set_opacity(opacity);
            }

            // Log frame info to stderr (stdout is for escape sequences).
            if ctx.frame_count() % 30 == 0 {
                eprintln!(
                    "frame {} | dt={:?} | elapsed={:.1}s | {}x{}",
                    ctx.frame_count(),
                    ctx.delta_time(),
                    ctx.elapsed().as_secs_f32(),
                    ctx.width(),
                    ctx.height(),
                );
            }

            ctx.request_redraw();
        });
    }

    // Single-shot demo mode (original behavior).
    let (mut stack, bg, rect) = build_demo_stack(size);

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

    // 7. Control at will: remove the background,
    //    re-add a new accent layer with a z-override, re-render.
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

#[cfg(test)]
mod tests {
    use super::*;

    // ── parse_protocol_flag_from ──────────────────────────────

    #[test]
    fn parse_protocol_kitty() {
        let args: Vec<String> = vec!["--protocol", "kitty"]
            .into_iter()
            .map(String::from)
            .collect();
        assert_eq!(parse_protocol_flag_from(&args), Some(Protocol::Kitty));
    }

    #[test]
    fn parse_protocol_sixel() {
        let args: Vec<String> = vec!["--protocol", "sixel"]
            .into_iter()
            .map(String::from)
            .collect();
        assert_eq!(parse_protocol_flag_from(&args), Some(Protocol::Sixel));
    }

    #[test]
    fn parse_protocol_auto() {
        let args: Vec<String> = vec!["--protocol", "auto"]
            .into_iter()
            .map(String::from)
            .collect();
        assert_eq!(parse_protocol_flag_from(&args), Some(Protocol::Auto));
    }

    #[test]
    fn parse_protocol_unknown_value_falls_back_to_auto() {
        let args: Vec<String> = vec!["--protocol", "unknown"]
            .into_iter()
            .map(String::from)
            .collect();
        // Prints a warning to stderr, returns Some(Auto)
        assert_eq!(parse_protocol_flag_from(&args), Some(Protocol::Auto));
    }

    #[test]
    fn parse_protocol_missing_value_warns() {
        let args: Vec<String> = vec!["--protocol"].into_iter().map(String::from).collect();
        assert_eq!(parse_protocol_flag_from(&args), None);
    }

    #[test]
    fn parse_protocol_first_wins_on_duplicates() {
        let args: Vec<String> = vec!["--protocol", "kitty", "--protocol", "sixel"]
            .into_iter()
            .map(String::from)
            .collect();
        assert_eq!(parse_protocol_flag_from(&args), Some(Protocol::Kitty));
    }

    #[test]
    fn parse_protocol_absent_returns_none() {
        let args: Vec<String> = vec![];
        assert_eq!(parse_protocol_flag_from(&args), None);
    }

    #[test]
    fn parse_protocol_with_other_flags() {
        let args: Vec<String> = vec!["--probe", "--protocol", "sixel", "--tmux-passthrough"]
            .into_iter()
            .map(String::from)
            .collect();
        assert_eq!(parse_protocol_flag_from(&args), Some(Protocol::Sixel));
    }

    // ── parse_probe_flag_from ─────────────────────────────────

    #[test]
    fn parse_probe_present() {
        let args: Vec<String> = vec!["--probe"].into_iter().map(String::from).collect();
        assert!(parse_probe_flag_from(&args));
    }

    #[test]
    fn parse_probe_absent() {
        let args: Vec<String> = vec!["--protocol", "kitty"]
            .into_iter()
            .map(String::from)
            .collect();
        assert!(!parse_probe_flag_from(&args));
    }

    #[test]
    fn parse_probe_with_other_flags() {
        let args: Vec<String> = vec!["--protocol", "sixel", "--probe", "--tmux-passthrough"]
            .into_iter()
            .map(String::from)
            .collect();
        assert!(parse_probe_flag_from(&args));
    }

    // ── parse_tmux_passthrough_flag_from ──────────────────────

    #[test]
    fn parse_tmux_passthrough_present() {
        let args: Vec<String> = vec!["--tmux-passthrough"]
            .into_iter()
            .map(String::from)
            .collect();
        assert!(parse_tmux_passthrough_flag_from(&args));
    }

    #[test]
    fn parse_tmux_passthrough_absent() {
        let args: Vec<String> = vec![];
        assert!(!parse_tmux_passthrough_flag_from(&args));
    }

    // ── parse_animate_flag_from ───────────────────────────────

    #[test]
    fn parse_animate_present() {
        let args: Vec<String> = vec!["--animate"].into_iter().map(String::from).collect();
        assert!(parse_animate_flag_from(&args));
    }

    #[test]
    fn parse_animate_absent() {
        let args: Vec<String> = vec!["--protocol", "kitty"]
            .into_iter()
            .map(String::from)
            .collect();
        assert!(!parse_animate_flag_from(&args));
    }

    #[test]
    fn parse_animate_with_fps() {
        let args: Vec<String> = vec!["--animate", "--fps", "60"]
            .into_iter()
            .map(String::from)
            .collect();
        assert!(parse_animate_flag_from(&args));
        assert_eq!(parse_fps_flag_from(&args), Some(60.0));
    }

    // ── parse_fps_flag_from ───────────────────────────────────

    #[test]
    fn parse_fps_present() {
        let args: Vec<String> = vec!["--fps", "30"].into_iter().map(String::from).collect();
        assert_eq!(parse_fps_flag_from(&args), Some(30.0));
    }

    #[test]
    fn parse_fps_absent() {
        let args: Vec<String> = vec!["--animate"].into_iter().map(String::from).collect();
        assert_eq!(parse_fps_flag_from(&args), None);
    }

    #[test]
    fn parse_fps_invalid_value() {
        let args: Vec<String> = vec!["--fps", "abc"].into_iter().map(String::from).collect();
        assert_eq!(parse_fps_flag_from(&args), None);
    }

    #[test]
    fn parse_fps_zero_value() {
        let args: Vec<String> = vec!["--fps", "0"].into_iter().map(String::from).collect();
        assert_eq!(parse_fps_flag_from(&args), None);
    }

    #[test]
    fn parse_fps_missing_value_warns() {
        let args: Vec<String> = vec!["--fps"].into_iter().map(String::from).collect();
        assert_eq!(parse_fps_flag_from(&args), Some(30.0));
    }

    // ── build_demo_stack ──────────────────────────────────────

    #[test]
    fn build_demo_stack_returns_three_layers() {
        let size = TerminalSize { rows: 24, cols: 80 };
        let (stack, _bg, _rect) = build_demo_stack(size);
        assert_eq!(stack.len(), 3);
    }

    #[test]
    fn build_demo_stack_rect_scales_with_terminal_size() {
        let (stack1, _, _) = build_demo_stack(TerminalSize { rows: 24, cols: 80 });
        let (stack2, _, _) = build_demo_stack(TerminalSize {
            rows: 48,
            cols: 160,
        });
        assert_eq!(stack1.len(), 3);
        assert_eq!(stack2.len(), 3);
        // Rect in the larger terminal should be wider/taller.
        let bounds1 = stack1.entries()[1].layer().bounds();
        let bounds2 = stack2.entries()[1].layer().bounds();
        // The rect is the 2nd layer (index 1).
        if let (Some(r1), Some(r2)) = (bounds1, bounds2) {
            assert!(
                r2.width > r1.width,
                "larger terminal should produce wider rect"
            );
            assert!(
                r2.height > r1.height,
                "larger terminal should produce taller rect"
            );
        }
    }

    #[test]
    fn build_demo_stack_small_terminal_min_dimensions() {
        // Even a 1x1 terminal should produce valid dimensions (max(1))
        let (stack, _bg, _rect) = build_demo_stack(TerminalSize { rows: 1, cols: 1 });
        assert_eq!(stack.len(), 3);
    }

    #[test]
    fn build_demo_stack_label_opacity() {
        let size = TerminalSize { rows: 24, cols: 80 };
        let (stack, _bg, _rect) = build_demo_stack(size);
        // The label (3rd layer, index 2) should have opacity 0.9
        assert_eq!(stack.entries()[2].opacity(), 0.9);
    }

    // ── TmuxPassthroughGuard ──────────────────────────────────

    /// Consolidated test for TmuxPassthroughGuard env var management.
    /// All env var tests are in a single function to avoid parallel
    /// execution race conditions (env vars are process-global state).
    #[test]
    fn guard_all_env_var_scenarios() {
        std::env::remove_var("TMUXPASSTHROUGH");

        // --- Test 1: set(Some) when absent sets var, drop removes it ---
        {
            let _guard1 = TmuxPassthroughGuard::set(Some("1"));
            assert_eq!(std::env::var("TMUXPASSTHROUGH").unwrap(), "1");
        }
        assert!(
            std::env::var("TMUXPASSTHROUGH").is_err(),
            "should be removed after drop when absent before"
        );

        // --- Test 2: set(None) when present removes var, drop restores ---
        std::env::set_var("TMUXPASSTHROUGH", "existing");
        {
            let _guard = TmuxPassthroughGuard::set(None);
            assert!(std::env::var("TMUXPASSTHROUGH").is_err());
        }
        assert_eq!(std::env::var("TMUXPASSTHROUGH").unwrap(), "existing");

        // --- Test 3: set(Some) when present overrides, drop restores ---
        std::env::set_var("TMUXPASSTHROUGH", "old");
        {
            let _guard2 = TmuxPassthroughGuard::set(Some("new"));
            assert_eq!(std::env::var("TMUXPASSTHROUGH").unwrap(), "new");
        }
        assert_eq!(std::env::var("TMUXPASSTHROUGH").unwrap(), "old");

        // --- Test 4: nested guards restore in LIFO order ---
        std::env::set_var("TMUXPASSTHROUGH", "original");
        {
            let _outer = TmuxPassthroughGuard::set(Some("outer"));
            assert_eq!(std::env::var("TMUXPASSTHROUGH").unwrap(), "outer");
            {
                let _inner = TmuxPassthroughGuard::set(Some("inner"));
                assert_eq!(std::env::var("TMUXPASSTHROUGH").unwrap(), "inner");
            }
            assert_eq!(std::env::var("TMUXPASSTHROUGH").unwrap(), "outer");
        }
        assert_eq!(std::env::var("TMUXPASSTHROUGH").unwrap(), "original");

        // --- Test 5: set(None) when absent leaves absent ---
        std::env::remove_var("TMUXPASSTHROUGH");
        {
            let _guard = TmuxPassthroughGuard::set(None);
            assert!(std::env::var("TMUXPASSTHROUGH").is_err());
        }
        assert!(std::env::var("TMUXPASSTHROUGH").is_err());

        // Cleanup
        std::env::remove_var("TMUXPASSTHROUGH");
    }
}
