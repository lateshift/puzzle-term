//! Two real native windows exercise one App, render actor and text context (M17.embed).
//!
//! This explicit diagnostic owns both children and never types into a user shell.
//! Host input uses Surface methods; the observer only records successful submissions.
//! Both native windows stay visible without requesting keyboard focus. Hardware is
//! selected and witnessed at runtime, rather than inferred from a requested position.
//! A separate no-flood control retains exactly the same geometry and scroll inputs.
//! Reports keep raw intervals, child receipts and paired owner/writer clocks.
//! No experiment changes product pacing, GPU latency or glyph preparation policy.
//! Lifecycle is intentionally asynchronous until the final App drop barrier.
//! Closing a view invalidates its Surface before worker cancellation is queued.
//! Windows are retained in the Probe until the rendering actor has joined.
//! No measurement handle can keep a cancelled Session in its held-screen wait.
//!
//! Host callbacks are routed by SurfaceId, never by a shared current-window flag.
//! A title or clipboard request from the flood must not reach its sibling view.
//! The original single-window event handler applies each result after App::tick.
//! The same handler forwards native resize, visibility and display observations.
//! The fixture never reaches into App's worker registry or GPU resource pools.
//!
//! Keyboard focus is not required to submit authored host input through Surface.
//! Synthetic callbacks use the checked capability on the creating thread.
//! Any real native key callback instead invalidates the measurement immediately.
//! A close callback also returns an explicit error and cancels both owned children.
//! Neither interruption can silently shorten the interval and raise apparent fps.
//!
//! Both windows are sized to coexist on the LG without overlapping each other.
//! Their requested desktop positions use the destination display's logical units.
//! The driver validates actual monitor identity after native creation settles.
//! A missing or incorrectly placed display is setup failure, never a 120-Hz result.
//! The built-in screen is currently closed and is outside this cohort's evidence.
use super::{WakeEvent, apply_engine_event, host_surface, ui::InputWindow};
use puzzle_embed::{App, AppConfig, AppEvent, DesktopSettings, SurfaceOptions};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use winit::{
    application::ApplicationHandler,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, EventLoop, EventLoopProxy},
    window::{Window, WindowId},
};
/// Finite input schedule and exact receipt matching.
mod driver;
/// Render observations never substitute for actual successful presentations.
mod observer;
/// Summary and unchanged cadence/key criteria over the raw evidence.
mod report;
/// Private authored child program and executable shell wrapper.
mod workload;
use observer::Samples;

/// One host loop and exactly one App own two independently cancellable surfaces.
struct Probe {
    /// Wakeups can arrive from either Session without a host callback on that worker.
    proxy: EventLoopProxy<WakeEvent>,
    /// Declared before windows so its joined drop releases GPU native targets first.
    app: App,
    /// Native handles remain live through the library's surface shutdown.
    windows: Vec<InputWindow>,
    /// Shared observations contain clocks only, never Session or GPU resources.
    samples: [Arc<Mutex<Samples>>; 2],
    /// One immutable config gives both surfaces the same text-context key.
    settings: DesktopSettings,
    /// Every invocation has a private output directory and child receipt file.
    out: PathBuf,
    /// Controls use an idle first window; treatment uses a continuous PTY flood.
    flood: bool,
    /// The finite schedule begins after both windows have been created.
    created: Option<Instant>,
    /// Both observers share precisely the same measurement interval.
    started: Option<Instant>,
    /// Notch deadlines are separate from key deadlines, just as native inputs are.
    next_scroll: Option<Instant>,
    /// Sparse keys exercise the real writer while another surface is flooding.
    next_key: Option<Instant>,
    /// Exact send clocks pair by sequence with the owned child's receipt journal.
    keys: Vec<u128>,
    /// Failure is returned after joined cleanup, rather than hidden in a callback log.
    result: Option<Result<(), String>>,
}

