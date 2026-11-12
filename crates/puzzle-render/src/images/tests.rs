//! Pixel checks prove lazy residency, alpha composition, LRU and device recreation.
#![allow(
    clippy::unwrap_used,
    reason = "GPU fixtures fail with exact assertion context"
)]
use super::*;
use crate::FrameRenderer;
use puzzle_frame::{RasterRef, RectInstance, ZSlot};
use std::{
    future::Future,
    sync::Arc,
    task::{Context, Poll, Wake, Waker},
};
/// Device initialization remains test-local and introduces no runtime dependency.
struct ThreadWake(std::thread::Thread);
impl Wake for ThreadWake {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }
}
/// Tests block only for device setup and GPU readback, outside product hot paths.
fn block_on<T>(future: impl Future<Output = T>) -> T {
    let waker = Waker::from(Arc::new(ThreadWake(std::thread::current())));
    let mut cx = Context::from_waker(&waker);
    let mut future = std::pin::pin!(future);
    loop {
        match future.as_mut().poll(&mut cx) {
            Poll::Ready(value) => return value,
            Poll::Pending => std::thread::park(),
        }
    }
}
/// One source pixel creates a transparent image with no dependency on native codecs.
fn source(color: [u8; 4]) -> RasterRef {
    RasterRef {
        image: ImageSource::new([1, 1], Arc::from(color)).unwrap(),
        uv: [0.0, 0.0, 1.0, 1.0],
    }
}
/// The same executor is used for native surfaces, recovery and offscreen fixtures.
fn render(
    renderer: &Renderer,
    gpu: &mut FrameRenderer,
    frame: &mut RenderFrame,
) -> crate::RgbaImage {
    gpu.render_to_image(renderer, frame).unwrap().0
}
/// Real raster pixels retain strict z-order, alpha, crop and grid scissors.
#[test]
#[allow(
    clippy::too_many_lines,
    reason = "one GPU lifecycle fixture keeps readback ownership explicit"
)]
fn images_use_raster_pixels_and_independent_bounded_residency() {
    let renderer = block_on(Renderer::new(&wgpu::Instance::default(), None)).unwrap();
    let mut gpu = FrameRenderer::new(&renderer, wgpu::TextureFormat::Rgba8Unorm);
    let red = source([255, 0, 0, 128]);
    let green = source([0, 255, 0, 255]);
    let mut frame = RenderFrame {
        target_size: [8, 8],
        clear: [0.0, 0.0, 1.0, 1.0],
        ..Default::default()
    };
    frame.append_image(&red, [0.0, 0.0, 8.0, 8.0], ZSlot::Background, None);
    frame.rects.push(RectInstance {
        rect: [0.0, 0.0, 4.0, 8.0],
        color: 0xff00ff00,
        corner_radius: 0.0,
        stroke_width: 0.0,
        flags: 0,
    });
    frame.batches.push(puzzle_frame::Batch {
        slot: ZSlot::Background,
        pipeline: PipelineKind::Rect,
        instances: 0..1,
        scissor: None,
    });
    frame.images.as_mut().unwrap().budget = 8;
    let image = render(&renderer, &mut gpu, &mut frame);
    assert_eq!(&image.pixels[0..4], &[0, 255, 0, 255]);
    assert_eq!(&image.pixels[28..32], &[128, 0, 127, 255]);
    assert_eq!(gpu.images.as_ref().unwrap().resident_bytes(), 4);
    // Warm redraws reuse source textures. A crop outside the viewport uploads nothing.
    render(&renderer, &mut gpu, &mut frame);
    assert_eq!(gpu.images.as_ref().unwrap().clock, 2);
    assert_eq!(gpu.images.as_ref().unwrap().entries.len(), 1);
    frame.clear_instances();
    frame.append_image(&green, [20.0, 0.0, 8.0, 8.0], ZSlot::AboveText, None);
    assert!(frame.images.as_ref().unwrap().sources.is_empty());
    frame.append_image(&green, [0.0, 0.0, 8.0, 8.0], ZSlot::AboveText, None);
    frame.images.as_mut().unwrap().budget = 4;
    assert_eq!(
        &render(&renderer, &mut gpu, &mut frame).pixels[0..4],
        &[0, 255, 0, 255]
    );
    assert_eq!(gpu.images.as_ref().unwrap().entries.len(), 1);
    assert!(gpu.images.as_ref().unwrap().entries[0].matches(&green.image));
    // A fresh GPU executor has no residency; retained immutable frame pixels restore it.
    gpu = FrameRenderer::new(&renderer, wgpu::TextureFormat::Rgba8Unorm);
    assert_eq!(
        &render(&renderer, &mut gpu, &mut frame).pixels[0..4],
        &[0, 255, 0, 255]
    );
    assert_eq!(gpu.images.as_ref().unwrap().resident_bytes(), 4);
    // Disabling releases image residency without evicting or reallocating glyph pages.
    let glyph_bytes = gpu.raster_bytes();
    frame.clear_instances();
    frame.images.as_mut().unwrap().budget = 0;
    render(&renderer, &mut gpu, &mut frame);
    assert_eq!(gpu.images.as_ref().unwrap().resident_bytes(), 0);
    assert_eq!(gpu.raster_bytes(), glyph_bytes);
    tile_seam(&renderer);
}

/// A scaled crop crossing the hardware seam must filter neighboring source texels.
fn tile_seam(renderer: &Renderer) {
    let limit = renderer.device().limits().max_texture_dimension_2d;
    let width = limit + 1;
    let mut pixels = Vec::with_capacity(width as usize * 4);
    for x in 0..width {
        pixels.extend_from_slice(if x < limit - 2 {
            &[255, 0, 0, 255]
        } else {
            &[0, 0, 255, 255]
        });
    }
    let image = ImageSource::new([width, 1], pixels.into()).unwrap();
    let source = RasterRef {
        image,
        uv: [(limit - 4) as f32 / width as f32, 0.0, 1.0, 1.0],
    };
    let mut frame = RenderFrame {
        target_size: [20, 4],
        ..Default::default()
    };
    frame.append_image(&source, [0.0, 0.0, 20.0, 4.0], ZSlot::AboveText, None);
    frame.images.as_mut().unwrap().budget = puzzle_frame::image_resident_bytes([width, 1], limit);
    let mut gpu = FrameRenderer::new(renderer, wgpu::TextureFormat::Rgba8Unorm);
    let result = render(renderer, &mut gpu, &mut frame);
    // At x=8.5 the source center is seam+0.125, so interpolation is 3/8 red, 5/8 blue.
    let pixel = &result.pixels[8 * 4..9 * 4];
    assert!((i16::from(pixel[0]) - 96).abs() <= 2, "{pixel:?}");
    assert!((i16::from(pixel[2]) - 159).abs() <= 2, "{pixel:?}");
    assert_eq!(
        gpu.images.as_ref().unwrap().resident_bytes(),
        (width as usize + 2) * 4
    );
}
