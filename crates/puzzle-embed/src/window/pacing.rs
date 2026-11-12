//! Predicts display turns so VT snapshots are taken near presentation (§10.4).
//!
//! PTY writes still happen immediately. Only the visible snapshot waits;
//! several parser updates before one deadline therefore become one frame.
//! Surface acquisition supplies a phase sample only when it actually waits
//! for a Fifo slot. A missed deadline advances to the next refresh rather
//! than presenting a partial application update in the middle of a turn.
//! Refresh reservations survive lead changes: reducing build cost may move the
//! snapshot later, but cannot reuse a refresh already targeted by a submission.
//! Unknown phase spaces requests by one monitor period and learns from FIFO;
//! it never invents a phase from an acquisition which did not actually block.
/// Post-publication reserve stays separate from refresh-source policy.
mod cost;
/// Native display-link clocks, also exercised by injected host ticks.
mod link;
/// Existing timer phase policy stays separate from physical link opportunities.
mod prediction;
use puzzle_term::session::presentation::SettleWindow;
use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};

/// Number of recent frame costs used for the conservative p90 lead.
const SAMPLE_COUNT: usize = 32;

/// One pending redraw and the monitor's estimated presentation clock.
pub struct Pacer {
    /// A native link supplies refresh opportunities; timers never schedule its frames.
    linked: bool,
    /// Source identity changes once when an embedding host takes over its clock.
    host_source: bool,
    /// Coalesced damage remains pending until a legal physical refresh arrives.
    wanted: bool,
    /// Active local animation bypasses output settling, preserving refresh cadence.
    animation: bool,
    /// Monotonic origin converts host instants for the neutral settle rule.
    origin: Instant,
    /// The same burst rule is usable with browser rAF ticks in M7.
    settle: SettleWindow,
    /// Monitor period, using the highest available refresh rate.
    period: Duration,
    /// Fixed refresh-grid anchor learned from a qualifying acquisition.
    phase: Option<Instant>,
    /// A phase sample grows stale when the monitor's display mode changes.
    #[cfg(any(not(target_os = "macos"), test))]
    phase_observed: Option<Instant>,
    /// Last attempt time; no-sample attempts need a fresh idle opportunity.
    #[cfg(any(not(target_os = "macos"), test))]
    last_calibration: Option<Instant>,
    /// Failed early attempts retry after idle, learned samples age for five seconds.
    #[cfg(any(not(target_os = "macos"), test))]
    calibration_sampled: bool,
    /// Last output activity prevents calibration during interactive typing.
    #[cfg(any(not(target_os = "macos"), test))]
    last_output: Option<Instant>,
    /// Last successful completion bounds idle calibration, never fallback pacing.
    #[cfg(any(not(target_os = "macos"), test))]
    last_presented: Option<Instant>,
    /// Index of the current phase anchor in this pacer's monotonic refresh sequence.
    phase_index: u64,
    /// Chosen refresh index/instant are retained independently of the changing lead.
    refresh: Option<(u64, Instant)>,
    /// A consumed index cannot be submitted again after a cheaper frame changes lead.
    last_refresh_index: Option<u64>,
    /// Both calibrated and fallback clocks share the same earliest next refresh.
    last_refresh_target: Option<Instant>,
    /// A phase can be absent only before its first qualifying acquisition sample.
    reset_reason: &'static str,
    /// Actual complete snapshot/build/render/present costs for D-33 lead prediction.
    costs: VecDeque<Duration>,
    /// Post-publication CPU work excludes the independently paced FIFO wait.
    build_costs: VecDeque<Duration>,
    /// Receipt plus current wait and actual owner delay partition complete frame cost.
    snapshot_received: Option<(Instant, [Duration; 2])>,
    /// Earliest time at which the next snapshot may be taken.
    due: Option<Instant>,
    /// Startup alone bypasses pacing, so the initial window is visible.
    first: bool,
}

