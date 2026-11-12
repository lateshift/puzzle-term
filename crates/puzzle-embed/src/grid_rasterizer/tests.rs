//! D-47 guards real device-size coverage, warm reuse and mirror restoration.
#![allow(
    clippy::unwrap_used,
    reason = "native GPU fixtures must fail on any setup error"
)]
use super::*;
use puzzle_term::{frame::RenderFrame, frame_builder::FrameBuilder, vt::terminal::Terminal};
/// Configured GPU bytes and context reload keep the same atlas ownership.
mod budget;
/// Actual device replacement must rebuild GPU masks and retain image-only sources.
mod recovery;

/// Latin, CJK and image-only emoji keep the vector scene's pixels at both physical scales.
#[test]
fn grid_bitmap_matches_vectors_at_both_dprs_and_restores() {
    let _gpu = crate::gpu_test::lock();
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let renderer = crate::block_on::block_on(Renderer::new(&instance, None)).unwrap();
    for dpr in [1.0, 2.0] {
        let (expected, _) = specimen(&renderer, dpr, "slug");
        for choice in ["auto", "oracle"] {
            let (actual, rasters) = specimen(&renderer, dpr, choice);
            assert!(rasters > 20, "fixture must actually choose bitmap grid ink");
            assert_eq!(actual.pixels.len(), expected.pixels.len());
            let excess = actual
                .pixels
                .chunks_exact(4)
                .zip(expected.pixels.chunks_exact(4))
                .filter(|(a, b)| a.iter().zip(*b).any(|(a, b)| a.abs_diff(*b) > 2))
                .count();
            let allowed = (actual.width as usize * actual.height as usize) / 1000;
            assert!(
                excess <= allowed,
                "DPR {dpr}: {excess} pixels beyond ±2 (allowed {allowed})"
            );
            let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/m16-4");
            std::fs::create_dir_all(&out).unwrap();
            crate::png::write(&out.join(format!("grid-{dpr}-{choice}.png")), &actual).unwrap();
            crate::png::write(&out.join(format!("grid-{dpr}-vector.png")), &expected).unwrap();
        }
    }
    renderer.device().destroy();
}

/// Fresh contexts prevent the comparison from accidentally sharing prepared vector handles.
fn specimen(renderer: &Renderer, dpr: f32, choice: &str) -> (puzzle_render::RgbaImage, usize) {
    let mut context =
        TextContext::new(crate::font_data::bundled_font_set().unwrap(), 14.0, dpr).unwrap();
    crate::system_fonts::install(&mut context, &[]).unwrap();
    if choice == "oracle" {
        context.set_grid_renderer(puzzle_term::text::glyph::RendererChoice::Auto);
    } else {
        install(&mut context, renderer, choice).unwrap();
    }
    let mut builder = FrameBuilder::new(&context);
    let mut vt = Terminal::new(80, 6, 0, 0);
    vt.advance(b"\x1b[?25l");
    vt.advance("Hamburgefonstiv 0Oo1lI {}[] -> => != ==\r\n\x1b[32m漢字かな한글中文測試\r\n\x1b[1mBold Latin 漢字 한글\r\n\x1b[3mItalic Latin 漢字\r\n\x1b[0m🦀 ✌️ 👨‍👩‍👧‍👦 🇩🇪".as_bytes());
    let mut frame = RenderFrame::default();
    builder.build(&mut context, &vt, &mut frame).unwrap();
    let rasters = frame.rasters.len();
    let mut executor = puzzle_render::FrameRenderer::new(renderer, wgpu::TextureFormat::Rgba8Unorm);
    let (image, _) = executor.render_to_image(renderer, &mut frame).unwrap();
    let count = context.prepared_glyphs();
    // The forced-style path exercises both row banks before measuring steady-state rebuilds.
    builder.set_context(&context);
    builder.build(&mut context, &vt, &mut frame).unwrap();
    builder.set_context(&context);
    // Warm dirty rows must bypass rasterization and source/curve allocation entirely.
    let warm =
        allocation_counter::measure(|| builder.build(&mut context, &vt, &mut frame).unwrap());
    assert_eq!(
        warm.count_total, 0,
        "warm {choice} DPR {dpr} frame allocated {} bytes",
        warm.bytes_total
    );
    assert_eq!(context.prepared_glyphs(), count);
    let (second, _) = executor.render_to_image(renderer, &mut frame).unwrap();
    assert_eq!(second.pixels, image.pixels, "warm frame changed coverage");
    context.reset_atlas();
    builder.build(&mut context, &vt, &mut frame).unwrap();
    let mut fresh = puzzle_render::FrameRenderer::new(renderer, wgpu::TextureFormat::Rgba8Unorm);
    let (restored, _) = fresh.render_to_image(renderer, &mut frame).unwrap();
    assert_eq!(
        restored.pixels, image.pixels,
        "GPU masks were not re-created after cache invalidation"
    );
    (image, rasters)
}

