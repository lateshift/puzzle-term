//! Native M6.4 event loop for a live terminal in a local PTY (§13.1).
//!
//! The host owns the winit window; App owns its surface, PTY and worker lifetimes.
//! The library reader hands pool buffers to the sole Session owner. Winit sends input values without
//! inspecting VT or waiting for parsing; GPU work stays on the process actor.
//! EventLoopProxy wakes without polling an idle shell. The grid follows
//! the latest physical size each frame; PTY SIGWINCH trails by 25 ms.
//!
//! Ownership and callback order:
//! - `resumed` creates a window before asking wgpu for a compatible adapter.
//!   A surface without its Arc-owned OS handle would become invalid.
//! - GPU format selection uses that adapter's surface config; native Metal
//!   and WebGPU need not choose the same color attachment format.
//! - The PTY starts with the grid derived from the configured surface's
//!   physical extent and measured device cells. The shell's first prompt
//!   therefore sees the visible grid without waiting for a resize.
//! - The reader owns only a cloned master reader. The Session worker owns VT
//!   by value, draining priority controls before each bounded slice (§4.4).
//! - A frame wake unions damage in a surface mailbox. The actor requests a late slot,
//!   so several PTY chunks can become one presentation (P2).
//! - Session's blink/sync deadlines are absolute instants. The event loop
//!   forwards deadlines to the actor, which sleeps until useful work is due.
//! - During a drag, a resize configures the GPU immediately, the latest
//!   VT grid applies before a frame, and the PTY sees one trailing size.
//! - Minimized windows report zero backing pixels. No surface configure or
//!   grid resize runs for those transient dimensions.
//! - All errors use tracing, including startup failure. The binary's only
//!   stderr output is its command-line failure boundary (R10).
//! - The product title is the initial value; OSC 0/2 may replace it after
//!   the child writes a bounded title through VT (§6.5).
//! - Keyboard input enters the Session bus as `Origin::User`; it reaches
//!   the PTY writer during `pump`, before any redraw callback executes.
//! - The winit callback is converted immediately to owned RawInput. The
//!   command adapter never sees winit's private key fields, which lets
//!   headless scenarios and recordings replay identical decisions.
//! - Modifier and IME callbacks retain their OS arrival order; collapsing
//!   them into one synthetic key would hide the reported nvim Ctrl+D bug.
//! - A frame's parsed-byte counter is sampled with its VT snapshot, so
//!   recordings can identify a redraw that lagged behind parsed output.
//! - Clipboard requests travel back to this host, which alone uses arboard;
//!   headless scenarios retain the deterministic memory implementation.
//! - Native font completions arrive through EventLoopProxy as an internal
//!   platform command. Font preparation never accesses Session or its models.
//! - The finite Unicode probe is opt-in, with a neutral child and an isolated
//!   window. Normal user shells receive none of its synthetic inputs or timers.
use crate::{RunOptions, recording::Recorder, settings::DesktopSettings};
use std::sync::Arc;
/// The harness implements the host-native surface observation seam.
mod host_surface;
/// Native input has no GPU or font resource owner.
mod ui;
use winit::{
    application::ApplicationHandler,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, EventLoop, EventLoopProxy},
    window::{Window, WindowId},
};
/// Converts native callback order into recordable RawInput values.
mod input;
/// Finite native geometry recordings exercise the host seam without pacing changes.
#[cfg(feature = "lock-metrics")]
mod lifecycle_probe;
pub(crate) mod live;
/// Explicit development monitor placement and raw display metadata.
#[cfg(feature = "lock-metrics")]
mod measurement_display;
/// Two owned windows measure the shared App without taking keyboard focus.
#[cfg(feature = "lock-metrics")]
mod multi_probe;
/// Developer probes inspect the embedding executor's snapshot clock.
#[cfg(feature = "lock-metrics")]
mod pacing;
/// Real-window counters for the T5 before/after drag review.
mod perf;
/// Copies winit key, modifier and IME callbacks into recordable raw data.
mod raw;
/// Writes the first recording entry before a PTY can produce output.
mod record;
/// Exact native restart clocks remain diagnostic facts, independent of pacing.
#[cfg(feature = "lock-metrics")]
mod restart;
/// Explicit development-only scroll cadence recording on the real native surface.
#[cfg(feature = "lock-metrics")]
mod scroll_probe;
/// Measures the first physical grid from the configured native window.
mod start;
/// Opt-in Unicode flood measures native presentation and actual child key receipt.
#[cfg(feature = "lock-metrics")]
mod unicode_probe;
/// Shared physical-pixel grid path for the live window and T0 storm probe.
pub(crate) use live::grid_for_surface;
use puzzle_embed::{SurfaceAction as HostAction, SurfaceId};
use ui::InputWindow;

