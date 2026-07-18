//! Integration tests: animation rendering pipeline.
//!
//! These tests exercise the rendering pipeline that the animation
//! loop uses internally: LayerStack composition, render_diff with
//! dirty regions, and encoding to protocol output.  They verify
//! that the full render → encode path produces correct pixel data
//! and valid protocol bytes, matching what `run_with_config` does
//! inside its main loop.

use termcompositor::{DirtyRect, DirtyRegion, FrameBuffer, LayerStack, RectLayer, SolidColor};

// ── render_diff with dirty regions ───────────────────────────

mod dirty_region_pipeline {
    use super::*;

    #[test]
    fn render_diff_full_dirty_renders_all_layers() {
        let mut stack = LayerStack::new();
        stack.push(SolidColor::new(100, 0, 0, 255).with_z(0));
        stack.push(RectLayer::new(2, 2, 4, 4, [0, 200, 0, 255]).with_z(10));

        let mut fb = FrameBuffer::new(10, 10);
        let mut dirty = DirtyRegion::new();
        dirty.mark_full();
        stack.render_diff(&mut fb, &mut dirty);

        // Background at (0,0) should be the SolidColor.
        assert_eq!(fb.get_pixel(0, 0), Some(&[100, 0, 0, 255]));
        // Rect at (3,3) should be green.
        assert_eq!(fb.get_pixel(3, 3), Some(&[0, 200, 0, 255]));
    }

    #[test]
    fn render_diff_partial_dirty_only_renders_marked_region() {
        let mut stack = LayerStack::new();
        stack.push(SolidColor::new(255, 255, 255, 255).with_z(0));
        stack.push(RectLayer::new(0, 0, 5, 5, [255, 0, 0, 255]).with_z(10));

        let mut fb = FrameBuffer::new(10, 10);
        // First: full render to populate the buffer.
        let mut dirty = DirtyRegion::new();
        dirty.mark_full();
        stack.render_diff(&mut fb, &mut dirty);

        // Overwrite the entire framebuffer with black.
        for px in fb.pixels_mut() {
            *px = [0, 0, 0, 0];
        }

        // Now render only a small dirty rect that does NOT cover
        // the rect layer — nothing should change.
        let mut dirty2 = DirtyRegion::new();
        dirty2.mark_rect(DirtyRect::new(7, 7, 2, 2));
        stack.render_diff(&mut fb, &mut dirty2);

        // The rect area (2,2) was NOT in the dirty region, so it
        // should still be black (from the overwrite).
        assert_eq!(
            fb.get_pixel(2, 2),
            Some(&[0, 0, 0, 0]),
            "partial dirty should not re-render untouched regions"
        );
    }

    #[test]
    fn render_diff_multiple_dirty_rects() {
        let mut stack = LayerStack::new();
        stack.push(SolidColor::new(0, 0, 200, 255).with_z(0));

        let mut fb = FrameBuffer::new(20, 20);
        // Full render first.
        let mut dirty = DirtyRegion::new();
        dirty.mark_full();
        stack.render_diff(&mut fb, &mut dirty);

        // Overwrite everything.
        for px in fb.pixels_mut() {
            *px = [0, 0, 0, 0];
        }

        // Mark two separate dirty rects.
        let mut dirty2 = DirtyRegion::new();
        dirty2.mark_rect(DirtyRect::new(0, 0, 5, 5));
        dirty2.mark_rect(DirtyRect::new(15, 15, 5, 5));
        stack.render_diff(&mut fb, &mut dirty2);

        // Both dirty rects should be re-rendered with blue.
        assert_eq!(fb.get_pixel(2, 2), Some(&[0, 0, 200, 255]));
        assert_eq!(fb.get_pixel(17, 17), Some(&[0, 0, 200, 255]));
        // Non-dirty area should still be black.
        assert_eq!(fb.get_pixel(10, 10), Some(&[0, 0, 0, 0]));
    }

