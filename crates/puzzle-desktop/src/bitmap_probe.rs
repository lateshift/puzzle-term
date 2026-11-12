//! Real installed emoji exercise the desktop PNG → fit → atlas → GPU path.
//!
//! This opt-in probe never changes the harness's bundled-font-only policy.
//! It reads Apple Color Emoji only where that file is installed, reports a
//! concrete skip otherwise, and writes diagnostic crops under ignored target.
//! A colored pixel outside the fixed two-cell span fails independently of
//! visual approval. Both DPRs use primary JetBrains Mono metrics and baseline.
//! The source is deliberately outside the bundled font set: it tests the codec
//! without changing the harness policy or adding a third-party repository asset.
//! System discovery is wired separately in M11.6, after this rendering contract.
//! No expected screenshot depends on macOS’s particular emoji font revision.
//! Instead, the mechanical guard asks only for ink bounds and authored color.
//! The outputs allow the user to assess appearance without self-certification.
//! A machine without this native font records why the probe did not run.
//! The decoder tests still exercise real compressed PNG streams on that machine.
//! Baselines travel with each placement so sized effects can reuse this adapter.
//! The adapter never assumes the live terminal has the probe’s original metrics.
use crate::{bitmap::Decoder, block_on::block_on};
use puzzle_render::{FrameRenderer, Renderer};
use puzzle_term::text::face::FontFace;
use puzzle_term::{
    frame::{Batch, PipelineKind, RasterInstance, RectU32, RenderFrame, Upload, ZSlot},
    text::{
        FontData, bitmap,
        glyph::{GlyphKey, GlyphPlacement, HintMode},
        raster::{PAGE, RasterAtlas, RasterKey},
    },
};

/// Uses a separate GPU device so the opt-in probe does not disturb a live window.
/// Reads the collection once and shares those bytes across the two device scales.
/// Missing installation is a concrete SKIP; missing expected glyphs are errors.
/// The device is explicitly destroyed after blocking readbacks finish.
/// Draws the installed crab at both device scales with the production renderer.
pub(crate) fn run() -> Result<(), String> {
    let path = std::path::Path::new("/System/Library/Fonts/Apple Color Emoji.ttc");
    if !path.is_file() {
        println!("SKIP installed bitmap emoji: Apple Color Emoji.ttc is not installed");
        return Ok(());
    }
    // Platform I/O owns font bytes; no system font is copied into the repository.
    let data = FontData::new(std::fs::read(path).map_err(|e| e.to_string())?, 0);
    let face = FontFace::new(data.clone(), 400)?;
    let glyph = face
        .glyph_id('🦀')?
        .ok_or("installed emoji font lacks crab")?;
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let renderer = block_on(Renderer::new(&instance, None)).map_err(|e| e.to_string())?;
    for dpr in [1.0, 2.0] {
        draw(&renderer, &data, glyph, dpr)?;
    }
    // All readbacks are complete before releasing this probe’s isolated device.
    renderer.device().destroy();
    Ok(())
}

/// TextContext supplies the same snapped font size and baseline as live layout.
/// Its font set stays bundled, keeping cell geometry independent of system fonts.
/// The probe does not submit a Session or alter grapheme widths: the explicit
/// placement pins the two-cell allocation that the production fitter must honor.
/// Keeps a bitmap's device rectangle inside its immutable row/span at both scales.
fn draw(renderer: &Renderer, data: &FontData, glyph: u32, dpr: f32) -> Result<(), String> {
    // Cell geometry comes from the primary face, never the emoji font’s advances.
    let context = puzzle_term::frame_builder::context::TextContext::new(
        crate::font_data::bundled_font_set()?,
        14.0,
        dpr,
    )?;
    let metrics = context.metrics();
    // Baseline origins are device pixels; DPR changes size but not the two-cell width.
    let placement = GlyphPlacement {
        // This standalone glyph probe has no preceding terminal cells.
        cell_column: 0,
        // The probe’s single font needs no source namespace or variation instance.
        key: GlyphKey {
            face: 0,
            glyph_id: glyph,
            instance: 0,
            hint_ppem: 0,
            hint_mode: HintMode::None,
        },
        // Padding leaves a visible guard region around every edge of the row.
        origin_px: [16, 16 + metrics.baseline_px],
        baseline_px: metrics.baseline_px as f32,
        size_px: metrics.device_size,
        advance_px: 2 * metrics.width_px,
        ink_limit_px: 2 * metrics.width_px,
        cell_height_px: metrics.height_px,
        cell_span: 2,
        // Green tint must not replace the crab’s red authored palette.
        color: 0xff00ff00,
        effect: false,
        color_glyph: true,
    };
    let (handle, mut atlas, strike) = prepare(data, &placement, metrics.baseline_px as f32, dpr)?;
    let mut frame = frame(&placement, &mut atlas, handle);
    // An offscreen RGBA target captures the production raster pipeline’s pixels.
    let mut executor = FrameRenderer::new(renderer, wgpu::TextureFormat::Rgba8Unorm);
    let (image, _) = executor
        .render_to_image(renderer, &mut frame)
        .map_err(|e| e.to_string())?;
    // Inspect submitted pixels, rather than assuming the CPU bitmap was uploaded.
    let ink = guard(&image, placement.ink_limit_px, placement.cell_height_px)?;
    // Diagnostic images stay outside the cross-machine bundled-font goldens.
    // Cargo tests run from the crate directory; keep all images in the root’s ignored target.
    let output = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/m11-bitmap");
    std::fs::create_dir_all(&output).map_err(|e| e.to_string())?;
    let path = output.join(format!("crab-{}x.png", dpr as u32));
    crate::png::write(&path, &image)?;
    println!(
        "bitmap dpr={dpr} strike={strike} ink={ink} atlas_bytes={} path={}",
        atlas.allocated_bytes(),
        path.display()
    );
    Ok(())
}

