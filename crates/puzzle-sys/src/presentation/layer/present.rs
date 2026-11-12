//! One CA transaction pairs the themed backing with matching immutable contents.
//!
//! The content image and layer bounds have independent lifetimes. During resize,
//! the old image remains unscaled while the child already has its new bounds.
//! Its background fills only the uncovered space with the default cell colour.
//! Both properties are assigned inside one disabled-actions transaction, so CA
//! cannot animate either the crop or the backing between unrelated geometries.
//!
//! The render worker publishes only immutable, GPU-completed image ownership.
//! Main is the only owner allowed to assign layer contents or a native colour.
//! A ready image must match both actual physical bounds and backing scale.
//! A different-size image cannot certify this transaction, even if it is newer.
//! Host geometry is authoritative while CA layout catches up after reparenting.
//!
//! There are two callers: CA display and the host's ordinary asynchronous tick.
//! Only CA display may spend the one geometry-change waiting credit (D-60).
//! An unanswered request retains that credit after timeout; newer sizes still
//! post asynchronously, and the render mailbox coalesces their numeric facts.
//! Pool preflight can decline waiting when the compositor owns every target.
//! The main transaction must then return so those native references can retire.
//!
//! Waiting uses one absolute display-period deadline. Spurious wakeups never
//! extend it, and failure keeps the previous image anchored at visual top left.
//! No timeout drops pending geometry or suppresses eventual asynchronous delivery.
//! A ready image already matching bounds bypasses the wait and any new request.
//! Image serials identify submissions, independently of display-link indices.
//!
//! Assignment receipts carry cursor/hit regions for exactly the adopted image.
//! Callback-activity receipts carry no image and cannot emit presentation events.
//! They measure residence inside this native callback, including the bounded wait.
//! Time with no OS callback is excluded from the main-thread busy counter.
//! Trace-only display results distinguish new assignments from retained images.
//! These observations never select deadlines, pool depth or presentation policy.
use super::layout::{binding, geometry};
use super::*;
use crate::presentation::{
    Geometry, Receipt,
    mailbox::{take_ready, wait_deadline},
};
use std::time::{Duration, Instant};

