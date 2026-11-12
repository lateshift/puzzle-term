//! A one-page-per-format budget must preserve every glyph across repeated flushes.
#![allow(
    clippy::expect_used,
    reason = "valid page fixtures and native GPU must execute"
)]
use crate::block_on::block_on;
use puzzle_render::{FrameRenderer, Renderer};
use puzzle_term::{
    frame::{Batch, PipelineKind, RasterInstance, RectU32, RenderFrame, Upload, ZSlot},
    text::{
        glyph::{BitmapKind, GlyphKey, HintMode, RasterGlyph},
        raster::{PAGE, RasterAtlas, RasterHandle, RasterKey},
    },
};

/// Several mask/color reuses in one image, a later old-ink redraw, and a warm next frame.
#[test]
fn within_frame_flushes_preserve_exact_pixels_and_final_residency() {
    let _gpu = crate::gpu_test::lock();
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let renderer = block_on(Renderer::new(&instance, None)).expect("native GPU");
    let mut executor = FrameRenderer::new(&renderer, wgpu::TextureFormat::Rgba8Unorm);
    executor.set_glyph_budget(&renderer, puzzle_term::text::raster::MIN_BUDGET_BYTES);
    let (atlas, mut frame, last) = filled_frame();
    assert_eq!(atlas.allocated_bytes(), 20 * 1024 * 1024);
    assert_eq!(
        frame
            .uploads
            .iter()
            .filter(|u| matches!(u, Upload::RasterPage { .. }))
            .count(),
        10
    );
    for _ in 0..2 {
        // The second render has no uploads; retained diagnostic/calibration ink still works.
        let (image, draws) = executor
            .render_to_image(&renderer, &mut frame)
            .expect("streamed GPU image");
        assert_eq!(draws, 6, "mixed compatible layers should share a segment");
        assert_pixels(&image, &[0, 1, 2, 3, 4, 0]);
    }
    // The final old-ink draw must not leave physical layers on the old incarnation.
    frame.clear_instances();
    // Background-only diagnostics keep staging but must still clear an empty pass.
    frame.clear = [0.2, 0.4, 0.6, 1.0];
    let (paper, _) = executor
        .render_to_image(&renderer, &mut frame)
        .expect("empty staged frame");
    assert!(
        paper
            .pixels
            .chunks_exact(4)
            .all(|pixel| pixel == [51, 102, 153, 255])
    );
    frame.clear = [0.0, 0.0, 0.0, 1.0];
    frame
        .rasters
        .extend(last.iter().map(|handle| draw(*handle, 0)));
    frame.batches.push(batch(0..2));
    frame.uploads.push(Upload::RasterReset);
    let (image, draws) = executor
        .render_to_image(&renderer, &mut frame)
        .expect("warm final pages");
    assert_eq!(draws, 1);
    assert_pixels(&image, &[4]);
}

/// Fills both physical formats and records a later reference to their first contents.
fn filled_frame() -> (RasterAtlas, RenderFrame, Vec<RasterHandle>) {
    let mut atlas = RasterAtlas::with_budget(puzzle_term::text::raster::MIN_BUDGET_BYTES, true);
    atlas.set_synchronous();
    atlas.begin_frame();
    let mut frame = RenderFrame {
        target_size: [96, 32],
        clear: [0.0, 0.0, 0.0, 1.0],
        ..RenderFrame::default()
    };
    let mut first = Vec::new();
    let mut last = Vec::new();
    for index in 0..5 {
        for kind in [BitmapKind::Mask, BitmapKind::Color] {
            let handle = atlas
                .prepare(key(index, kind), full(index, kind))
                .expect("capacity cannot abort");
            frame.rasters.push(draw(handle, index));
            if index == 0 {
                first.push(handle);
            }
            if index == 4 {
                last.push(handle);
            }
        }
    }
    // Cursor/selection streams can refer back to a glyph drawn before four flushes.
    for handle in &first {
        frame.rasters.push(draw(*handle, 5));
    }
    frame.batches.push(batch(0..12));
    uploads(&mut atlas, &mut frame);
    (atlas, frame, last)
}

