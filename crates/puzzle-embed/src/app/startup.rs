//! Native surface and Session assembly moved from the harness (M17.embed).
//!
//! Physical extent and measured glyph metrics establish PTY dimensions before
//! the child can print. Appearance and Session policy are installed before the
//! fixed-pool reader's bytes are parsed. No toolkit is referenced by this path.
//!
//! Startup transfers three distinct lifetimes rather than placing a Session
//! behind a host mutex. The native surface owns a retained view, the pipeline
//! owns the child and every join barrier, and the driver owns frame assembly.
//! Their checked input endpoint contains no borrow of any of those resources.
//!
//! This ordering also covers partial failure. Before spawning a child, font
//! and GPU errors can return directly. Once IO starts, Pipeline immediately
//! owns cancellation. Once the Session starts, its join handle is attached
//! before registration can fail. Dropping a failed registration therefore
//! kills the child, wakes the owner and joins the same workers as normal close.
//!
//! The render actor receives the snapshot receiver exactly once. The host
//! keeps its event receiver; neither can consume the other's queue. This is
//! the ownership reason title/clipboard delivery cannot stall frame building
//! and why a GPU wait cannot delay input encoding on the Session thread.
//!
//! Surface creation may perform cold native initialization. Subsequent methods
//! never recreate this assembly: resize, focus, input and settings are copied
//! updates. A native target must outlive its wgpu surface; safe Arc targets
//! enforce that lifetime even if the host closes its own window collection.
//!
//! Recordings keep the true startup geometry and revision before the first
//! reader byte. They remain opt-in and private because they include everything
//! the shell prints and every input byte. Diagnostic observers are likewise
//! opt-in; their authored workloads are absent from ordinary host surfaces.
//!
//! Render-settle duration comes from the validated per-surface snapshot. It
//! cannot acquire a second default while moving through the embedding seam.
//! Main-thread callback delivery remains centralized in App::tick even for
//! effects produced during these earliest Session initialization turns.
use super::*;
use crate::render_thread::{
    host::{HostView, Visibility},
    surface::SurfaceInfo,
};
use crate::{
    session_thread,
    window::{
        driver::pooled::{Initial, Pooled},
        live, perf,
    },
};
use puzzle_term::Session;
use std::time::Instant;

impl App {
    /// Attaches a safe native target; subsequent operations only enqueue messages.
    pub fn create_surface(
        &mut self,
        native: impl Into<wgpu::SurfaceTarget<'static>>,
        info: SurfaceInfo,
        visibility: Visibility,
        options: SurfaceOptions,
    ) -> Result<Surface, ApiError> {
        // Diagnostic counters are not part of the native embedding contract.
        self.create_surface_inner(
            |instance| {
                instance
                    .create_surface(native)
                    .map_err(|error| error.to_string())
            },
            info,
            visibility,
            options,
        )
        .map(|(surface, _)| surface)
    }
    /// Harness-only creation counters; not host API and available only with test-support.
    #[cfg(feature = "test-support")]
    pub fn create_surface_with_diagnostics(
        &mut self,
        native: impl Into<wgpu::SurfaceTarget<'static>>,
        info: SurfaceInfo,
        visibility: Visibility,
        options: SurfaceOptions,
    ) -> Result<(Surface, perf::Counters), ApiError> {
        self.create_surface_inner(
            |instance| {
                instance
                    .create_surface(native)
                    .map_err(|error| error.to_string())
            },
            info,
            visibility,
            options,
        )
    }
    /// Attaches a host-created target using this App's shared instance (D-30).
    ///
    /// The factory runs on the host thread before any child starts. A C adapter can
    /// establish its borrowed native-layer contract at this boundary; safe Rust
    /// hosts normally use create_surface with an owned window instead.
    pub fn create_surface_from(
        &mut self,
        factory: impl FnOnce(&wgpu::Instance) -> Result<wgpu::Surface<'static>, String>,
        info: SurfaceInfo,
        visibility: Visibility,
        options: SurfaceOptions,
    ) -> Result<Surface, ApiError> {
        // This constructor changes only native attachment, not pipeline ownership.
        // The same cold failure cleanup and runtime affinity checks apply below.
        self.create_surface_inner(factory, info, visibility, options)
            .map(|(surface, _)| surface)
    }

