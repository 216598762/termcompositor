//! Benchmarks for the `termcompositor` compositor core.
//!
//! Measures throughput of framebuffer operations, layer rendering,
//! and (when the relevant Cargo features are enabled) text glyph
//! rasterization and protocol encoding.
//!
//! Run with: `cargo bench`
//!
//! Feature-gated benchmarks use stub functions when the feature is
//! disabled, so the entire benchmark suite always compiles.
//!
//! Run with `--all-features` for the full suite:
//!
//! ```bash
//! cargo bench --all-features
//! ```

// The `criterion_group!` macro generates functions without doc comments.
// Allow missing docs at the crate level so clippy does not complain.
#![allow(missing_docs)]

use criterion::{black_box, criterion_group, criterion_main, Criterion};

// -- Framebuffer benchmarks -----------------------------------------

fn bench_framebuffer_new(c: &mut Criterion) {
    use termcompositor::FrameBuffer;

    c.bench_function("framebuffer/new_80x24", |b| {
        b.iter(|| FrameBuffer::new(black_box(80), black_box(24)))
    });

    c.bench_function("framebuffer/new_1920x1080", |b| {
        b.iter(|| FrameBuffer::new(black_box(1920), black_box(1080)))
    });
}

fn bench_framebuffer_clear(c: &mut Criterion) {
    use termcompositor::FrameBuffer;

    let mut fb = FrameBuffer::new(800, 600);
    c.bench_function("framebuffer/clear_800x600", |b| b.iter(|| fb.clear()));
}

fn bench_blend_over(c: &mut Criterion) {
    use termcompositor::blend_over;

    let src = [200, 100, 50, 200];
    c.bench_function("framebuffer/blend_over_opaque", |b| {
        b.iter(|| {
            let mut dst = [0, 0, 0, 255];
            blend_over(black_box(&mut dst), black_box(&src), black_box(1.0));
            dst
        })
    });

    c.bench_function("framebuffer/blend_over_translucent", |b| {
        b.iter(|| {
            let mut dst = [10, 20, 30, 128];
            blend_over(black_box(&mut dst), black_box(&src), black_box(0.5));
            dst
        })
    });

    c.bench_function("framebuffer/blend_over_zero_alpha", |b| {
        b.iter(|| {
            let mut dst = [10, 20, 30, 128];
            blend_over(black_box(&mut dst), black_box(&src), black_box(0.0));
            dst
        })
    });
}

fn bench_framebuffer_get_pixel(c: &mut Criterion) {
    use termcompositor::FrameBuffer;

    let fb = FrameBuffer::new(800, 600);
    c.bench_function("framebuffer/get_pixel_in_bounds", |b| {
        b.iter(|| fb.get_pixel(black_box(400), black_box(300)))
    });

    c.bench_function("framebuffer/get_pixel_out_of_bounds", |b| {
        b.iter(|| fb.get_pixel(black_box(9999), black_box(9999)))
    });
}

// -- Layer benchmarks ------------------------------------------------

fn bench_solid_color_render(c: &mut Criterion) {
    use termcompositor::{FrameBuffer, Layer, SolidColor};

    let layer = SolidColor::new(100, 150, 200, 255);
    let mut fb = FrameBuffer::new(800, 600);

    c.bench_function("layer/solid_color_render_800x600", |b| {
        b.iter(|| layer.render(black_box(&mut fb), black_box((0, 0)), black_box(1.0)))
    });

    c.bench_function("layer/solid_color_half_opacity", |b| {
        b.iter(|| layer.render(black_box(&mut fb), black_box((0, 0)), black_box(0.5)))
    });
}

fn bench_rect_layer_render(c: &mut Criterion) {
    use termcompositor::{FrameBuffer, Layer, RectLayer};

    let layer = RectLayer::new(100, 50, 600, 500, [0, 255, 0, 200]);
    let mut fb = FrameBuffer::new(800, 600);

    c.bench_function("layer/rect_render_600x500", |b| {
        b.iter(|| layer.render(black_box(&mut fb), black_box((0, 0)), black_box(1.0)))
    });

    c.bench_function("layer/rect_render_with_offset", |b| {
        b.iter(|| layer.render(black_box(&mut fb), black_box((10, 10)), black_box(0.75)))
    });
}

fn bench_rect_layer_small(c: &mut Criterion) {
    use termcompositor::{FrameBuffer, Layer, RectLayer};

    let layer = RectLayer::new(0, 0, 10, 10, [255, 0, 0, 255]);
    let mut fb = FrameBuffer::new(100, 100);

    c.bench_function("layer/rect_render_10x10", |b| {
        b.iter(|| layer.render(black_box(&mut fb), black_box((0, 0)), black_box(1.0)))
    });
}

