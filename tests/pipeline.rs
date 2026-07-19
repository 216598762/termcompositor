//! Integration tests: full render → encode pipeline.
//!
//! These tests exercise the end-to-end path from layer stack
//! composition through protocol encoding, verifying that the
//! output bytes contain expected escape sequences.
//!
//! All tests use only the public API surface:
//! - `Protocol::encode()` (via `ProtocolEncoder` trait)
//! - `dispatch_to_writer()`
//! - `wrap_for_tmux()` / `wrap_for_tmux_to_writer()`
//! - `encode_passthrough_to_writer()`

#[allow(unused_imports)]
use std::io::Write;
#[allow(unused_imports)]
use termcompositor::*;

// ── Kitty protocol ───────────────────────────────────────────

#[cfg(feature = "kitty-encoder")]
mod kitty_pipeline {
    use super::*;

    #[test]
    fn single_chunk_kitty_output_starts_with_esc_g() {
        let mut stack = LayerStack::new();
        stack.push(SolidColor::new(255, 0, 0, 255));
        let mut fb = FrameBuffer::new(10, 10);
        stack.render(&mut fb);

        let bytes = Protocol::Kitty.encode(&fb).unwrap();
        // Kitty APC starts with ESC_G
        assert!(
            bytes.starts_with(b"\x1b_G"),
            "output must start with ESC_G, got: {:?}",
            &bytes[..20.min(bytes.len())]
        );
        // Kitty APC ends with ESC_backslash
        assert!(
            bytes.ends_with(b"\x1b\\"),
            "output must end with ESC_backslash"
        );
    }

    #[test]
    fn multi_chunk_kitty_output_has_m_key() {
        // 100x100 = 10000 pixels > 768 (PIXELS_PER_CHUNK),
        // so this requires multi-chunk encoding.
        let mut fb = FrameBuffer::new(100, 100);
        for px in fb.pixels_mut() {
            *px = [0, 128, 255, 255];
        }

        let bytes = Protocol::Kitty.encode(&fb).unwrap();
        // Multi-chunk output contains "m=1" (intermediate) and
        // "m=0" (final) markers.
        let s = String::from_utf8_lossy(&bytes);
        assert!(s.contains("m=1"), "multi-chunk must contain m=1");
        assert!(s.contains("m=0"), "multi-chunk must contain m=0");
    }

    #[test]
    fn kitty_dispatch_to_writer_matches_encode() {
        let fb = {
            let mut fb = FrameBuffer::new(5, 5);
            for px in fb.pixels_mut() {
                *px = [10, 20, 30, 255];
            }
            fb
        };

        let via_encode = Protocol::Kitty.encode(&fb).unwrap();

        let mut via_writer = Vec::new();
        dispatch_to_writer(Protocol::Kitty, &fb, &mut via_writer).unwrap();

        assert_eq!(
            via_encode, via_writer,
            "encode() and dispatch_to_writer() must produce identical output"
        );
    }