    /// Creates a surface on the one App instance before transferring it to rendering.
    ///
    /// Pass an Arc of a host window (implementing wgpu::WindowHandle), copied backing
    /// extent/scale/display facts, and process options. Creation is a cold operation;
    /// all subsequent Surface input and resize methods enqueue without waiting.
    fn create_surface_inner(
        &mut self,
        factory: impl FnOnce(&wgpu::Instance) -> Result<wgpu::Surface<'static>, String>,
        info: SurfaceInfo,
        visibility: Visibility,
        mut options: SurfaceOptions,
    ) -> Result<(Surface, perf::Counters), ApiError> {
        // Native handles and cold startup belong to the same thread as App::tick.
        self.check_thread()?;
        // Reject invalid scale before making a GPU surface or starting a shell.
        super::surface::validate(info)?;
        // Failed startups consume an ID too: a delayed event must never address
        // another surface that happened to reuse its attempted creation number.
        let id = self.next_id()?;
        let cold = super::creation::Cold::new(self, id, factory, info, visibility, &options)?;
        let (grid, gutter_columns) = startup_grid(&options, &cold.settings, &cold.prepared, info)?;
        let (started, pipeline) = self.start_pipeline(
            id,
            info,
            &cold.settings,
            &options,
            cold.prepared,
            (grid, cold.visibility()),
        )?;
        // Geometry is ordered before the first render request; no provisioning state is read.
        started
            .handle
            .send(puzzle_term::SessionMessage::GutterColumns(gutter_columns))?;
        cold.finish(self, id, gutter_columns, started, pipeline, &mut options)
    }

    /// Retains the Session join handle before a later driver registration can fail.
    pub(super) fn start_session(
        &self,
        session: Session,
        pool: crate::pty_pool::Consumer,
        exit: std::sync::mpsc::Receiver<puzzle_term::messages::ExitInfo>,
        recorder: Option<crate::recording::Recorder>,
    ) -> Result<session_thread::Started, String> {
        // The fixed pool may already hold output; attachment gives one owner
        // exclusive Session access before the first host input can be submitted.
        let started = session_thread::start(
            session,
            pool,
            session_thread::Config {
                exit: Some(exit),
                notify: Some(Arc::clone(&self.config.wakeup)),
                recorder,
                // Decode jobs share the App worker instead of spawning one per surface.
                decoder: self.decoder.sender(),
            },
        )?;
        Ok(started)
    }

    /// Native callbacks become queue entries, not host function calls from rendering.
    pub(super) fn host_view(
        &self,
        id: SurfaceId,
        info: SurfaceInfo,
        visibility: Visibility,
    ) -> HostView {
        let posts = self.posts.clone();
        let wake = Arc::clone(&self.config.wakeup);
        // HostView contains copied facts and a queue-only action sink, no toolkit.
        HostView::new(
            info,
            visibility,
            Arc::new(move |action| {
                // Enqueue before waking so an immediately draining host sees the action.
                if posts
                    .send(Posted::Event(id, AppEvent::Render(action)))
                    .is_ok()
                {
                    wake();
                }
            }),
        )
    }
}

/// Diagnostic observation wraps the same pooled driver used by normal host surfaces.
pub(super) fn driver(
    initial: Initial,
    lease: crate::render_thread::resources::Lease,
    options: &mut SurfaceOptions,
) -> (Pooled, perf::Counters) {
    // Taking the observer transfers only its ownership, never the process options.
    // Ordinary hosts use this assembly without creating diagnostic state.
    let _ = options;
    let perf = perf::Counters::new(Instant::now());
    let driver = Pooled::new(initial, lease, perf.clone());
    #[cfg(feature = "lock-metrics")]
    let driver = if let Some(observer) = options.observer() {
        driver.with_observer(observer)
    } else {
        driver
    };
    (driver, perf)
}

// Additional grid columns are chosen only by the cold extension policy.
include!("startup/selected.rs");

/// Establish reduced PTY geometry and its recording before any child output.
fn startup_grid(
    options: &SurfaceOptions,
    settings: &crate::settings::DesktopSettings,
    prepared: &crate::render_thread::resources::Prepared,
    info: SurfaceInfo,
) -> Result<(puzzle_term::platform::PtySize, u8), ApiError> {
    // Reserve UI before the PTY starts: its first size must match rendering.
    // A registration alone is not approval and must not change a shell's grid.
    let gutter_columns = producer_gutter(options);
    let grid = live::grid_with_gutter(
        info.extent,
        prepared.metrics.cell,
        live::padding_for(settings, prepared.metrics.dpr),
        gutter_columns,
    );
    // Recording metadata precedes every read, including output emitted at spawn.
    record::surface(options, grid, info, settings)?;
    Ok((grid, gutter_columns))
}
