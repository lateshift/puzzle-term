//! A deliberately changed pixel must identify its cell and save all evidence.
#![allow(
    clippy::expect_used,
    reason = "test mismatch and cleanup are assertion boundaries"
)]
use super::*;
use crate::block_on::block_on;
use puzzle_term::frame::{Batch, PipelineKind, RasterInstance, RectU32, Upload, ZSlot};

/// Builds the small atlas-backed glyph used by the deferred-upload test.
fn glyph_frame(upload: bool) -> RenderFrame {
    // The mask enters the GPU only in the first frame. Later instances keep
    // the same UV and therefore need the retained atlas contents.
    let mut frame = RenderFrame {
        target_size: [32, 32],
        clear: [0.0, 0.0, 0.0, 1.0],
        ..RenderFrame::default()
    };
    if upload {
        frame.uploads.push(Upload::Mask {
            page: 0,
            rect: RectU32 {
                x: 0,
                y: 0,
                width: 4,
                height: 4,
            },
            pixels: vec![255; 16],
        });
    }
    frame.rasters.push(RasterInstance {
        rect: [4.0, 4.0, 4.0, 4.0],
        uv: [0.0, 0.0, 4.0 / 2048.0, 4.0 / 2048.0],
        tint: 0xffff_ffff,
        flags: 0,
    });
    frame.batches.push(Batch {
        slot: ZSlot::TextRaster,
        pipeline: PipelineKind::Raster,
        instances: 0..1,
        scissor: None,
    });
    frame
}

/// Deferred checks retain glyphs introduced by frames they did not inspect.
#[test]
#[ignore = "requires a native GPU adapter; accept M6 runs it explicitly"]
fn skipped_frame_uploads_match_cold_and_stale_atlas_is_detected() {
    let _gpu = crate::gpu_test::lock();
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let renderer = block_on(Renderer::new(&instance, None)).expect("test GPU renderer");
    let mut verifier = FrameVerifier {
        warm: FrameRenderer::new(&renderer, wgpu::TextureFormat::Rgba8Unorm),
    };
    let mut first = glyph_frame(true);
    let mut later = glyph_frame(false);
    // Frame one is presented but its idle check is replaced by frame two.
    verifier
        .observe_uploads(&renderer, &first)
        .expect("first upload");
    let mut cold = FrameRenderer::new(&renderer, wgpu::TextureFormat::Rgba8Unorm);
    for _ in 0..3 {
        // Each later frame carries no upload, as the real frame builder's
        // incremental contract requires after the glyph is cached.
        verifier
            .observe_uploads(&renderer, &later)
            .expect("later upload");
        let (warm_image, _) = verifier
            .warm
            .render_to_image(&renderer, &mut later)
            .expect("warm image");
        let (cold_image, _) = cold
            .render_to_image(&renderer, &mut first)
            .expect("cold image");
        assert_eq!(warm_image.pixels, cold_image.pixels);
    }
    // A renderer that missed frame one's upload must still fail the same
    // exact comparison, so the test cannot pass by ignoring glyph pixels.
    let mut stale = FrameRenderer::new(&renderer, wgpu::TextureFormat::Rgba8Unorm);
    let (stale_image, _) = stale
        .render_to_image(&renderer, &mut later)
        .expect("stale image");
    let (cold_image, _) = cold
        .render_to_image(&renderer, &mut first)
        .expect("cold image");
    assert_ne!(stale_image.pixels, cold_image.pixels);
}

/// Differential evidence uses the same padded cell coordinates as frames.
#[test]
fn mismatch_reports_cell_and_three_pngs() {
    let dir = std::env::temp_dir().join(format!("puzzle-verify-{}", std::process::id()));
    let cold = RgbaImage {
        width: 32,
        height: 32,
        pixels: vec![0; 32 * 32 * 4],
    };
    let mut warm = RgbaImage {
        width: 32,
        height: 32,
        pixels: cold.pixels.clone(),
    };
    warm.pixels[(17 * 32 + 18) * 4] = 255;
    let evidence = FrameVerifier::save_difference(&warm, &cold, 8, 8, 999_999, &dir)
        .expect("changed pixel evidence");
    assert!(evidence.contains("(0, 0)"));
    assert!(evidence.contains("frame-999999-difference.png"));
    std::fs::remove_dir_all(dir).expect("remove private test images");
}