/// Synthetic glyph identities exercise the same raster seam as vector overflow.
fn key(index: u32, kind: BitmapKind) -> RasterKey {
    RasterKey {
        glyph: GlyphKey {
            face: 0,
            glyph_id: index + if kind == BitmapKind::Color { 10 } else { 0 },
            instance: 0,
            hint_ppem: 0,
            hint_mode: HintMode::None,
        },
        size: 24_f32.to_bits(),
        fit: [0; 3],
        dpr: 1_f32.to_bits(),
    }
}

/// One finite sample consumes the whole shelf, so every later glyph forces a flush.
fn full(index: u32, kind: BitmapKind) -> RasterGlyph {
    let pixel = ink(index, kind);
    let pixels = if kind == BitmapKind::Color {
        pixel.repeat(((PAGE - 2) * (PAGE - 2)) as usize)
    } else {
        vec![pixel[0]; ((PAGE - 2) * (PAGE - 2)) as usize]
    };
    RasterGlyph {
        offset_px: [0; 2],
        width: PAGE - 2,
        height: PAGE - 2,
        pixels,
        kind,
    }
}

/// Distinct exact bytes detect accidental use of any preceding or following page image.
fn ink(index: u32, kind: BitmapKind) -> [u8; 4] {
    let value = 40 + index as u8 * 40;
    if kind == BitmapKind::Color {
        [value, 255 - value, 23, 255]
    } else {
        [value, value, value, 255]
    }
}

/// Uniform full-page samples make every output pixel independent of interpolation noise.
fn draw(handle: RasterHandle, column: u32) -> RasterInstance {
    let rect = handle.rect;
    RasterInstance {
        rect: [
            column as f32 * 16.0,
            if handle.kind == BitmapKind::Color {
                16.0
            } else {
                0.0
            },
            16.0,
            16.0,
        ],
        uv: [
            rect.x as f32 / PAGE as f32,
            rect.y as f32 / PAGE as f32,
            (rect.x + rect.width) as f32 / PAGE as f32,
            (rect.y + rect.height) as f32 / PAGE as f32,
        ],
        tint: 0xffff_ffff,
        flags: (u32::from(handle.epoch) << 16)
            | (handle.page << 8)
            | u32::from(handle.kind == BitmapKind::Color),
    }
}

/// One ordinary painter slot lets capacity, rather than authored batches, force splitting.
fn batch(instances: std::ops::Range<u32>) -> Batch {
    Batch {
        slot: ZSlot::TextRaster,
        pipeline: PipelineKind::Raster,
        instances,
        scissor: None,
    }
}

/// The same neutral uploads used by the production frame builder drive this executor.
fn uploads(atlas: &mut RasterAtlas, frame: &mut RenderFrame) {
    frame.uploads.push(Upload::RasterReset);
    for upload in atlas.take_dirty() {
        let r = upload.rect;
        let rect = RectU32 {
            x: r.x,
            y: r.y,
            width: r.width,
            height: r.height,
        };
        frame.uploads.push(if upload.kind == BitmapKind::Color {
            Upload::Color {
                page: upload.page,
                rect,
                pixels: upload.pixels,
            }
        } else {
            Upload::Mask {
                page: upload.page,
                rect,
                pixels: upload.pixels,
            }
        });
    }
    for version in atlas.take_versions() {
        frame.uploads.push(Upload::RasterPage {
            color: version.kind == BitmapKind::Color,
            page: version.page,
            epoch: version.epoch,
            pixels: version.pixels,
        });
    }
}

/// Checks every pixel of every glyph before and after each flush, including final old ink.
fn assert_pixels(image: &puzzle_render::RgbaImage, indices: &[u32]) {
    for y in 0..32 {
        for x in 0..96 {
            let expected = indices
                .get((x / 16) as usize)
                .map_or([0, 0, 0, 255], |index| {
                    ink(
                        *index,
                        if y < 16 {
                            BitmapKind::Mask
                        } else {
                            BitmapKind::Color
                        },
                    )
                });
            let at = ((y * image.width + x) * 4) as usize;
            assert_eq!(&image.pixels[at..at + 4], &expected, "pixel ({x},{y})");
        }
    }
}
