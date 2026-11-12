//! Host-fed flood uses real Session publications and GPU draws on the render actor.
//! Timing is asserted only in release; debug retains the same functional workflow.
//! The offscreen target avoids native-window automation and framebuffer readback
//! during measurement. This measures transport/render isolation, not display cadence.
use super::*;
use puzzle_render::{FrameRenderer, Renderer};
use puzzle_term::{
    FrameResult, SessionMessage,
    frame::RenderFrame,
    frame_builder::{FrameBuilder, context::TextContext},
};
use std::sync::{
    Mutex,
    atomic::{AtomicUsize, Ordering},
};

/// Thread-owned drawing state consumes the actual three-slot publication channel.
struct FloodDriver {
    /// No thread borrows the Session; requests are control messages.
    session: session_thread::Handle,
    /// Only this driver consumes and recycles sealed publications.
    frames: session_thread::Frames,
    /// Persistent shaping and glyph storage stay on the render actor.
    context: TextContext,
    /// Row instances likewise remain with their drawing owner.
    builder: FrameBuilder,
    /// Upload arrays survive every measured frame.
    frame: RenderFrame,
    /// GPU submissions use the production executor.
    renderer: Renderer,
    /// Pipelines and atlas textures are persistent.
    executor: FrameRenderer,
    /// One persistent target excludes readback from the throughput measurement.
    view: wgpu::TextureView,
    /// Host output wakes do not create additional probe refreshes.
    due: Option<Instant>,
    /// Test observation only, not product scheduling state.
    count: Arc<AtomicUsize>,
    /// Errors cross the driver boundary explicitly rather than panicking a worker.
    error: Arc<Mutex<Option<String>>>,
}
impl crate::render_thread::Driver for FloodDriver {
    fn advance(
        &mut self,
        _: crate::render_thread::surface::Pending,
        now: Instant,
    ) -> Option<Instant> {
        // The finite offscreen probe requests a 60-Hz workload, independent of monitor state.
        let period = Duration::from_secs_f64(1.0 / 60.0);
        if let Some(due) = self.due
            && now < due
        {
            return Some(due);
        }
        self.due = Some(now + period);
        if let Err(error) = self.draw(now, now + period) {
            *self.error.lock().unwrap() = Some(error);
            return None;
        }
        Some(now + period)
    }
}
impl FloodDriver {
    /// One bounded publication wait and one real GPU submission per probe refresh.
    fn draw(&mut self, now: Instant, deadline: Instant) -> Result<(), String> {
        self.session.request_frame(
            now,
            self.context.atlas_generation(),
            self.builder.style_epoch(),
            deadline,
        )?;
        let Some(mut reply) = self.frames.receive_until(deadline) else {
            return Ok(());
        };
        let result = if reply.published {
            self.builder
                .build_published(&mut self.context, &mut reply.slot, &mut self.frame)
                .and_then(|()| {
                    self.executor
                        .render_view(&self.renderer, &mut self.frame, &self.view)
                        .map(|_| ())
                        .map_err(|e| e.to_string())
                })
        } else {
            Ok(())
        };
        if result.is_ok() && reply.published {
            self.count.fetch_add(1, Ordering::Release);
            self.session
                .send(SessionMessage::FrameResult(FrameResult::Presented(
                    Default::default(),
                )))?;
        }
        self.frames.recycle(reply.slot);
        result
    }
}

/// The same host/source/GPU lifetime runs without parallel-debug speed assertions.
#[test]
fn host_flood_keeps_frames_and_input_functionally() {
    run(false);
}

/// §15 flood bounds apply to optimized code, with unchanged 30-fps/50-ms limits.
#[cfg_attr(debug_assertions, ignore = "requires the release profile")]
#[test]
fn host_flood_and_key_budgets() {
    run(true);
}

/// GPU setup and the first submitted frame are excluded from the warm flood interval.
fn run(budget: bool) {
    let _gpu = crate::gpu_test::lock();
    let (mut app, wake) = app();
    let (surface, count, error) = attach(&mut app);
    let corpus = std::fs::read("../../assets/fixtures/corpus/flood.vt").unwrap();
    let guard = Instant::now() + test_support::HANG_GUARD;
    let mut offset = 0;
    while count.load(Ordering::Acquire) == 0 {
        offset = (offset + surface.feed_output(&corpus[offset..]).unwrap()) % corpus.len();
        app.tick(|_, _| {}).unwrap();
        assert!(
            Instant::now() < guard,
            "first host flood frame absent: {:?}",
            *error.lock().unwrap()
        );
        let _ = wake.recv_timeout(Duration::from_millis(1));
    }
    measure(
        &mut app,
        &surface,
        (&count, &error),
        &corpus,
        offset,
        guard,
        budget,
    );
}