/// GPU placeholders must be completed before the same frame retains and reuses their page.
#[test]
fn gpu_grid_flushes_preserve_exact_pixels_before_and_after_reuse() {
    let _gpu = crate::gpu_test::lock();
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let renderer = crate::block_on::block_on(Renderer::new(&instance, None)).unwrap();
    let mut atlas = minimum_gpu_atlas(&renderer);
    let mut frame = RenderFrame {
        target_size: [192, 32],
        clear: [0.0, 0.0, 0.0, 1.0],
        ..Default::default()
    };
    let mut expected = Vec::new();
    let mut first = None;
    for i in 0..5 {
        let p = oversized_placement(i);
        let key = puzzle_term::text::raster::RasterKey {
            glyph: p.key,
            size: p.size_px.to_bits(),
            fit: [0; 3],
            dpr: 1_f32.to_bits(),
        };
        let ink = hole_outline(i);
        let handle = atlas.prepare_grid(key, &p, ink, [false; 3]).unwrap();
        if i == 0 {
            first = Some(handle);
            add_authored_mask(&mut atlas, key);
        }
        frame.rasters.push(mask_draw(handle, i));
        expected.push(isolated_mask(&renderer, key, &p, hole_outline(i)));
    }
    frame.rasters.push(mask_draw(first.unwrap(), 5));
    mask_uploads(&mut atlas, &mut frame);
    assert_eq!(
        atlas.evictions(),
        4,
        "one physical page must force four in-build reuses"
    );
    assert_eq!(atlas.allocated_bytes(), 4 * 1024 * 1024);
    assert_eq!(
        atlas.cpu_mirror_bytes(),
        0,
        "GPU masks retained CPU source pixels"
    );
    let mut executor =
        puzzle_render::FrameRenderer::new(&renderer, wgpu::TextureFormat::Rgba8Unorm);
    executor.set_glyph_budget(&renderer, puzzle_term::text::raster::MIN_BUDGET_BYTES);
    for _ in 0..2 {
        let (image, draws) = executor.render_to_image(&renderer, &mut frame).unwrap();
        assert_eq!(draws, 6);
        assert_mask_columns(&image, &expected);
    }
}

/// Installs GPU preparation under the derived complete-frame floor.
fn minimum_gpu_atlas(renderer: &Renderer) -> puzzle_term::text::raster::RasterAtlas {
    let mut atlas = puzzle_term::text::raster::RasterAtlas::with_budget(
        puzzle_term::text::raster::MIN_BUDGET_BYTES,
        true,
    );
    atlas.begin_frame();
    atlas
        .set_grid_rasterizer(Box::new(Gpu {
            renderer: renderer.clone(),
            baker: None,
            instances: Vec::new(),
        }))
        .unwrap();
    atlas
}

/// Different holes ensure stale page contents cannot masquerade as the same white ink.
fn hole_outline(index: u32) -> puzzle_term::text::preparation::Outline {
    let mut p = puzzle_term::slug::synthetic::rectangle(0.0, 0.0, 1.0, 1.0);
    let x = 0.1 + f64::from(index) * 0.13;
    p.move_to((x, 0.2));
    p.line_to((x, 0.8));
    p.line_to((x + 0.2, 0.8));
    p.line_to((x + 0.2, 0.2));
    p.close_path();
    puzzle_term::text::preparation::Outline::new(p)
        .unwrap()
        .unwrap()
}

/// A small displayed crop still samples the full page-sized glyph for reuse comparisons.
fn mask_draw(
    h: puzzle_term::text::raster::RasterHandle,
    column: u32,
) -> puzzle_term::frame::RasterInstance {
    let r = h.rect;
    puzzle_term::frame::RasterInstance {
        rect: [column as f32 * 32.0, 0.0, 32.0, 32.0],
        uv: [
            r.x as f32 / 2048.0,
            r.y as f32 / 2048.0,
            (r.x + r.width) as f32 / 2048.0,
            (r.y + r.height) as f32 / 2048.0,
        ],
        tint: 0xffffffff,
        flags: (u32::from(h.epoch) << 16) | (h.page << 8),
    }
}

