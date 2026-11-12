//! A 28-MiB policy must allocate three mask layers, never round to four.
use super::*;
use puzzle_term::frame::{RectU32, Upload};

/// GPU allocation follows policy and releases excess capacity on a shrink.
#[test]
fn gpu_capacity_respects_non_power_of_two_budget_and_reload() {
    let _guard = crate::gpu_test::lock();
    let renderer =
        crate::block_on::block_on(Renderer::new(&wgpu::Instance::default(), None)).unwrap();
    let mut executor =
        puzzle_render::FrameRenderer::new(&renderer, wgpu::TextureFormat::Rgba8Unorm);
    assert!(executor.set_glyph_budget(&renderer, 28 * 1024 * 1024));
    let mut frame = RenderFrame {
        target_size: [8, 8],
        ..Default::default()
    };
    frame.uploads.push(mask(2));
    executor.render_to_image(&renderer, &mut frame).unwrap();
    assert_eq!(executor.raster_bytes(), 28 * 1024 * 1024);
    frame.uploads.push(mask(3));
    assert!(executor.render_to_image(&renderer, &mut frame).is_err());
    assert!(executor.set_glyph_budget(&renderer, 20 * 1024 * 1024));
    assert_eq!(executor.raster_bytes(), 20 * 1024 * 1024);
    assert!(!executor.set_glyph_budget(&renderer, 20 * 1024 * 1024));
    // The CPU owner supplies fresh spans after retiring the old atlas incarnation.
    frame.uploads.clear();
    frame.uploads.push(mask(1));
    assert!(executor.render_to_image(&renderer, &mut frame).is_err());
}

/// A small upload still requires the full addressed texture-array capacity.
fn mask(page: u32) -> Upload {
    Upload::Mask {
        page,
        rect: RectU32 {
            x: 0,
            y: 0,
            width: 1,
            height: 1,
        },
        pixels: vec![255],
    }
}