/// Font bytes are borrowed and decoded samples are owned until atlas insertion.
/// RasterKey includes device size and DPR, while origins stay outside that key.
/// The returned atlas retains the samples as the CPU restoration source.
/// Strike ppem is reported as evidence of the actual selection policy.
/// Separates cold font/PNG preparation from GPU submission and readback.
fn prepare(
    data: &FontData,
    placement: &GlyphPlacement,
    baseline: f32,
    dpr: f32,
) -> Result<(puzzle_term::text::raster::RasterHandle, RasterAtlas, f32), String> {
    // Strike selection is based on the snapped primary device ppem.
    let source = bitmap::load(
        data,
        placement.key.glyph_id,
        placement.size_px,
        &mut Decoder,
    )?
    .ok_or("crab bitmap absent")?;
    let strike = source.ppem;
    // Area integration happens before atlas upload, so texel-exact sampling stays sharp.
    let bitmap = bitmap::fitted(source, placement, baseline)?;
    // One color page suffices; unused mask pages remain unallocated.
    let mut atlas = RasterAtlas::new(1, 1);
    // Size and DPR identify this prepared bitmap independently of future placements.
    let handle = atlas.prepare(
        RasterKey {
            fit: [
                placement.ink_limit_px,
                placement.cell_height_px,
                placement.baseline_px.to_bits(),
            ],
            glyph: placement.key,
            size: placement.size_px.to_bits(),
            dpr: dpr.to_bits(),
        },
        bitmap,
    )?;
    Ok((handle, atlas, strike))
}

/// Upload rectangles include their gutters; the draw samples only interior ink.
/// Baseline-relative bitmap offsets convert to target coordinates exactly once.
/// No scissor clips the emoji here, so misplaced samples cannot hide from guard().
/// Authored-color flags bypass tint even though the instance contains green RGB.
/// Emits the same upload and 40-byte raster instance used by live sessions.
fn frame(
    p: &GlyphPlacement,
    atlas: &mut RasterAtlas,
    h: puzzle_term::text::raster::RasterHandle,
) -> RenderFrame {
    // The handle rectangle excludes the transparent shelf border.
    let r = h.rect;
    // The clear target includes a 16-pixel guard strip on all sides.
    let mut frame = RenderFrame {
        target_size: [p.ink_limit_px + 32, p.cell_height_px + 32],
        clear: [0.0, 0.0, 0.0, 1.0],
        ..RenderFrame::default()
    };
    // Transfer every dirty rectangle with its page address before drawing.
    for u in atlas.take_dirty() {
        frame.uploads.push(Upload::Color {
            page: u.page,
            rect: RectU32 {
                x: u.rect.x,
                y: u.rect.y,
                width: u.rect.width,
                height: u.rect.height,
            },
            pixels: u.pixels,
        });
    }
    frame.rasters.push(RasterInstance {
        // Add baseline-relative bearings only when producing the final draw rectangle.
        rect: [
            (p.origin_px[0] + h.offset[0]) as f32,
            (p.origin_px[1] + h.offset[1]) as f32,
            r.width as f32,
            r.height as f32,
        ],
        // Normalize atlas coordinates; the GPU uses nearest sampling at this exact size.
        uv: [
            r.x as f32 / PAGE as f32,
            r.y as f32 / PAGE as f32,
            (r.x + r.width) as f32 / PAGE as f32,
            (r.y + r.height) as f32 / PAGE as f32,
        ],
        tint: p.color,
        // Bit zero selects authored RGBA; upper page bits are zero for this first page.
        flags: 1,
    });
    // A single raster batch draws the whole emoji; no glyph-layer draw calls exist here.
    frame.batches.push(Batch {
        slot: ZSlot::TextRaster,
        pipeline: PipelineKind::Raster,
        instances: 0..1,
        scissor: None,
    });
    frame
}

/// The target has a black clear color and an opaque alpha channel.
/// Comparing RGB therefore counts visible composited samples, including edges.
/// This check is intentionally independent of any particular emoji artwork.
/// It never establishes visual quality, which remains a human review item.
/// Every authored-color sample must stay inside precisely the assigned two cells.
fn guard(image: &puzzle_render::RgbaImage, width: u32, height: u32) -> Result<usize, String> {
    // Readback is row-major RGBA on black, making visible nonblack samples unambiguous.
    let mut ink = 0;
    let mut authored = false;
    for (i, p) in image.pixels.chunks_exact(4).enumerate() {
        if p[..3] == [0, 0, 0] {
            continue;
        }
        // Convert the linear sample index into target device coordinates.
        let (x, y) = (i as u32 % image.width, i as u32 / image.width);
        if x < 16 || x >= 16 + width || y < 16 || y >= 16 + height {
            return Err(format!("emoji ink outside two cells at {x},{y}"));
        }
        ink += 1;
        // At least one red or blue sample distinguishes authored color from green tint.
        authored |= p[0] > p[1] || p[2] > p[1];
    }
    // Blank output and accidental monochrome tint fail independently of visual judgment.
    if ink < 8 || !authored {
        return Err("installed emoji is blank or lost its palette".into());
    }
    Ok(ink)
}