// -- LayerStack compositor benchmarks ---

fn bench_layerstack_empty_render(c: &mut Criterion) {
    use termcompositor::{FrameBuffer, LayerStack};

    let stack = LayerStack::new();
    let mut fb = FrameBuffer::new(800, 600);

    c.bench_function("compositor/empty_stack_render", |b| {
        b.iter(|| stack.render(black_box(&mut fb)))
    });
}

fn bench_layerstack_single_solid(c: &mut Criterion) {
    use termcompositor::{FrameBuffer, LayerStack, SolidColor};

    let mut stack = LayerStack::new();
    stack.push(SolidColor::new(0, 64, 128, 255));
    let mut fb = FrameBuffer::new(800, 600);

    c.bench_function("compositor/single_solid_render", |b| {
        b.iter(|| stack.render(black_box(&mut fb)))
    });
}

fn bench_layerstack_multi_layer(c: &mut Criterion) {
    use termcompositor::{FrameBuffer, LayerStack, RectLayer, SolidColor, TextLayer};

    let mut stack = LayerStack::new();
    stack.push(SolidColor::new(0, 0, 64, 255).with_name("bg"));
    stack.push(
        RectLayer::new(50, 30, 200, 100, [0, 200, 0, 180])
            .with_z(10)
            .with_name("rect1"),
    );
    stack.push(
        RectLayer::new(300, 150, 200, 100, [200, 0, 0, 180])
            .with_z(5)
            .with_name("rect2"),
    );
    stack.push(
        TextLayer::new(10, 5, "Hello, compositor!", [255, 255, 255, 255])
            .with_z(20)
            .with_name("text"),
    );

    let mut fb = FrameBuffer::new(800, 600);

    c.bench_function("compositor/multi_layer_4_entries", |b| {
        b.iter(|| stack.render(black_box(&mut fb)))
    });
}

fn bench_layerstack_many_rects(c: &mut Criterion) {
    use termcompositor::{FrameBuffer, LayerStack, RectLayer, SolidColor};

    let mut stack = LayerStack::new();
    stack.push(SolidColor::new(0, 0, 0, 255).with_name("bg"));
    // 50 overlapping rectangles at random positions.
    for i in 0..50 {
        let x = (i * 17) % 700;
        let y = (i * 31) % 500;
        stack.push(
            RectLayer::new(x, y, 60, 40, [(i * 50) as u8, 100, 200, 150])
                .with_z(i)
                .with_name(format!("rect_{i}")),
        );
    }
    let mut fb = FrameBuffer::new(800, 600);

    c.bench_function("compositor/50_rects_render", |b| {
        b.iter(|| stack.render(black_box(&mut fb)))
    });
}

// -- TextLayer benchmarks (font-rasterizer feature) -----------------
//
// When the feature is off, the benchmarks run empty stubs.

#[cfg(feature = "font-rasterizer")]
mod text_benches {
    use super::*;
    use termcompositor::{FrameBuffer, Layer, TextLayer};

    pub fn bench_text_layer_text_width(c: &mut Criterion) {
        let short = TextLayer::new(0, 0, "Hello, world!", [255; 4]);
        let long = TextLayer::new(0, 0, "x".repeat(200), [255; 4]);
        let multi = TextLayer::new(0, 0, "line1\nline2\nline3\nline4\nline5", [255; 4]);

        c.bench_function("text/text_width_short", |b| b.iter(|| short.text_width()));
        c.bench_function("text/text_width_200_chars", |b| {
            b.iter(|| long.text_width())
        });
        c.bench_function("text/text_width_multi_line", |b| {
            b.iter(|| multi.text_width())
        });
    }

    pub fn bench_text_layer_render(c: &mut Criterion) {
        let short = TextLayer::new(0, 0, "Hello", [200, 100, 50, 255]).with_font_size(14.0);
        let short_large = TextLayer::new(0, 0, "Hello", [200, 100, 50, 255]).with_font_size(48.0);
        let multi =
            TextLayer::new(0, 0, "line1\nline2\nline3", [200, 100, 50, 255]).with_font_size(14.0);

        let mut fb_short = FrameBuffer::new(100, 20);
        c.bench_function("text/render_short_14px", |b| {
            b.iter(|| short.render(black_box(&mut fb_short), black_box((0, 0)), black_box(1.0)))
        });

        let mut fb_large = FrameBuffer::new(500, 60);
        c.bench_function("text/render_short_48px", |b| {
            b.iter(|| {
                short_large.render(black_box(&mut fb_large), black_box((0, 0)), black_box(1.0))
            })
        });

        let mut fb_multi = FrameBuffer::new(100, 60);
        c.bench_function("text/render_multi_line_14px", |b| {
            b.iter(|| multi.render(black_box(&mut fb_multi), black_box((0, 0)), black_box(1.0)))
        });
    }
}

