//! A stationary Unicode screen must replace every provisional glyph, including queue pressure.
#![allow(
    clippy::unwrap_used,
    reason = "fixture failures must fail this regression"
)]
use super::*;
use puzzle_term::{frame::RenderFrame, frame_builder::FrameBuilder, vt::terminal::Terminal};

/// A covered CJK screen must have real ink on its first build and GPU submission.
#[test]
fn stationary_cjk_screen_finishes_fallback() {
    first_frame(0x4e00..0x6200, "cjk");
}

/// The exact 5,120-Hangul pressure fixture must finish in its first frame (M16.2).
#[test]
fn frozen_hangul_first_frame_has_complete_gpu_ink() {
    first_frame(0xc000..0xd400, "hangul");
}

/// Fresh source and atlas ownership ensure cached history cannot rescue a cold first frame.
fn first_frame(range: std::ops::Range<u32>, label: &str) {
    let mut context =
        TextContext::new(crate::font_data::bundled_font_set().unwrap(), 14.0, 1.0).unwrap();
    install(&mut context, &[]).unwrap();
    let mut builder = FrameBuilder::new(&context);
    let mut vt = Terminal::new(160, 64, 0, 0);
    let text: String = range.filter_map(char::from_u32).collect();
    vt.advance(b"\x1b[?25l");
    vt.advance(text.as_bytes());
    let mut frame = RenderFrame::default();
    builder.build(&mut context, &vt, &mut frame).unwrap();
    assert!(
        !context.sources_pending(),
        "first frame left glyph work queued"
    );
    assert!(
        complete_ink(&builder, &mut context),
        "covered first-frame cells have provisional ink"
    );
    assert_first_gpu(&mut frame, context.metrics(), label);
    assert_warm_redraw(&mut context, &vt, &mut builder, &mut frame);
}

/// Reads actual first-submission pixels, independently of CPU glyph readiness flags.
fn assert_first_gpu(
    frame: &mut RenderFrame,
    metrics: puzzle_term::text::metrics::CellMetrics,
    label: &str,
) {
    let _gpu = crate::gpu_test::lock();
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let renderer =
        crate::block_on::block_on(puzzle_render::Renderer::new(&instance, None)).unwrap();
    let mut executor =
        puzzle_render::FrameRenderer::new(&renderer, wgpu::TextureFormat::Rgba8Unorm);
    let (image, _) = executor.render_to_image(&renderer, frame).unwrap();
    let pixel = |x: u32, y: u32| {
        &image.pixels
            [((y * image.width + x) * 4) as usize..((y * image.width + x) * 4 + 4) as usize]
    };
    let clip = frame
        .batches
        .iter()
        .find_map(|batch| batch.scissor)
        .unwrap();
    let mut missing = Vec::new();
    for row in 0..64 {
        for col in 0..80 {
            let x = clip.x + col * 2 * metrics.width_px;
            let y = clip.y + row * metrics.height_px;
            let paper = pixel(x, y);
            // Inner crops prevent a neighboring glyph's overhang from rescuing a blank cell.
            let has_ink = (y + 1..y + metrics.height_px)
                .any(|py| (x + 1..x + 2 * metrics.width_px).any(|px| pixel(px, py) != paper));
            if !has_ink {
                missing.push((row, col));
            }
        }
    }
    // Cargo tests run from the crate directory; evidence belongs to the workspace target.
    let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/m16-2");
    std::fs::create_dir_all(&directory).unwrap();
    crate::png::write(&directory.join(format!("{label}-first.png")), &image).unwrap();
    renderer.device().destroy();
    assert!(
        missing.is_empty(),
        "first-frame GPU cells without ink: {missing:?}"
    );
}

/// Full redraws reuse ink beyond the temporary registry allowance, without new source jobs.
fn assert_warm_redraw(
    context: &mut TextContext,
    vt: &Terminal,
    builder: &mut FrameBuilder,
    frame: &mut RenderFrame,
) {
    let prepared = context.prepared_glyphs();
    // Force every row to rebuild; unchanged cached rows cannot prove stable source keys.
    builder.set_context(context);
    let warm = allocation_counter::measure(|| builder.build(context, vt, frame).unwrap());
    assert_eq!(warm.count_total, 0, "warm Unicode redraw allocated");
    assert_eq!(context.prepared_glyphs(), prepared);
    assert!(!context.sources_pending());
}

/// Every covered source must resolve, including pressure-selected oracle bitmap ink.
pub(super) fn complete_ink(builder: &FrameBuilder, context: &mut TextContext) -> bool {
    let cells = builder.retained_ink(context);
    cells.len() == 5120 && cells.iter().all(|cell| cell[3] == 1)
}
