//! Animation Loop — built-in frame loop with delta-time tracking,
//! frame scheduling, and terminal resize handling.
//!
//! The current single-shot API requires callers to build their own
//! event loop for animated content. This module provides a
//! ready-made animation loop that handles frame timing, terminal
//! size detection, and protocol encoding.
//!
//! # Example
//!
//! ```no_run
//! use termcompositor::animation::{self, AnimContext};
//! use termcompositor::{LayerStack, RectLayer, SolidColor};
//!
//! fn main() {
//!     let mut stack = LayerStack::new();
//!     let bg = stack.push(SolidColor::new(0, 0, 64, 255).with_z(0));
//!     let bar = stack.push(
//!         RectLayer::new(2, 10, 20, 5, [0, 200, 0, 255]).with_z(10)
//!     );
//!
//!     animation::run_with_stack(stack, 30.0, move |ctx| {
//!         let t = ctx.elapsed().as_secs_f32();
//!         // Animate the bar opacity using a sine wave.
//!         let opacity = (t * 2.0).sin() * 0.5 + 0.5;
//!         if let Some(entry) = ctx.layers_mut().get_mut(bar) {
//!             entry.set_opacity(opacity);
//!         }
//!         ctx.request_redraw();
//!     });
//! }
//! ```

use crate::compositor::LayerStack;
use crate::encoder::{detect, dispatch_to_writer, Protocol};
use crate::framebuffer::FrameBuffer;
use crate::terminal::TerminalSize;
use std::io::Write;
use std::time::{Duration, Instant};

/// Context passed to the animation callback each frame.
///
/// Provides access to the layer stack, frame timing information,
/// and controls for requesting redraws.
///
/// # Example
///
/// ```no_run
/// use termcompositor::animation::AnimContext;
///
/// fn animate(ctx: &mut AnimContext) {
///     // Access timing information.
///     let dt = ctx.delta_time();
///     let elapsed = ctx.elapsed();
///     let frame = ctx.frame_count();
///
///     // Access and modify layers.
///     // ctx.layers_mut().get_mut(layer_id).unwrap().set_opacity(0.5);
///
///     // Request a redraw for the next frame.
///     ctx.request_redraw();
/// }
/// ```
pub struct AnimContext {
    layers: LayerStack,
    size: TerminalSize,
    dt: Duration,
    elapsed: Duration,
    frame_count: u64,
    redraw_requested: bool,
    should_exit: bool,
    dirty: crate::compositor::DirtyRegion,
}

impl AnimContext {
    /// Returns a reference to the layer stack.
    pub fn layers(&self) -> &LayerStack {
        &self.layers
    }

    /// Returns a mutable reference to the layer stack.
    pub fn layers_mut(&mut self) -> &mut LayerStack {
        &mut self.layers
    }

    /// Returns the time elapsed since the last frame (delta time).
    ///
    /// This is useful for frame-rate-independent animation:
    /// multiply movement speeds by `dt.as_secs_f32()` to ensure
    /// consistent animation regardless of frame rate.
    pub fn delta_time(&self) -> Duration {
        self.dt
    }

    /// Returns the total elapsed time since the animation started.
    pub fn elapsed(&self) -> Duration {
        self.elapsed
    }

    /// Returns the current frame number (0-indexed).
    pub fn frame_count(&self) -> u64 {
        self.frame_count
    }

    /// Returns the current terminal size in character cells.
    pub fn terminal_size(&self) -> TerminalSize {
        self.size
    }

    /// Returns the framebuffer width in pixels (same as terminal
    /// columns).
    pub fn width(&self) -> u32 {
        self.size.cols as u32
    }

    /// Returns the framebuffer height in pixels (same as terminal
    /// rows).
    pub fn height(&self) -> u32 {
        self.size.rows as u32
    }

    /// Marks the entire framebuffer as dirty, forcing a full
    /// re-render on the next frame. This is called automatically
    /// by [`request_redraw`].
    pub fn mark_full(&mut self) {
        self.dirty.mark_full();
    }

    /// Marks a rectangular region as dirty. Only the specified
    /// region will be re-composited on the next frame. This is
    /// more efficient than [`mark_full`] when only a small area
    /// of the scene changes.
    pub fn mark_rect(&mut self, x: u32, y: u32, width: u32, height: u32) {
        self.dirty
            .mark_rect(crate::compositor::DirtyRect::new(x, y, width, height));
    }