    #[test]
    fn kitty_empty_framebuffer_returns_error() {
        let fb = FrameBuffer::new(0, 0);
        let result = Protocol::Kitty.encode(&fb);
        assert!(result.is_err(), "encoding a 0x0 framebuffer must fail");
        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("invalid dimensions"),
            "error must mention invalid dimensions: {err}"
        );
    }

    #[test]
    fn kitty_1x1_framebuffer_encodes() {
        let mut fb = FrameBuffer::new(1, 1);
        fb.pixels_mut()[0] = [42, 42, 42, 255];
        let bytes = Protocol::Kitty.encode(&fb).unwrap();
        assert!(!bytes.is_empty());
        assert!(bytes.starts_with(b"\x1b_G"));
    }

    #[test]
    fn kitty_layer_stack_pipeline() {
        let mut stack = LayerStack::new();
        stack.push(SolidColor::new(0, 0, 64, 255).with_z(0));
        stack.push(RectLayer::new(2, 2, 3, 3, [255, 255, 0, 255]).with_z(10));
        stack.push(TextLayer::new(0, 0, "hi", [255; 4]).with_z(20));

        let mut fb = FrameBuffer::new(20, 10);
        stack.render(&mut fb);

        let bytes = Protocol::Kitty.encode(&fb).unwrap();
        assert!(bytes.starts_with(b"\x1b_G"));
        assert!(bytes.ends_with(b"\x1b\\"));
    }

    #[test]
    fn kitty_dispatch_auto_picks_kitty_in_kitty_terminal() {
        let detected = detect();
        if detected == Protocol::Kitty {
            let fb = FrameBuffer::new(2, 2);
            let mut out = Vec::new();
            dispatch_to_writer(Protocol::Auto, &fb, &mut out).unwrap();
            assert!(
                out.starts_with(b"\x1b_G"),
                "Auto in a Kitty terminal must emit Kitty output"
            );
        }
    }

    #[test]
    fn kitty_chunk_boundary_768_pixels() {
        // Exactly 768 pixels = one full chunk (no m key).
        let mut fb = FrameBuffer::new(768, 1);
        for px in fb.pixels_mut() {
            *px = [128, 128, 128, 255];
        }
        let bytes = Protocol::Kitty.encode(&fb).unwrap();
        let s = String::from_utf8_lossy(&bytes);
        // Single chunk: no m= key present.
        assert!(
            !s.contains("m="),
            "768-pixel single chunk must not contain m= key"
        );
    }

    #[test]
    fn kitty_chunk_boundary_769_pixels() {
        // 769 pixels = two chunks (m=1 then m=0).
        let mut fb = FrameBuffer::new(769, 1);
        for px in fb.pixels_mut() {
            *px = [128, 128, 128, 255];
        }
        let bytes = Protocol::Kitty.encode(&fb).unwrap();
        let s = String::from_utf8_lossy(&bytes);
        assert!(s.contains("m=1"), "769-pixel must contain m=1");
        assert!(s.contains("m=0"), "769-pixel must contain m=0");
    }
}

// ── Sixel protocol ───────────────────────────────────────────

#[cfg(feature = "sixel-encoder")]
mod sixel_pipeline {
    use super::*;

    #[test]
    fn sixel_output_is_non_empty() {
        let mut stack = LayerStack::new();
        stack.push(SolidColor::new(0, 200, 100, 255));
        let mut fb = FrameBuffer::new(10, 10);
        stack.render(&mut fb);

        let bytes = Protocol::Sixel.encode(&fb).unwrap();
        assert!(!bytes.is_empty(), "Sixel output must not be empty");
    }

    #[test]
    fn sixel_dispatch_to_writer_matches_encode() {
        let fb = {
            let mut fb = FrameBuffer::new(5, 5);
            for px in fb.pixels_mut() {
                *px = [100, 200, 50, 255];
            }
            fb
        };

        let via_encode = Protocol::Sixel.encode(&fb).unwrap();

        let mut via_writer = Vec::new();
        dispatch_to_writer(Protocol::Sixel, &fb, &mut via_writer).unwrap();

        assert_eq!(
            via_encode, via_writer,
            "Sixel encode() and dispatch_to_writer() must match"
        );
    }

    #[test]
    fn sixel_empty_framebuffer_returns_error() {
        let fb = FrameBuffer::new(0, 0);
        let result = Protocol::Sixel.encode(&fb);
        assert!(result.is_err(), "encoding a 0x0 framebuffer must fail");
    }

    #[test]
    fn sixel_1x1_framebuffer_encodes() {
        let mut fb = FrameBuffer::new(1, 1);
        fb.pixels_mut()[0] = [255, 0, 0, 255];
        let bytes = Protocol::Sixel.encode(&fb).unwrap();
        assert!(!bytes.is_empty());
    }

    #[test]
    fn sixel_layer_stack_pipeline() {
        let mut stack = LayerStack::new();
        stack.push(SolidColor::new(0, 0, 0, 255).with_z(0));
        stack.push(RectLayer::new(1, 1, 4, 4, [0, 255, 0, 255]).with_z(10));

        let mut fb = FrameBuffer::new(10, 10);
        stack.render(&mut fb);

        let bytes = Protocol::Sixel.encode(&fb).unwrap();
        assert!(!bytes.is_empty());
    }
}

// ── Protocol dispatch ────────────────────────────────────────

#[cfg(all(feature = "kitty-encoder", feature = "sixel-encoder"))]
mod dispatch_pipeline {
    use super::*;