#[cfg(not(feature = "font-rasterizer"))]
mod text_benches {
    pub fn bench_text_layer_text_width(_c: &mut criterion::Criterion) {}
    pub fn bench_text_layer_render(_c: &mut criterion::Criterion) {}
}

// -- Encoder benchmarks (feature-gated) ------------------------------
//
// When the relevant encoder feature is off, the benchmarks run empty
// stubs.

#[cfg(feature = "kitty-encoder")]
mod kitty_benches {
    use super::*;
    use termcompositor::{FrameBuffer, Protocol};

    pub fn bench_kitty_encode(c: &mut Criterion) {
        let small = FrameBuffer::new(80, 24);
        let medium = FrameBuffer::new(800, 600);

        c.bench_function("encoder/kitty_small_80x24", |b| {
            b.iter(|| {
                let mut out = Vec::with_capacity(4096);
                termcompositor::encoder::dispatch_to_writer(
                    Protocol::Kitty,
                    black_box(&small),
                    black_box(&mut out),
                )
                .unwrap()
            })
        });

        c.bench_function("encoder/kitty_medium_800x600", |b| {
            b.iter(|| {
                let mut out = Vec::new();
                termcompositor::encoder::dispatch_to_writer(
                    Protocol::Kitty,
                    black_box(&medium),
                    black_box(&mut out),
                )
                .unwrap()
            })
        });
    }
}

#[cfg(not(feature = "kitty-encoder"))]
mod kitty_benches {
    pub fn bench_kitty_encode(_c: &mut criterion::Criterion) {}
}

#[cfg(feature = "sixel-encoder")]
mod sixel_benches {
    use super::*;
    use termcompositor::{FrameBuffer, Protocol};

    pub fn bench_sixel_encode(c: &mut Criterion) {
        let small = FrameBuffer::new(80, 24);
        let medium = FrameBuffer::new(800, 600);

        c.bench_function("encoder/sixel_small_80x24", |b| {
            b.iter(|| {
                let mut out = Vec::new();
                termcompositor::encoder::dispatch_to_writer(
                    Protocol::Sixel,
                    black_box(&small),
                    black_box(&mut out),
                )
                .unwrap()
            })
        });

        c.bench_function("encoder/sixel_medium_800x600", |b| {
            b.iter(|| {
                let mut out = Vec::new();
                termcompositor::encoder::dispatch_to_writer(
                    Protocol::Sixel,
                    black_box(&medium),
                    black_box(&mut out),
                )
                .unwrap()
            })
        });
    }
}

#[cfg(not(feature = "sixel-encoder"))]
mod sixel_benches {
    pub fn bench_sixel_encode(_c: &mut criterion::Criterion) {}
}

// -- Assembly --------------------------------------------------------

criterion_group!(
    name = framebuffer;
    config = Criterion::default().sample_size(100);
    targets =
        bench_framebuffer_new,
        bench_framebuffer_clear,
        bench_blend_over,
        bench_framebuffer_get_pixel,
);

criterion_group!(
    name = layers;
    config = Criterion::default().sample_size(100);
    targets =
        bench_solid_color_render,
        bench_rect_layer_render,
        bench_rect_layer_small,
);

criterion_group!(
    name = compositor;
    config = Criterion::default().sample_size(100);
    targets =
        bench_layerstack_empty_render,
        bench_layerstack_single_solid,
        bench_layerstack_multi_layer,
        bench_layerstack_many_rects,
);

criterion_group!(
    name = text;
    config = Criterion::default().sample_size(100);
    targets =
        text_benches::bench_text_layer_text_width,
        text_benches::bench_text_layer_render,
);

criterion_group!(
    name = encoder_kitty;
    config = Criterion::default().sample_size(50);
    targets = kitty_benches::bench_kitty_encode,
);

criterion_group!(
    name = encoder_sixel;
    config = Criterion::default().sample_size(50);
    targets = sixel_benches::bench_sixel_encode,
);

criterion_main!(
    framebuffer,
    layers,
    compositor,
    text,
    encoder_kitty,
    encoder_sixel
);