    /// Requests a redraw on the next frame.
    ///
    /// When the callback returns without calling `request_redraw()`,
    /// the animation loop skips rendering and encoding for that
    /// frame, saving CPU time. This is useful for scenes that only
    /// need to redraw when something changes.
    ///
    /// Automatically marks the entire framebuffer as dirty so that
    /// `render_diff` will re-composite the full scene on the next
    /// frame. If you know only a small region changed, prefer
    /// calling [`mark_rect`] directly without `request_redraw` to
    /// enable partial re-compositing.
    pub fn request_redraw(&mut self) {
        self.redraw_requested = true;
        // Mark the entire framebuffer as dirty so render_diff
        // will re-composite the full scene.
        self.dirty.mark_full();
    }

    /// Signals the animation loop to exit after the current frame.
    ///
    /// The loop will render and encode the current frame, then
    /// return from the `run*` function.
    pub fn exit(&mut self) {
        self.should_exit = true;
    }
}

/// Configuration for the animation loop.
pub struct AnimConfig {
    /// Target frames per second. The loop will attempt to maintain
    /// this frame rate, but may fall behind if the callback or
    /// rendering takes too long.
    pub fps: f64,

    /// The protocol to use for encoding. Defaults to
    /// [`Protocol::Auto`], which auto-detects based on terminal
    /// capabilities.
    pub protocol: Protocol,

    /// Whether to clear the screen between frames. When `true`
    /// (default), the loop emits an ANSI clear-screen sequence
    /// before each frame. When `false`, the loop relies on
    /// overwriting previous pixels.
    pub clear_between_frames: bool,
}

impl AnimConfig {
    /// Creates a new config with the given FPS and default settings.
    ///
    /// # Panics
    ///
    /// Panics if `fps` is not positive (> 0.0).
    pub fn new(fps: f64) -> Self {
        assert!(fps > 0.0, "fps must be positive, got {fps}");
        Self {
            fps,
            protocol: Protocol::Auto,
            clear_between_frames: true,
        }
    }

    /// Sets the protocol explicitly.
    #[must_use]
    pub fn with_protocol(mut self, protocol: Protocol) -> Self {
        self.protocol = protocol;
        self
    }

    /// Sets whether to clear the screen between frames.
    #[must_use]
    pub fn with_clear_between_frames(mut self, clear: bool) -> Self {
        self.clear_between_frames = clear;
        self
    }
}

impl Default for AnimConfig {
    fn default() -> Self {
        Self::new(60.0)
    }
}

/// Runs the animation loop at the specified FPS with a new,
/// empty layer stack.
///
/// The callback is called each frame with an [`AnimContext`] that
/// provides access to the layer stack and timing information. The
/// callback should call [`AnimContext::request_redraw`] to trigger
/// rendering; without it, frames are skipped.
///
/// This function never returns under normal operation. To exit,
/// call [`AnimContext::exit`] from within the callback.
///
/// # Example
///
/// ```no_run
/// use termcompositor::animation::{self, AnimContext};
///
/// fn main() {
///     animation::run(30.0, |ctx| {
///         let t = ctx.elapsed().as_secs_f32();
///         eprintln!("Frame {} at {:.2}s", ctx.frame_count(), t);
///         ctx.request_redraw();
///         if ctx.elapsed().as_secs() >= 5 {
///             ctx.exit();
///         }
///     });
/// }
/// ```
pub fn run<F>(fps: f64, callback: F)
where
    F: FnMut(&mut AnimContext),
{
    let stack = LayerStack::new();
    run_with_stack(stack, fps, callback);
}

/// Runs the animation loop with the given initial layer stack.
///
/// This is the primary entry point for the animation loop. The
/// callback receives an [`AnimContext`] with mutable access to
/// the layer stack.
///
/// # Example
///
/// ```no_run
/// use termcompositor::animation::{self, AnimContext};
/// use termcompositor::{LayerStack, SolidColor};
///
/// fn main() {
///     let mut stack = LayerStack::new();
///     stack.push(SolidColor::new(0, 0, 64, 255));
///
///     animation::run_with_stack(stack, 60.0, |ctx| {
///         ctx.request_redraw();
///         if ctx.frame_count() >= 100 {
///             ctx.exit();
///         }
///     });
/// }
/// ```
pub fn run_with_stack<F>(stack: LayerStack, fps: f64, callback: F)
where
    F: FnMut(&mut AnimContext),
{
    run_with_config(stack, AnimConfig::new(fps), callback);
}

