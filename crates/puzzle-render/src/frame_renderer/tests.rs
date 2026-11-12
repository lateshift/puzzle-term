//! Resize-race regression: a published frame outlives its surface extent.
#![allow(
    clippy::unwrap_used,
    clippy::too_many_lines,
    reason = "GPU regression fixture assertions and complete pixel comparisons"
)]
use super::*;
use std::{
    future::Future,
    sync::Arc,
    task::{Context, Poll, Wake, Waker},
};

/// Safe native executor used only to initialize the test device.
struct ThreadWake(std::thread::Thread);
impl Wake for ThreadWake {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }
}
/// GPU initialization may yield; no product runtime is introduced.
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

/// Shrinking/growing between publication and acquisition must clip, never scale.
#[test]
fn resize_between_publish_and_acquire_keeps_top_left_pixels() {
    let renderer = block_on(Renderer::new(&wgpu::Instance::default(), None)).unwrap();
    let mut executor = FrameRenderer::new(&renderer, wgpu::TextureFormat::Rgba8Unorm);
    for dpr in [1, 2] {
        for published in [96, 48] {
            // One sealed publication survives every jump, with no intervening recapture.
            let mut frame = published_frame(published, dpr);
            for (acquired, acquired_dpr) in [
                (48, dpr),
                (96, dpr),
                (48, dpr),
                (96, 3 - dpr),
                (48, 3 - dpr),
                // A target narrower than the old inset has an empty clip.
                (8, dpr),
            ] {
                let target = resized_target(&renderer, acquired, acquired_dpr);
                // This is exactly the product executor, with the post-resize drawable view.
                executor
                    .render_view(
                        &renderer,
                        &mut frame,
                        &target.create_view(&Default::default()),
                    )
                    .unwrap();
                let image = read_drawable(&renderer, &target);
                assert_clipped_ink(&image, published, dpr);
            }
        }
    }
}

/// Compare every pixel, including newly exposed background after a grow.
fn assert_clipped_ink(image: &crate::RgbaImage, published: u32, dpr: u32) {
    for y in 0..image.height {
        for x in 0..image.width {
            // The old rectangle keeps physical coordinates even after the DPR jump.
            // Scissors intersect the old canvas, which can be smaller than the attachment.
            let ink = x >= 16 * dpr
                && x < (48 * dpr).min(published * dpr)
                && y >= 16 * dpr
                && y < 40 * dpr;
            let offset = ((y * image.width + x) * 4) as usize;
            let expected = if ink {
                [255, 0, 0, 255]
            } else {
                [0, 0, 0, 255]
            };
            assert_eq!(
                &image.pixels[offset..offset + 4],
                &expected,
                "pixel ({x},{y}), DPR {dpr}"
            );
        }
    }
}

/// A sealed old frame is independent of the later physical extent and DPR.
fn published_frame(published: u32, dpr: u32) -> RenderFrame {
    // Publication finishes before the host changes the backing texture.
    let mut frame = RenderFrame {
        target_size: [published * dpr, 64 * dpr],
        clear: [0., 0., 0., 1.],
        ..Default::default()
    };
    frame.rects.push(puzzle_frame::RectInstance {
        rect: [
            16. * dpr as f32,
            16. * dpr as f32,
            32. * dpr as f32,
            24. * dpr as f32,
        ],
        color: 0xff00_00ff,
        corner_radius: 0.,
        stroke_width: 0.,
        flags: 0,
    });
    frame.batches.push(Batch {
        slot: puzzle_frame::ZSlot::Background,
        pipeline: PipelineKind::Rect,
        instances: 0..1,
        scissor: Some(puzzle_frame::RectU32 {
            x: 16 * dpr,
            y: 16 * dpr,
            width: 96 * dpr,
            height: 48 * dpr,
        }),
    });
    frame
}

/// A backing size/DPR jump occurs after the old frame has been sealed.
fn resized_target(renderer: &Renderer, acquired: u32, acquired_dpr: u32) -> wgpu::Texture {
    renderer.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("resized drawable regression"),
        size: wgpu::Extent3d {
            width: acquired * acquired_dpr,
            height: 64 * acquired_dpr,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    })
}

/// Exact device pixels distinguish clipping from accidental old-frame scaling.
fn read_drawable(renderer: &Renderer, target: &wgpu::Texture) -> crate::RgbaImage {
    let width = target.width();
    let height = target.height();
    let padded = (width * 4).div_ceil(256) * 256;
    let buffer = renderer.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("resize evidence"),
        size: u64::from(padded) * u64::from(height),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    renderer.copy_to_buffer(target, &buffer, padded, width, height);
    crate::RgbaImage {
        width,
        height,
        pixels: renderer
            .map_pixels(&buffer, width * 4, padded, height)
            .unwrap(),
    }
}

/// Product callbacks return an error for recovery; the default strict GPU hook is unchanged.
#[test]
fn uncaptured_error_is_observable_without_product_panic() {
    let renderer = block_on(Renderer::new(&wgpu::Instance::default(), None)).unwrap();
    let wakes = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let notified = Arc::clone(&wakes);
    let errors = crate::ErrorMonitor::install(renderer.device(), false, move || {
        notified.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    });
    errors.context(crate::FrameContext {
        number: 7,
        published: [96, 64],
        acquired: [48, 64],
    });
    // The invalid resource invokes wgpu's real uncaptured callback, not a mock error.
    let _invalid = renderer.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("intentional recovery regression"),
        size: wgpu::Extent3d {
            width: 0,
            height: 64,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    assert!(errors.take_error().unwrap().contains("Validation"));
    assert_eq!(wakes.load(std::sync::atomic::Ordering::Relaxed), 1);
    assert!(errors.take_error().is_none());
    // The next valid submission and exact pixels prove the device remains usable.
    let mut executor = FrameRenderer::new(&renderer, wgpu::TextureFormat::Rgba8Unorm);
    let mut frame = RenderFrame {
        target_size: [16, 16],
        clear: [1., 0., 0., 1.],
        ..Default::default()
    };
    let (image, _) = executor.render_to_image(&renderer, &mut frame).unwrap();
    assert_eq!(&image.pixels[..4], &[255, 0, 0, 255]);
}

/// The acceptance hook must fail on the same validation error the product recovers from.
#[test]
#[should_panic(expected = "GPU test validation failure")]
fn strict_uncaptured_error_remains_a_test_failure() {
    let renderer = block_on(Renderer::new(&wgpu::Instance::default(), None)).unwrap();
    let _errors = crate::ErrorMonitor::install(renderer.device(), true, || {});
    let _invalid = renderer.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("intentional strict-hook regression"),
        size: wgpu::Extent3d {
            width: 0,
            height: 16,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
}