    #[test]
    fn render_diff_empty_dirty_triggers_full_render() {
        // An empty DirtyRegion (no rects marked) is treated as
        // "render everything" by render_diff as a safety measure.
        let mut stack = LayerStack::new();
        stack.push(SolidColor::new(255, 0, 0, 255).with_z(0));

        let mut fb = FrameBuffer::new(5, 5);
        // Overwrite with black.
        for px in fb.pixels_mut() {
            *px = [0, 0, 0, 0];
        }

        // Empty dirty region should still trigger a full render.
        let mut dirty = DirtyRegion::new();
        stack.render_diff(&mut fb, &mut dirty);

        // All pixels should now be red.
        for y in 0..5 {
            for x in 0..5 {
                assert_eq!(
                    fb.get_pixel(x, y),
                    Some(&[255, 0, 0, 255]),
                    "empty dirty should trigger full render"
                );
            }
        }
    }

    #[test]
    fn render_diff_dirty_rect_clamps_to_framebuffer() {
        let mut stack = LayerStack::new();
        stack.push(SolidColor::new(0, 100, 200, 255).with_z(0));

        let mut fb = FrameBuffer::new(10, 10);
        // Dirty rect that extends beyond the framebuffer should not panic.
        let mut dirty = DirtyRegion::new();
        dirty.mark_rect(DirtyRect::new(8, 8, 100, 100));
        stack.render_diff(&mut fb, &mut dirty);

        // The intersection (8..10, 8..10) should be rendered.
        assert_eq!(fb.get_pixel(9, 9), Some(&[0, 100, 200, 255]));
    }
}

// ── Layer composition + encoding pipeline ─────────────────────

#[cfg(feature = "kitty-encoder")]
mod animation_kitty_pipeline {
    use super::*;

    #[test]
    fn solid_color_and_rect_encode_to_valid_kitty() {
        let mut stack = LayerStack::new();
        stack.push(SolidColor::new(0, 0, 64, 255).with_z(0));
        stack.push(RectLayer::new(5, 5, 10, 5, [255, 200, 0, 255]).with_z(10));

        let mut fb = FrameBuffer::new(30, 20);
        stack.render(&mut fb);

        // Verify pixel values before encoding.
        assert_eq!(fb.get_pixel(0, 0), Some(&[0, 0, 64, 255]));
        assert_eq!(fb.get_pixel(7, 7), Some(&[255, 200, 0, 255]));

        let bytes = Protocol::Kitty.encode(&fb).unwrap();
        assert!(bytes.starts_with(b"\x1b_G"));
        assert!(bytes.ends_with(b"\x1b\\"));
    }

    #[test]
    fn animated_opacity_change_reflected_in_render() {
        // Simulate what the animation loop does: create layers,
        // render, modify opacity, render again.
        let mut stack = LayerStack::new();
        let bg = stack.push(SolidColor::new(0, 0, 0, 255).with_z(0));
        let fg = stack.push(SolidColor::new(255, 0, 0, 255).with_z(10));

        let mut fb = FrameBuffer::new(10, 10);

        // Frame 1: full opacity.
        stack.render(&mut fb);
        let px_full = fb.get_pixel(5, 5).unwrap().clone();

        // Frame 2: reduce opacity to 50% and clear framebuffer.
        if let Some(entry) = stack.get_mut(fg) {
            entry.set_opacity(0.5);
        }
        for px in fb.pixels_mut() {
            *px = [0, 0, 0, 0];
        }
        stack.render(&mut fb);
        let px_half = fb.get_pixel(5, 5).unwrap().clone();

        // Full-opacity red should have higher R than half-opacity.
        assert!(
            px_full[0] > px_half[0],
            "full opacity R ({}) should be > half opacity R ({})",
            px_full[0],
            px_half[0]
        );

        // Both should have non-zero alpha.
        assert!(px_full[3] > 0, "full opacity alpha should be > 0");
        assert!(px_half[3] > 0, "half opacity alpha should be > 0");
    }

    #[test]
    fn layer_removal_reflected_in_render() {
        // Simulate adding and removing layers during animation.
        let mut stack = LayerStack::new();
        stack.push(SolidColor::new(0, 0, 0, 255).with_z(0));
        let tmp = stack.push(RectLayer::new(0, 0, 10, 10, [255, 255, 0, 255]).with_z(10));

        let mut fb = FrameBuffer::new(10, 10);
        stack.render(&mut fb);
        let px_with = fb.get_pixel(5, 5).unwrap().clone();

        // Remove the yellow rect layer.
        stack.remove(tmp);

        // Clear and re-render.
        for px in fb.pixels_mut() {
            *px = [0, 0, 0, 0];
        }
        stack.render(&mut fb);
        let px_without = fb.get_pixel(5, 5).unwrap().clone();

        // With the yellow rect: R=255, G=255.
        assert_eq!(px_with[0], 255, "yellow rect R");
        assert_eq!(px_with[1], 255, "yellow rect G");
        // Without it: should be black background.
        assert_eq!(px_without, [0, 0, 0, 255]);
    }