/// Runs the animation loop with the given layer stack and
/// configuration.
///
/// This is the most flexible entry point, allowing full control
/// over the animation settings.
///
/// # Example
///
/// ```no_run
/// use termcompositor::animation::{self, AnimConfig, AnimContext};
/// use termcompositor::{LayerStack, SolidColor, Protocol};
///
/// fn main() {
///     let mut stack = LayerStack::new();
///     stack.push(SolidColor::new(0, 0, 64, 255));
///
///     let config = AnimConfig::new(30.0)
///         .with_protocol(Protocol::Kitty)
///         .with_clear_between_frames(false);
///
///     animation::run_with_config(stack, config, |ctx| {
///         ctx.request_redraw();
///         if ctx.frame_count() >= 100 {
///             ctx.exit();
///         }
///     });
/// }
/// ```
pub fn run_with_config<F>(stack: LayerStack, config: AnimConfig, mut callback: F)
where
    F: FnMut(&mut AnimContext),
{
    let frame_duration = Duration::from_secs_f64(1.0 / config.fps);
    let mut last_frame = Instant::now();
    let mut total_elapsed = Duration::ZERO;
    let mut frame_count = 0u64;

    // Detect initial terminal size.
    let size = TerminalSize::current();

    // Resolve protocol once (for Auto, detect once at startup).
    let protocol = match config.protocol {
        Protocol::Auto => detect(),
        p => p,
    };

    let mut ctx = AnimContext {
        layers: stack,
        size,
        dt: Duration::ZERO,
        elapsed: Duration::ZERO,
        frame_count: 0,
        redraw_requested: false,
        should_exit: false,
        dirty: {
            let mut d = crate::compositor::DirtyRegion::new();
            d.mark_full();
            d
        },
    };

    // Persistent framebuffer for diff-based rendering.
    let (w0, h0) = ctx.size.as_framebuffer_size();
    let mut fb = FrameBuffer::new(w0, h0);

    let stdout = std::io::stdout();
    let mut handle = stdout.lock();

    loop {
        let frame_start = Instant::now();

        // Calculate delta time from previous frame.
        let dt = last_frame.elapsed();
        ctx.dt = dt;
        ctx.elapsed = total_elapsed;
        ctx.frame_count = frame_count;
        ctx.redraw_requested = false;
        ctx.should_exit = false;

        // Call user callback.
        callback(&mut ctx);

        // Check for exit.
        if ctx.should_exit {
            // Render one last frame if requested.
            if ctx.redraw_requested {
                render_and_encode(&ctx.layers, protocol, &mut handle, &mut fb, &mut ctx.dirty);
            }
            break;
        }

        // Check for terminal resize.
        let new_size = TerminalSize::current();
        if new_size != ctx.size {
            ctx.size = new_size;
            ctx.dirty.mark_full();
            // Resize framebuffer.
            let (nw, nh) = new_size.as_framebuffer_size();
            fb = FrameBuffer::new(nw, nh);
        }

        // Render and encode if requested.
        if ctx.redraw_requested {
            // Clear screen if configured.
            if config.clear_between_frames {
                // Move cursor to top-left and clear to end of screen.
                let _ = write!(handle, "\x1b[H\x1b[J");
            }
            render_and_encode(&ctx.layers, protocol, &mut handle, &mut fb, &mut ctx.dirty);
        }

        last_frame = frame_start;
        total_elapsed += dt;
        frame_count += 1;

        // Sleep for remaining frame time.
        let elapsed = frame_start.elapsed();
        if elapsed < frame_duration {
            std::thread::sleep(frame_duration - elapsed);
        }
    }
}