/// The scrolling fixture keeps its viewport while a real child receives keys.
pub(super) fn options(mut options: crate::RunOptions) -> crate::RunOptions {
    if std::env::var_os("PUZZLE_EMBED_PROBE").is_some() {
        // Normal onInput behavior follows output; that would end this local scroll.
        // The explicit test override also makes stationary control surfaces idle.
        options.sets.push("scrolling.onInput=false".into());
        options.sets.push("cursor.blink=false".into());
    }
    options
}

/// Runs only when explicitly selected by the existing diagnostic feature.
pub(super) fn run(settings: DesktopSettings) -> Result<(), String> {
    // One output directory owns its scripts and receipt journals.
    // Absolute paths survive the PTY child starting in a different directory.
    let out = PathBuf::from(
        std::env::var("PUZZLE_EMBED_PROBE_OUT")
            .unwrap_or_else(|_| "target/m17-two-windows/latest".into()),
    );
    std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
    let out = out.canonicalize().map_err(|e| e.to_string())?;
    workload::prepare(&out)?;
    // A separate winit loop is needed only for this finite two-view host.
    // Both terminals still use one ordinary App and one rendering worker.
    let event_loop = EventLoop::<WakeEvent>::with_user_event()
        .build()
        .map_err(|e| e.to_string())?;
    let proxy = event_loop.create_proxy();
    let wake = proxy.clone();
    let app = App::new(AppConfig {
        settings: settings.clone(),
        wakeup: Arc::new(move || {
            let _ = wake.send_event(WakeEvent::SessionResults);
        }),
    })
    .map_err(|e| e.to_string())?;
    // No recording or fallback configuration is shared mutably by the host.
    // The two observation journals have distinct owners, like their row caches.
    let mut probe = Probe {
        proxy,
        app,
        windows: Vec::new(),
        settings,
        out,
        samples: std::array::from_fn(|_| Arc::new(Mutex::new(Samples::default()))),
        flood: std::env::var("PUZZLE_EMBED_PROBE").as_deref() != Ok("control"),
        created: None,
        started: None,
        next_scroll: None,
        next_key: None,
        keys: Vec::with_capacity(32),
        result: None,
    };
    event_loop.run_app(&mut probe).map_err(|e| e.to_string())?;
    // App drop joins the actual PTY/read/Session/render workers before returning.
    probe
        .result
        .take()
        .unwrap_or_else(|| Err("two-window probe interrupted".into()))
}

