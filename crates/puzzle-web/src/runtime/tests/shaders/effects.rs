//! Rotated bitmap edges must filter premultiplied taps, including the cleared gutter.
use super::*;
/// An independent bilinear oracle detects dark fringes and transparent RGB leakage.
#[wasm_bindgen_test(async)]
async fn rotated_bitmap_filters_premultiplied_border() {
    let gpu = super::super::super::resources::gpu();
    super::super::super::resources::wait_gpu(&gpu)
        .await
        .unwrap();
    let canvas = canvas();
    window()
        .unwrap()
        .document()
        .unwrap()
        .body()
        .unwrap()
        .append_child(&canvas)
        .unwrap();
    let surface = gpu
        .instance
        .create_surface(wgpu::SurfaceTarget::Canvas(canvas.clone()))
        .unwrap();
    let renderer = gpu.renderer.borrow();
    let renderer = renderer.as_ref().unwrap();
    renderer.configure(&surface, 32, 16).unwrap();
    let format = renderer.surface_format(&surface, 32, 16).unwrap();
    let mut painter = puzzle_render::FrameRenderer::new(renderer, format);
    let map = puzzle_term::text::geometry::Affine::rotate(0.4)
        * puzzle_term::text::geometry::Affine::scale(3.0);
    let mut frame = specimen(map);
    painter
        .render_surface(renderer, &mut frame, &surface)
        .unwrap();
    let actual = pixels(&canvas);
    oracle(&actual, map);
    canvas.remove();
}
/// Computes coverage independently of GPU UVs, paint rows and instance emission.
fn oracle(actual: &[u8], map: puzzle_term::text::geometry::Affine) {
    let inverse = map.inverse();
    let mut partial = 0;
    for y in 0..16 {
        for x in 0..32 {
            let p = inverse
                * puzzle_term::text::geometry::Point::new(
                    x as f64 + 0.5 - 10.0,
                    y as f64 + 0.5 - 3.0,
                );
            // Only the center source texel is opaque blue; all other taps have zero alpha.
            let coverage = (1.0 - (p.x - 1.5).abs()).max(0.0) * (1.0 - (p.y - 1.5).abs()).max(0.0);
            let expected = (255.0 * coverage).round() as u8;
            let at = (y * 32 + x) * 4;
            assert!(
                actual[at] <= 2 && actual[at + 1] <= 2,
                "transparent red leaked at {x},{y}"
            );
            assert!(
                actual[at + 2].abs_diff(expected) <= 2,
                "premultiplied edge {x},{y}: {} versus {expected}",
                actual[at + 2]
            );
            partial += usize::from(expected > 2 && expected < 253);
        }
    }
    assert!(
        partial > 10,
        "rotation did not exercise fractional edge samples"
    );
}

/// Transparent red deliberately poisons straight-alpha interpolation without premultiplication.
fn specimen(map: puzzle_term::text::geometry::Affine) -> RenderFrame {
    let c = map.as_coeffs();
    let mut backend = puzzle_term::text::glyph::slug::SlugBackend::default();
    let index = backend
        .prepare_raster_effect(
            [c[0] as f32, c[1] as f32, c[2] as f32, c[3] as f32],
            0xffffffff,
        )
        .unwrap();
    let mut source = [255, 0, 0, 0].repeat(9);
    source[16..20].copy_from_slice(&[0, 0, 255, 255]);
    let mut uploads = vec![Upload::Color {
        page: 0,
        rect: RectU32 {
            x: 2,
            y: 2,
            width: 3,
            height: 3,
        },
        pixels: source,
    }];
    uploads.extend(
        backend
            .take_paint_uploads()
            .into_iter()
            .map(|(first, colors)| Upload::Paint { first, colors }),
    );
    RenderFrame {
        target_size: [32, 16],
        clear: [0.0, 0.0, 0.0, 1.0],
        uploads,
        rasters: vec![RasterInstance {
            rect: [10.0, 3.0, 3.0, 3.0],
            uv: [2.0 / 2048.0, 2.0 / 2048.0, 5.0 / 2048.0, 5.0 / 2048.0],
            tint: index,
            flags: 3,
        }],
        batches: vec![Batch {
            slot: ZSlot::TextRaster,
            pipeline: PipelineKind::Raster,
            instances: 0..1,
            scissor: None,
        }],
        ..Default::default()
    }
}
