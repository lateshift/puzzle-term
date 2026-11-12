//! D-50 control residence and actual ordered-writer enqueue observations.
//!
//! The Session owner brackets each applied control. Its LocalPty transport
//! observes successful queue submission on that same thread, before an OS
//! writer may block behind earlier input. Neither callback owns a Session
//! reference and neither acquires a measurement lock. Fixed atomic storage
//! permits the host to retain the observer without sharing mutable VT state.
//! Only the Session owner writes these fields; reports are cohort-final.
//! CPU spans start at application on that owner, never on the sending thread.
//! Queue residence has wall time only: a sender cannot read another thread's
//! CPU clock. A blocked or consumed shortcut produces no writer sample.
//! Finishing its control clears the stamp, so a later parser reply cannot
//! accidentally become the bytes of that swallowed key. The first successful
//! enqueue records a key once, even if its action writes several transactions.
//! Every over-budget key logs both clocks and its control wait; the fixed tail
//! additionally retains the five longest observations without growing storage.
use super::hold::tail::Tail;
use std::{
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
    time::{Duration, Instant},
};

/// Diagnostic stamps connect two callbacks on the same Session owner.
pub struct Controls {
    /// Local monotonic origin supports fixed-size stamps without a shared Instant.
    origin: Instant,
    /// A consumed shortcut cannot leave a stamp for later output replies.
    active: AtomicBool,
    /// Application start on the Session owner, relative to this observer's origin.
    applied: AtomicU64,
    /// Key arrival to application includes adapter work and control residence.
    key_wait: AtomicU64,
    /// Control send to application excludes the preceding native adapter work.
    wait: AtomicU64,
    /// CPU start is read on the owner; MAX denotes an unsupported clock.
    cpu: AtomicU64,
    /// Every applied message contributes to control-channel observations.
    controls: AtomicU64,
    /// Sum of control residence, kept separately from actual owner CPU work.
    wait_total: AtomicU64,
    /// Longest message residence remains visible beside its average.
    wait_max: AtomicU64,
    /// Successful first key enqueues only, never parser replies or swallowed keys.
    keys: AtomicU64,
    /// Key wall maxima include the whole host-arrival-to-queue interval.
    key_max: AtomicU64,
    /// Every over-budget key is logged; this count verifies log completeness.
    over_budget: AtomicU64,
    /// Fixed paired wall/owner-CPU outliers have no hot-path allocation.
    tail: Tail,
    /// Host residence is unknown when the toolkit supplies no original event clock.
    host_residence: AtomicU64,
    /// Marshalling and API checks precede the ordered channel submission.
    api_to_channel: AtomicU64,
    /// Runtime trace observations do not depend on the lock-metrics build feature.
    traced: AtomicBool,
    /// Surface identity is metadata only, never an authority to access its Session.
    surface: AtomicU64,
}

impl Default for Controls {
    /// Allocates observer storage before the PTY begins producing output.
    fn default() -> Self {
        Self {
            origin: Instant::now(),
            active: AtomicBool::new(false),
            applied: AtomicU64::new(0),
            key_wait: AtomicU64::new(0),
            wait: AtomicU64::new(0),
            cpu: AtomicU64::new(u64::MAX),
            controls: AtomicU64::new(0),
            wait_total: AtomicU64::new(0),
            wait_max: AtomicU64::new(0),
            keys: AtomicU64::new(0),
            key_max: AtomicU64::new(0),
            over_budget: AtomicU64::new(0),
            tail: Tail::default(),
            host_residence: AtomicU64::new(u64::MAX),
            api_to_channel: AtomicU64::new(0),
            traced: AtomicBool::new(false),
            surface: AtomicU64::new(u64::MAX),
        }
    }
}

impl Controls {
    /// Records one control's residence and arms only commands that can encode a key.
    #[cfg(any(test, feature = "lock-metrics"))]
    pub fn begin(&self, sent: Instant, arrival: Instant, key: bool) {
        self.traced.store(false, Ordering::Relaxed);
        self.begin_at(sent, arrival, key, Instant::now(), crate::thread_cpu::now());
    }

