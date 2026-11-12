//! Opt-in installed-font rendering evidence stays outside deterministic goldens.
//!
//! The specimen rows use green SGR text so monochrome fallback tint is visible
//! while authored emoji colors remain independent of the foreground choice.
//! Padding and blank cells make geometric overflow observable in the readback.
//! Cursor visibility is disabled, since a cursor would legitimately add ink
//! in a guard cell and could obscure an otherwise empty-glyph failure.
//! Each cluster is fed whole through the same UTF-8 parser used by a shell.
//! D-31 determines its one/two-cell width before the native source is queried.
//! The guard obtains that stored width rather than guessing from Unicode.
//! Combining selectors and joiners consequently cannot create extra instances
//! in adjacent cells without failing the spatial test.
//! Production TextContext computes the primary baseline and cell dimensions.
//! System font ascenders cannot influence the screenshot's terminal geometry.
//! The first render exercises cold mapping, shaping, extraction and uploads.
//! A second executor then starts with empty textures. CPU restoration marks
//! the retained mirrors dirty, and a new frame uploads those same bytes.
//! Pixel equality after this step catches missing restore records or stale
//! bindings. An unchanged preparation count also guards against reparsing
//! the font merely to recover from a discarded GPU resource.
//! No tolerance or reference blessing hides differences in this restore check.
//! The native GPU evidence measures visible behavior, while the separate
//! coverage test records the exact matched family and excludes LastResort.
//! A scene with missing-glyph boxes cannot replace that coverage contract.
//! System font revisions may change appearance between machines; user review
//! evaluates these diagnostic PNGs alongside the ordinary interactive app.
//! These frames use the ordinary bundled primary and native sources. Complete
//! clusters enter VT and layout before drawing, so widths and atlas uploads are
//! exercised together. Readbacks check every cluster's cells, not just a scene's
//! aggregate ink. The separate native coverage witness rejects missing glyphs.
//! The probe never blesses system-font pixels as cross-machine references.
//! Files under ignored target let the user judge weight and baseline at both DPRs.
//! A restored CPU mirror must reproduce pixels after source bytes are released.
//! The GPU device is isolated from any interactive review window and destroyed
//! explicitly once blocking readbacks finish. No normal harness installs sources.
use crate::block_on::block_on;
use puzzle_render::{FrameRenderer, Renderer};
use puzzle_term::{
    frame::RenderFrame,
    frame_builder::{FrameBuilder, context::TextContext},
    vt::terminal::Terminal,
};
/// The same cluster table exercises text, selectors, joined emoji and native scripts.
const CLUSTERS: &[&str] = &[
    "🦀",
    "✌️",
    "☺️",
    "👨‍👩‍👧‍👦",
    "🇩🇪",
    "👍🏽",
    "漢",
    "字",
    "か",
    "な",
    "한",
    "글",
    "𝄞",
];
/// Draws each installed-font cluster at DPR 1/2 through production frame assembly.
pub(crate) fn run() -> Result<(), String> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let renderer = block_on(Renderer::new(&instance, None)).map_err(|e| e.to_string())?;
    for dpr in [1.0, 2.0] {
        draw(&renderer, dpr)?;
    }
    renderer.device().destroy();
    Ok(())
}
/// Each row starts with two blank guard cells, followed by exactly one cluster.
fn fixture(dpr: f32) -> Result<(TextContext, FrameBuilder, Terminal, RenderFrame), String> {
    let mut context = TextContext::new(crate::font_data::bundled_font_set()?, 14.0, dpr)?;
    crate::system_fonts::install(&mut context, &[])?;
    let mut builder = FrameBuilder::new(&context);
    let m = context.metrics();
    builder.set_surface_size([
        32 + 8 * m.width_px,
        32 + CLUSTERS.len() as u32 * m.height_px,
    ]);
    let mut vt = Terminal::new(8, CLUSTERS.len(), 0, 0);
    // Hide the cursor so guard pixels cannot be confused with cursor geometry.
    vt.advance(b"\x1b[?25l\x1b[32m");
    for (row, cluster) in CLUSTERS.iter().enumerate() {
        vt.advance(format!("\x1b[{};3H{cluster}", row + 1).as_bytes());
    }
    Ok((context, builder, vt, RenderFrame::default()))
}
/// Readback protects cold uploads, warm reuse and CPU-mirror restoration.
fn draw(renderer: &Renderer, dpr: f32) -> Result<(), String> {
    let (mut context, mut builder, vt, mut frame) = fixture(dpr)?;
    crate::system_fonts::settle::terminal(&mut context, &mut builder, &vt, &mut frame)?;
    let mut executor = FrameRenderer::new(renderer, wgpu::TextureFormat::Rgba8Unorm);
    let (image, _) = executor
        .render_to_image(renderer, &mut frame)
        .map_err(|e| e.to_string())?;
    guard(&image, context.metrics(), &vt)?;
    // Retained geometry must remain reusable after the first executor disappears.
    let count = context.prepared_glyphs();
    context.reset_atlas();
    builder.build(&mut context, &vt, &mut frame)?;
    let mut fresh = FrameRenderer::new(renderer, wgpu::TextureFormat::Rgba8Unorm);
    let (restored, _) = fresh
        .render_to_image(renderer, &mut frame)
        .map_err(|e| e.to_string())?;
    if image.pixels != restored.pixels || context.prepared_glyphs() != count {
        return Err("native source restore changed pixels or re-prepared outlines".into());
    }
    let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/m11-system");
    std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
    crate::png::write(&out.join(format!("specimen-{}x.png", dpr as u32)), &image)?;
    println!(
        "system GPU dpr={dpr} clusters={} restored_pixels=identical prepared={count}",
        CLUSTERS.len()
    );
    Ok(())
}
/// Requires visible ink in each assigned span and none in its surrounding guard cells.
fn guard(
    image: &puzzle_render::RgbaImage,
    m: puzzle_term::text::metrics::CellMetrics,
    vt: &Terminal,
) -> Result<(), String> {
    // The corner is clear padding and cannot contain a glyph or cursor sample.
    let bg = &image.pixels[..4];
    for (row, cluster) in CLUSTERS.iter().enumerate() {
        let cell = vt.screen().row(row).ok_or("probe row missing")?.cells[2];
        let span = if cell.flags.contains(puzzle_term::vt::cell::CellFlags::WIDE) {
            2
        } else {
            1
        };
        // These bounds use the exact primary cell dimensions consumed by frame assembly.
        let start = 16 + 2 * m.width_px;
        let end = start + span * m.width_px;
        let mut ink = 0;
        for y in 16 + row as u32 * m.height_px..16 + (row as u32 + 1) * m.height_px {
            for x in 16..image.width - 16 {
                let at = ((y * image.width + x) * 4) as usize;
                if &image.pixels[at..at + 4] != bg {
                    if x < start || x >= end {
                        return Err(format!(
                            "native {cluster} escaped its {span} cells at {x},{y}"
                        ));
                    }
                    ink += 1;
                }
            }
        }
        if ink == 0 {
            return Err(format!("native {cluster} has no cell ink"));
        }
        println!("system GPU cluster={cluster} span={span} ink={ink}");
    }
    Ok(())
}
