//! WebGPU must validate every production pipeline and draw real raster ink.
//!
//! A synthetic five-by-seven A is uploaded to both atlas formats. Reading the
//! submitted canvas verifies mask tint and RGBA branches, rather than letting
//! a successful Slug-only frame conceal a rejected raster shader (Tint).
use super::*;
use puzzle_term::frame::{
    Batch, PipelineKind, RasterInstance, RectU32, RenderFrame, Upload, ZSlot,
};

/// All pipeline construction and rendering errors belong to this validation scope.
#[wasm_bindgen_test(async)]
async fn pipelines_validate_and_raster_glyph_draws_ink() {
    let gpu = super::super::resources::gpu();
    super::super::resources::wait_gpu(&gpu).await.unwrap();
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
    let scope = renderer
        .device()
        .push_error_scope(wgpu::ErrorFilter::Validation);
    renderer.configure(&surface, 32, 16).unwrap();
    let format = renderer.surface_format(&surface, 32, 16).unwrap();
    // FrameRenderer compiles rect, Slug and raster, not just the branch this scene draws.
    let mut painter = puzzle_render::FrameRenderer::new(renderer, format);
    let validation = scope.pop().await;
    assert!(
        validation.is_none(),
        "a production shader or pipeline failed validation: {validation:?}"
    );
    let scope = renderer
        .device()
        .push_error_scope(wgpu::ErrorFilter::Validation);
    array_growth(renderer, &mut painter, &surface, &canvas);
    assert!(
        scope.pop().await.is_none(),
        "raster submission failed validation"
    );
    canvas.remove();
}

/// The witness uses small device pixels rather than a CSS-resampled terminal.
fn canvas() -> web_sys::HtmlCanvasElement {
    let canvas = window()
        .unwrap()
        .document()
        .unwrap()
        .create_element("canvas")
        .unwrap()
        .dyn_into::<web_sys::HtmlCanvasElement>()
        .unwrap();
    canvas.set_width(32);
    canvas.set_height(16);
    canvas
}

/// Capture in the submission turn, before browser composition discards the texture.
fn pixels(canvas: &web_sys::HtmlCanvasElement) -> Vec<u8> {
    let copy = self::canvas();
    let context = copy
        .get_context("2d")
        .unwrap()
        .unwrap()
        .dyn_into::<web_sys::CanvasRenderingContext2d>()
        .unwrap();
    // Read before the next refresh discards the WebGPU drawing buffer.
    context
        .draw_image_with_html_canvas_element(&canvas, 0.0, 0.0)
        .unwrap();
    context
        .get_image_data(0.0, 0.0, 32.0, 16.0)
        .unwrap()
        .data()
        .0
}

/// Every stroke and counter must survive both mask tint and RGBA sampling.
fn assert_ink(pixels: &[u8]) {
    for y in 0..7 {
        for x in 0..5 {
            let ink = mask()[y * 5 + x] != 0;
            let white = &pixels[((y + 2) * 32 + x + 2) * 4..][..4];
            let green = &pixels[((y + 2) * 32 + x + 12) * 4..][..4];
            assert_eq!(
                white,
                if ink {
                    &[255, 255, 255, 255]
                } else {
                    &[0, 0, 0, 255]
                }
            );
            assert_eq!(
                green,
                if ink {
                    &[0, 255, 0, 255]
                } else {
                    &[0, 0, 0, 255]
                }
            );
        }
    }
}

/// A with a counter and crossbar catches both missing ink and a solid smeared quad.
fn mask() -> Vec<u8> {
    [
        "01110", "10001", "10001", "11111", "10001", "10001", "10001",
    ]
    .into_iter()
    .flat_map(|row| row.bytes().map(|b| if b == b'1' { 255 } else { 0 }))
    .collect()
}

/// Both atlas kinds share UVs but have different source and tint semantics.
fn raster_frame() -> RenderFrame {
    let mask = mask();
    let rect = RectU32 {
        x: 0,
        y: 0,
        width: 5,
        height: 7,
    };
    let color = mask
        .iter()
        .flat_map(|coverage| [0, 255, 0, *coverage])
        .collect();
    RenderFrame {
        target_size: [32, 16],
        clear: [0.0, 0.0, 0.0, 1.0],
        uploads: vec![
            Upload::Mask {
                page: 0,
                rect,
                pixels: mask,
            },
            Upload::Color {
                page: 0,
                rect,
                pixels: color,
            },
        ],
        rasters: [0, 1]
            .into_iter()
            .map(|flags| RasterInstance {
                rect: [2.0 + flags as f32 * 10.0, 2.0, 5.0, 7.0],
                uv: [0.0, 0.0, 5.0 / 2048.0, 7.0 / 2048.0],
                tint: 0xffff_ffff,
                flags,
            })
            .collect(),
        batches: vec![Batch {
            slot: ZSlot::TextRaster,
            pipeline: PipelineKind::Raster,
            instances: 0..2,
            scissor: None,
        }],
        ..RenderFrame::default()
    }
}

/// Growth must retain page-zero ink while both formats acquire later layers.
fn array_growth(
    renderer: &puzzle_render::Renderer,
    painter: &mut puzzle_render::FrameRenderer,
    surface: &wgpu::Surface<'_>,
    canvas: &web_sys::HtmlCanvasElement,
) {
    let mut frame = raster_frame();
    painter
        .render_surface(renderer, &mut frame, surface)
        .unwrap();
    assert_ink(&pixels(canvas));
    // Jump to the maximum allowed mask page and second color page. The GPU
    // must grow each array independently and rebind the replaced textures.
    let mut later = raster_frame();
    for upload in &mut later.uploads {
        match upload {
            Upload::Mask { page, .. } => *page = 3,
            Upload::Color { page, .. } => *page = 1,
            _ => unreachable!(),
        }
    }
    later.rasters[0].flags = 3 << 8;
    later.rasters[1].flags = (1 << 8) | 1;
    painter
        .render_surface(renderer, &mut later, surface)
        .unwrap();
    assert_ink(&pixels(canvas));
    // No new uploads: this can pass only if growth copied the old pages.
    frame.uploads.clear();
    painter
        .render_surface(renderer, &mut frame, surface)
        .unwrap();
    assert_ink(&pixels(canvas));
}

/// Fractional bitmap transforms preserve premultiplied coverage at atlas borders.
mod effects;