    /// A raw callback's owner stamp includes translation before Session routing.
    /// The host's arbitrary clock epoch never enters Instant arithmetic: only
    /// its checked residence duration is carried alongside process-local clocks.
    /// A typed command has no raw adapter and begins owner work at this method.
    pub(crate) fn begin_timed(&self, timing: crate::InputTiming, key: bool) {
        let (now, cpu) = timing
            .owner
            .unwrap_or_else(|| (Instant::now(), crate::thread_cpu::diagnostic_now()));
        // MAX denotes an unknown host timestamp, distinct from a genuinely
        // immediate event whose measured host residence is exactly zero.
        self.host_residence
            .store(timing.residence.map_or(u64::MAX, nanos), Ordering::Relaxed);
        self.api_to_channel.store(
            nanos(timing.sent.saturating_duration_since(timing.api)),
            Ordering::Relaxed,
        );
        self.surface
            .store(timing.surface.unwrap_or(u64::MAX), Ordering::Relaxed);
        self.traced.store(true, Ordering::Relaxed);
        self.begin_at(timing.sent, timing.api, key, now, cpu);
    }

    /// One writer and owner share these scalar clocks; no measurement lock exists.
    /// API-to-writer equals API-to-send + channel wait + owner-to-writer.
    /// Adding known host residence gives the full event-to-writer interval;
    /// unsupported host/CPU clocks remain absent rather than looking fast.
    fn begin_at(
        &self,
        sent: Instant,
        arrival: Instant,
        key: bool,
        now: Instant,
        cpu: Option<Duration>,
    ) {
        // Channel wait includes a wake or the remainder of the owner's existing
        // bounded parse slice. It is wall-only, not CPU attributed to the host.
        let wait = nanos(now.saturating_duration_since(sent));
        self.controls.fetch_add(1, Ordering::Relaxed);
        self.wait_total.fetch_add(wait, Ordering::Relaxed);
        self.wait_max.fetch_max(wait, Ordering::Relaxed);
        self.wait.store(wait, Ordering::Relaxed);
        self.key_wait.store(
            nanos(now.saturating_duration_since(arrival)),
            Ordering::Relaxed,
        );
        self.applied
            .store(nanos(now.duration_since(self.origin)), Ordering::Relaxed);
        self.cpu
            .store(cpu.map_or(u64::MAX, nanos), Ordering::Relaxed);
        // There is one writer and one transport callback on this same owner.
        self.active.store(key, Ordering::Relaxed);
    }

    /// Swallowed keys cannot be confused with bytes produced by the next feed.
    pub fn finish(&self) {
        self.active.store(false, Ordering::Relaxed);
    }

