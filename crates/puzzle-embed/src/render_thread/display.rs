//! One App actor's detachable per-display subscriptions (D-53).
//!
//! Native callbacks own only the bounded wake sender. Surface demand is read
//! between frames, and closing/moving a surface updates aggregate counts before
//! the actor sleeps. Idle displays stop; sibling surfaces reuse the same source.
//! The map grows only when a distinct display is seen, never per frame.
//! puzzle-sys combines demand across Apps; this actor's detach cannot stop a
//! display still used by another App, and their refresh readers are independent.
//! Subscriber counts are recomputed, not incrementally patched from window events:
//! a removal, hidden window or display crossing therefore cannot leave a stale
//! subscription keeping the old display awake. Only changed counts reach the OS.
use super::{Entry, SurfaceId, surface::Refresh};
use puzzle_sys::display_link::DisplayLinks;
use std::{collections::BTreeMap, sync::mpsc};

/// Reused source bookkeeping; no surface reference escapes into CoreVideo.
struct Demand {
    /// Count recomputed from visible surfaces and queued damage each actor turn.
    count: usize,
    /// Last successfully applied subscription count avoids redundant controls.
    active: usize,
    /// Latest tick is distributed to every subscribed surface in this turn.
    tick: Option<Refresh>,
    /// First callback and start entry accompany only this restart's first delivery.
    #[cfg(feature = "lock-metrics")]
    restart: Option<(std::time::Instant, std::time::Instant)>,
    /// Repeated unplugged-display failures must not flood the user's log.
    warned: bool,
}

/// This actor owns logical demand; native links are shared across all live Apps.
pub(super) struct Displays {
    /// Safe foreign glue contains no toolkit or rendering object.
    links: DisplayLinks,
    /// Cold display discovery alone adds a node; warm turns reuse all entries.
    demand: BTreeMap<u32, Demand>,
}

/// The callback never borrows this actor's local surface or demand bookkeeping.
impl Displays {
    /// Wakes exactly the actor's independent inbox, including during nested GPU waits.
    pub(super) fn new(wake: mpsc::SyncSender<()>) -> Self {
        Self {
            links: DisplayLinks::new(move || {
                // One queued token coalesces all surfaces and callbacks; no allocation.
                let _ = wake.try_send(());
            }),
            demand: BTreeMap::new(),
        }
    }

    /// Captures native facts once before the actor distributes this turn's updates.
    pub(super) fn receive(&mut self) {
        // Missing publications remain None; this operation supplies no timer and
        // cannot cause a frame unless a source actually called the actor's wake.
        for (&display_id, demand) in &mut self.demand {
            // A restart receipt belongs to one native delivery, so clear it even
            // on a turn without a tick; otherwise probes would count it twice.
            #[cfg(feature = "lock-metrics")]
            {
                demand.restart = None;
            }
            demand.tick = self.links.take_refresh(display_id).map(|tick| {
                // Keep the original monotonic instants: probe origins can fall
                // between start and callback, and elapsed durations lose that order.
                #[cfg(feature = "lock-metrics")]
                {
                    demand.restart = tick.restart;
                }
                // Cold activation evidence is independent of frame reservation.
                // All periodic observations remain trace-only (R10).
                if let Some((start, first)) = tick.restart {
                    tracing::trace!(target: "puzzle_desktop::restart",
                        event = "desktop.display_restart", display_id,
                        start_to_callback_us = first.saturating_duration_since(start).as_secs_f64() * 1e6,
                        first_callback_age_us = first.elapsed().as_secs_f64() * 1e6,
                        tick_index = tick.index);
                }
                // Pair callback age with the log timestamp to align all display-switch clocks.
                tracing::trace!(target:"puzzle_desktop::resize",event="desktop.display_tick",
                    display_id,index=tick.index,period_us=tick.period.as_secs_f64()*1e6,
                    callback_age_us=tick.callback.elapsed().as_secs_f64()*1e6);
                Refresh {
                    display: u64::from(display_id),
                    index: tick.index,
                    output: tick.output,
                    period: tick.period,
                    callback: tick.callback,
                }
            });
        }
    }

    /// One physical refresh can serve several independent surfaces on its display.
    pub(super) fn tick(&self, display_id: u32) -> Option<Refresh> {
        self.demand.get(&display_id)?.tick
    }

    /// Observation only: a native restart is not a new pacing deadline.
    #[cfg(feature = "lock-metrics")]
    pub(super) fn restart(
        &self,
        display_id: u32,
    ) -> Option<(std::time::Instant, std::time::Instant)> {
        // Every surface on this display gets the same copied receipt; a sibling
        // cannot consume it before another surface records its own observation.
        self.demand.get(&display_id)?.restart
    }

    /// Pending output during a build preserves continuous demand without stop/start.
    pub(super) fn reconcile(&mut self, surfaces: &BTreeMap<SurfaceId, Entry>) {
        // Retained displays absent from the new demand set become stopped below.
        for demand in self.demand.values_mut() {
            demand.count = 0;
        }
        for entry in surfaces.values() {
            // This lock protects copied host facts only; it is released before any
            // native control operation and is never taken by the link callback.
            let pending = entry
                .pending
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            // Selecting the host source detaches the native subscription before
            // another tick can reserve a competing frame for this surface.
            if !entry.host_driven
                && !pending.host_driven
                && let Some(display_id) = entry.driver.refresh_display(&pending)
            {
                self.demand
                    // Only first encounter allocates; restarting a retained source
                    // reuses this node and CoreVideo's permanent callback context.
                    .entry(display_id)
                    .or_insert(Demand {
                        count: 0,
                        active: 0,
                        tick: None,
                        #[cfg(feature = "lock-metrics")]
                        restart: None,
                        warned: false,
                    })
                    .count += 1;
            }
        }
        for (&display_id, demand) in &mut self.demand {
            if demand.count == demand.active {
                // Continuous floods leave the already-running native source alone.
                continue;
            }
            match self.links.subscribe(display_id, demand.count) {
                Ok(()) => {
                    demand.active = demand.count;
                    demand.warned = false;
                    // Subscription edges can be frequent for isolated output bursts;
                    // record them only at trace, never as per-frame debug traffic.
                    tracing::trace!(
                        event = "desktop.display_subscription",
                        display_id,
                        surfaces = demand.count
                    );
                }
                Err(error) if !demand.warned => {
                    // Preserve the failed desired count for a later host event retry.
                    demand.warned = true;
                    tracing::warn!(event = "desktop.display_link_failed", display_id, %error);
                }
                Err(_) => {}
            }
        }
    }
}

/// Close the subscription gates before any driver or the outer inbox is dropped.
impl Drop for Displays {
    fn drop(&mut self) {
        // Future callback snapshots omit this actor; an in-flight snapshot may
        // still signal its closed inbox safely. Other Apps retain their own wakes.
        if let Err(error) = self.links.stop_all() {
            tracing::warn!(event = "desktop.display_link_shutdown_failed", %error);
        }
    }
}