/// Host minimum in logical points, matching apps/macos/PuzzleShell/Windows/TerminalWindowController.swift:40.
/// A multi-line shell prompt cannot redraw cleanly in one row; D-43 leaves this policy
/// with the host, while the terminal core continues to accept one-row grids.
const MIN_INNER_SIZE: winit::dpi::LogicalSize<f64> = winit::dpi::LogicalSize::new(480.0, 320.0);

/// Wake reasons that can cross the PTY reader thread or Session seam.
#[derive(Debug)]
enum WakeEvent {
    /// An explicitly selected fixture posts ordinary native host operations.
    #[cfg(feature = "lock-metrics")]
    Lifecycle(lifecycle_probe::Action),
    /// Session has queued owned UI results; draining never reaches back into VT.
    SessionResults,
}

/// Native-thread window/input owner and the process render-thread lifetime.
struct WindowApp {
    /// UI wake proxy is also the renderer's host notification endpoint.
    proxy: EventLoopProxy<WakeEvent>,
    /// Exactly one rendering actor serves every registered surface in this process.
    engine: puzzle_embed::App,
    /// Native Arc stays alive until App joins rendering (field drop order).
    live: Option<InputWindow>,
    /// Current harness surface identity; embedding hosts can register additional surfaces.
    surface_id: Option<SurfaceId>,
    /// Optional private recording is shared with the Session and PTY workers.
    recorder: Option<Recorder>,
    /// Validated settings remain here only until native startup creates the Session.
    settings: Option<DesktopSettings>,
    /// One explicitly approved command, consumed before its surface is created.
    producer: Option<crate::producer::Request>,
}

/// Creates only the native window; the embedding App owns GPU and terminal setup.
fn create_window(loop_handle: &ActiveEventLoop) -> Result<Arc<Window>, String> {
    // Surface creation must happen after `resumed`, when winit has an
    // OS window handle on the thread Metal expects.
    let attributes = window_attributes(loop_handle)?;
    let window = Arc::new(
        loop_handle
            .create_window(attributes)
            .map_err(|error| error.to_string())?,
    );
    #[cfg(feature = "lock-metrics")]
    measurement_display::place(loop_handle, &window)?;
    window.set_ime_allowed(true);
    Ok(window)
}

/// Creates host attributes without putting diagnostic placement inside GPU startup.
fn window_attributes(
    loop_handle: &ActiveEventLoop,
) -> Result<winit::window::WindowAttributes, String> {
    // Owned measurement windows must not consume keys typed into another application.
    // Recorded diagnostic chords still enter the ordinary UI adapter through HostAction.
    let diagnostic = std::env::var_os("PUZZLE_SURFACE_COLOUR_REPORT").is_some()
        || std::env::var_os("PUZZLE_UNICODE_PROBE").is_some()
        || std::env::var_os("PUZZLE_SCROLL_PROBE").is_some()
        || std::env::var_os("PUZZLE_LIFECYCLE_SCRIPT").is_some();
    let attributes = Window::default_attributes()
        .with_title("Puzzle Terminal")
        .with_min_inner_size(MIN_INNER_SIZE)
        .with_active(!diagnostic)
        // Winit's macOS non-resizable zoom uses the main screen, ignoring the destination.
        // A maximized probe enables native zoom only until its placement completes.
        .with_resizable(!diagnostic || std::env::var_os("PUZZLE_UNICODE_MAXIMIZED").is_some())
        // Finite readback probes need visible surfaces even while another app receives keys.
        .with_window_level(if diagnostic {
            winit::window::WindowLevel::AlwaysOnTop
        } else {
            winit::window::WindowLevel::Normal
        });
    #[cfg(feature = "lock-metrics")]
    let attributes = measurement_display::attributes(loop_handle, attributes)?;
    #[cfg(not(feature = "lock-metrics"))]
    let _ = loop_handle;
    Ok(attributes)
}

