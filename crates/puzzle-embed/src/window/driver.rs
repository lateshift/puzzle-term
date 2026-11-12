//! Render-thread driver for one native layer, independent of winit (D-41/D-43).
//!
//! Host geometry, display facts and damage enter through the actor mailbox.
//! D-50 publications and the D-33 refresh-source policy run exclusively here.
//! Input and OS callbacks have a separate owner and never invoke this driver.
//!
//! Each surface retains its own clock: moving one window to another display
//! must not retime another surface. The actor calls drivers in serial order.
//! Geometry replaces earlier drag samples; Session reflows the latest size.
//! PTY SIGWINCH takes that grid at first-event 25-ms deadlines, independently of FIFO.
//! Timer and damage updates are independent so a later blink request cannot
//! erase pending output. Unknown visibility remains a host observation.
//! Close is a posted notification; rendering never waits for the UI callback.
//! Recovery happens before surface configuration or clock calibration touches
//! the lost device. On failure, the host closes only this affected surface.
//! Empty drivers supply no deadline and consume no recurring actor wake.
use super::{live::LiveWindow, pacing::Pacer, perf};
use crate::render_thread::{Driver, surface::Pending};
use std::time::{Duration, Instant};
/// Snapshot deadlines, extent changes and optional glyph completion publication.
mod events;
/// Shared contexts are moving loans from the exclusive render actor.
pub(crate) mod pooled;
/// Earliest explicit timer, resize, calibration or frame deadline.
mod schedule;

/// Rendering state for a surface registered with the one process actor.
pub struct RenderApp {
    /// Surface, shared text context and reusable frame resources belong to this worker.
    pub live: Option<LiveWindow>,
    /// Earliest blink/sync/overlay request supplied by Session.
    pub timer_due: Option<Instant>,
    /// Latest forwarded grid applies once before the next snapshot.
    pub grid_pending: bool,
    /// Only a CA geometry callback may replace the normal snapshot receive deadline.
    #[cfg(target_os = "macos")]
    pub geometry_deadline: Option<Instant>,
    /// First-event 25-ms PTY throttle deadline, independent of surface configuration.
    pub pty_resize_due: Option<Instant>,
    /// Existing throughput and successful-presentation observations.
    pub perf: perf::Counters,
    /// Refresh prediction remains independent of the native toolkit.
    pub pacer: Option<Pacer>,
    /// User-configured D-33 settle duration.
    pub render_settle: Duration,
    /// Optional harness observer owns authored workloads, never refresh policy.
    #[cfg(feature = "lock-metrics")]
    pub observer: Option<Box<dyn super::observer::SurfaceObserver>>,
}

/// A surface’s cold updates remain serialized with its presentation clock.
impl RenderApp {
    /// Transfers startup resources onto the process rendering actor.
    pub fn new(live: LiveWindow, render_settle: Duration, perf: perf::Counters) -> Self {
        // Startup reserves no clock phase before copied monitor facts are installed.
        Self {
            live: Some(live),
            timer_due: None,
            grid_pending: false,
            #[cfg(target_os = "macos")]
            geometry_deadline: None,
            pty_resize_due: None,
            perf,
            pacer: None,
            render_settle,
            #[cfg(feature = "lock-metrics")]
            observer: None,
        }
    }

    /// Stops this surface on recovery failure while keeping host input independent.
    fn recover_device(&mut self) -> bool {
        // A loss callback publishes a flag; GPU replacement runs on this owner.
        if let Some(live) = &mut self.live {
            match live.recover_device() {
                // A replacement swapchain has no valid previous acquisition phase.
                Ok(true) => self.pacer = None,
                Ok(false) => {}
                Err(error) => {
                    tracing::error!(event = "desktop.device_restore_failed", %error);
                    // Post host close without entering toolkit operations from this thread.
                    live.window.close();
                    return false;
                }
            }
        }
        true
    }

