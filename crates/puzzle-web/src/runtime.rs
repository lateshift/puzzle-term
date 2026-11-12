//! One browser terminal owns its Session; the page shares only GPU and text.
//!
//! JS objects never enter the Send core. A synchronous turn collects typed
//! transport effects and bus facts, releases its borrow, then invokes host
//! callbacks. Output parsing is budgeted independently of rAF presentation.
use js_sys::{Function, Promise};
use puzzle_term::{Session, platform::TransportEffect};
use std::{cell::RefCell, collections::VecDeque, rc::Rc};
use wasm_bindgen::prelude::*;
use web_time::Instant;
/// Validated JS key, lifecycle and transport arguments.
mod arguments;
/// DOM bootstrap with pack-first resources.
mod create;
/// The browser paint deadline is the sole presentation lane.
mod draw;
/// Native input ownership and current device-pixel geometry.
mod host;
/// Promise barriers and reply flushing around Session's budgeted output path.
pub(crate) mod output;
/// Custom interfaces accompany wasm-bindgen's generated class declarations.
mod types;
pub(crate) use create::create;
/// Minimal documented JS class and disposable registrations.
pub(crate) mod api;
/// Cell queries and explicit sizing accompany the same exported class.
mod api_geometry;
/// Lazy search operations and direct live progress getters.
mod api_search;
/// Selection, search and viewport facades wrap shared Rust operations (M8.7).
mod api_selection;
/// Coarse navigation and exact public identity validation.
mod api_viewport;
/// Domain observer facts are delivered only after synchronous policy.
mod events;
/// Browser-only cluster rasterization is passed through the neutral Send seam.
mod fallback;
/// Weak subscription and connection handles.
mod handles;
/// Typed host results are dispatched only after the Session turn has ended.
mod host_results;
/// System-color and host CSS integration remain browser-only.
mod media;
/// Permanent per-terminal drawing ownership, separate from the Session model.
mod render_state;
/// Shared page resources have weak registry entries and terminal-owned lifetimes.
mod resources;
/// Send services defer foreign work until the synchronous core turn ends.
mod services;
/// Settings actions and DPR changes select another shared text resource.
mod settings;
/// Client transports keep paste tails in order under granted input credit.
mod socket;
/// Bounded option and input conversion without a JSON dependency.
mod value;

/// Test/dev counters observe actual submission and pending bytes, never timers.
#[cfg(any(test, feature = "measurements"))]
mod measurements;

/// All nonshared state belongs to exactly one JS terminal wrapper.
pub(crate) struct Runtime {
    /// Explicit dev-only counters are absent from the shipped runtime.
    #[cfg(any(test, feature = "measurements"))]
    measurements: measurements::Measurements,
    /// Parser, modes, screen and interaction models stay private to this instance.
    session: Session,
    /// Rebuildable drawing caches and the circulating slot belong to this host.
    render: render_state::RenderState,
    /// Foreign effects remain deferred until their platform consumer runs.
    results: host_results::HostResults,
    /// Host may dispose during a callback; later effects then stop safely.
    disposed: bool,
    /// A reentrant callback appends effects without overtaking the current delivery.
    flushing: bool,
    /// Encoded-input subscribers retain registration identities for disposal.
    data: Vec<(u64, Function)>,
    /// Convenience subscriptions are separate from raw stdin subscribers.
    events: Vec<(u64, String, Function)>,
    /// Host write callbacks are called only after a core borrow is released.
    transport: Option<JsValue>,
    /// A detached old handle cannot remove a newer host transport.
    transport_id: u64,
    /// Socket state owns only this terminal connection.
    socket: Option<socket::Socket>,
    /// Exit is deferred until all preceding output has been parsed.
    exit: Option<puzzle_term::messages::ExitInfo>,
    /// Monotonic subscription identity avoids removing a later replacement.
    next_id: u64,
    /// Accepted output retains provenance through budget continuations.
    parsed: crate::transport::parsed::Parsed,
    /// Promise completions are barriers in that same accepted byte stream.
    waiting: VecDeque<output::Barrier>,
    /// A reply may target both a socket and a host callback; credit waits for both.
    reply_routes: VecDeque<(u64, usize)>,
    /// Owns the timeout and its closure so disposal cancels retained parser work (P3).
    continuation: Option<output::Continuation>,
    /// The platform supplies monotonic times; no core clock accesses JS state.
    started: Instant,
    /// Shared text resources are released when this terminal is disposed.
    text: Option<Rc<resources::Text>>,
    /// The canvas surface remains per terminal even on a shared device.
    surface: Option<wgpu::Surface<'static>>,
    /// Input elements and listeners have one explicit lifetime.
    dom: Option<crate::dom::DomInput<host::Host>>,
    /// Original CSS layout owner supplies fit dimensions.
    parent: Option<web_sys::HtmlElement>,
    /// Effective session options also select DOM key policies.
    settings: puzzle_term::settings::Settings,
    /// Layer ownership remains per terminal; it is never stored in the registry.
    store: puzzle_term::settings::SettingsStore,
    /// Media preference suppresses fades without overwriting user configuration.
    reduced_motion: bool,
    /// DPR converts container CSS pixels into exact backing dimensions.
    dpr: f64,
    /// Hidden pages retain parsing but stop requesting refresh callbacks.
    visible: bool,
    /// Coalesced observer size waits for a refresh callback.
    resize: Option<[u32; 2]>,
    /// One outstanding animation callback at most.
    raf: Option<i32>,
    /// Owned callback can be cancelled and released on dispose.
    draw: Option<wasm_bindgen::closure::Closure<dyn FnMut(f64)>>,
    /// Blink and trailing resize deadlines use one event-driven timer.
    timer: Option<wasm_bindgen::closure::Closure<dyn FnMut()>>,
    /// Cancelling this ID releases hidden-page work without polling rAF.
    timer_id: Option<i32>,
    /// The earliest core deadline wins across successive synchronous turns.
    deadline: Option<Instant>,
    /// Retains damage while the settle window suppresses a refresh tick.
    damage: bool,
    /// The core's injected-time settle rule is identical on all targets.
    settle: puzzle_term::session::presentation::SettleWindow,
    /// Measured rAF spacing supplies the period, not a separate pacing rule.
    period: std::time::Duration,
    /// Recent ordinary frame CPU excludes glyph publication from its own allowance.
    ordinary_costs: VecDeque<std::time::Duration>,
    /// Last observed refresh callback used to estimate a bounded period.
    tick: Option<Instant>,
    /// PTY resize is sent fifty milliseconds after the last observer event.
    pty_due: Option<Instant>,
    /// Current physical surface size is independent of fixed grid dimensions.
    pixels: [u32; 2],
    /// Explicit rows/columns opt out of automatic container fitting.
    fixed_grid: Option<[u16; 2]>,
}