impl Pacer {
    /// Initializes a display clock from a monitor rate in millihertz.
    pub fn new(millihertz: u32, settle: Duration) -> Self {
        // A monitor that cannot report its rate arrives as the 60 Hz
        // fallback from WindowApp; zero remains guarded for unit tests.
        let rate = millihertz.max(1);
        // Winit reports millihertz, so 1000/rate converts to seconds.
        // Duration keeps phase arithmetic on a monotonic native clock.
        let period = Duration::from_secs_f64(1000.0 / f64::from(rate));
        Self {
            linked: false,
            host_source: false,
            wanted: false,
            animation: false,
            origin: Instant::now(),
            settle: SettleWindow::new(settle),
            period,
            phase: None,
            #[cfg(any(not(target_os = "macos"), test))]
            phase_observed: None,
            #[cfg(any(not(target_os = "macos"), test))]
            last_calibration: None,
            #[cfg(any(not(target_os = "macos"), test))]
            calibration_sampled: false,
            #[cfg(any(not(target_os = "macos"), test))]
            last_output: None,
            #[cfg(any(not(target_os = "macos"), test))]
            last_presented: None,
            phase_index: 0,
            refresh: None,
            last_refresh_index: None,
            last_refresh_target: None,
            reset_reason: "startup",
            costs: VecDeque::new(),
            build_costs: VecDeque::new(),
            snapshot_received: None,
            due: None,
            first: true,
        }
    }

    /// Coalesces damage; a resize bypasses the clock to follow a live drag.
    pub fn request(&mut self, now: Instant, resize: bool) {
        self.wanted = true;
        if !resize {
            #[cfg(any(not(target_os = "macos"), test))]
            {
                self.last_output = Some(now);
            }
            self.settle
                .request(now.saturating_duration_since(self.origin), self.period);
        }
        // Damage can arrive dozens of times between refreshes under a
        // full-screen animation. Replacing a deadline would starve it.
        if resize || self.first {
            // The first window image and live drag are explicit exceptions:
            // displaying an empty window or stretched backing is worse.
            self.due = Some(now);
            // Startup and interactive resize are explicit unpaced exceptions.
            self.refresh = None;
            return;
        }
        if self.linked {
            return;
        }
        if self.due.is_none() {
            // Leave an existing deadline intact so the latest VT state
            // is copied at that time, with no intermediate snapshot.
            let (due, refresh) = self.next_due(now);
            self.due = Some(due);
            self.refresh = refresh;
        }
    }

    /// Schedules coalesced output from its actual arrival, preserving D-33 silence.
    pub fn request_output(
        &mut self,
        arrival: crate::render_thread::surface::DamageArrival,
        now: Instant,
    ) {
        self.wanted = true;
        // UI delivery frequency cannot stand in for application output frequency.
        // The last suppressed write distinguishes continuous floods from held keys.
        if let Some(previous) = arrival.previous {
            self.settle
                .observe_output(previous.saturating_duration_since(self.origin));
        }
        #[cfg(any(not(target_os = "macos"), test))]
        {
            self.last_output = Some(arrival.first);
        }
        self.settle.request(
            arrival.first.saturating_duration_since(self.origin),
            self.period,
        );
        if self.first {
            self.due = Some(now);
            self.refresh = None;
        } else if !self.linked && self.due.is_none() {
            // Deadline calculation uses delivery time, so a delayed event cannot reserve
            // a refresh in the past; settling still starts at the actual first write.
            let (due, refresh) = self.next_due(now);
            self.due = Some(due);
            self.refresh = refresh;
        }
    }

    /// Returns the OS deadline without spinning while the terminal is idle.
    pub fn due(&self) -> Option<Instant> {
        self.due
    }

    /// Retains both the chosen snapshot deadline and predicted refresh for diagnostics.
    #[cfg(feature = "lock-metrics")]
    pub fn prediction(&self) -> Option<(Instant, Instant, u64, &'static str)> {
        self.due.zip(self.refresh).map(|(due, (index, at))| {
            (
                due,
                at,
                index,
                if self.linked {
                    "display_link"
                } else if self.phase.is_none() {
                    self.reset_reason
                } else {
                    "known"
                },
            )
        })
    }

    /// Distinguishes first startup from a clock reset at a monitor/DPR crossing.
    pub fn reset_reason(&mut self, reason: &'static str) {
        self.reset_reason = reason;
    }

    /// Schedules a local animation at the next refresh without starting an output burst.
    pub fn request_animation(&mut self, now: Instant) {
        self.wanted = true;
        self.animation = true;
        if self.linked && !self.first {
            return;
        }
        let (due, refresh) = if self.first {
            (now, None)
        } else {
            self.next_due(now)
        };
        if self.due.is_none_or(|old| due < old) {
            self.due = Some(due);
            self.refresh = refresh;
        }
    }