    #[test]
    fn multi_frame_render_with_changing_layers() {
        // Simulate a multi-frame animation where layers move.
        let mut stack = LayerStack::new();
        stack.push(SolidColor::new(0, 0, 0, 255).with_z(0));

        let mut fb = FrameBuffer::new(20, 10);

        // Frame 1: rect at (0,0).
        let r = stack.push(RectLayer::new(0, 0, 5, 5, [255, 0, 0, 255]).with_z(10));
        stack.render(&mut fb);
        assert_eq!(fb.get_pixel(2, 2), Some(&[255, 0, 0, 255]));

        // Frame 2: remove old rect, add at new position.
        stack.remove(r);
        for px in fb.pixels_mut() {
            *px = [0, 0, 0, 0];
        }
        stack.push(RectLayer::new(10, 0, 5, 5, [0, 255, 0, 255]).with_z(10));
        stack.render(&mut fb);
        assert_eq!(fb.get_pixel(12, 2), Some(&[0, 255, 0, 255]));
        // Old position should be black.
        assert_eq!(fb.get_pixel(2, 2), Some(&[0, 0, 0, 255]));
    }

    #[test]
    fn gradient_layer_renders_and_encodes() {
        let mut stack = LayerStack::new();
        stack.push(
            GradientLayer::linear(
                0,
                0,
                20,
                10,
                [255, 0, 0, 255],
                [0, 0, 255, 255],
                0,
                0,
                20,
                10,
            )
            .with_z(5),
        );

        let mut fb = FrameBuffer::new(20, 10);
        stack.render(&mut fb);

        let bytes = Protocol::Kitty.encode(&fb).unwrap();
        assert!(bytes.starts_with(b"\x1b_G"));
        assert!(bytes.ends_with(b"\x1b\\"));

        // Gradient should produce different R values at edges.
        let left = fb.get_pixel(0, 0).unwrap();
        let right = fb.get_pixel(19, 0).unwrap();
        assert_ne!(left[0], right[0], "gradient R must vary");
    }

    #[test]
    fn drop_shadow_renders_and_encodes() {
        let inner = RectLayer::new(5, 5, 5, 5, [255, 255, 255, 255]);
        let shadow = DropShadow::new(Box::new(inner))
            .with_offset(2, 2)
            .with_blur(1)
            .with_shadow_color([0, 0, 0, 200])
            .with_z(5);

        let mut stack = LayerStack::new();
        stack.push(SolidColor::new(0, 0, 0, 255).with_z(0));
        stack.push(shadow);

        let mut fb = FrameBuffer::new(20, 20);
        stack.render(&mut fb);

        let bytes = Protocol::Kitty.encode(&fb).unwrap();
        assert!(bytes.starts_with(b"\x1b_G"));
        assert!(bytes.ends_with(b"\x1b\\"));
        // Original rect should be white.
        assert_eq!(fb.get_pixel(5, 5), Some(&[255, 255, 255, 255]));
    }

    #[test]
    fn border_layer_renders_and_encodes() {
        let mut stack = LayerStack::new();
        stack.push(SolidColor::new(0, 0, 64, 255).with_z(0));
        stack.push(BorderLayer::new(2, 2, 6, 4, [255, 200, 0, 255], 1).with_z(10));

        let mut fb = FrameBuffer::new(20, 10);
        stack.render(&mut fb);

        let bytes = Protocol::Kitty.encode(&fb).unwrap();
        assert!(bytes.starts_with(b"\x1b_G"));
        assert!(bytes.ends_with(b"\x1b\\"));
        assert_eq!(fb.get_pixel(2, 2), Some(&[255, 200, 0, 255]));
    }