/// Shared pointer exists only on the browser's single main thread (R9).
pub(crate) type Owner = Rc<RefCell<Runtime>>;

/// A missing browser global is a structured public error, not a panic.
pub(crate) fn window() -> Result<web_sys::Window, JsValue> {
    web_sys::window().ok_or_else(|| value::error("InvalidArgument", "window is unavailable"))
}

/// Initial host construction is separate from resource and DOM mounting.
impl Runtime {
    /// Creates a parser with deferred I/O; render resources arrive separately.
    pub(crate) fn new(config: puzzle_term::SessionConfig) -> Owner {
        let mut services = puzzle_term::platform::Services::new(
            Box::<services::DeferredTransport>::default(),
            Box::<services::DeferredWaker>::default(),
        );
        services.clipboard = Box::<services::DeferredClipboard>::default();
        services.urls = Box::<services::DeferredUrl>::default();
        let mut session = Session::new(config, services);
        session.install_model_overlays();
        if let Err(reason) = session.enable_host_events() {
            tracing::warn!(event = "web.event_registration_failed", reason);
        }
        Rc::new(RefCell::new(Self {
            #[cfg(any(test, feature = "measurements"))]
            measurements: Default::default(),
            session,
            render: Default::default(),
            results: Default::default(),
            disposed: false,
            flushing: false,
            data: Vec::new(),
            events: Vec::new(),
            transport: None,
            transport_id: 0,
            socket: None,
            exit: None,
            next_id: 0,
            parsed: crate::transport::parsed::Parsed::default(),
            waiting: VecDeque::new(),
            reply_routes: VecDeque::new(),
            continuation: None,
            started: Instant::now(),
            text: None,
            surface: None,
            dom: None,
            parent: None,
            settings: Default::default(),
            store: Default::default(),
            reduced_motion: false,
            dpr: 1.0,
            visible: true,
            resize: None,
            raf: None,
            draw: None,
            timer: None,
            timer_id: None,
            deadline: None,
            damage: false,
            settle: puzzle_term::session::presentation::SettleWindow::new(config.render_settle),
            period: std::time::Duration::from_micros(16_667),
            ordinary_costs: VecDeque::new(),
            tick: None,
            pty_due: None,
            pixels: [800, 480],
            fixed_grid: None,
        }))
    }
    /// Public calls after disposal report one stable error code.
    pub(crate) fn ensure_live(&self) -> Result<(), JsValue> {
        if self.disposed {
            Err(value::error("Disposed", "terminal was disposed"))
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
/// Actual browser promises, native DOM dispatch and public API methods.
mod tests;
