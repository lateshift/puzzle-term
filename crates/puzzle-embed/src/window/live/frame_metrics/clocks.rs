//! Per-frame deadline arithmetic, collected as values and formatted after a cohort.
//!
//! These clocks never log or allocate during a frame; all owners share Instant.
//! Current requests and the older slot actually drawn have distinct identities.
//! Actor deadlines can belong to a probe or resize rather than the snapshot;
//! keeping both prevents an early mailbox wake from looking like timer jitter.
//! The wait's thread CPU includes only executed semaphore bookkeeping, while
//! return-to-request wall time includes the driver's recovery and probe work.
use crate::session_thread::PublicationTiming;
use std::time::{Duration, Instant};

/// Optional diagnostic stamps have no policy or scheduling authority.
#[derive(Clone, Copy, Default)]
pub(super) struct Clocks {
    /// Native callback/output timestamps are independent of the actor's timer waits.
    pub(super) link: Option<crate::render_thread::surface::Refresh>,
    /// Worker wait and driver entry isolate OS wake delivery from driver work.
    pub(super) wake: Option<(crate::render_thread::wake::Sample, Instant)>,
    /// Current request, bounded receive deadline and correlation identifier.
    pub(super) request: Option<(Instant, Instant, u64)>,
    /// The answer actually used may belong to a previous display opportunity.
    pub(super) source: Option<PublicationTiming>,
    /// Rendering receives a sealed slot before starting glyph and row work.
    pub(super) received: Option<Instant>,
    /// Row building starts after adopting the immutable source value.
    pub(super) build: Option<Instant>,
    /// Acquisition return, queue submission and present return are separate clocks.
    pub(super) presentation: Option<[Instant; 3]>,
    /// Requested snapshot deadline, reserved edge, index and honest phase status.
    pub(super) plan: Option<(Instant, Instant, u64, &'static str)>,
    /// Existing measured lead and post-receipt reserve; no added margin.
    pub(super) costs: [Duration; 2],
}
impl Clocks {
    /// Unit conversion and JSON construction run only after the measured interval.
    pub(super) fn report(self, origin: Instant) -> serde_json::Value {
        // Signed offsets preserve setup observations that preceded producer release.
        let ms = |at: Instant| {
            if at >= origin {
                at.duration_since(origin).as_secs_f64() * 1000.0
            } else {
                -origin.duration_since(at).as_secs_f64() * 1000.0
            }
        };
        serde_json::json!({
            "link": self.link.map(|l| serde_json::json!({
                "display": l.display, "index": l.index, "callback_ms": ms(l.callback),
                "output_ms": ms(l.output), "period_ms": l.period.as_secs_f64() * 1000.0,
            })),
            "actor_wait_started_ms": self.wake.map(|w| ms(w.0.bounds[0])),
            "actor_wait_deadline_ms": self.wake.and_then(|w| w.0.deadline.map(ms)),
            "actor_wait_returned_ms": self.wake.map(|w| ms(w.0.bounds[1])),
            "actor_wait_thread_cpu_ms": self.wake.and_then(|w| w.0.cpu.map(|v| v.as_secs_f64() * 1000.0)),
            "actor_wait_reason": self.wake.map(|w| w.0.reason),
            "driver_entered_ms": self.wake.map(|w| ms(w.1)),
            "requested_ms": self.request.map(|r| ms(r.0)),
            "receive_deadline_ms": self.request.map(|r| ms(r.1)),
            "request_id": self.request.map(|r| r.2),
            "source": self.source.map(|s| s.clock_report(origin)),
            "received_ms": self.received.map(ms), "build_started_ms": self.build.map(ms),
            "acquired_ms": self.presentation.map(|p| ms(p[0])),
            "submitted_ms": self.presentation.map(|p| ms(p[1])),
            "presented_ms": self.presentation.map(|p| ms(p[2])),
            "snapshot_due_ms": self.plan.map(|p| ms(p.0)),
            "refresh_target_ms": self.plan.map(|p| ms(p.1)),
            "refresh_index": self.plan.map(|p| p.2), "phase_status": self.plan.map(|p| p.3),
            "lead_ms": self.costs[0].as_secs_f64() * 1000.0,
            "build_reserve_ms": self.costs[1].as_secs_f64() * 1000.0,
        })
    }
}
