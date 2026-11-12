//! D-50 request timestamps separate delivery delay, owner work and missed deadlines.
//!
//! Wall timestamps share the process monotonic clock. CPU timestamps are read
//! only by the Session owner and never subtracted across different threads.
//! Queue wait has no claimed CPU value: its sender cannot sample another
//! thread's running CPU clock. Applied-to-published CPU includes any intervening
//! parse work when a request retained its turn while no slot was free.
//! Completed records travel with all replies, including unchanged publications.
//! Formatting happens in a finite harness, outside both measured intervals.
use super::endpoint::FrameRequest;
use std::time::{Duration, Instant};

/// Frozen scalar evidence carries no source cells or rendering resources.
#[derive(Clone, Copy)]
pub struct PublicationTiming {
    /// Absolute request/publication/deadline clocks are retained only by probes.
    #[cfg(feature = "lock-metrics")]
    clocks: (u64, [Instant; 3]),
    /// Full send-to-publish wall latency, including control-channel residence.
    request_wall: Duration,
    /// Delay before the Session owner first applied this request.
    control_wait: Duration,
    /// Owner-side wall time until publication, including free-slot deferral.
    owner_wall: Duration,
    /// CPU from the same owner-side interval; unsupported clocks stay absent.
    owner_cpu: Option<Duration>,
    /// Answer publication itself crossed the rendering owner's supplied deadline.
    late: bool,
}

impl PublicationTiming {
    /// The pacer learns the actual owner delay even when a reply arrived last turn.
    pub fn request_wall(self) -> Duration {
        self.request_wall
    }

    /// Records each received outcome without allocating a JSON measurement record.
    /// The owner interval includes semantic boundary work when synchronization
    /// deferred a request; queue residence remains a separate wall-only field.
    /// The receiving turn's id identifies replies retained from an older refresh.
    pub fn trace_reply(self, id: u64, current: u64, published: bool) {
        tracing::trace!(target: "puzzle_desktop::scheduling", event="desktop.snapshot_answer",
            id, current, outcome=if published { "Published" } else { "NothingNew" },
            late=self.late, request_wall_us=self.request_wall.as_secs_f64()*1e6,
            control_wait_us=self.control_wait.as_secs_f64()*1e6,
            owner_wall_us=self.owner_wall.as_secs_f64()*1e6,
            owner_cpu_us=self.owner_cpu.map(|cpu|cpu.as_secs_f64()*1e6));
    }

    /// Reads clocks on the Session owner just before moving the completed reply.
    pub(super) fn completed(request: &FrameRequest) -> Self {
        // The CPU interval begins at control application on this same worker.
        let cpu = request
            .applied
            .and_then(|(_, cpu)| crate::thread_cpu::elapsed(cpu));
        let published = Instant::now();

        // A missing application is an internal sequencing error, not fabricated CPU time.
        let applied = request.applied.map_or(published, |(instant, _)| instant);
        Self {
            #[cfg(feature = "lock-metrics")]
            clocks: (request.id, [request.sent, published, request.deadline]),
            request_wall: published.saturating_duration_since(request.sent),
            control_wait: applied.saturating_duration_since(request.sent),
            owner_wall: published.saturating_duration_since(applied),
            owner_cpu: cpu,
            late: published > request.deadline,
        }
    }

    /// Serializes explicit interval names; queue delay is not reported as CPU work.
    pub fn report(self) -> serde_json::Value {
        // Durations retain nanosecond resolution until this off-path unit conversion.
        serde_json::json!({
            "request_to_publish_wall_ms": self.request_wall.as_secs_f64() * 1000.0,
            "control_wait_wall_ms": self.control_wait.as_secs_f64() * 1000.0,
            "applied_to_publish_wall_ms": self.owner_wall.as_secs_f64() * 1000.0,
            "applied_to_publish_thread_cpu_ms": self.owner_cpu.map(|value| value.as_secs_f64() * 1000.0),
            "published_after_build_deadline": self.late,
        })
    }
}

/// Process-wide timestamps correlate owner answers with the rendering opportunity.
#[cfg(feature = "lock-metrics")]
impl PublicationTiming {
    /// The observer supplies its origin; setup timestamps retain negative offsets.
    pub fn clock_report(self, origin: Instant) -> serde_json::Value {
        let ms = |at: Instant| {
            if at >= origin {
                at.duration_since(origin).as_secs_f64() * 1000.0
            } else {
                -origin.duration_since(at).as_secs_f64() * 1000.0
            }
        };
        serde_json::json!({"id": self.clocks.0,
            "request_ms": ms(self.clocks.1[0]), "published_ms": ms(self.clocks.1[1]),
            "deadline_ms": ms(self.clocks.1[2]), "late": self.late,
            "owner_wall_ms": self.owner_wall.as_secs_f64() * 1000.0,
            "owner_thread_cpu_ms": self.owner_cpu.map(|cpu| cpu.as_secs_f64() * 1000.0)})
    }
}