    /// Allows one snapshot at its deadline and consumes its request.
    pub fn take(&mut self, now: Instant) -> bool {
        // Winit can deliver spontaneous redraws for expose events. Those
        // callbacks must not reveal nvim's transient showcmd state.
        if !self.due.is_some_and(|due| now >= due) {
            return false;
        }
        // Consume the reserved index, not a deadline recomputed from today's lead.
        // A cheaper next frame cannot submit another drawable for this display turn.
        if let Some((index, target)) = self.refresh.take() {
            self.last_refresh_index = Some(index);
            // Without phase this is a submission slot, not a predicted refresh.
            // A late OS wake must anchor the next rate limit at its actual start.
            self.last_refresh_target = Some(if self.phase.is_none() { now } else { target });
        }
        self.due = None;
        self.wanted = false;
        // After the first frame, all ordinary damage uses a deadline;
        // only the explicit resize flag may bypass it again.
        self.first = false;
        true
    }

    /// Measures work after the snapshot and learns phase from a Fifo wait.
    pub fn presented(
        &mut self,
        start: Instant,
        acquired: Instant,
        blocked: Duration,
        end: Instant,
    ) {
        #[cfg(all(target_os = "macos", not(test)))]
        let _ = acquired;
        // A quick acquisition returned an already free texture and says
        // nothing about the refresh phase. Keep the previous observation.
        #[cfg(any(not(target_os = "macos"), test))]
        if !self.linked && blocked > Duration::from_millis(1) {
            // Keep refresh identity and the full-period target invariant while
            // admitting a later phase sample; acquisition jitter cannot move
            // the active grid backwards and skip the immediately following turn.
            self.observe_phase(acquired);
            self.phase_observed = Some(acquired);
        }
        if self.costs.len() == SAMPLE_COUNT {
            // Older GPU costs must not dominate after a resize or a move
            // between monitors with different display timing.
            self.costs.pop_front();
        }
        // D-33 samples the actual complete frame, including synchronous glyph work.
        // FIFO acquisition is recorded as part of that frame, not optional admission.
        let mut complete = end.saturating_duration_since(start);
        if let Some((received, [wait, latency])) = self.snapshot_received.take() {
            // An older answer may be ready before this turn starts. Replacing
            // today's receive wait with that answer's measured owner delay avoids
            // teaching zero publication cost from an immediate channel receive.
            complete = complete.saturating_sub(wait) + latency;
            if self.build_costs.len() == SAMPLE_COUNT {
                self.build_costs.pop_front();
            }
            // FIFO supplies phase and cannot consume the CPU reserve before acquisition.
            self.build_costs.push_back(
                end.saturating_duration_since(received)
                    .saturating_sub(blocked),
            );
        }
        self.costs.push_back(complete);
        // Completion informs idle calibration and settling. Unknown phase pacing
        // uses submission starts so build/acquire work cannot add period drift.
        #[cfg(any(not(target_os = "macos"), test))]
        {
            self.last_presented = Some(end);
        }
        self.settle
            .presented(end.saturating_duration_since(self.origin));
        // A consumed animation frame cannot exempt later unrelated output.
        self.animation = false;
    }

    /// Measurement reports the monitor period without a glyph-admission allowance.
    #[cfg(feature = "lock-metrics")]
    pub fn refresh_period(&self) -> Duration {
        self.period
    }

    /// The p90 cost leaves one extra millisecond for OS wakeup jitter.
    fn lead(&self) -> Duration {
        // Complete synchronous glyph preparation contributes to the actual frame cost.
        let p90 = self.base_cost();
        // At exotic rates above 500 Hz, half a period is under 1 ms.
        // Use it as the lower bound too so clamp cannot panic.
        let half = self.period / 2;
        (p90 + Duration::from_millis(1)).clamp(Duration::from_millis(1).min(half), half)
    }

    /// Recent complete-frame p90 sets the next snapshot lead (D-33).
    fn base_cost(&self) -> Duration {
        // A fixed stack array avoids a heap request on every frame wake.
        // Sorting at most 32 durations is cheap next to GPU submission.
        let mut costs = [Duration::ZERO; SAMPLE_COUNT];
        let count = self.costs.len();
        for (slot, cost) in costs.iter_mut().zip(&self.costs) {
            *slot = *cost;
        }
        costs[..count].sort_unstable();
        costs[..count]
            .get((count * 9).div_ceil(10).saturating_sub(1))
            .copied()
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests;