    #[test]
    fn auto_dispatch_returns_non_empty_output() {
        let mut fb = FrameBuffer::new(4, 4);
        for px in fb.pixels_mut() {
            *px = [128, 64, 32, 255];
        }
        let mut out = Vec::new();
        let result = dispatch_to_writer(Protocol::Auto, &fb, &mut out);
        assert!(
            result.is_ok(),
            "Auto dispatch must succeed: {:?}",
            result.err()
        );
        assert!(!out.is_empty(), "Auto dispatch must produce output");
    }

    #[test]
    fn explicit_kitty_and_sixel_differ() {
        let mut fb = FrameBuffer::new(3, 3);
        for px in fb.pixels_mut() {
            *px = [200, 100, 50, 255];
        }

        let kitty = Protocol::Kitty.encode(&fb).unwrap();
        let sixel = Protocol::Sixel.encode(&fb).unwrap();

        assert!(kitty.starts_with(b"\x1b_G"));
        assert_ne!(
            kitty, sixel,
            "Kitty and Sixel must produce different output"
        );
    }

    #[test]
    fn dispatch_to_writer_auto_picks_correct_protocol() {
        let detected = detect();
        let fb = FrameBuffer::new(2, 2);
        let mut out = Vec::new();
        dispatch_to_writer(Protocol::Auto, &fb, &mut out).unwrap();

        match detected {
            Protocol::Kitty => assert!(out.starts_with(b"\x1b_G")),
            Protocol::Sixel => assert!(!out.is_empty()),
            Protocol::Auto => unreachable!("detect() never returns Auto"),
        }
    }

    #[test]
    fn encode_various_sizes() {
        // Test a range of framebuffer sizes to exercise
        // different chunking paths.
        for &(w, h) in &[(1, 1), (10, 10), (80, 24), (100, 100)] {
            let mut fb = FrameBuffer::new(w, h);
            for px in fb.pixels_mut() {
                *px = [50, 100, 200, 255];
            }
            let result = Protocol::Auto.encode(&fb);
            assert!(
                result.is_ok(),
                "encoding {w}x{h} must succeed: {:?}",
                result.err()
            );
            assert!(
                !result.unwrap().is_empty(),
                "encoding {w}x{h} must produce output"
            );
        }
    }
}

// ── Tmux passthrough ─────────────────────────────────────────

#[cfg(feature = "kitty-encoder")]
mod tmux_passthrough {
    use super::*;

    #[test]
    fn wrap_for_tmux_doubles_esc_bytes() {
        let inner = b"\x1b_Gtest\x1b\\";
        let wrapped = termcompositor::wrap_for_tmux(inner.to_vec());

        // Wrapped output starts with the tmux DCS prefix.
        assert!(
            wrapped.starts_with(b"\x1bPtmux;"),
            "tmux wrap must start with ESC P tmux ;"
        );
        // Wrapped output ends with DCS terminator.
        assert!(
            wrapped.ends_with(b"\x1b\\"),
            "tmux wrap must end with ESC backslash"
        );
        // Inner ESC bytes (0x1b) should be doubled.
        let s = String::from_utf8_lossy(&wrapped);
        assert!(
            s.contains("\x1b\x1b"),
            "tmux wrap must double inner ESC bytes"
        );
    }

    #[test]
    fn wrap_for_tmux_round_trip_identity() {
        let inner = b"\x1b_Ga=T,f=32;\x1b\\";
        let vec_result = termcompositor::wrap_for_tmux(inner.to_vec());

        let mut writer_result = Vec::new();
        termcompositor::wrap_for_tmux_to_writer(inner, &mut writer_result).unwrap();

        assert_eq!(
            vec_result, writer_result,
            "wrap_for_tmux and wrap_for_tmux_to_writer must agree"
        );
    }

    #[test]
    fn encode_passthrough_to_writer_without_passthrough_env() {
        let mut fb = FrameBuffer::new(3, 3);
        for px in fb.pixels_mut() {
            *px = [100, 200, 50, 255];
        }

        // Without TMUXPASSTHROUGH env var, encode_passthrough_to_writer
        // should produce the same output as raw Kitty encode.
        let raw = Protocol::Kitty.encode(&fb).unwrap();

        let mut auto_out = Vec::new();
        termcompositor::encode_passthrough_to_writer(&fb, &mut auto_out).unwrap();

        assert_eq!(
            auto_out, raw,
            "without TMUXPASSTHROUGH, passthrough must be identity"
        );
    }

