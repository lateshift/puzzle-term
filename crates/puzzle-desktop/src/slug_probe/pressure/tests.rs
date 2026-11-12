//! D-39 page eviction preserves live GPU addresses and invalidates reused ranges.
#![allow(
    clippy::expect_used,
    reason = "finite GPU pressure fixture must prepare"
)]
use crate::block_on::block_on;
use puzzle_render::{FrameRenderer, Renderer};
use puzzle_term::{
    frame::{Batch, PipelineKind, RenderFrame, SlugInstance, Upload, ZSlot},
    slug::{
        ShapeHandle, ShapeKey, SlugAtlas,
        atlas::{FillRule, TEXTURE_WIDTH},
    },
};

#[path = "packed/tests.rs"]
/// Startup packs must keep every drawable scalar alive before font takeover.
mod packed;
#[path = "raster/tests.rs"]
/// Exact GPU pixels before and after several within-frame raster flushes.
mod raster;

/// Draws unchanged ink after eviction, then verifies new ink in the reused range.
#[test]
fn page_eviction_preserves_addresses_and_draws_reused_range() {
    let _gpu = crate::gpu_test::lock();
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let renderer = block_on(Renderer::new(&instance, None)).expect("native GPU fixture");
    let mut executor = FrameRenderer::new(&renderer, wgpu::TextureFormat::Rgba8Unorm);
    let (mut atlas, key, old) = filled();
    let mut frame = draw(old);
    uploads(&mut atlas, &mut frame);
    let (before, _) = executor
        .render_to_image(&renderer, &mut frame)
        .expect("initial GPU ink");
    // Only the last rectangle is drawn in frame two; older filler is cold.
    atlas.mark_used(key, 2);
    assert!(atlas.begin_frame(3).expect("between-frame page eviction"));
    assert!(
        atlas.contains_handle(old),
        "uncleared page must keep its handle"
    );
    let retained = atlas.get(key).expect("current shape retained");
    assert_eq!(old, retained, "eviction moved surviving ink");
    frame = draw(retained);
    uploads(&mut atlas, &mut frame);
    assert!(
        frame.uploads.is_empty(),
        "eviction reuploaded surviving texels"
    );
    let (after, _) = executor
        .render_to_image(&renderer, &mut frame)
        .expect("retained GPU ink");
    assert_eq!(
        before.pixels, after.pixels,
        "eviction changed surviving pixels"
    );
    let center = ((16 * after.width + 16) * 4) as usize;
    assert_eq!(
        &after.pixels[center..center + 3],
        &[255; 3],
        "real interior ink required"
    );
    assert_eq!(&after.pixels[..3], &[0; 3], "outside remains clear");
    // Reusing the cleared page must upload fresh coverage rather than sample
    // its previous square. The narrower rectangle leaves x=20 clear.
    let narrow = puzzle_term::slug::synthetic::rectangle(0.0, 0.0, 0.5, 1.0);
    let replacement = atlas
        .prepare(ShapeKey(key.0 + 1), &narrow, FillRule::NonZero, 3)
        .expect("reused page");
    assert_eq!(replacement.glyph_loc, [0, 0]);
    frame = draw(replacement);
    uploads(&mut atlas, &mut frame);
    let (new, _) = executor
        .render_to_image(&renderer, &mut frame)
        .expect("fresh page ink");
    let pixel = |x| &new.pixels[((16 * new.width + x) * 4) as usize..][..3];
    assert_eq!(pixel(12), &[255; 3], "replacement interior missing");
    assert_eq!(pixel(20), &[0; 3], "cleared page sampled stale square");
}

/// Saturates a small existing budget without inventing an oversized outline.
fn filled() -> (SlugAtlas, ShapeKey, ShapeHandle) {
    let mut atlas = SlugAtlas::with_budget(10 * 1024 * 1024).expect("minimum pressure budget");
    let path = puzzle_term::slug::synthetic::rectangle(0.0, 0.0, 1.0, 1.0);
    let mut key = ShapeKey(1);
    let mut last = atlas
        .prepare(key, &path, FillRule::NonZero, 1)
        .expect("first rectangle");
    while let Ok(handle) = atlas.prepare(ShapeKey(key.0 + 1), &path, FillRule::NonZero, 1) {
        key.0 += 1;
        last = handle;
    }
    (atlas, key, last)
}

/// A 16-pixel square has independently known opaque interior and blank exterior.
fn draw(handle: ShapeHandle) -> RenderFrame {
    let mut frame = RenderFrame {
        target_size: [32, 32],
        ..RenderFrame::default()
    };
    frame.slugs.push(SlugInstance {
        m: [16.0, 0.0, 0.0, 16.0],
        t: [8.0, 8.0],
        bbox: handle.bbox,
        glyph_loc: puzzle_term::slug::encoding::encode_glyph_loc(handle.glyph_loc),
        band_info: handle.band_info,
        color: 0xffffffff,
        paint: 0,
    });
    frame.batches.push(Batch {
        slot: ZSlot::Text,
        pipeline: PipelineKind::Slug,
        instances: 0..1,
        scissor: None,
    });
    frame
}

/// Dirty mirrors contain only newly written spans; page eviction adds no uploads.
fn uploads(atlas: &mut SlugAtlas, frame: &mut RenderFrame) {
    let (curves, bands) = atlas.take_dirty();
    for span in curves {
        let start = usize::from(span.row) * TEXTURE_WIDTH + usize::from(span.start);
        let end = usize::from(span.row) * TEXTURE_WIDTH + usize::from(span.end);
        frame.uploads.push(Upload::Curves {
            origin: [u32::from(span.start), u32::from(span.row)],
            texels: atlas.curve_texels()[start..end].to_vec(),
        });
    }
    for span in bands {
        let start = usize::from(span.row) * TEXTURE_WIDTH + usize::from(span.start);
        let end = usize::from(span.row) * TEXTURE_WIDTH + usize::from(span.end);
        frame.uploads.push(Upload::Bands {
            origin: [u32::from(span.start), u32::from(span.row)],
            texels: atlas.band_texels()[start..end].to_vec(),
        });
    }
}
