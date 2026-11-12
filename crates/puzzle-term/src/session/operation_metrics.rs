//! A host can observe actual parser operations without introducing a core clock.
//! Intervals begin only while the host owns Session; fixed counter words require
//! no allocation, callback, atom or platform dependency. Scalar printing is not
//! instrumented. Row recycling is a lifetime ring count sampled alongside edits.
use super::{Duration, Instant, Session};

/// Host-owned timing samples bracket these platform-neutral scalar observations.
impl Session {
    /// Begins a VT operation interval; ordinary sessions leave the observer disabled.
    pub fn start_parse_operation_metrics(&mut self) {
        self.terminal.start_operation_metrics();
        self.parse_feed_phases = Some([Duration::ZERO; 6]);
    }
    /// Returns scroll up/down, erase rows/cells, cluster joins/max bytes, OSC
    /// handlers/bytes, reflow rows, hard-reset rows and lifetime recycled rows.
    pub fn parse_operation_metrics(&self) -> [u64; 11] {
        let mut values = [0; 11];
        // The final lifetime value is subtracted by the host at its interval boundary.
        values[..10].copy_from_slice(&self.terminal.operation_metrics().unwrap_or([0; 10]));
        values[10] = self.terminal.primary.ring.recycled_rows()
            + self.terminal.alternate.ring.recycled_rows();
        values
    }
    /// Returns nested copy/VT/reply/wake/reconcile/pump nanoseconds for this lease.
    /// The Session has time services already; puzzle-vt remains clock-free.
    pub fn parse_feed_phases(&self) -> [u64; 6] {
        self.parse_feed_phases
            .unwrap_or([Duration::ZERO; 6])
            .map(|value| value.as_nanos().min(u128::from(u64::MAX)) as u64)
    }
    /// A disabled observer never calls the clock in ordinary hot feed stages.
    pub(super) fn begin_feed_phase(&self) -> Option<Instant> {
        self.parse_feed_phases.as_ref().map(|_| Instant::now())
    }
    /// Fixed durations accumulate only coarse Session work, outside scalar printing.
    pub(super) fn end_feed_phase(&mut self, index: usize, started: Option<Instant>) {
        if let Some(started) = started
            && let Some(phases) = &mut self.parse_feed_phases
        {
            phases[index] += started.elapsed();
        }
    }
}
