//! Opt-in VT operation counts attribute actual parser work without time or I/O.
//! Counts are updated only at scroll/edit/cluster/control sites, never for each
//! scalar or byte. The host clears an interval while it owns Terminal and reads
//! fixed words afterwards; this observer cannot allocate or affect semantics.
use super::Terminal;

/// Stable operation indices shared by the host's diagnostic report names.
#[derive(Clone, Copy)]
pub(super) enum Operation {
    /// Actual clamped rows moved upward, including full-screen LF recycling.
    ScrollUp,
    /// Actual clamped rows moved downward, without admitting history.
    ScrollDown,
    /// Erase spans presented to a row, including selective erases.
    EraseRows,
    /// Selected erase cells; protected cells may retain their original ink.
    EraseCells,
    /// Multi-scalar storage extensions, absent from scalar-only Unicode rows.
    ClusterJoins,
    /// Maximum stored cluster length for this interval, rather than a sum.
    ClusterBytes,
    /// Complete OSC handlers entered; split introducers are not counted early.
    Osc,
    /// OSC parameter bytes offered after bounded parser separation.
    OscBytes,
    /// Rows retained before a real width-changing primary reflow.
    ReflowRows,
    /// Rows cleared or retired by a complete hard reset.
    ResetRows,
}

/// A host enables interval observations before its parser begins.
impl Terminal {
    /// Begins a new interval of fixed-word operation counts without allocating.
    pub fn start_operation_metrics(&mut self) {
        self.operation_metrics = Some([0; 10]);
    }
    /// Returns the current interval; absence means instrumentation was not enabled.
    /// Order: scroll-up/down rows, erase rows/cells, cluster joins/max bytes,
    /// OSC handlers/parameter bytes, reflow rows and hard-reset rows.
    pub fn operation_metrics(&self) -> Option<[u64; 10]> {
        self.operation_metrics
    }
    /// Counter updates belong to rare semantic operations, outside scalar stores.
    pub(super) fn note_operation(&mut self, operation: Operation, count: u64) {
        if let Some(metrics) = &mut self.operation_metrics {
            let value = &mut metrics[operation as usize];
            // A lifetime or adversarial count never wraps to a misleading small value.
            *value = value.saturating_add(count);
        }
    }
    /// Cluster storage work records both extension count and its worst bounded length.
    pub(super) fn note_cluster(&mut self, bytes: usize) {
        if let Some(metrics) = &mut self.operation_metrics {
            metrics[Operation::ClusterJoins as usize] += 1;
            metrics[Operation::ClusterBytes as usize] =
                metrics[Operation::ClusterBytes as usize].max(bytes as u64);
        }
    }
    /// Only a real primary width reflow visits these retained rows.
    pub(super) fn note_reflow(&mut self) {
        self.note_operation(
            Operation::ReflowRows,
            (self.primary.ring.visible_rows() + self.primary.ring.scrollback_len()) as u64,
        );
    }
    /// Ordinary hosts avoid even walking OSC parameter lengths for an unused observer.
    pub(crate) fn note_osc(&mut self, params: &[&[u8]]) {
        if let Some(metrics) = &mut self.operation_metrics {
            metrics[Operation::Osc as usize] += 1;
            metrics[Operation::OscBytes as usize] +=
                params.iter().map(|p| p.len() as u64).sum::<u64>();
        }
    }
}

#[cfg(test)]
/// Counts are pinned against real parser behavior and reset continuity.
mod tests;