    /// Called immediately after LocalPty successfully submits its actual writer queue.
    pub fn queued(&self) {
        if !self.active.swap(false, Ordering::Relaxed) {
            return;
        }
        // Freeze clocks before counters and tracing; observer work is not key latency.
        // Both CPU readings belong to this Session thread. An OS writer may
        // still be blocked later; successful queue admission is our endpoint.
        let started = self.cpu.load(Ordering::Relaxed);
        let cpu = (started != u64::MAX)
            .then(|| Duration::from_nanos(started))
            .and_then(|start| {
                crate::thread_cpu::diagnostic_now().and_then(|now| now.checked_sub(start))
            });
        let owner =
            nanos(self.origin.elapsed()).saturating_sub(self.applied.load(Ordering::Relaxed));
        let wall = owner.saturating_add(self.key_wait.load(Ordering::Relaxed));
        self.trace_queued(wall, owner, cpu);
        // Retain the original arrival-to-writer budget including known host
        // residence; the trace separately identifies which stage owns a miss.
        let host = self.host_residence.load(Ordering::Relaxed);
        let wall = if self.traced.load(Ordering::Relaxed) && host != u64::MAX {
            wall.saturating_add(host)
        } else {
            wall
        };
        // Explicit resize observers retain every key, including those within budget.
        tracing::trace!(target:"puzzle_desktop::resize", event="desktop.key_queued",
            wall_ms=wall as f64 / 1e6, owner_wall_ms=owner as f64 / 1e6,
            owner_cpu_ms=cpu.map(|v| v.as_secs_f64()*1000.0));
        self.keys.fetch_add(1, Ordering::Relaxed);
        self.key_max.fetch_max(wall, Ordering::Relaxed);
        self.tail.record((Duration::from_nanos(wall), cpu));
        // This is the existing 0.5-ms key budget, not a new scheduling constant.
        if wall > 500_000 {
            self.over_budget.fetch_add(1, Ordering::Relaxed);
            tracing::debug!(
                target: "puzzle_desktop::input",
                event = "desktop.key_queue_over_budget",
                wall_ms = wall as f64 / 1e6,
                owner_wall_ms = owner as f64 / 1e6,
                owner_thread_cpu_ms = cpu.map(|v| v.as_secs_f64() * 1000.0),
                control_wait_ms = self.wait.load(Ordering::Relaxed) as f64 / 1e6,
                "key reached the ordered writer after its unchanged budget"
            );
        }
    }

    /// Emit only the explicitly enabled private input diagnostic, without text.
    /// One record holds all stages so an embedder need not join separate clocks
    /// across threads. Surface identity distinguishes simultaneous panes, while
    /// their original key text and native key codes are deliberately omitted.
    fn trace_queued(&self, wall: u64, owner: u64, cpu: Option<Duration>) {
        if self.traced.load(Ordering::Relaxed) {
            let host = self.host_residence.load(Ordering::Relaxed);
            let surface = self.surface.load(Ordering::Relaxed);
            tracing::debug!(target: "puzzle_desktop::input", event="desktop.input_latency",
                surface=(surface != u64::MAX).then_some(surface),
                host_residence_ms=(host != u64::MAX).then_some(host as f64 / 1e6),
                api_to_channel_ms=self.api_to_channel.load(Ordering::Relaxed) as f64 / 1e6,
                control_wait_ms=self.wait.load(Ordering::Relaxed) as f64 / 1e6,
                owner_wall_ms=owner as f64 / 1e6,
                owner_cpu_ms=cpu.map(|value| value.as_secs_f64() * 1000.0),
                api_to_writer_ms=wall as f64 / 1e6,
                event_to_writer_ms=(host != u64::MAX).then_some(wall.saturating_add(host) as f64 / 1e6));
        }
    }

    /// Formats complete fixed observations outside the measured callbacks.
    #[cfg(any(test, feature = "lock-metrics"))]
    pub fn report(&self) -> serde_json::Value {
        // CPU is explicitly owner-side; total key wall includes control queue residence.
        serde_json::json!({
            "control_count":self.controls.load(Ordering::Relaxed),
            "control_wait_total_ms":self.wait_total.load(Ordering::Relaxed) as f64/1e6,
            "control_wait_max_ms":self.wait_max.load(Ordering::Relaxed) as f64/1e6,
            "queued_keys":self.keys.load(Ordering::Relaxed),
            "key_to_writer_max_ms":self.key_max.load(Ordering::Relaxed) as f64/1e6,
            "keys_over_500_us":self.over_budget.load(Ordering::Relaxed),
            "worst_five_key_wall_and_applied_to_queue_cpu":self.tail.report(),
        })
    }
}

/// Saturates diagnostic conversion; observations cannot wrap after pathological uptime.
fn nanos(value: Duration) -> u64 {
    value.as_nanos().min(u128::from(u64::MAX - 1)) as u64
}

/// A swallowed key must not label a later parser reply as its successful enqueue.
#[cfg(test)]
mod tests;
