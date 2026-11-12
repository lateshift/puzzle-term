//! Cold raster batches retain resource owners while timing only raster work.
//!
//! Each batch receives preconverted outlines before its clock starts. Fresh keys
//! force coverage work without repeating source discovery or shape conversion.
//! CPU evaluation creates owned sample bytes through the Slug oracle; GPU work
//! packs scratch geometry, renders one R8 sheet and copies its crops into the
//! actual sampled atlas. The final pending sheet is flushed before enqueue time.
//! A device poll observes completion for this measurement only, without mapping
//! any pixel data. Production queue ordering requires no such CPU-side fence.
//! GPU crop leases keep source sheets alive until their atlas transfers succeed.
//! Successful upload writing acknowledges those sources and drops their markers.
//! The atlas retains exactly the normal eight-mask/one-color native allowance.
//! This glyph cohort fits that allowance, so page-pressure capture operations do
//! not enter this isolated per-glyph probe; separate capacity tests cover them.
//! Repeating a batch retains pipelines and instance buffers but changes every key.
//! The first sample therefore includes lazy shader compilation while later ones
//! exercise the same resource owner after startup. Every sample remains recorded.
//! Per-glyph cost divides batch time by 128; device-area normalization uses the
//! actual tight crop, excluding atlas shelf padding. Curves are counted upstream.
//! Isolated CPU and GPU owners cannot share a cache hit or another sample's key.
use super::*;

/// Cold batches reuse GPU/oracle owners but preconvert every outline before the sample clock.
pub(super) fn samples(
    renderer: &Renderer,
    p: &puzzle_term::text::glyph::GlyphPlacement,
    path: &puzzle_term::text::geometry::BezPath,
    gpu: bool,
) -> Result<(Vec<f64>, u32, Vec<f64>), String> {
    let mut atlas = RasterAtlas::new(8, 1);
    // Match the native mask/color partition without enlarging its 48-MiB allowance.
    atlas.set_synchronous();
    if gpu {
        // The CPU branch intentionally has no GPU adapter or coverage texture.
        atlas.set_grid_rasterizer(Box::new(crate::grid_rasterizer::producer(renderer)))?;
    }
    let mut oracle = OracleRasterizer::default(); // Reuse the isolated CPU packing scratch.
    let mut executor = puzzle_render::FrameRenderer::new(renderer, wgpu::TextureFormat::Rgba8Unorm);
    let mut times = Vec::new();
    // Parallel chronological arrays retain enqueue and completion observations for each batch.
    let mut queued = Vec::new();
    let mut pixels = 0; // Every repetition of this placement has the same tight crop area.
    for sample in 0..21 {
        atlas.begin_frame();
        // Geometry preparation is the separately budgeted curve stage, never raster time.
        let mut outlines = Vec::new();
        for _ in 0..128 {
            // Allocate the converted inputs before timing, just as upstream preparation does.
            outlines.push(Outline::new(path.clone())?.ok_or("empty outline")?);
        }
        let started = std::time::Instant::now();
        pixels = raster_batch(&mut atlas, &mut oracle, p, outlines, sample, gpu)?;
        atlas.flush_grid()?; // Complete the final scratch batch before measuring its enqueue cost.
        // GPU tickets become sampled-atlas copies before measuring completion; there is no readback.
        if gpu {
            transfer(&mut atlas, &mut executor, renderer)?;
        }
        queued.push(started.elapsed().as_secs_f64() * 1_000_000.0 / 128.0); // CPU enqueue average.
        if gpu {
            renderer
                .device()
                .poll(wgpu::PollType::wait_indefinitely()) // Wait without mapping pixel storage.
                .map_err(|e| e.to_string())?;
        }
        times.push(started.elapsed().as_secs_f64() * 1_000_000.0 / 128.0); // Batch amortization.
    }
    Ok((times, pixels, queued))
}

/// A successful writer owns every GPU crop and releases its temporary source ticket.
fn transfer(
    atlas: &mut RasterAtlas,
    executor: &mut puzzle_render::FrameRenderer,
    renderer: &Renderer,
) -> Result<(), String> {
    use puzzle_term::frame::{RectU32, RenderFrame, Upload};
    let mut frame = RenderFrame::default();
    for upload in atlas.take_dirty() {
        // This isolated probe has no authored images or page pressure operations.
        let (ticket, source, lease) = upload.gpu.ok_or("benchmark expected GPU crop")?;
        let r = upload.rect;
        frame.uploads.push(Upload::GpuMask {
            ticket,
            lease, // Upload ownership keeps the GPU staging sheet live until its copy succeeds.
            source,
            page: upload.page,
            rect: RectU32 {
                // Crops are physical texels and retain the production atlas's destination UVs.
                x: r.x,
                y: r.y,
                width: r.width,
                height: r.height,
            },
        });
    }
    executor
        .write_frame_uploads(renderer, &mut frame)
        .map_err(|e| e.to_string())
}

/// Rasterizes fresh identities with the same geometry through either complete producer.
fn raster_batch(
    atlas: &mut RasterAtlas,
    oracle: &mut OracleRasterizer,
    placement: &puzzle_term::text::glyph::GlyphPlacement,
    outlines: Vec<Outline>,
    sample: u32,
    gpu: bool,
) -> Result<u32, String> {
    let mut pixels = 0;
    for (glyph, ink) in outlines.into_iter().enumerate() {
        // Fresh IDs force real raster work while preserving the same physical geometry.
        let mut p = *placement;
        p.key.glyph_id = sample * 128 + glyph as u32; // No sample can hit an earlier key.
        if gpu {
            let key = RasterKey {
                // Each key includes the exact device size and baseline-relative grid fitting.
                glyph: p.key,
                size: p.size_px.to_bits(), // Avoid integer rounding of fitted font geometry.
                fit: [p.ink_limit_px, p.cell_height_px, p.baseline_px.to_bits()],
                dpr: 1_f32.to_bits(), // Each probe atlas is isolated; placement already has DPR.
            };
            let h = atlas.prepare_grid(key, &p, ink, [true, false, false])?;
            pixels = h.rect.width * h.rect.height; // Actual GPU coverage crop, not cell area.
        } else {
            let ink = oracle.grid(&p, ink, [true, false, false])?;
            pixels = ink.width * ink.height; // Normalize by generated samples, not cell bounds.
            std::hint::black_box(ink); // Owned CPU output remains observable to optimization.
        }
    }
    Ok(pixels)
}