    #[test]
    fn passthrough_writer_wraps_when_env_set() {
        use termcompositor::PassthroughWriter;

        let inner = b"hello world";
        let mut buf = Vec::new();
        {
            let mut pw = PassthroughWriter::new(&mut buf);
            std::io::Write::write_all(&mut pw, inner).unwrap();
            pw.finish().unwrap();
        }

        assert!(
            buf.starts_with(b"\x1bPtmux;"),
            "PassthroughWriter must start with tmux DCS prefix"
        );
        assert!(
            buf.ends_with(b"\x1b\\"),
            "PassthroughWriter must end with DCS terminator"
        );
        // Inner content is preserved (no ESC in this test data).
        let s = String::from_utf8_lossy(&buf);
        assert!(s.contains("hello world"));
    }
}

// ── Unsupported protocol errors ──────────────────────────────

mod error_paths {

    #[test]
    fn unsupported_protocol_returns_error() {
        // Build without enabling the specific protocol feature
        // is hard to test, but we can verify the error type exists
        // and the Display impl works.
        let err = termcompositor::EncoderError::UnsupportedProtocol("test");
        assert_eq!(
            err.to_string(),
            "protocol test is not supported in this build"
        );
    }

    #[test]
    fn invalid_dimensions_error_display() {
        let err = termcompositor::EncoderError::InvalidDimensions {
            width: 0,
            height: 10,
        };
        assert_eq!(err.to_string(), "framebuffer has invalid dimensions: 0x10");
    }

    #[test]
    fn encode_error_display() {
        let err = termcompositor::EncoderError::Encode("something broke".into());
        assert_eq!(err.to_string(), "encoder failed: something broke");
    }
}

// -- CanvasLayer pipeline tests ---------------------------------

#[cfg(all(feature = "kitty-encoder", feature = "sixel-encoder"))]
mod canvas_pipeline {
    use super::*;

    #[test]
    fn kitty_canvas_layer_pipeline() {
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
        // Diagonal pixel at canvas-local (1,1) -> fb (3,3) should be green.
        assert_eq!(fb.get_pixel(3, 3), Some(&[0, 255, 0, 255]));
        // Circle cardinal point at canvas-local (5,2) -> fb (7,4) should be red.
        assert_eq!(fb.get_pixel(7, 4), Some(&[255, 0, 0, 255]));
    }

    #[test]
    fn sixel_canvas_layer_pipeline() {
        let mut stack = LayerStack::new();
        stack.push(SolidColor::new(0, 0, 0, 255).with_z(0));
        let mut canvas = CanvasLayer::new(10, 10).at(2, 2).with_z(10);
        canvas.draw_pixel(0, 0, [255, 255, 255, 255]);
        canvas.draw_line(0, 0, 9, 9, [0, 255, 0, 255]);
        canvas.draw_circle(5, 5, 3, [255, 0, 0, 255]);
        stack.push(canvas);

        let mut fb = FrameBuffer::new(20, 20);
        stack.render(&mut fb);

        let bytes = Protocol::Sixel.encode(&fb).unwrap();
        assert!(!bytes.is_empty(), "Sixel canvas output must not be empty");
        // Diagonal pixel at canvas-local (1,1) -> fb (3,3) should be green.
        assert_eq!(fb.get_pixel(3, 3), Some(&[0, 255, 0, 255]));
    }
}

// -- DropShadow pipeline tests ----------------------------------

#[cfg(all(feature = "kitty-encoder", feature = "sixel-encoder"))]
mod shadow_pipeline {
    use super::*;

    #[test]
    fn kitty_drop_shadow_pipeline() {
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
        // Original rect at (5,5) should be white.
        assert_eq!(fb.get_pixel(5, 5), Some(&[255, 255, 255, 255]));
        // Shadow at offset (7,7) should be dark.
        let shadow_px = fb.get_pixel(7, 7).unwrap();
        assert!(shadow_px[3] > 0, "shadow pixel should have alpha");
    }