impl Probe {
    /// Creates both host views on the explicitly selected physical display.
    fn start(&mut self, event_loop: &ActiveEventLoop) -> Result<(), String> {
        // Select actual active hardware, and fail instead of using another screen.
        // The user currently has only the external display active.
        let monitor = event_loop
            .available_monitors()
            .find(|m| m.size().width == 5120 && m.refresh_rate_millihertz() == Some(60_000))
            .ok_or("external LG 5K 60-Hz display is unavailable")?;
        // Winit window positions are desktop logical coordinates.
        // Convert with the chosen monitor scale, not the first window scale.
        let origin = monitor.position().to_logical::<f64>(monitor.scale_factor());
        for index in 0..2 {
            let window = self.create_window(event_loop, index, origin)?;
            // Each child receives its own role without changing the process environment.
            // Observer callbacks retain clocks only; they cannot pace either surface.
            let options = SurfaceOptions {
                shell: Some(self.out.join("shell.sh").into_os_string()),
                env: workload::environment(&self.out, index, self.flood),
                observer: Some(Box::new(observer::Observer {
                    samples: self.samples[index].clone(),
                })),
                ..SurfaceOptions::default()
            };
            // App keeps the native Arc through joined render teardown.
            // This is the same creation API used by the normal single-window harness.
            let (surface, perf) = self
                .app
                .create_surface_with_diagnostics(
                    window.clone(),
                    host_surface::info(&window),
                    host_surface::visibility(&window),
                    options,
                )
                .map_err(|e| e.to_string())?;
            self.windows.push(InputWindow::new(
                window,
                surface,
                perf,
                self.settings.clone(),
                None,
            ));
        }
        // Warm-up starts after the slower of the two surface creations.
        // This keeps first-surface initialization out of the sibling interval.
        self.created = Some(Instant::now());
        Ok(())
    }
    /// Initial native placement occurs before surface creation, without an asynchronous move.
    fn create_window(
        &self,
        event_loop: &ActiveEventLoop,
        index: usize,
        origin: winit::dpi::LogicalPosition<f64>,
    ) -> Result<Arc<Window>, String> {
        let window = Arc::new(
            event_loop
                .create_window(
                    Window::default_attributes()
                        .with_min_inner_size(super::MIN_INNER_SIZE)
                        .with_title(format!(
                            "M17 shared App — {}",
                            if index == 0 { "flood" } else { "scroll + keys" }
                        ))
                        .with_active(false)
                        .with_resizable(false)
                        .with_window_level(winit::window::WindowLevel::AlwaysOnTop)
                        .with_inner_size(winit::dpi::LogicalSize::new(1000.0, 1000.0))
                        .with_position(winit::dpi::LogicalPosition::new(
                            origin.x + 100.0 + index as f64 * 1100.0,
                            origin.y + 100.0,
                        )),
                )
                .map_err(|e| e.to_string())?,
        );
        Ok(window)
    }
    /// The same order as product teardown cancels Sessions before render joins.
    fn stop(&mut self, event_loop: &ActiveEventLoop, result: Result<(), String>) {
        // Preserve a budget failure before any asynchronous close event arrives.
        // Cancellation wakes held EOF and saturated drain paths explicitly.
        self.result = Some(result);
        for window in &self.windows {
            let _ = self.app.close_surface(window.session.id());
        }
        event_loop.exit();
    }
}

impl ApplicationHandler<WakeEvent> for Probe {
    /// OS handles must exist before safe surface creation on the native thread.
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.created.is_none()
            && let Err(error) = self.start(event_loop)
        {
            self.stop(event_loop, Err(error));
        }
    }
    /// Deliver each event to its own view; identity never aliases the other surface.
    fn user_event(&mut self, event_loop: &ActiveEventLoop, _event: WakeEvent) {
        // Split host borrows: callback delivery must not borrow the App again.
        // It may enqueue through a Surface without synchronously touching Session.
        let windows = &mut self.windows;
        let proxy = &self.proxy;
        let mut closed = false;
        let result = self
            .app
            .tick(|id, event: AppEvent| {
                if let Some(window) = windows.iter_mut().find(|w| w.session.id() == id) {
                    closed |= apply_engine_event(window, proxy, event);
                }
            })
            .map_err(|e| e.to_string());
        // A child exit invalidates the workload rather than ending its denominator early.
        if closed || result.is_err() {
            self.stop(
                event_loop,
                result.and(Err("child or surface closed during measurement".into())),
            );
        }
    }
    /// Geometry/visibility use ordinary host forwarding; human keys invalidate the run.
    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        // Synthetic keys call Surface directly and never produce a native callback.
        // Thus a keyboard callback here proves external input contaminated the run.
        if matches!(
            event,
            WindowEvent::CloseRequested | WindowEvent::KeyboardInput { .. }
        ) {
            self.stop(
                event_loop,
                Err("measurement interrupted by native input/close".into()),
            );
            return;
        }
        if let Some(window) = self.windows.iter_mut().find(|w| w.window.id() == id) {
            window.handle_event(event);
        }
    }
    /// Only the finite input fixture has host timers; ordinary idle behavior is unchanged.
    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        // Absolute finite input deadlines do not participate in frame reservations.
        // Render pacing remains entirely on the existing per-display link.
        match self.advance(Instant::now()) {
            Ok(Some(due)) => {
                event_loop.set_control_flow(winit::event_loop::ControlFlow::WaitUntil(due))
            }
            Ok(None) => {
                let result = self.finish();
                self.stop(event_loop, result);
            }
            Err(error) => self.stop(event_loop, Err(error)),
        }
    }
}
