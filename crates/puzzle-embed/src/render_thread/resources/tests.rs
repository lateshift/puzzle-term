//! Shared context ink, independent settings and process-wide recovery (D-30).
#![allow(
    clippy::unwrap_used,
    reason = "GPU fixture setup failures must retain their exact cause"
)]
use super::*;
use puzzle_term::{frame::RenderFrame, frame_builder::FrameBuilder, vt::terminal::Terminal};

/// Isolated defaults keep a user's font configuration out of a mechanical GPU witness.
fn settings() -> DesktopSettings {
    DesktopSettings::open(Some("/nonexistent/puzzle-resource-test.json"), None, &[])
        .unwrap()
        .0
}
/// A real GPU with the production loss/error callbacks; no native window is needed here.
fn resources() -> (Resources, std::sync::mpsc::Receiver<()>) {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let renderer = crate::block_on::block_on(Renderer::new(&instance, None)).unwrap();
    let (send, receive) = std::sync::mpsc::channel();
    let gpu = Gpu::watch(
        renderer,
        Arc::new(move || {
            let _ = send.send(());
        }),
        1,
    );
    (
        Resources {
            gpu: Some(gpu),
            ..Default::default()
        },
        receive,
    )
}
/// Both surfaces draw exactly the same covered Latin/CJK ink through one atlas executor.
fn terminal() -> Terminal {
    let mut terminal = Terminal::new(40, 3, 0, 0);
    terminal.advance("\x1b[?25lHamburgefonstiv -> 漢字かな한글".as_bytes());
    terminal
}
/// Build and read back one surface while its bundle is exclusively checked out.
fn image(
    resources: &mut Resources,
    id: u64,
    format: wgpu::TextureFormat,
    builder: &mut FrameBuilder,
    terminal: &Terminal,
) -> puzzle_render::RgbaImage {
    let mut shared = resources.take(id, format).unwrap();
    let mut frame = RenderFrame::default();
    builder
        .build(&mut shared.text_context, terminal, &mut frame)
        .unwrap();
    let image = shared
        .frame_renderer
        .render_to_image(&shared.renderer, &mut frame)
        .unwrap()
        .0;
    resources.put(id, shared);
    image
}
/// Matching windows reuse prepared glyphs; another format changes shaders without losing ink.
#[test]
fn matching_surfaces_share_ink_and_release_the_last_context() {
    let _gpu = crate::gpu_test::lock();
    let (mut resources, _) = resources();
    let settings = settings();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let a = resources.select(&settings, 2.0, format).unwrap();
    let mut first = builder(&resources, a.key);
    let terminal = terminal();
    let expected = image(&mut resources, a.key, format, &mut first, &terminal);
    let count = resources.entries[&a.key]
        .shared
        .as_ref()
        .unwrap()
        .text_context
        .prepared_glyphs();
    let b = resources.select(&settings, 2.0, format).unwrap();
    assert_eq!(a.key, b.key);
    assert_eq!(resources.entries.len(), 1);
    let mut second = builder(&resources, b.key);
    let actual = image(&mut resources, b.key, format, &mut second, &terminal);
    assert_eq!(actual.pixels, expected.pixels);
    assert_eq!(
        resources.entries[&b.key]
            .shared
            .as_ref()
            .unwrap()
            .text_context
            .prepared_glyphs(),
        count
    );
    let other = resources
        .select(&settings, 2.0, wgpu::TextureFormat::Bgra8Unorm)
        .unwrap();
    assert_eq!(a.key, other.key, "attachment format must not reload fonts");
    let switched = image(
        &mut resources,
        other.key,
        other.format,
        &mut second,
        &terminal,
    );
    assert_eq!(switched.pixels, expected.pixels);
    let back = image(&mut resources, a.key, format, &mut first, &terminal);
    assert_eq!(
        back.pixels, expected.pixels,
        "format switch discarded sampled atlas ink"
    );
    resources.release(a.key);
    resources.release(b.key);
    assert_eq!(resources.entries.len(), 1);
    resources.release(other.key);
    assert!(resources.entries.is_empty());
}
/// A budget or geometry change selects another context instead of mutating its neighbour.
#[test]
fn policy_changes_keep_other_surfaces_and_device_recovery_restores_all_contexts() {
    let _gpu = crate::gpu_test::lock();
    let (mut resources, wake) = resources();
    let mut settings = settings();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let a = resources.select(&settings, 2.0, format).unwrap();
    settings.store.apply_key(
        "render.glyphMemoryMb",
        SettingsValue::Number(28.0),
        puzzle_term::settings::Layer::Session,
    );
    let b = resources.select(&settings, 2.0, format).unwrap();
    assert_ne!(a.key, b.key);
    let c = resources.select(&settings, 1.0, format).unwrap();
    assert_ne!(b.key, c.key);
    assert_eq!(resources.gpu.as_ref().unwrap().pipelines.len(), 1);
    let mut builders: Vec<_> = [a, b, c]
        .iter()
        .map(|p| builder(&resources, p.key))
        .collect();
    let terminal = terminal();
    let expected: Vec<_> = [a, b, c]
        .iter()
        .zip(&mut builders)
        .map(|(p, builder)| image(&mut resources, p.key, format, builder, &terminal))
        .collect();
    assert_eq!(budget(&resources, a.key), 48 * 1024 * 1024);
    assert_eq!(budget(&resources, b.key), 28 * 1024 * 1024);
    lose_device(&resources, &wake);
    resources.recover().unwrap();
    assert_eq!(resources.epoch(), 2);
    for ((p, builder), expected) in [a, b, c].iter().zip(&mut builders).zip(expected) {
        builder.set_context(
            &resources.entries[&p.key]
                .shared
                .as_ref()
                .unwrap()
                .text_context,
        );
        let restored = image(&mut resources, p.key, format, builder, &terminal);
        assert_eq!(
            restored.pixels, expected.pixels,
            "context {} did not restore",
            p.key
        );
        resources.release(p.key);
    }
    assert!(resources.entries.is_empty());
}

/// Construct a surface-owned row cache against the selected shared font policy.
fn builder(resources: &Resources, id: u64) -> FrameBuilder {
    FrameBuilder::new(&resources.entries[&id].shared.as_ref().unwrap().text_context)
}
/// Observe an actual atlas allowance rather than a duplicated settings value.
fn budget(resources: &Resources, id: u64) -> usize {
    resources.entries[&id]
        .shared
        .as_ref()
        .unwrap()
        .text_context
        .glyph_budget_bytes()
}

/// Dispatch and await the real device callback, ignoring unrelated font-source wakes.
fn lose_device(resources: &Resources, wake: &std::sync::mpsc::Receiver<()>) {
    // Only the loss flag proves this notification came from the device callback.
    resources.gpu.as_ref().unwrap().renderer.device().destroy();
    // wgpu dispatches destruction callbacks during explicit device polling.
    let _ = resources
        .gpu
        .as_ref()
        .unwrap()
        .renderer
        .device()
        .poll(wgpu::PollType::Poll);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(300);
    while !resources
        .gpu
        .as_ref()
        .unwrap()
        .lost
        .load(std::sync::atomic::Ordering::Acquire)
    {
        // This is a deadlock guard, not a device-recovery speed budget.
        wake.recv_timeout(deadline.saturating_duration_since(std::time::Instant::now()))
            .unwrap();
    }
}