    #[test]
    fn sixel_drop_shadow_pipeline() {
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

        let bytes = Protocol::Sixel.encode(&fb).unwrap();
        assert!(!bytes.is_empty(), "Sixel shadow output must not be empty");
        // Original rect at (5,5) should be white.
        assert_eq!(fb.get_pixel(5, 5), Some(&[255, 255, 255, 255]));
    }
}

// -- GradientLayer pipeline tests ---------------------------------

#[cfg(all(feature = "kitty-encoder", feature = "sixel-encoder"))]
mod gradient_pipeline {
    use super::*;

    #[allow(deprecated)]
    #[test]
    fn kitty_gradient_layer_pipeline() {
        let mut stack = LayerStack::new();
        stack.push(SolidColor::new(0, 0, 64, 255).with_z(0));
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
            .with_z(10),
        );

        let mut fb = FrameBuffer::new(20, 10);
        stack.render(&mut fb);

        let bytes = Protocol::Kitty.encode(&fb).unwrap();
        assert!(bytes.starts_with(b"\x1b_G"));
        assert!(bytes.ends_with(b"\x1b\\"));
        // Verify the gradient produced non-uniform pixels.
        let top_left = fb.get_pixel(0, 0).unwrap();
        let top_right = fb.get_pixel(19, 0).unwrap();
        assert_ne!(
            top_left[0], top_right[0],
            "gradient must produce different R values at edges"
        );
    }

    #[allow(deprecated)]
    #[test]
    fn kitty_gradient_layer_radial_pipeline() {
        let mut stack = LayerStack::new();
        stack.push(
            GradientLayer::radial(
                0,
                0,
                20,
                20,
                [255, 255, 255, 255],
                [0, 0, 0, 255],
                10,
                10,
                10,
            )
            .with_z(5),
        );

        let mut fb = FrameBuffer::new(20, 20);
        stack.render(&mut fb);

        let bytes = Protocol::Kitty.encode(&fb).unwrap();
        assert!(bytes.starts_with(b"\x1b_G"));
        // Center should differ from corner.
        let center = fb.get_pixel(10, 10).unwrap();
        let corner = fb.get_pixel(0, 0).unwrap();
        assert_ne!(
            center, corner,
            "radial gradient must produce different pixels at center vs corner"
        );
    }

    #[allow(deprecated)]
    #[test]
    fn sixel_gradient_layer_pipeline() {
        let mut stack = LayerStack::new();
        stack.push(SolidColor::new(0, 0, 0, 255).with_z(0));
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
            .with_z(10),
        );

        let mut fb = FrameBuffer::new(20, 10);
        stack.render(&mut fb);

        let bytes = Protocol::Sixel.encode(&fb).unwrap();
        assert!(!bytes.is_empty(), "Sixel gradient output must not be empty");
    }

    #[allow(deprecated)]
    #[test]
    fn sixel_gradient_layer_radial_pipeline() {
        let mut stack = LayerStack::new();
        stack.push(
            GradientLayer::radial(
                0,
                0,
                20,
                20,
                [255, 255, 255, 255],
                [0, 0, 0, 255],
                10,
                10,
                10,
            )
            .with_z(5),
        );

        let mut fb = FrameBuffer::new(20, 20);
        stack.render(&mut fb);

        let bytes = Protocol::Sixel.encode(&fb).unwrap();
        assert!(
            !bytes.is_empty(),
            "Sixel radial gradient output must not be empty"
        );
    }
}

// -- BorderLayer pipeline tests -----------------------------------

#[cfg(all(feature = "kitty-encoder", feature = "sixel-encoder"))]
mod border_pipeline {
    use super::*;

    #[test]
    fn kitty_border_layer_pipeline() {
        let mut stack = LayerStack::new();
        stack.push(SolidColor::new(0, 0, 64, 255).with_z(0));
        stack.push(BorderLayer::new(2, 2, 6, 4, [255, 200, 0, 255], 1).with_z(10));

        let mut fb = FrameBuffer::new(20, 10);
        stack.render(&mut fb);

        let bytes = Protocol::Kitty.encode(&fb).unwrap();
        assert!(bytes.starts_with(b"\x1b_G"));
        assert!(bytes.ends_with(b"\x1b\\"));
        // Border edge should be the border colour.
        assert_eq!(fb.get_pixel(2, 2), Some(&[255, 200, 0, 255]));
        // Interior should be the background colour (solid blue).
        assert_eq!(fb.get_pixel(4, 3), Some(&[0, 0, 64, 255]));
    }