impl ApplicationHandler<WakeEvent> for WindowApp {
    /// Winit creates native handles only after this callback.
    fn resumed(&mut self, loop_handle: &ActiveEventLoop) {
        if self.live.is_some() {
            return;
        }
        if let Err(error) = self.start(loop_handle) {
            tracing::error!(event="desktop.window_init_failed", %error, "live window initialization failed");
            loop_handle.exit();
            // Diagnostic setup must not run after startup has already failed.
            #[cfg(feature = "lock-metrics")]
            return;
        }
        #[cfg(feature = "lock-metrics")]
        if std::env::var_os("PUZZLE_LIFECYCLE_AT_UNICODE").is_none()
            && let Err(error) = lifecycle_probe::start(self.proxy.clone())
        {
            tracing::error!(event="desktop.lifecycle_probe_failed", %error);
            self.close_surface(loop_handle);
        }
    }

    /// Forwards coalesced damage/timers and applies posted host operations.
    fn user_event(&mut self, loop_handle: &ActiveEventLoop, event: WakeEvent) {
        // Tick and lifecycle work may synchronously enter CA display as well.
        let _busy = self.live.as_ref().map(InputWindow::callback);
        match event {
            #[cfg(feature = "lock-metrics")]
            WakeEvent::Lifecycle(action) => {
                if let Some(live) = &mut self.live
                    && lifecycle_probe::apply(live, action)
                {
                    self.close_surface(loop_handle);
                }
            }
            WakeEvent::SessionResults => {
                let mut close = false;
                if let Some(live) = &mut self.live {
                    let proxy = &self.proxy;
                    if let Err(error) = self.engine.tick(|_, event| {
                        close |= apply_engine_event(live, proxy, event);
                    }) {
                        tracing::error!(event="desktop.app_tick_failed", %error);
                        close = true;
                    }
                }
                if close {
                    self.close_surface(loop_handle);
                }
            }
        }
    }

    /// Handles native input and forwards physical layer changes without rendering.
    fn window_event(&mut self, loop_handle: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        if matches!(event, WindowEvent::CloseRequested) {
            // Keep the host Window Arc until the render owner's surface has been released.
            self.close_surface(loop_handle);
            return;
        }
        let Some(live) = &mut self.live else {
            return;
        };
        live.handle_event(event);
    }

    /// An idle native input loop has no render timer or blocking acquisition.
    fn about_to_wait(&mut self, loop_handle: &ActiveEventLoop) {
        if let Some(live) = &mut self.live {
            live.flush_drop(false);
        }
        loop_handle.set_control_flow(winit::event_loop::ControlFlow::Wait);
    }
}

/// Native window destruction must remain on the UI thread after surface teardown.
impl WindowApp {
    /// Stops input/PTY work now and requests asynchronous render ownership release.
    fn close_surface(&mut self, loop_handle: &ActiveEventLoop) {
        if let Some(id) = self.surface_id.take()
            && let Err(error) = self.engine.close_surface(id)
        {
            tracing::warn!(event="desktop.surface_close_failed", %error);
        }
        // The UI retains its native Window Arc until the process shutdown barrier.
        loop_handle.exit();
    }
}

/// Validates configuration before selecting the optional file log sink.
// File logging must start before any owned diagnostic creates a child.
// Startup warnings still use the same sink as a normal native terminal.
fn open_settings(options: &RunOptions) -> Result<DesktopSettings, String> {
    // Parse personal JSON before selecting the log sink: logging.file is
    // itself a user setting. Errors stay as warnings so a malformed file
    // never prevents the login shell from opening (§14.4).
    let (settings, warnings) = DesktopSettings::open(
        options.config.as_deref(),
        options.profile.as_deref(),
        &options.sets,
    )?;
    let configured_log = &settings.store.effective().logging_file;
    // A one-off command flag wins over an environment override, which in
    // turn wins over the persisted setting. Empty configured paths mean off.
    let env_log = std::env::var("PUZZLE_LOG_FILE").ok();
    let log_file = options
        .log
        .as_deref()
        .or(env_log.as_deref())
        .or((!configured_log.is_empty()).then_some(configured_log.as_str()));
    crate::logging::init(log_file)?;
    for warning in warnings {
        tracing::warn!(event = "settings.load_failed", %warning);
    }
    Ok(settings)
}