    #[test]
    fn canvas_layer_renders_and_encodes() {
        let mut stack = LayerStack::new();
        stack.push(SolidColor::new(0, 0, 0, 255).with_z(0));
        let mut canvas = CanvasLayer::new(10, 10).at(2, 2).with_z(10);
        canvas.draw_pixel(0, 0, [255, 255, 255, 255]);
        canvas.draw_line(0, 0, 9, 9, [0, 255, 0, 255]);
        canvas.draw_circle(5, 5, 3, [255, 0, 0, 255]);
        stack.push(canvas);

        let mut fb = FrameBuffer::new(20, 20);
        stack.render(&mut fb);

        let bytes = Protocol::Kitty.encode(&fb).unwrap();
        assert!(bytes.starts_with(b"\x1b_G"));
        assert!(bytes.ends_with(b"\x1b\\"));
        // Canvas pixel at local (1,1) → fb (3,3) should be green.
        assert_eq!(fb.get_pixel(3, 3), Some(&[0, 255, 0, 255]));
    }

    #[test]
    fn clip_layer_renders_and_encodes() {
        let inner = RectLayer::new(0, 0, 20, 10, [255, 0, 0, 255]);
        let clip = ClipLayer::new(Box::new(inner), 0, 0, 5, 5).with_z(10);

        let mut stack = LayerStack::new();
        stack.push(SolidColor::new(0, 0, 0, 255).with_z(0));
        stack.push(clip);

        let mut fb = FrameBuffer::new(20, 10);
        stack.render(&mut fb);

        let bytes = Protocol::Kitty.encode(&fb).unwrap();
        assert!(bytes.starts_with(b"\x1b_G"));
        assert!(bytes.ends_with(b"\x1b\\"));
        // Pixel inside clip region should be red.
        assert_eq!(fb.get_pixel(2, 2), Some(&[255, 0, 0, 255]));
        // Pixel outside clip region should be black.
        assert_eq!(fb.get_pixel(10, 5), Some(&[0, 0, 0, 255]));
    }
}

// ── Sixel encoding pipeline ──────────────────────────────────

#[cfg(feature = "sixel-encoder")]
mod animation_sixel_pipeline {
    use super::*;

    #[test]
    fn solid_color_and_rect_encode_to_sixel() {
        let mut stack = LayerStack::new();
        stack.push(SolidColor::new(0, 0, 64, 255).with_z(0));
        stack.push(RectLayer::new(5, 5, 10, 5, [255, 200, 0, 255]).with_z(10));

        let mut fb = FrameBuffer::new(30, 20);
        stack.render(&mut fb);

        let bytes = Protocol::Sixel.encode(&fb).unwrap();
        assert!(!bytes.is_empty(), "Sixel output must not be empty");
        assert_eq!(fb.get_pixel(0, 0), Some(&[0, 0, 64, 255]));
        assert_eq!(fb.get_pixel(7, 7), Some(&[255, 200, 0, 255]));
    }

    #[test]
    fn animated_opacity_sixel_pipeline() {
        let mut stack = LayerStack::new();
        let bg = stack.push(SolidColor::new(0, 0, 0, 255).with_z(0));
        let fg = stack.push(SolidColor::new(255, 0, 0, 255).with_z(10));

        let mut fb = FrameBuffer::new(10, 10);

        // Full opacity.
        stack.render(&mut fb);
        let bytes_full = Protocol::Sixel.encode(&fb).unwrap();

        // Half opacity — clear framebuffer first.
        if let Some(entry) = stack.get_mut(fg) {
            entry.set_opacity(0.5);
        }
        for px in fb.pixels_mut() {
            *px = [0, 0, 0, 0];
        }
        stack.render(&mut fb);
        let bytes_half = Protocol::Sixel.encode(&fb).unwrap();

        assert!(!bytes_full.is_empty());
        assert!(!bytes_half.is_empty());
        // Different opacity should produce different output.
        assert_ne!(
            bytes_full, bytes_half,
            "opacity change should alter Sixel output"
        );
    }