    /// Updates physical geometry before a frame and re-learns phase only on display changes.
    fn geometry(&mut self, info: crate::render_thread::surface::SurfaceInfo) {
        let Some(live) = &mut self.live else {
            return;
        };
        // Compare copied values rather than querying OS display handles on the worker.
        let old = live.window.info;
        // Record cache identity before applying geometry, so a destructive reset is visible.
        tracing::trace!(target: "puzzle_desktop::resize", event="desktop.render_geometry", ?old, ?info,
            generation=live.text_context.atlas_generation(), cache=?live.text_context.grid_cache_metrics());
        live.window.geometry_changed(info);
        // Text hinting and metrics must follow scale before the next snapshot.
        if old.scale != info.scale {
            self.dpr(info.scale);
        }
        // Equal-rate display crossings still invalidate the learned phase.
        if old.millihertz != info.millihertz || old.display != info.display {
            self.init_pacer();
        }
        // Zero extents defer GPU configuration until a usable backing layer returns.
        if old.extent != info.extent && info.extent.width > 0 && info.extent.height > 0 {
            self.resize(info.extent);
        }
    }
    /// Applies cold host state and explicit deadlines before reserving a frame.
    fn apply_updates(&mut self, updates: &mut Pending) {
        // Focus and occlusion are host facts, never worker-thread OS queries.
        if let Some(visibility) = updates.visibility.take()
            && let Some(live) = &mut self.live
        {
            // Calibration diagnostics need the actual host focus observation.
            live.focused = visibility.focused;
            if let Some(occluded) = visibility.occluded {
                // Uncovering restores frame damage; unknown occlusion does not hide output.
                live.occluded(occluded);
            }
            live.window.visibility = visibility;
        }
        // Font work uses the last validated snapshot, leaving key policy on the host.
        if let Some(settings) = updates.settings.take()
            && let Some(live) = &mut self.live
        {
            let padding_changed = live.settings.store.effective().render_padding
                != settings.store.effective().render_padding;
            live.settings = *settings;
            live.apply_settings(padding_changed);
        }
        // These values arrive from Session's outbox, never a synchronous query.
        if let Some(live) = &mut self.live {
            if let Some(animating) = updates.animation.take() {
                live.animating = animating;
            }
            if let Some(title) = updates.title.take() {
                live.title = title;
            }
            if let Some(regions) = updates.regions.take() {
                live.regions = regions;
            }
        }
        // Earliest explicit deadline wins even while newer timers arrive.
        if let Some(due) = updates.timer {
            self.timer_due = Some(self.timer_due.map_or(due, |old| old.min(due)));
        }
        // Startup index publication needs no provisional-row invalidation. A
        // first fallback lookup waits once and presents complete ink in that frame.
    }
}

/// The process actor is the only caller of this per-surface scheduler.
impl Driver for RenderApp {
    /// Standalone drivers use the same crossing gate as actor-pooled surfaces.
    fn attach_updates(&mut self, updates: std::sync::Weak<std::sync::Mutex<Pending>>) {
        if let Some(live) = &mut self.live {
            live.window.attach_updates(updates);
        }
    }
    /// Closed, hidden and minimized surfaces detach from the native refresh source.
    ///
    /// This predicate reads only copied host facts and render-owned demand. The
    /// actor evaluates it before tick delivery and again after drawing, including
    /// damage queued during the build. Callback code never calls this predicate
    /// or reaches the native window. Unknown visibility remains eligible until
    /// the host supplies an authoritative hidden/occluded observation.
    #[cfg(target_os = "macos")]
    fn refresh_display(&self, pending: &Pending) -> Option<u32> {
        let live = self.live.as_ref()?;
        let visible = &live.window.visibility;
        let extent = live.window.info.extent;
        // A zero backing extent cannot acquire a drawable; subscribing would
        // waste refresh wakes until the host restores usable surface geometry.
        if visible.visible == Some(false)
            || visible.occluded == Some(true)
            || extent.width == 0
            || extent.height == 0
        {
            tracing::trace!(target: "puzzle_desktop::scheduling", event="desktop.unsubscribe_reason",
                reason="hidden_occluded_or_zero_extent", visible=?visible.visible, occluded=?visible.occluded, ?extent);
            return None;
        }
        // Damage posted during a build remains pending in the actor mailbox.
        // Keeping that subscription active avoids restarting between flood frames.
        // Taking an opportunity only starts its snapshot request. It cannot
        // acknowledge owner work or bytes that have not reached the drawable.
        // Those source facts keep the same link alive across late or suppressed turns.
        let needs = pending.frame
            || pending.output.is_some()
            || pending.fallback
            || live.animating
            || live.output_pending()
            || self.pacer.as_ref().is_some_and(Pacer::wants_refresh);
        if !needs {
            tracing::trace!(target: "puzzle_desktop::scheduling", event="desktop.unsubscribe_reason",
                reason="no_render_demand", pending_bytes=live.session.pending_bytes(),
                timer_pending=self.timer_due.is_some());
        }
        needs
            .then(|| u32::try_from(live.window.info.display).ok())
            .flatten()
    }

    /// All snapshot, build, glyph work, surface submission and calibration happens here.
    fn advance(&mut self, updates: Pending, now: Instant) -> Option<Instant> {
        // Standalone diagnostic drivers own their device; pooled surfaces recover
        // only through the registry before receiving their exclusive resource loan.
        if !self.recover_device() {
            return None;
        }
        self.advance_prepared(updates, now)
    }
}