impl PresentationLayer {
    /// Matching ready images are committed directly in this callback's transaction.
    pub(super) fn present(&self, allow_wait: bool) {
        if self.ivars().shared.lock().closed {
            return;
        }
        let started = Instant::now();
        // Background and contents change in the same transaction. On timeout,
        // newly exposed bounds still have the active default cell background.
        CATransaction::begin();
        CATransaction::setDisableActions(true);
        self.apply_background();
        let assigned = self.present_frame(allow_wait);
        CATransaction::commit();
        if let Some(requested) = geometry(self) {
            let committed = self.ivars().committed.get();
            tracing::trace!(target:"puzzle_desktop::resize",event="native.display_result",
                display=allow_wait,assigned,exact=committed==Some(requested),?requested,?committed);
            // CA can call display from its own tracking transaction, outside
            // every winit handler. Report that residence separately, not gaps.
            (self.ivars().observe)(Receipt {
                serial: None,
                requested,
                committed,
                main_busy: Some(started.elapsed()),
                waited: Duration::ZERO,
                timed_out: false,
            });
        }
    }
    /// Image admission retains D-60 exactly; background/telemetry wrap this work.
    fn present_frame(&self, allow_wait: bool) -> bool {
        let ivars = self.ivars();
        if !self.refresh_gravity() {
            return false;
        }
        let Some(geometry) = self.current_geometry() else {
            return false;
        };
        // Measure the whole opportunity, including posting its geometry request.
        // The request cannot extend the absolute bound if rendering starts late.
        let started = Instant::now();
        // Already-finished work needs no new request or waiting credit.
        if let (Some(ready), _) = take_ready(&ivars.shared, geometry, None) {
            self.commit(ready, geometry, Duration::ZERO, false);
            return true;
        }
        let deadline = self.request_geometry(geometry, allow_wait, started);
        let wait = deadline.is_some();
        let (ready, timed_out) = take_ready(&ivars.shared, geometry, deadline);
        tracing::trace!(target:"puzzle_desktop::resize",event="native.display_answer",?geometry,
            wait,waited_ms=started.elapsed().as_secs_f64()*1000.,commit=ready.is_some(),timed_out);
        if let Some(ready) = ready {
            self.commit(
                ready,
                geometry,
                if wait {
                    started.elapsed()
                } else {
                    Duration::ZERO
                },
                timed_out,
            );
            return true;
        } else if wait {
            // A timeout is observable rather than a fabricated successful frame.
            // Keeping old contents also retains their pool ownership until replaced.
            (ivars.observe)(Receipt {
                serial: None,
                main_busy: None,
                requested: geometry,
                committed: ivars.committed.get(),
                waited: started.elapsed(),
                timed_out,
            });
        }
        false
    }
    /// A geometry chain has one waiting credit until its completion reaches main.
    fn request_geometry(
        &self,
        geometry: Geometry,
        allow_wait: bool,
        started: Instant,
    ) -> Option<Instant> {
        let ivars = self.ivars();
        // Waiting is optional; delivery is not. Even ordinary ticks must renew
        // an unmet geometry after preflight, a stale completion or a transfer.
        let admission = {
            let mut state = ivars.shared.lock();
            if state.closed || state.desired.is_some_and(|wanted| wanted != geometry) {
                // CA layout can lag the explicit host update by a transaction.
                // Do not replace its newer render extent with these old bounds.
                None
            } else {
                let writable = !state.pool_blocked;
                state
                    .requests
                    .opportunity(ivars.committed.get(), geometry, allow_wait, writable)
            }
        };
        let due = wait_deadline(
            ivars.committed.get(),
            geometry,
            admission.is_some(),
            started,
            ivars.shared.rate.load(std::sync::atomic::Ordering::Acquire),
        );
        tracing::trace!(target:"puzzle_desktop::resize",event="native.geometry_request",?geometry,?admission);
        let deadline = due.filter(|_| admission == Some(true));
        if let Some(due) = due {
            // Latest sizes still reach rendering without parking main behind an
            // older unanswered request. No native or mailbox lock crosses the send.
            (ivars.request)(geometry, due);
        }
        deadline
    }
    /// Apply host pixels before image admission; CA bounds cannot veto that key.
    /// Initial native bounds are used only before any explicit host geometry.
    fn current_geometry(&self) -> Option<Geometry> {
        let ivars = self.ivars();
        // Both render admission and native layout use this same physical key.
        // A stale/rounded CA extent must never gate the authoritative host frame.
        let wanted = ivars.shared.lock().desired;
        if let Some(wanted) = wanted {
            if wanted.size.contains(&0) {
                return None;
            }
            super::layout::apply_host(self, wanted);
            return Some(wanted);
        }
        // Before the first explicit geometry (only), retain the initial native size.
        let scale = f64::from_bits(
            ivars
                .shared
                .scale
                .load(std::sync::atomic::Ordering::Acquire),
        );
        if self.contentsScale() != scale {
            self.setContentsScale(scale);
            // A backing-scale change need not also change logical layer bounds.
            // Schedule CA display so it receives the same D-60 geometry opportunity.
            self.setNeedsDisplay();
        }
        geometry(self)
    }
    /// Assigns a completed image and acknowledges its exact presentation identity.
    #[allow(
        unsafe_code,
        reason = "R8/D-60: IOSurface toll-free bridging for CALayer contents"
    )]
    fn commit(
        &self,
        ready: crate::presentation::Ready,
        geometry: Geometry,
        waited: Duration,
        timed_out: bool,
    ) {
        let ivars = self.ivars();
        // The current display transaction now has both the new bounds and a
        // finished matching-size image. Disable actions so CA does not animate
        // between texture sizes or interpolate a transient stretched image.
        CATransaction::begin();
        CATransaction::setDisableActions(true);
        // SAFETY: IOSurfaceRef is toll-free bridged to __IOSurface on supported
        // macOS versions. ready retains it throughout setContents, which retains it.
        let object = unsafe { &*(&*ready.slot.image.surface as *const _ as *const AnyObject) };
        // SAFETY: main-thread CALayer accepts an IOSurface as contents; its
        // exact size matches the bounds/scale key verified above, GPU writes finished.
        self.setContentsRect(crate::presentation::geometry::crop(
            geometry.size,
            ready.slot.image.size,
        ));
        // SAFETY: the retained IOSurface object has the documented contents type.
        unsafe { self.setContents(Some(object)) };
        CATransaction::commit();
        // Only an actual contents assignment advances committed geometry.
        // A resize notification, build or GPU submission cannot do so.
        ivars.committed.set(Some(geometry));
        // Capture exactly the assigned image/crop, not a later completed frame.
        ivars.snapshot.presented(Arc::clone(&ready.slot), geometry);
        {
            let mut state = ivars.shared.lock();
            // The image may have completed before the matching request was sent.
            // Its actual adoption still answers that geometry, returning credit.
            let ordinal = state.requests.ordinal(geometry);
            state.requests.complete(ordinal);
            state.current = Some(ready.slot);
        }
        (ivars.observe)(Receipt {
            serial: Some(ready.serial),
            main_busy: None,
            requested: geometry,
            committed: Some(geometry),
            waited,
            timed_out,
        });
    }
}

impl PresentationLayer {
    /// Reparented host layers can inherit another coordinate orientation at equal size.
    fn refresh_gravity(&self) -> bool {
        // Native host reparenting can change an ancestor without changing size.
        // Re-evaluate effective orientation, but avoid a property write on every
        // ordinary frame when the hierarchy has kept the same visual direction.
        let identity = binding(self);
        let moved = self.ivars().shared.lock().requests.bind(identity);
        if moved {
            // The contents stay visible, but an abandoned old-host request can
            // no longer suppress a fresh assignment in the new CA hierarchy.
            self.ivars().committed.set(None);
        }
        let flip = self.contentsAreFlipped();
        if self.ivars().flip.get() == Some(flip) {
            return true;
        }
        match crate::layer::set_layer(self) {
            Ok(_) => {
                self.ivars().flip.set(Some(flip));
                true
            }
            Err(error) => {
                tracing::error!(event="native.contents_gravity_failed",%error);
                false
            }
        }
    }
}
