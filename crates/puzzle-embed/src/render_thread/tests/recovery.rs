//! Forced device destruction and byte-identical restoration run on the actor.
//!
//! Reusing a cached frame without fresh mirror uploads would return blank or
//! stale glyphs after replacement. Exact pixels and unchanged preparation count
//! prove that retained CPU data, rather than re-extracted font outlines, restores it.
use super::*;
use puzzle_render::{FrameRenderer, Renderer};

/// Five minutes only detects a stranded worker/callback, not slow debug work.
/// The isolated debug witness normally takes about 13 seconds; speed budgets
/// belong to optimized acceptance tests, never this functional recovery guard.
use crate::test_support::HANG_GUARD;

/// Finite GPU witness with one result; native host callbacks remain absent.
struct Recovery {
    /// Reports GPU readback or recovery failures instead of hanging acceptance.
    result: mpsc::Sender<Result<(), String>>,
}
impl Driver for Recovery {
    /// Creates, destroys and restores real resources only on the process worker.
    fn advance(&mut self, _updates: Pending, _now: Instant) -> Option<Instant> {
        let _ = self.result.send(restore());
        None
    }
}

/// Two identical images must straddle actual device replacement.
fn restore() -> Result<(), String> {
    if thread::current().name() != Some("puzzle-render") {
        return Err("recovery did not run on the puzzle-render thread".into());
    }
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let renderer =
        crate::block_on::block_on(Renderer::new(&instance, None)).map_err(|e| e.to_string())?;
    let (lost, notices) = watch_loss(&renderer);
    let mut context = TextContext::new(crate::font_data::bundled_font_set()?, 14.0, 1.0)?;
    let mut session = Session::new(
        SessionConfig::default(),
        Services::new(Box::new(NullTransport), Box::new(Quiet)),
    );
    session.set_text_context(&context);
    session.feed(b"retained glyphs A0 xyz", Instant::now());
    let mut executor = FrameRenderer::new(&renderer, wgpu::TextureFormat::Rgba8Unorm);
    let mut drawing = crate::drawing::Drawing::default();
    let frame = drawing
        .draw(&mut session, Instant::now(), &mut context)?
        .ok_or("initial frame absent")?;
    let before = executor
        .render_to_image(&renderer, frame)
        .map_err(|e| e.to_string())?
        .0;
    let prepared = context.prepared_glyphs();
    let restored = replace_device(&renderer, &lost, &notices)?;
    let mut executor = FrameRenderer::new(&restored, wgpu::TextureFormat::Rgba8Unorm);
    context.reset_atlas();
    session.set_text_context(&context);
    let frame = drawing
        .draw(&mut session, Instant::now(), &mut context)?
        .ok_or("restored frame absent")?;
    let after = executor
        .render_to_image(&restored, frame)
        .map_err(|e| e.to_string())?
        .0;
    if before.pixels != after.pixels {
        return Err("device replacement lost cached atlas ink".into());
    }
    if prepared != context.prepared_glyphs() {
        return Err(format!(
            "recovery re-prepared glyphs: before {prepared}, after {}",
            context.prepared_glyphs()
        ));
    }
    Ok(())
}

/// Connects device-loss notification to a finite host without creating a native window.
fn watch_loss(
    renderer: &Renderer,
) -> (
    Arc<std::sync::atomic::AtomicBool>,
    mpsc::Receiver<host::HostAction>,
) {
    let (notify, notices) = mpsc::channel();
    let host = host::HostView::new(
        surface::SurfaceInfo {
            extent: surface::Extent {
                width: 800,
                height: 600,
            },
            scale: 1.0,
            display: 0,
            millihertz: 60_000,
        },
        host::Visibility::default(),
        Arc::new(move |event| {
            let _ = notify.send(event);
        }),
    );
    let lost = crate::render_thread::recovery::watch(renderer, host);
    (lost, notices)
}

/// Invokes the production loss notification before requesting replacement resources.
fn replace_device(
    renderer: &Renderer,
    lost: &Arc<std::sync::atomic::AtomicBool>,
    notices: &mpsc::Receiver<host::HostAction>,
) -> Result<Renderer, String> {
    renderer.device().destroy();
    let _ = renderer.device().poll(wgpu::PollType::Poll);
    notices
        .recv_timeout(HANG_GUARD)
        .map_err(|e| format!("device-loss callback hang guard: {e}"))?;
    if !lost.load(Ordering::Acquire) {
        return Err("device callback did not mark loss".into());
    }
    let restored = crate::block_on::block_on(renderer.recreate()).map_err(|e| e.to_string())?;
    Ok(restored)
}

/// Real GPU loss cannot leak a driver or strand process shutdown.
#[test]
fn device_loss_restores_ink_on_render_thread() -> Result<(), String> {
    // Acquire before starting the worker: time waiting for other GPU fixtures
    // does not consume even the generous recovery hang guard.
    let _gpu = crate::gpu_test::lock();
    let (result, received) = mpsc::channel();
    let mut actor = RenderThread::start()?;
    let (id, _) = actor.add(Box::new(Recovery { result }))?;
    let outcome = received
        .recv_timeout(HANG_GUARD)
        .map_err(|e| format!("render-thread recovery hang guard: {e}"))?;
    // Join before reporting a failed check, so the next fixture gets a clean GPU.
    actor.remove(id);
    actor.shutdown();
    outcome
}