/// Uses the production neutral upload protocol after completing every pending GPU crop.
fn mask_uploads(atlas: &mut puzzle_term::text::raster::RasterAtlas, frame: &mut RenderFrame) {
    use puzzle_term::frame::{Batch, PipelineKind, RectU32, Upload, ZSlot};
    atlas.flush_grid().unwrap();
    frame.uploads.push(Upload::RasterReset);
    for u in atlas.take_dirty() {
        let rect = RectU32 {
            x: u.rect.x,
            y: u.rect.y,
            width: u.rect.width,
            height: u.rect.height,
        };
        frame.uploads.push(if let Some(epoch) = u.capture {
            Upload::RasterCapture {
                color: false,
                page: u.page,
                epoch,
            }
        } else if let Some((ticket, source, lease)) = u.gpu {
            Upload::GpuMask {
                ticket,
                lease,
                source,
                page: u.page,
                rect,
            }
        } else if u.pixels.is_empty() {
            Upload::RasterClear { page: u.page }
        } else {
            Upload::Mask {
                page: u.page,
                rect,
                pixels: u.pixels,
            }
        });
    }
    for v in atlas.take_versions() {
        frame.uploads.push(if v.pixels.is_empty() {
            Upload::RasterCapture {
                color: false,
                page: v.page,
                epoch: v.epoch,
            }
        } else {
            Upload::RasterPage {
                color: false,
                page: v.page,
                epoch: v.epoch,
                pixels: v.pixels,
            }
        });
    }
    frame.batches.push(Batch {
        slot: ZSlot::TextRaster,
        pipeline: PipelineKind::Raster,
        instances: 0..frame.rasters.len() as u32,
        scissor: None,
    });
}

/// Large source geometry forces every successive glyph to require another physical incarnation.
fn oversized_placement(index: u32) -> puzzle_term::text::glyph::GlyphPlacement {
    use puzzle_term::text::glyph::{GlyphPlacement, HintMode};
    GlyphPlacement {
        key: puzzle_term::text::glyph::GlyphKey {
            face: 0,
            glyph_id: index,
            instance: 0,
            hint_ppem: 0,
            hint_mode: HintMode::None,
        },
        cell_column: 0,
        origin_px: [0, 0],
        baseline_px: 2042.0,
        size_px: 2042.0,
        advance_px: 2042,
        ink_limit_px: 2042,
        cell_height_px: 2042,
        cell_span: 1,
        color: 0xffffffff,
        effect: false,
        color_glyph: false,
    }
}
/// A separate one-glyph image establishes exact expected pixels without page reuse.
fn isolated_mask(
    renderer: &Renderer,
    key: puzzle_term::text::raster::RasterKey,
    p: &puzzle_term::text::glyph::GlyphPlacement,
    ink: puzzle_term::text::preparation::Outline,
) -> puzzle_render::RgbaImage {
    let mut atlas = puzzle_term::text::raster::RasterAtlas::new(1, 1);
    atlas.set_synchronous();
    atlas.begin_frame();
    atlas
        .set_grid_rasterizer(Box::new(Gpu {
            renderer: renderer.clone(),
            baker: None,
            instances: Vec::new(),
        }))
        .unwrap();
    let h = atlas.prepare_grid(key, p, ink, [false; 3]).unwrap();
    let mut frame = RenderFrame {
        target_size: [32, 32],
        clear: [0.0, 0.0, 0.0, 1.0],
        ..Default::default()
    };
    frame.rasters.push(mask_draw(h, 0));
    mask_uploads(&mut atlas, &mut frame);
    let mut executor = puzzle_render::FrameRenderer::new(renderer, wgpu::TextureFormat::Rgba8Unorm);
    executor.render_to_image(renderer, &mut frame).unwrap().0
}

/// Exact crops compare every incarnation, including the cursor-like late draw of the first.
fn assert_mask_columns(image: &puzzle_render::RgbaImage, expected: &[puzzle_render::RgbaImage]) {
    for y in 0..32 {
        for column in 0..6 {
            let wanted = &expected[if column == 5 { 0 } else { column }];
            let from = (y * wanted.width * 4) as usize;
            let to = ((y * image.width + column as u32 * 32) * 4) as usize;
            assert_eq!(
                &image.pixels[to..to + 128],
                &wanted.pixels[from..from + 128],
                "row {y}, glyph {column}"
            );
        }
    }
}

/// An image-only mask makes the first page mixed; generated coverage must still survive its reuse.
fn add_authored_mask(
    atlas: &mut puzzle_term::text::raster::RasterAtlas,
    key: puzzle_term::text::raster::RasterKey,
) {
    assert_eq!(atlas.cpu_mirror_bytes(), 0);
    // A tiny image-only mask shares the first shelf: its mirror cannot hold GPU ink.
    let authored = puzzle_term::text::glyph::RasterGlyph {
        kind: puzzle_term::text::glyph::BitmapKind::Mask,
        width: 1,
        height: 1,
        pixels: vec![255],
        offset_px: [0; 2],
    };
    let mut authored_key = key;
    authored_key.glyph.glyph_id = 777;
    atlas.prepare(authored_key, authored).unwrap();
    assert_eq!(atlas.cpu_mirror_bytes(), 4 * 1024 * 1024);
}
