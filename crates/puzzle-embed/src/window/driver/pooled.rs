//! Per-surface state borrows one shared text bundle for each actor turn (D-30).
//!
//! The original RenderApp scheduler remains the sole pacing implementation.
//! Joining/splitting only transfers fields; snapshots and row caches are never
//! copied between surfaces. Font/DPR policy changes select a different context.
use super::*;
use crate::render_thread::{
    host::HostView,
    resources::{Lease, Prepared, Resources},
};
use crate::window::live::{
    LiveGpu, LiveResources,
    loan::{Shared, SurfaceState},
};
use crate::{
    recording::Recorder,
    session_thread::{Frames, Handle},
    settings::DesktopSettings,
};

/// Native startup values wait for the first actor turn, never borrowing host objects.
pub(crate) struct Initial {
    /// Native layer contents replace drawable acquisition on macOS product surfaces.
    #[cfg(target_os = "macos")]
    pub presentation: Option<crate::window::live::presentation::contents::Native>,
    /// Copied geometry and queue-only host action sink.
    pub host: HostView,
    /// Reserved left UI columns selected before child startup.
    pub gutter_columns: u8,
    /// Safe native owner is retained inside this target through shutdown.
    pub surface: wgpu::Surface<'static>,
    /// Copied font/device incarnation from cold resource preparation.
    pub prepared: Prepared,
    /// Host's true creation timestamp preserves the startup budget.
    pub created: Instant,
    /// Configure precedes shell spawn and initial snapshot publication.
    pub configured: Instant,
    /// Validated per-surface settings never alias the host store.
    pub settings: DesktopSettings,
    /// Priority input sender, with no live Session borrow.
    pub session: Handle,
    /// Sealed publication receiver transfers once to the rendering actor.
    pub frames: Frames,
    /// Optional ordered private evidence retains original byte/read boundaries.
    pub recorder: Option<Recorder>,
}
/// The same scheduler, with its resource loan absent between turns.
pub(crate) struct Pooled {
    /// Context lifetime includes partially completed cold startup.
    lease: Lease,
    /// Exactly one of initial/state is present, before/after first assembly.
    initial: Option<Initial>,
    /// Surface-specific geometry and row caches survive every text loan.
    state: Option<SurfaceState>,
    /// Existing scheduling code runs only while live has been joined.
    app: RenderApp,
    /// Device replacement is adopted once per surface.
    epoch: u64,
    /// This target's GPU attachment format is independent of its window size.
    format: wgpu::TextureFormat,
}
impl Pooled {
    /// Starts with copied font metrics, without loading a second font system.
    pub(crate) fn new(initial: Initial, lease: Lease, perf: perf::Counters) -> Self {
        // Settle time belongs to the surface policy, even when fonts are shared.
        // Copy it before moving startup values into the deferred first turn.
        let settle = Duration::from_millis(u64::from(
            initial.settings.store.effective().render_settle_ms,
        ));
        // Epoch distinguishes device loss from a font-policy context change.
        let epoch = initial.prepared.epoch;
        let format = initial.prepared.format;
        // No bundle is checked out here: a second surface may prepare before this turn.
        Self {
            lease,
            initial: Some(initial),
            state: None,
            epoch,
            format,
            // Keep the original scheduler state so pooling cannot invent another pacer.
            // live is populated only during a serialized actor call.
            app: RenderApp {
                live: None,
                timer_due: None,
                grid_pending: false,
                #[cfg(target_os = "macos")]
                geometry_deadline: None,
                pty_resize_due: None,
                perf,
                pacer: None,
                render_settle: settle,
                #[cfg(feature = "lock-metrics")]
                observer: None,
            },
        }
    }
    /// Authored probes observe the existing scheduler and cannot replace its policy.
    #[cfg(feature = "lock-metrics")]
    pub(crate) fn with_observer(
        mut self,
        observer: Box<dyn super::super::observer::SurfaceObserver>,
    ) -> Self {
        // Observers retain authored workloads outside ordinary hosts.
        self.app.observer = Some(observer);
        self
    }
    /// Policy is compared only on the rare settings or scale message, not per frame.
    fn select(&mut self, updates: &Pending, resources: &mut Resources) -> Result<bool, String> {
        // Settings can arrive before the initial driver turn; use its startup policy too.
        let (current_settings, current_info) = if let Some(state) = &self.state {
            (&state.settings, state.window.info)
        } else if let Some(initial) = &self.initial {
            (&initial.settings, initial.host.info)
        } else {
            return Err("pooled surface has no policy state".into());
        };
        // Pure geometry moves retain the same atlas, including cross-display moves.
        // Only real scale changes or explicit settings can select a different key.
        if updates.settings.is_none()
            && updates
                .geometry
                .is_none_or(|info| info.scale == current_info.scale)
        {
            return Ok(false);
        }
        // Latest policy wins without first mutating the old shared bundle.
        let settings = updates.settings.as_deref().unwrap_or(current_settings);
        // The pending scale and pending settings form one atomic selection key.
        let scale = updates
            .geometry
            .map_or(current_info.scale, |info| info.scale) as f32;
        // Selecting retains a reservation before the prior one can reach zero.
        // Even a key hit increments users, so release below balances every path.
        let prepared = resources.select(settings, scale, self.format)?;
        // Row caches need invalidation only if atlas identity actually changed.
        let changed = self.lease.key != prepared.key;
        // select retains one reservation; release the old one even on a key hit.
        resources.release(self.lease.key);
        // Retarget the eventual drop message to the newly retained context.
        // The old context may now be freed if this was its final user.
        self.lease.key = prepared.key;
        Ok(changed)
    }
    /// Loan errors close only this target and preserve cancellation on the App owner.
    fn run(
        &mut self,
        updates: Pending,
        now: Instant,
        resources: &mut Resources,
    ) -> Result<Option<Instant>, String> {
        // Recovery runs with every bundle returned, never during another surface loan.
        // The new epoch is visible to all siblings on their next turn.
        resources.recover()?;
        // Row caches need invalidation only if atlas identity actually changed.
        let changed = self.select(&updates, resources)?;
        // This move is exclusive; the registry cannot lend the same bundle twice.
        // Instance arrays are reusable context storage, while row caches remain per surface.
        let shared = resources.take(self.lease.key, self.format)?;
        let mut live = if let Some(initial) = self.initial.take() {
            // Retain startup settings until apply_updates compares them with a queued
            // reload. The selected shared bundle already carries the latest budget.
            initial.join(shared)
        } else {
            // Subsequent turns restore the same snapshot, geometry and row-cache owners.
            self.state
                .take()
                .ok_or("pooled surface has no frame state")?
                .join(shared)
        };
        // A format change alone retains the context; a lost device replaces its handles.
        let restored = self.epoch != resources.epoch();
        if changed || restored {
            // Old pending bytes name the old atlas incarnation; row caches must rebuild.
            live.adopt_context(restored);
            self.app.grid_pending = true;
        }
        // A sibling may already have restored the shared GPU. Adopt that incarnation once.
        self.epoch = resources.epoch();
        if restored {
            // Previous drawable timing cannot calibrate a replacement surface/device.
            self.app.pacer = None;
        }
        // Original callbacks now see the loan exactly as they saw the old window owner.
        self.app.live = Some(live);
        // This calls the original single-surface timing and submission implementation.
        let due = self.app.advance_prepared(updates, now);
        // advance retains live on ordinary completion; close is a host action.
        // A missing value indicates an ownership defect, never an idle frame.
        let live = self
            .app
            .live
            .take()
            .ok_or("surface driver discarded its text loan")?;
        // Return before the actor advances another surface, including an occluded one.
        // Moving these vectors preserves capacities and pending row-cache contents.
        let (state, shared) = live.split();
        self.state = Some(state);
        // The atlas upload executor follows its text context, never the row cache.
        // The next sibling can sample glyphs uploaded by this surface immediately.
        resources.put(self.lease.key, shared);
        Ok(due)
    }
}
impl Initial {
    /// Creates only per-surface frame caches, reusing the context's GPU/text owners.
    fn join(self, shared: Shared) -> LiveWindow {
        // Reassemble existing constructors without creating a second GPU or callback.
        // Renderer clones share the device/queue handles selected by Resources.
        let gpu = LiveGpu {
            window_created: self.created,
            surface_configured: self.configured,
            renderer: shared.renderer,
            gpu_errors: shared.gpu_errors,
            frame_renderer: shared.frame_renderer,
        };
        // The font service and atlas move together, preserving producer/device identity.
        // A Session receives metrics only and cannot access any of these owners.
        let resources = LiveResources {
            font_service: shared.font_service,
            font_wake: shared.font_wake,
            text_context: shared.text_context,
            settings: self.settings,
        };
        // Supplying the shared loss flag avoids replacing the process device callback.
        // Native host ownership stays retained by the safe wgpu surface.
        let mut live = LiveWindow::new_with_loss(
            self.host,
            self.surface,
            gpu,
            resources,
            self.session,
            self.frames,
            self.recorder,
            Some(shared.device_lost),
        );
        // Native image ownership is per surface; lending a text context must
        // never replace another pane's currently displayed IOSurface.
        live.gutter_columns = self.gutter_columns;
        #[cfg(target_os = "macos")]
        {
            live.presentation = self.presentation;
        }
        live
    }
}
impl Driver for Pooled {
    /// The mailbox outlives each resource loan, exactly like the native target observations.
    fn attach_updates(&mut self, updates: std::sync::Weak<std::sync::Mutex<Pending>>) {
        // Registration precedes first assembly; HostView then moves with surface state.
        if let Some(initial) = &mut self.initial {
            initial.host.attach_updates(updates);
        }
    }
    /// Actor resource-aware dispatch is mandatory for this driver.
    fn advance(&mut self, _updates: Pending, _now: Instant) -> Option<Instant> {
        None
    }
    /// The sole actor lends resources before invoking the unchanged scheduler.
    fn advance_shared(
        &mut self,
        updates: Pending,
        now: Instant,
        resources: &mut Resources,
    ) -> Option<Instant> {
        // Fail this target without unwinding the sole actor used by sibling windows.
        match self.run(updates, now, resources) {
            Ok(due) => due,
            Err(error) => {
                tracing::error!(event="embed.surface_turn_failed", %error);
                // Closing posts to App::tick; this thread never calls window toolkit code.
                if let Some(state) = &self.state {
                    state.window.close();
                } else if let Some(initial) = &self.initial {
                    initial.host.close();
                }
                None
            }
        }
    }
    /// Link subscription uses the same copied per-surface demand as ordinary drivers.
    #[cfg(target_os = "macos")]
    fn refresh_display(&self, pending: &Pending) -> Option<u32> {
        // Subscription sees copied demand while no text bundle is checked out.
        // The same predicate keeps a continuously producing Session on its link.
        if let Some(state) = &self.state {
            state.refresh_display(pending, self.app.pacer.as_ref(), self.app.timer_due)
        } else {
            // First assembly needs a tick even before any publication has reached this driver.
            self.initial
                .as_ref()
                .and_then(|initial| u32::try_from(initial.host.info.display).ok())
        }
    }
}