/// Warm throughput and host callbacks are observed without GPU readback.
fn measure(
    app: &mut App,
    surface: &Surface,
    observations: (&AtomicUsize, &Mutex<Option<String>>),
    corpus: &[u8],
    mut offset: usize,
    guard: Instant,
    budget: bool,
) {
    let (count, error) = observations;
    let baseline = count.load(Ordering::Acquire);
    let start = Instant::now();
    let mut keys = Vec::new();
    let mut received = 0;
    let mut maximum = Duration::ZERO;
    while start.elapsed() < Duration::from_secs(2) || received < keys.len() {
        if keys.len() < 6 && start.elapsed().as_secs_f64() >= keys.len() as f64 / 3.0 {
            keys.push(Instant::now());
            surface.text("\x03".into()).unwrap();
        }
        offset = (offset + surface.feed_output(&corpus[offset..]).unwrap()) % corpus.len();
        app.tick(|_, event| {
            if let AppEvent::TransportInput(bytes) = event {
                for byte in bytes {
                    assert_eq!(byte, 3);
                    maximum = maximum.max(keys[received].elapsed());
                    received += 1;
                }
            }
        })
        .unwrap();
        assert!(surface.session.pending_bytes() <= 4 * 1024 * 1024);
        assert!(Instant::now() < guard, "host key/flood hung");
        thread::yield_now();
    }
    let seconds = start.elapsed().as_secs_f64();
    let frames = count.load(Ordering::Acquire) - baseline;
    assert!(
        error.lock().unwrap().is_none(),
        "{:?}",
        *error.lock().unwrap()
    );
    assert_eq!(received, 6);
    assert!(frames > 0);
    if budget {
        receipt(frames, seconds, maximum);
    }
}

/// The receipt records the exact interval and retains unchanged optimized limits.
fn receipt(frames: usize, seconds: f64, maximum: Duration) {
    let receipt = format!(
        "host transport: frames={frames}, seconds={seconds:.6}, fps={:.3}, key_max_ms={:.6}, pending_bound=4194304",
        frames as f64 / seconds,
        maximum.as_secs_f64() * 1000.0
    );
    std::fs::write(
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../target/m17-transport-flood.txt"
        ),
        receipt,
    )
    .unwrap();
    assert!(frames as f64 / seconds >= 30.0);
    assert!(maximum <= Duration::from_millis(50));
}

/// Cold resources use the same native device/frame executor and published row builder.
fn attach(app: &mut App) -> (Surface, Arc<AtomicUsize>, Arc<Mutex<Option<String>>>) {
    let (renderer, context, executor, view) = gpu(app);
    let id = app.next_id().unwrap();
    let (transport, pool, exit, mut pipeline) = super::super::transport::start(app, id);
    let mut session = Session::new(
        SessionConfig {
            columns: 240,
            rows: 70,
            render_settle: Duration::ZERO,
            ..SessionConfig::default()
        },
        Services::new(transport, Box::<PublicationWaker>::default()),
    );
    session.set_text_context(&context);
    let started = app.start_session(session, pool, exit, None).unwrap();
    pipeline.attach(started.handle.clone(), started.worker);
    let count = Arc::new(AtomicUsize::new(0));
    let error = Arc::new(Mutex::new(None));
    let driver = FloodDriver {
        session: started.handle.clone(),
        frames: started.frames,
        builder: FrameBuilder::new(&context),
        context,
        frame: RenderFrame::default(),
        renderer,
        executor,
        view,
        due: None,
        count: Arc::clone(&count),
        error: Arc::clone(&error),
    };
    let surface = app
        .register(
            id,
            Box::new(driver),
            started.handle,
            started.events,
            pipeline,
            None,
        )
        .unwrap();
    (surface, count, error)
}

/// Device initialization is cold and never enters the measured flood interval.
fn gpu(app: &App) -> (Renderer, TextContext, FrameRenderer, wgpu::TextureView) {
    let instance = app.instance.clone();
    app.render
        .resources(move |_| {
            let renderer = crate::block_on::block_on(Renderer::new(&instance, None)).unwrap();
            let context =
                TextContext::new(crate::font_data::bundled_font_set().unwrap(), 14.0, 1.0).unwrap();
            let executor = FrameRenderer::new(&renderer, wgpu::TextureFormat::Rgba8Unorm);
            let target = renderer.device().create_texture(&wgpu::TextureDescriptor {
                label: Some("host transport flood"),
                size: wgpu::Extent3d {
                    width: 240 * context.metrics().width_px,
                    height: 70 * context.metrics().height_px,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            });
            let view = target.create_view(&Default::default());
            (renderer, context, executor, view)
        })
        .unwrap()
}