/// A ten-ms diagnostic cannot start before a newly paced frame at 16 ms.
#[test]
fn slow_verification_skips_a_new_frame() {
    let zero = Instant::now();
    let mut checks = VerifySchedule::default();
    checks.presented(zero);
    let next = zero + Duration::from_millis(16);
    assert!(!checks.ready(zero + Duration::from_millis(10), Some(next)));
    checks.frame_started();
    checks.presented(next);
    assert!(!checks.ready(next + Duration::from_millis(10), None));
    assert!(checks.ready(next + Duration::from_millis(250), None));
    assert_eq!(checks.finish(), 1);
}

/// Glyph preparation must survive omitted presentation before the next cached frame.
#[test]
#[ignore = "requires native GPU; run explicitly with the startup upload guard"]
fn unavailable_frame_preserves_ink_in_every_alphabet_cell() {
    let _gpu = crate::gpu_test::lock();
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let renderer = block_on(Renderer::new(&instance, None)).expect("GPU");
    let mut context = puzzle_term::frame_builder::context::TextContext::new(
        puzzle_term::text::font_set::FontSet::new(
            puzzle_term::slug_scene::FontData::new(
                include_bytes!("../../../../../../assets/fonts/JetBrainsMono-Variable.ttf")
                    .to_vec(),
                0,
            ),
            None,
            None,
        )
        .expect("fonts"),
        15.0,
        2.0,
    )
    .expect("metrics");
    let metrics = context.metrics();
    let mut builder = FrameBuilder::new(&context);
    let mut terminal = puzzle_term::vt::Terminal::new(52, 2, 0, 0);
    terminal.advance(b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz");
    let mut frame = RenderFrame::default();
    builder
        .build(&mut context, &terminal, &mut frame)
        .expect("prepare without surface");
    assert!(!frame.uploads.is_empty());
    // No surface is available: deliberately omit the GPU consumer. This is
    // the same producer state after hidden/minimized, acquire failure or a
    // pacing decision discards an image. Only row geometry may be replaced.
    terminal.advance(b"\r\n");
    builder
        .build(&mut context, &terminal, &mut frame)
        .expect("next cached frame");
    let mut gpu = FrameRenderer::new(&renderer, wgpu::TextureFormat::Rgba8Unorm);
    let (image, _) = gpu
        .render_to_image(&renderer, &mut frame)
        .expect("next presentation");
    assert!(
        frame.uploads.is_empty(),
        "successful GPU writer must acknowledge uploads"
    );
    alphabet_ink(&image, metrics);
    // Recreated textures cannot use previously acknowledged placements alone.
    // The host invalidates the context generation, forcing every visible glyph
    // to be prepared/uploaded again without requiring another application write.
    context.reset_atlas();
    builder
        .build(&mut context, &terminal, &mut frame)
        .expect("device incarnation rebuild");
    let mut fresh = FrameRenderer::new(&renderer, wgpu::TextureFormat::Rgba8Unorm);
    let (image, _) = fresh
        .render_to_image(&renderer, &mut frame)
        .expect("fresh GPU");
    alphabet_ink(&image, metrics);
}

/// Count ink per authored cell, excluding the cursor on the following row.
fn alphabet_ink(image: &RgbaImage, metrics: puzzle_term::text::metrics::CellMetrics) {
    let background = &image.pixels[..3];
    for column in 0..52 {
        let mut ink = 0;
        for y in 17..16 + metrics.height_px {
            for x in 17 + column * metrics.width_px..16 + (column + 1) * metrics.width_px {
                let at = ((y * image.width + x) * 4) as usize;
                ink += usize::from(image.pixels[at..at + 3] != *background);
            }
        }
        assert!(ink >= 3, "alphabet cell {column} has no glyph ink");
    }
}