/// Starts the native login shell and returns after the window closes.
pub(crate) fn run(options: RunOptions) -> Result<(), String> {
    // Fixture policy is explicit before validation, with no product default change.
    #[cfg(feature = "lock-metrics")]
    let options = multi_probe::options(options);
    let settings = open_settings(&options)?;
    #[cfg(feature = "lock-metrics")]
    if std::env::var_os("PUZZLE_EMBED_PROBE").is_some() {
        return multi_probe::run(settings);
    }
    let recorder = if let Some(path) = options.record.as_ref() {
        let (recorder, path) = Recorder::open(path.as_deref())?;
        tracing::info!(path = %path.display(), "recording private terminal input and output");
        Some(recorder)
    } else {
        None
    };
    let event_loop = EventLoop::<WakeEvent>::with_user_event()
        .build()
        .map_err(|error| error.to_string())?;
    let proxy = event_loop.create_proxy();
    let wake = proxy.clone();
    let engine = puzzle_embed::App::new(puzzle_embed::AppConfig {
        settings: settings.clone(),
        wakeup: Arc::new(move || {
            let _ = wake.send_event(WakeEvent::SessionResults);
        }),
    })
    .map_err(|error| error.to_string())?;
    let mut app = WindowApp {
        proxy,
        live: None,
        engine,
        surface_id: None,
        recorder,
        settings: Some(settings),
        producer: options.producer,
    };
    event_loop
        .run_app(&mut app)
        .map_err(|error| error.to_string())
}
/// Authored workload observers stay outside the embedding product library.
#[cfg(feature = "lock-metrics")]
mod observer;

/// Native operations run only while App::tick delivers events on this winit thread.
fn apply_engine_event(
    live: &mut InputWindow,
    proxy: &EventLoopProxy<WakeEvent>,
    event: puzzle_embed::AppEvent,
) -> bool {
    #[cfg(not(feature = "lock-metrics"))]
    let _ = proxy;
    match event {
        puzzle_embed::AppEvent::Host(event) => live.handle_result(event),
        puzzle_embed::AppEvent::Render(action) => match action {
            HostAction::Close => true,
            HostAction::FlushPointer => {
                live.flush_pointer();
                false
            }
            // The App has already forwarded render damage to its own mailbox.
            // A finite real-surface colour witness closes through ordinary joined teardown.
            HostAction::FirstFrame => std::env::var_os("PUZZLE_SURFACE_COLOUR_REPORT").is_some(),
            HostAction::Frame | HostAction::CursorRect(_) => false,
            #[cfg(feature = "lock-metrics")]
            HostAction::Input { input, arrived } => {
                live.input_arrived(input, arrived);
                false
            }
            #[cfg(feature = "lock-metrics")]
            HostAction::ReplayTick { display, index } => {
                lifecycle_probe::magnet::tick(live, display, index);
                false
            }
            #[cfg(feature = "lock-metrics")]
            HostAction::StartReplay => {
                if let Err(error) = lifecycle_probe::start(proxy.clone()) {
                    tracing::error!(event="desktop.lifecycle_probe_failed", %error);
                    return true;
                }
                false
            }
        },
        // URL callbacks remain available to embedding hosts; the harness has no OS opener.
        puzzle_embed::AppEvent::OpenLink(url) => {
            tracing::debug!(%url, "host link request");
            false
        }
        // Default harness sessions own their PTY; host-source events are used only
        // by explicit transport probes and never interpreted as shell input here.
        puzzle_embed::AppEvent::TransportInput(_)
        | puzzle_embed::AppEvent::TransportReply(_)
        | puzzle_embed::AppEvent::TransportResize(_)
        | puzzle_embed::AppEvent::TransportClose
        | puzzle_embed::AppEvent::SurfaceReleased => false,
        puzzle_embed::AppEvent::DesktopNotification(effect) => notification_effect(live, effect),
        puzzle_embed::AppEvent::Notification { title, body } => {
            tracing::debug!(%title, %body, "host notification request");
            false
        }
    }
}

/// The Session overlay owns show/remove; only user-authorized focus reaches winit.
fn notification_effect(live: &mut InputWindow, effect: puzzle_embed::NotificationEffect) -> bool {
    // No protocol reply is constructed here; the owner already queued it in order.
    if matches!(effect, puzzle_embed::NotificationEffect::Focus) {
        live.window.focus_window();
    }
    false
}
