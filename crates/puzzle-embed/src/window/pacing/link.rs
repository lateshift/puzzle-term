//! Physical refresh opportunities replace predicted wakes on macOS (D-53).
//!
//! Damage registers demand; only a supplied link tick reserves an ordinary frame.
//! No timer predicts or calibrates a native phase, and a repeated native output
//! identity cannot submit twice. Startup and live resize retain D-33's exceptions.
//! Receipts distinguish source-namespace takeover from settling and snapshot gates.
//! Observing a valid callback does not consume its identity: only take() does,
//! so an accepted idle callback cannot prevent later damage using a fresh tick.
use super::*;
use crate::render_thread::surface::Refresh;

/// Link policy reuses the platform-neutral settling and measured build reserve.
impl Pacer {
    /// Host refresh selection disables timer reservations without resetting history.
    pub fn use_host_refresh(&mut self) {
        // Source selection persists, so ordinary mailbox turns cannot reset identity.
        if !self.host_source {
            self.host_source = true;
            self.linked = true;
            // Host and native indices are independent namespaces. Preserve the last
            // output time as the cross-source duplicate barrier, not the old index.
            tracing::debug!(target: "puzzle_desktop::refresh", event="desktop.host_clock_takeover",
                native_index=?self.last_refresh_index, native_target=?self.last_refresh_target,
                reservation=?self.refresh, wanted=self.wanted);
            self.last_refresh_index = None;
            // Keep the output barrier even though callback counters use another namespace.
            // Startup/resize exceptions remain immediate. Ordinary timer reservations
            // must wait for a real host callback instead of consuming a second clock.
            if !self.first && self.refresh.is_some() {
                self.due = None;
                self.refresh = None;
            }
        }
    }
    /// Initializes demand without a fabricated acquisition-derived phase.
    pub fn new_linked(millihertz: u32, settle: Duration) -> Self {
        let mut pacer = Self::new(millihertz, settle);
        pacer.linked = true;
        pacer
    }

    /// Initial subscription learns actual clock facts once, then idle stops it.
    pub fn wants_refresh(&self) -> bool {
        self.wanted || self.phase.is_none()
    }

    /// Pending bytes or an unanswered snapshot outlive a consumed refresh opportunity.
    /// Host-output notifications may already have been consumed by an earlier turn.
    /// Re-observing the outstanding source prevents that turn from looking idle.
    /// The phase bootstrap is not damage: test the actual demand bit here.
    pub fn output_pending(&mut self, pending: bool, now: Instant) {
        if pending && !self.wanted {
            // Preserve any existing burst; a retry cannot postpone its settle deadline.
            self.request(now, false);
        }
    }

    /// Reserves the supplied output, never a newly predicted timer deadline.
    pub fn refreshed(&mut self, tick: Refresh, now: Instant) {
        if !self.fresh_refresh(tick, now) {
            return;
        }
        tracing::trace!(target: "puzzle_desktop::refresh", event="desktop.refresh_accepted",
            host=self.host_source, display=tick.display, index=tick.index, output=?tick.output,
            last_index=?self.last_refresh_index, last_output=?self.last_refresh_target);
        self.period = tick.period;
        self.phase = Some(tick.output);
        // An output target is in the future, but the snapshot is taken now.
        // Testing settling against that future target would reveal an editor's
        // transient showcmd before its erase actually reaches the Session owner.
        let ready = self
            .settle
            .may_present(now.saturating_duration_since(self.origin), self.animation);
        let reason = if !self.wanted {
            "no_demand"
        } else if !ready {
            "settling"
        } else if self.due.is_some() {
            "already_reserved"
        } else {
            "request"
        };
        tracing::trace!(target: "puzzle_desktop::scheduling", event="desktop.tick_decision",
            tick_index=tick.index, reason, wanted=self.wanted, ready,
            until_output_us=tick.output.saturating_duration_since(now).as_secs_f64()*1e6,
            build_reserve_us=self.build_cost().as_secs_f64()*1e6);
        if self.wanted && ready && self.due.is_none() {
            self.refresh = Some((tick.index, tick.output));
            self.due = Some(now);
        }
    }
    /// Validates one source clock and reports exactly which consumption barriers apply.
    /// Per-refresh evidence stays trace-only; source takeover itself is a rare debug fact.
    fn fresh_refresh(&self, tick: Refresh, now: Instant) -> bool {
        // Invalid host clocks cannot manufacture a target or zero-period division.
        // Native generation gating handles detach; this also rejects duplicate
        // physical refreshes delivered again after a link stop/start cycle.
        if tick.period.is_zero()
            || tick.callback > now
            || tick.output < tick.callback
            // This cross-source barrier also rejects a host takeover at the same scanout.
            || self
                .last_refresh_target
                .is_some_and(|last| tick.output <= last)
            || self
                .last_refresh_index
                .is_some_and(|last| tick.index <= last)
        {
            tracing::trace!(target: "puzzle_desktop::refresh", event="desktop.refresh_rejected",
                host=self.host_source, display=tick.display, index=tick.index,
                last_index=?self.last_refresh_index, output=?tick.output, last_output=?self.last_refresh_target,
                callback_future=tick.callback>now, zero_period=tick.period.is_zero(),
                output_before_callback=tick.output<tick.callback);
            return false;
        }
        true
    }
}

#[cfg(test)]
mod tests;