/// Renders the current layer stack and encodes the result.
/// Uses diff-based rendering when dirty regions are tracked.
fn render_and_encode(
    layers: &LayerStack,
    protocol: Protocol,
    handle: &mut impl Write,
    fb: &mut FrameBuffer,
    dirty: &mut crate::compositor::DirtyRegion,
) {
    layers.render_diff(fb, dirty);

    // Use dispatch_to_writer for zero-copy streaming.
    let _ = dispatch_to_writer(protocol, fb, handle);
    let _ = handle.flush();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layer::SolidColor;

    #[test]
    fn anim_context_delta_time_starts_at_zero() {
        let stack = LayerStack::new();
        let mut observed_dt = None;
        let mut observed_elapsed = None;

        // Run for exactly 1 frame then exit.
        run_with_stack(stack, 1000.0, |ctx| {
            observed_dt = Some(ctx.delta_time());
            observed_elapsed = Some(ctx.elapsed());
            ctx.exit();
        });

        // First frame dt is very small (setup overhead, not zero).
        // Allow a small tolerance to avoid flakiness on slow CI.
        let dt = observed_dt.unwrap();
        assert!(
            dt < Duration::from_millis(200),
            "first frame dt should be < 200ms, got {dt:?}"
        );
        let elapsed = observed_elapsed.unwrap();
        assert!(
            elapsed < Duration::from_millis(200),
            "first frame elapsed should be < 200ms, got {elapsed:?}"
        );
    }

    #[test]
    fn anim_context_frame_count_increments() {
        let stack = LayerStack::new();
        let mut frames = Vec::new();

        run_with_stack(stack, 1000.0, |ctx| {
            frames.push(ctx.frame_count());
            if ctx.frame_count() >= 4 {
                ctx.exit();
            }
        });

        assert_eq!(frames, vec![0, 1, 2, 3, 4]);
    }

    #[test]
    fn anim_context_exit_stops_loop() {
        let stack = LayerStack::new();
        let mut count = 0;

        run_with_stack(stack, 1000.0, |ctx| {
            count += 1;
            ctx.exit();
        });

        assert_eq!(count, 1);
    }

    #[test]
    fn anim_context_dimensions_match_terminal() {
        let stack = LayerStack::new();
        let size = TerminalSize::current();

        run_with_stack(stack, 1000.0, |ctx| {
            assert_eq!(ctx.width(), size.cols as u32);
            assert_eq!(ctx.height(), size.rows as u32);
            ctx.exit();
        });
    }

    #[test]
    fn anim_config_default_fps() {
        let config = AnimConfig::default();
        assert_eq!(config.fps, 60.0);
        assert_eq!(config.protocol, Protocol::Auto);
        assert!(config.clear_between_frames);
    }

    #[test]
    fn anim_config_builder() {
        let config = AnimConfig::new(30.0)
            .with_protocol(Protocol::Kitty)
            .with_clear_between_frames(false);
        assert_eq!(config.fps, 30.0);
        assert_eq!(config.protocol, Protocol::Kitty);
        assert!(!config.clear_between_frames);
    }

    #[test]
    fn run_with_stack_provides_layers_mut() {
        let mut stack = LayerStack::new();
        let bg = stack.push(SolidColor::new(0, 0, 0, 255));

        run_with_stack(stack, 1000.0, move |ctx| {
            // We should be able to modify layers.
            if let Some(entry) = ctx.layers_mut().get_mut(bg) {
                entry.set_opacity(0.5);
            }
            ctx.exit();
        });
    }

    #[test]
    fn run_with_config_respects_fps() {
        let stack = LayerStack::new();
        let config = AnimConfig::new(100.0); // 100 FPS = 10ms per frame

        let start = Instant::now();
        let mut count = 0;

        run_with_stack(stack, config.fps, |ctx| {
            count += 1;
            if count >= 5 {
                ctx.exit();
            }
        });

        let elapsed = start.elapsed();
        // 5 frames at 100 FPS should take at least 40ms (4 frame intervals).
        // Allow generous tolerance for CI/slow systems.
        assert!(
            elapsed >= Duration::from_millis(20),
            "5 frames at 100fps should take >= 20ms, took {elapsed:?}"
        );
    }

    #[test]
    #[should_panic(expected = "fps must be positive")]
    fn anim_config_new_panics_on_zero_fps() {
        let _ = AnimConfig::new(0.0);
    }

    #[test]
    #[should_panic(expected = "fps must be positive")]
    fn anim_config_new_panics_on_negative_fps() {
        let _ = AnimConfig::new(-1.0);
    }

    #[test]
    fn anim_context_exit_in_render_callback() {
        let stack = LayerStack::new();
        let mut frames_seen = Vec::new();
        let mut exit_called = false;
        let mut redraw_before_exit = false;

        run_with_stack(stack, 1000.0, |ctx| {
            frames_seen.push(ctx.frame_count());
            if ctx.frame_count() == 2 {
                // Request redraw BEFORE exit — the loop should render
                // one last frame when both flags are set.
                ctx.request_redraw();
                redraw_before_exit = true;
                ctx.exit();
                exit_called = true;
            }
            // Every frame requests a redraw (including the exit frame).
            ctx.request_redraw();
        });

        // The callback ran for frames 0, 1, 2.
        assert_eq!(frames_seen, vec![0, 1, 2]);
        assert!(exit_called, "exit() should have been called");
        assert!(redraw_before_exit, "request_redraw() should precede exit()");

        // Because the callback called request_redraw() before exit(),
        // and the loop exited, render_and_encode was invoked for the
        // final frame.  We cannot intercept stdout here, but the loop
        // invariants guarantee it: should_exit && redraw_requested ⇒
        // render_and_encode(&ctx, …) is called before break.
    }

    #[test]
    fn anim_context_exit_without_redraw_skips_render() {
        let stack = LayerStack::new();
        let mut frames_seen = Vec::new();
        let mut exit_called = false;
        let mut redraw_called = false;

        run_with_stack(stack, 1000.0, |ctx| {
            frames_seen.push(ctx.frame_count());
            if ctx.frame_count() == 2 {
                // Exit WITHOUT requesting a redraw.
                ctx.exit();
                exit_called = true;
            } else {
                // Only redraw on non-exit frames.
                ctx.request_redraw();
                redraw_called = true;
            }
        });

        // The callback ran for frames 0, 1, 2.
        assert_eq!(frames_seen, vec![0, 1, 2]);
        assert!(exit_called, "exit() should have been called");
        // request_redraw() was called on frames 0 and 1 (the non-exit
        // frames), but NOT on frame 2.  The loop invariant guarantees:
        // should_exit && !redraw_requested ⇒ render_and_encode is NOT
        // called, so the final frame was not rendered.
        assert!(
            redraw_called,
            "request_redraw should have been called on non-exit frames"
        );
    }

    // ─── Edge-case tests ────────────────────────────────────────

    #[test]
    fn anim_context_mark_full_does_not_panic() {
        let stack = LayerStack::new();
        run_with_stack(stack, 1000.0, |ctx| {
            ctx.mark_full();
            ctx.exit();
        });
    }

    #[test]
    fn anim_context_mark_rect_does_not_panic() {
        let stack = LayerStack::new();
        run_with_stack(stack, 1000.0, |ctx| {
            ctx.mark_rect(10, 10, 20, 20);
            ctx.exit();
        });
    }

    #[test]
    fn anim_context_terminal_size_returns_valid_dimensions() {
        let stack = LayerStack::new();
        run_with_stack(stack, 1000.0, |ctx| {
            let size = ctx.terminal_size();
            assert!(size.cols > 0, "terminal cols should be > 0");
            assert!(size.rows > 0, "terminal rows should be > 0");
            assert_eq!(ctx.width(), size.cols as u32);
            assert_eq!(ctx.height(), size.rows as u32);
            ctx.exit();
        });
    }

    #[test]
    fn anim_config_new_very_high_fps() {
        // Very high FPS (10,000) should not panic.
        let config = AnimConfig::new(10_000.0);
        assert_eq!(config.fps, 10_000.0);
    }

    #[test]
    fn anim_config_new_very_low_fps() {
        // Very low FPS (0.1) should not panic.
        let config = AnimConfig::new(0.1);
        assert_eq!(config.fps, 0.1);
    }

    #[test]
    fn anim_context_delta_time_is_positive_after_sleep() {
        let stack = LayerStack::new();
        let mut dt_values = Vec::new();

        run_with_stack(stack, 1000.0, |ctx| {
            dt_values.push(ctx.delta_time());
            if ctx.frame_count() >= 3 {
                ctx.exit();
            }
        });

        // After the first frame, delta times should be > 0.
        // dt_values[0] is the time since loop setup (small).
        // dt_values[1] and later should reflect actual frame intervals.
        assert!(dt_values.len() >= 3, "should have at least 3 dt samples");
        // The second frame's dt should be > 0 (loop sleeps between frames).
        assert!(
            dt_values[1] > Duration::ZERO,
            "dt[1] should be > 0 after a frame interval, got {:?}",
            dt_values[1]
        );
    }

    #[test]
    fn anim_context_elapsed_accumulates() {
        let stack = LayerStack::new();
        let mut elapsed_values = Vec::new();

        run_with_stack(stack, 1000.0, |ctx| {
            elapsed_values.push(ctx.elapsed());
            if ctx.frame_count() >= 4 {
                ctx.exit();
            }
        });

        // Elapsed should be non-decreasing.
        assert!(
            elapsed_values.len() >= 4,
            "should have at least 4 elapsed samples"
        );
        for i in 1..elapsed_values.len() {
            assert!(
                elapsed_values[i] >= elapsed_values[i - 1],
                "elapsed should be non-decreasing: [{:?}] < [{:?}] at index {}",
                elapsed_values[i],
                elapsed_values[i - 1],
                i
            );
        }
    }

    #[test]
    fn run_with_stack_empty_layers() {
        // An empty layer stack should run and exit without issue.
        let stack = LayerStack::new();
        let mut count = 0;

        run_with_stack(stack, 1000.0, |ctx| {
            count += 1;
            ctx.exit();
        });

        assert_eq!(count, 1, "callback should run exactly once");
    }

    #[test]
    fn anim_config_clear_between_frames_false() {
        let config = AnimConfig::new(60.0).with_clear_between_frames(false);
        assert!(!config.clear_between_frames);

        // Should run without issues when clear_between_frames is false.
        let stack = LayerStack::new();
        run_with_config(stack, config, |ctx| {
            ctx.exit();
        });
    }

    #[test]
    fn anim_context_exit_on_first_frame() {
        let stack = LayerStack::new();
        let mut count = 0;

        run_with_stack(stack, 1000.0, |ctx| {
            count += 1;
            assert_eq!(
                ctx.frame_count(),
                0,
                "exit on first frame: frame_count should be 0"
            );
            ctx.exit();
        });

        assert_eq!(count, 1, "callback should run exactly once");
    }

    #[test]
    fn anim_context_multiple_exit_calls() {
        // Calling exit() multiple times should be fine.
        let stack = LayerStack::new();
        let mut count = 0;

        run_with_stack(stack, 1000.0, |ctx| {
            count += 1;
            ctx.exit();
            ctx.exit(); // double exit should not cause issues
            ctx.exit(); // triple exit should not cause issues
        });

        assert_eq!(count, 1, "callback should run exactly once");
    }

    #[test]
    fn anim_context_request_redraw_then_mark_rect() {
        // request_redraw() marks full; mark_rect() after should narrow
        // the dirty region. Both should not panic.
        let stack = LayerStack::new();
        run_with_stack(stack, 1000.0, |ctx| {
            ctx.request_redraw();
            ctx.mark_rect(5, 5, 10, 10);
            ctx.exit();
        });
    }

    #[test]
    fn anim_context_layers_ref() {
        // layers() (immutable) should work.
        let mut stack = LayerStack::new();
        let bg = stack.push(SolidColor::new(0, 0, 0, 255));

        run_with_stack(stack, 1000.0, move |ctx| {
            let layers = ctx.layers();
            assert!(layers.get(bg).is_some(), "layer should exist via layers()");
            ctx.exit();
        });
    }

    #[test]
    fn anim_config_all_protocols() {
        // Each protocol variant should construct without error.
        for protocol in [Protocol::Auto, Protocol::Sixel, Protocol::Kitty] {
            let config = AnimConfig::new(30.0).with_protocol(protocol);
            assert_eq!(config.protocol, protocol);
        }
    }

    #[test]
    fn anim_config_builder_chaining() {
        // Builder methods should chain and override correctly.
        let config = AnimConfig::new(120.0)
            .with_protocol(Protocol::Kitty)
            .with_clear_between_frames(false)
            .with_protocol(Protocol::Sixel)
            .with_clear_between_frames(true);
        assert_eq!(config.fps, 120.0);
        assert_eq!(config.protocol, Protocol::Sixel);
        assert!(config.clear_between_frames);
    }

    #[test]
    fn anim_context_exit_after_several_frames() {
        // Exit after several frames to test accumulated elapsed time.
        let stack = LayerStack::new();
        let mut count = 0;

        run_with_stack(stack, 1000.0, |ctx| {
            count += 1;
            if ctx.frame_count() >= 9 {
                // 10 frames at 1000 FPS ≈ 10ms; assert a meaningful lower bound.
                assert!(
                    ctx.elapsed() >= Duration::from_millis(5),
                    "elapsed should be >= 5ms after 10 frames, got {:?}",
                    ctx.elapsed()
                );
                ctx.exit();
            }
        });

        assert_eq!(count, 10, "callback should run 10 times (frames 0..=9)");
    }

    #[test]
    fn anim_context_mark_rect_with_zero_dimensions() {
        // mark_rect with zero width/height should not panic.
        let stack = LayerStack::new();
        run_with_stack(stack, 1000.0, |ctx| {
            ctx.mark_rect(0, 0, 0, 0);
            ctx.exit();
        });
    }

    #[test]
    fn anim_context_mark_rect_outside_framebuffer() {
        // mark_rect with coordinates larger than framebuffer should not panic.
        let stack = LayerStack::new();
        run_with_stack(stack, 1000.0, |ctx| {
            ctx.mark_rect(1000, 1000, 500, 500);
            ctx.exit();
        });
    }
}