    #[test]
    fn gradient_and_border_sixel_pipeline() {
        let mut stack = LayerStack::new();
        stack.push(
            GradientLayer::linear(
                0,
                0,
                20,
                10,
                [255, 0, 0, 255],
                [0, 0, 255, 255],
                0,
                0,
                20,
                10,
            )
            .with_z(5),
        );
        stack.push(BorderLayer::new(1, 1, 8, 6, [0, 255, 200, 255], 2).with_z(10));

        let mut fb = FrameBuffer::new(20, 10);
        stack.render(&mut fb);

        let bytes = Protocol::Sixel.encode(&fb).unwrap();
        assert!(!bytes.is_empty());
    }
}

// ── Dirty region + encoding pipeline ──────────────────────────

#[cfg(feature = "kitty-encoder")]
mod dirty_region_encoding {
    use super::*;

    #[test]
    fn render_diff_then_encode_produces_valid_output() {
        let mut stack = LayerStack::new();
        stack.push(SolidColor::new(0, 0, 64, 255).with_z(0));
        stack.push(RectLayer::new(2, 2, 5, 5, [255, 255, 0, 255]).with_z(10));

        let mut fb = FrameBuffer::new(15, 15);
        let mut dirty = DirtyRegion::new();
        dirty.mark_full();
        stack.render_diff(&mut fb, &mut dirty);

        let bytes = Protocol::Kitty.encode(&fb).unwrap();
        assert!(bytes.starts_with(b"\x1b_G"));
        assert!(bytes.ends_with(b"\x1b\\"));
    }

    #[test]
    fn dispatch_to_writer_after_render_diff() {
        let mut stack = LayerStack::new();
        stack.push(SolidColor::new(128, 64, 32, 255).with_z(0));

        let mut fb = FrameBuffer::new(5, 5);
        let mut dirty = DirtyRegion::new();
        dirty.mark_full();
        stack.render_diff(&mut fb, &mut dirty);

        let mut via_encode = Vec::new();
        dispatch_to_writer(Protocol::Kitty, &fb, &mut via_encode).unwrap();

        assert!(!via_encode.is_empty());
        assert!(via_encode.starts_with(b"\x1b_G"));
    }

    #[test]
    fn multiple_render_diff_cycles_produce_consistent_output() {
        let mut stack = LayerStack::new();
        stack.push(SolidColor::new(100, 100, 100, 255).with_z(0));

        let mut fb = FrameBuffer::new(8, 8);

        // Render cycle 1.
        let mut dirty = DirtyRegion::new();
        dirty.mark_full();
        stack.render_diff(&mut fb, &mut dirty);
        let bytes1 = Protocol::Kitty.encode(&fb).unwrap();

        // Render cycle 2 with full dirty (same content).
        let mut dirty2 = DirtyRegion::new();
        dirty2.mark_full();
        stack.render_diff(&mut fb, &mut dirty2);
        let bytes2 = Protocol::Kitty.encode(&fb).unwrap();

        // Same content, same encoding → same output.
        assert_eq!(
            bytes1, bytes2,
            "identical renders should produce identical bytes"
        );
    }
}

// ── Text layer in animation pipeline ──────────────────────────

#[cfg(feature = "kitty-encoder")]
mod text_layer_pipeline {
    use super::*;

    #[test]
    fn text_layer_renders_and_encodes() {
        let mut stack = LayerStack::new();
        stack.push(SolidColor::new(0, 0, 0, 255).with_z(0));
        stack.push(TextLayer::new(0, 0, "Hello", [255; 4]).with_z(10));

        let mut fb = FrameBuffer::new(40, 10);
        stack.render(&mut fb);

        let bytes = Protocol::Kitty.encode(&fb).unwrap();
        assert!(bytes.starts_with(b"\x1b_G"));
        assert!(bytes.ends_with(b"\x1b\\"));
    }

    #[test]
    fn text_layer_with_other_layers_encodes() {
        let mut stack = LayerStack::new();
        stack.push(SolidColor::new(0, 0, 64, 255).with_z(0));
        stack.push(RectLayer::new(0, 0, 20, 5, [50, 50, 50, 255]).with_z(5));
        stack.push(TextLayer::new(2, 1, "Hi", [255; 4]).with_z(20));

        let mut fb = FrameBuffer::new(40, 10);
        stack.render(&mut fb);

        let bytes = Protocol::Kitty.encode(&fb).unwrap();
        assert!(bytes.starts_with(b"\x1b_G"));
    }
}
