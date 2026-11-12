//! A replaced GPU must rebuild disposable masks while restoring image-only emoji.
use super::*;

/// Loss between a cold build and upload discards old tickets and recreates covered ink.
#[test]
fn real_device_loss_rebuilds_grid_masks_and_restores_authored_emoji() {
    let _gpu = crate::gpu_test::lock();
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let renderer = crate::block_on::block_on(Renderer::new(&instance, None)).unwrap();
    let mut text = context(&renderer);
    let mut builder = FrameBuilder::new(&text);
    let mut vt = Terminal::new(40, 2, 0, 0);
    vt.advance("\x1b[?25l漢字かな한글 🦀 ✌️".as_bytes());
    let mut frame = RenderFrame::default();
    builder.build(&mut text, &vt, &mut frame).unwrap();
    assert!(frame.rasters.len() > 5);
    let mut executor =
        puzzle_render::FrameRenderer::new(&renderer, wgpu::TextureFormat::Rgba8Unorm);
    executor.render_to_image(&renderer, &mut frame).unwrap();
    // This cold frame owns tickets that the lost device never uploads to its sampled atlas.
    vt.advance("\r\n語鬱뿔 🇩🇪".as_bytes());
    builder.build(&mut text, &vt, &mut frame).unwrap();
    assert!(
        frame
            .uploads
            .iter()
            .any(|u| matches!(u, puzzle_term::frame::Upload::GpuMask { .. }))
    );
    renderer.device().destroy();
    let restored = crate::block_on::block_on(renderer.recreate()).unwrap();
    // Production recovery invalidates disposable grid addresses and pending device-local writes.
    text.reset_atlas();
    frame.uploads.clear();
    install(&mut text, &restored, "auto").unwrap();
    builder.build(&mut text, &vt, &mut frame).unwrap();
    let mut executor =
        puzzle_render::FrameRenderer::new(&restored, wgpu::TextureFormat::Rgba8Unorm);
    let after = executor.render_to_image(&restored, &mut frame).unwrap().0;
    // A fresh text owner cannot share stale handles or restoration mirrors with the survivor.
    let mut fresh = context(&restored);
    let mut cold = FrameBuilder::new(&fresh);
    cold.build(&mut fresh, &vt, &mut frame).unwrap();
    let mut executor =
        puzzle_render::FrameRenderer::new(&restored, wgpu::TextureFormat::Rgba8Unorm);
    let expected = executor.render_to_image(&restored, &mut frame).unwrap().0;
    assert_eq!(
        after.pixels, expected.pixels,
        "device loss changed grid/emoji pixels"
    );
    restored.device().destroy();
}

/// Both owners use actual native fallback and the production GPU adapter at DPR 2.
fn context(renderer: &Renderer) -> TextContext {
    let mut text =
        TextContext::new(crate::font_data::bundled_font_set().unwrap(), 14.0, 2.0).unwrap();
    crate::system_fonts::install(&mut text, &[]).unwrap();
    install(&mut text, renderer, "auto").unwrap();
    text
}