    #[test]
    fn sixel_border_layer_pipeline() {
        let mut stack = LayerStack::new();
        stack.push(SolidColor::new(0, 0, 0, 255).with_z(0));
        stack.push(BorderLayer::new(1, 1, 8, 6, [0, 255, 200, 255], 2).with_z(10));

        let mut fb = FrameBuffer::new(20, 10);
        stack.render(&mut fb);

        let bytes = Protocol::Sixel.encode(&fb).unwrap();
        assert!(!bytes.is_empty(), "Sixel border output must not be empty");
        // Border edge should be the border colour.
        assert_eq!(fb.get_pixel(1, 1), Some(&[0, 255, 200, 255]));
        // Interior should be black (background).
        assert_eq!(fb.get_pixel(4, 3), Some(&[0, 0, 0, 255]));
    }
}
// SceneGraph pipeline tests

#[cfg(all(feature = "kitty-encoder", feature = "sixel-encoder"))]
mod scene_graph_pipeline {
    use super::*;

    #[test]
    fn kitty_scene_graph_renders_layers() {
        let mut scene = SceneGraph::new();
        let group = scene.add_group((5, 3), 0.8, true);
        scene.add_child_to(group, RectLayer::new(0, 0, 10, 5, [255, 0, 0, 255]));
        scene.add_child_to(group, RectLayer::new(11, 0, 10, 5, [0, 255, 0, 255]));
        let mut fb = FrameBuffer::new(30, 15);
        scene.render(&mut fb, (0, 0), 1.0);
        let bytes = Protocol::Kitty.encode(&fb).unwrap();
        assert!(bytes.starts_with(b"\x1b_G"));
        assert!(bytes.ends_with(b"\x1b\\"));
        let px1 = fb.get_pixel(5, 3).unwrap();
        let px2 = fb.get_pixel(16, 3).unwrap();
        assert_eq!(px1[0], 255, "first rect R");
        assert_eq!(px1[1], 0, "first rect G");
        assert_eq!(px2[1], 255, "second rect G");
        assert_eq!(px2[0], 0, "second rect R");
    }

    #[test]
    fn kitty_scene_graph_hidden_parent_hides_children() {
        let mut scene = SceneGraph::new();
        let group = scene.add_group((0, 0), 1.0, false);
        scene.add_child_to(group, RectLayer::new(0, 0, 10, 10, [255, 0, 0, 255]));
        let mut fb = FrameBuffer::new(10, 10);
        scene.render(&mut fb, (0, 0), 1.0);
        let px = fb.get_pixel(5, 5).unwrap();
        assert_eq!(px[3], 0, "hidden parent should not render");
        let bytes = Protocol::Kitty.encode(&fb).unwrap();
        assert!(!bytes.is_empty());
    }

    #[test]
    fn sixel_scene_graph_renders_layers() {
        let mut scene = SceneGraph::new();
        let group = scene.add_group((3, 2), 1.0, true);
        scene.add_child_to(group, RectLayer::new(0, 0, 8, 4, [0, 0, 255, 255]));
        let mut fb = FrameBuffer::new(20, 10);
        scene.render(&mut fb, (0, 0), 1.0);
        let bytes = Protocol::Sixel.encode(&fb).unwrap();
        assert!(!bytes.is_empty());
        let px = fb.get_pixel(3, 2).unwrap();
        assert_eq!(px[2], 255, "rect should be blue");
    }

    #[test]
    fn sixel_scene_graph_opacity_cascades() {
        let mut scene = SceneGraph::new();
        let group = scene.add_group((0, 0), 0.5, true);
        scene.add_child_to(group, SolidColor::new(255, 0, 0, 255).with_z(0));
        let mut fb = FrameBuffer::new(5, 5);
        scene.render(&mut fb, (0, 0), 1.0);
        let bytes = Protocol::Sixel.encode(&fb).unwrap();
        assert!(!bytes.is_empty());
        let px = fb.get_pixel(2, 2).unwrap();
        let alpha = px[3] as i32;
        assert!((126..=130).contains(&alpha), "alpha was {}", alpha);
    }
}