/// Pooled and standalone surfaces share scheduling after their owner has recovered.
impl RenderApp {
    /// Advances a surface whose GPU resources were restored by their actual owner.
    ///
    /// A pooled surface must never recreate a device independently: a loss arriving
    /// during this turn is handled for every context by the registry on its next turn.
    pub(crate) fn advance_prepared(
        &mut self,
        mut updates: Pending,
        now: Instant,
    ) -> Option<Instant> {
        #[cfg(feature = "lock-metrics")]
        if let Some(live) = &mut self.live {
            live.frame_metrics.actor_wake(updates.actor_wake, now);
        }
        // A new surface or restored device needs the same D-33 startup exception.
        if self.pacer.is_none() {
            self.init_pacer();
        }
        // Only the latest physical geometry survives high-frequency native callbacks.
        if let Some(info) = updates.geometry {
            self.geometry(info);
            // Coalescing is measured from the host event, not a blocked acquisition's end.
            if self.grid_pending
                && let Some(arrived) = updates.geometry_at
            {
                self.arm_pty_resize(arrived);
            }
        }
        self.use_host_clock(updates.host_driven);
        self.apply_updates(&mut updates);
        // Animation uses refresh timing without the output-burst settle exception.
        if updates.frame || updates.fallback || updates.output.is_some() {
            let animation = self.live.as_ref().is_some_and(|live| live.animating);
            if let Some(pacer) = &mut self.pacer {
                if animation {
                    pacer.request_animation(now);
                } else if let Some(arrival) = updates.output {
                    // Suppressed output still informs silence detection, without VT locks.
                    pacer.request_output(arrival, now);
                } else {
                    pacer.request(now, false);
                }
            }
        }
        self.apply_refresh(&updates, now);
        #[cfg(target_os = "macos")]
        self.geometry_opportunity(updates.geometry_deadline, now);
        // Throughput counters are published atomics; Session ownership stays separate.
        if let Some(live) = &self.live {
            self.perf.report(
                now,
                live.session.parsed_total(),
                live.session.pending_bytes(),
            );
        }
        // Return the earliest useful wake; the process actor owns the actual sleep.
        self.schedule()
    }
}

/// Native refresh facts arrive through the same seam available to embedding hosts.
impl RenderApp {
    /// A queued old-display tick cannot pace a surface after a monitor crossing.
    /// Applying geometry first makes the identity check authoritative even when
    /// the old callback and the host's monitor-change event arrive in one turn.
    fn apply_refresh(&mut self, updates: &Pending, now: Instant) {
        let Some(tick) = updates.refresh else {
            return;
        };
        let Some(live) = self.live.as_ref() else {
            return;
        };
        if tick.display != live.window.info.display {
            tracing::trace!(target: "puzzle_desktop::refresh", event="desktop.refresh_wrong_display",
                tick_display=tick.display, surface_display=live.window.info.display, index=tick.index);
            return;
        }
        #[cfg(feature = "lock-metrics")]
        if let Some(observer) = &mut self.observer {
            if let Some((start, first)) = updates.display_restart {
                observer.display_restarted(start, first);
            }
            observer.refresh(tick, live);
        }
        #[cfg(feature = "lock-metrics")]
        if let Some(live) = &mut self.live {
            live.frame_metrics.refresh_source(tick);
        }
        if let Some(pacer) = &mut self.pacer {
            // A consumed opportunity does not acknowledge a late owner or queued
            // output. Rearm from those observations without an OS stop/start.
            pacer.output_pending(
                self.live.as_ref().is_some_and(|live| live.output_pending()),
                now,
            );
            // Animations stay subscribed and request one frame at every real tick.
            // Ending animation arrives as a change-only Session observation.
            if self.live.as_ref().is_some_and(|live| live.animating) {
                pacer.request_animation(now);
            }
            pacer.refreshed(tick, now);
            // Record the callback actually reserved, independently of scanout time.
            // This trace cannot create demand or change the link's idle-stop policy.
            if pacer.due() == Some(now) {
                tracing::trace!(target: "puzzle_desktop::restart",
                    event = "desktop.restart_tick_used", display = tick.display,
                    tick_index = tick.index,
                    callback_age_us = tick.callback.elapsed().as_secs_f64() * 1e6);
            }
        }
    }
}

/// Instrumented hosts may observe this same driver instead of replacing its loop.
#[cfg(feature = "lock-metrics")]
impl RenderApp {
    /// Attach an authored finite workload before transferring the driver to the actor.
    pub fn with_observer(mut self, observer: Box<dyn super::observer::SurfaceObserver>) -> Self {
        // The observer is transferred with this driver, so it cannot race a
        // readback against another thread or retain a borrowed drawable.
        self.observer = Some(observer);
        self
    }
}
