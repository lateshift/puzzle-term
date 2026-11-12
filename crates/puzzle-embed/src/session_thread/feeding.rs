//! One owner-side parser slice preserves reply order and retained stage evidence.
//! Timing and scalar allocation counters are optional platform diagnostics.
//! Observation does not alter parser bytes or display deadlines. A pending frame
//! may end its existing bounded slice at a completed synchronized transaction.
//! Paired clocks stop at the completed feed, before diagnostic counters update.
//! Unlike D-29’s lease journal, this interval excludes pool handoffs and contains
//! exactly one existing slice. No guard or main-thread waiter exists (D-50).
//! Semantic operation deltas identify scroll, erase, cluster and reset work
//! without retaining application text or allocating a vector per scalar.
//! Growth counters sample both primary and alternate storage on the same owner;
//! a concurrent renderer cannot change either counter or be charged to this feed.
//! Session phase counters are nested wall times and must not be summed as CPU.
use puzzle_term::Session;
use std::time::Instant;

/// Replies leave Session before the next output slice can request a later reply.
pub(super) fn feed(
    #[cfg(any(debug_assertions, feature = "lock-metrics"))]
    metrics: &crate::pipeline_metrics::Metrics,
    session: &mut Session,
    bytes: &[u8],
    until_sync_end: bool,
) -> (usize, Instant) {
    #[cfg(any(debug_assertions, feature = "lock-metrics"))]
    let growth_before = start_interval(session);
    #[cfg(any(debug_assertions, feature = "lock-metrics"))]
    let operation_before = session.parse_operation_metrics();
    #[cfg(any(debug_assertions, feature = "lock-metrics"))]
    let phases_before = session.parse_feed_phases();
    #[cfg(any(debug_assertions, feature = "lock-metrics"))]
    let cpu = crate::thread_cpu::now();
    // Read paired clocks after metadata capture so attribution does not charge
    // the diagnostic baselines to parsing; the slice length remains unchanged.
    let started = Instant::now();
    // A pending frame yields at the next real transaction end. The ordinary
    // throughput path retains its existing slice size and parser implementation.
    let consumed = if until_sync_end {
        session.feed_to_sync_end(bytes, Instant::now())
    } else {
        session.feed(bytes, Instant::now());
        bytes.len()
    };
    #[cfg(any(debug_assertions, feature = "lock-metrics"))]
    let bytes = &bytes[..consumed];
    #[cfg(any(debug_assertions, feature = "lock-metrics"))]
    {
        // Worst-feed metadata distinguishes corpus output from final RIS teardown.
        // The owner alone changes both screens; a later frame reads a sealed slot.
        // These counters therefore belong to the same feed even if GPU work overlaps.
        let mut operations = session.parse_operation_metrics();
        for index in 0..11 {
            // Maximum cluster length is an interval peak, not an additive count.
            if index != 5 {
                operations[index] = operations[index].saturating_sub(operation_before[index]);
            }
        }
        if operations[4] == 0 {
            // A preceding cluster's interval peak does not describe this scalar-only call.
            operations[5] = 0;
        }
        let phases_after = session.parse_feed_phases();
        let elapsed = started.elapsed();
        // The journal describes a single owner turn, never a shared lock.
        // Both clocks stop before storing diagnostic maxima and formatting evidence.
        metrics.turn.record(
            (elapsed, crate::thread_cpu::elapsed(cpu)),
            bytes.len(),
            1,
            operations,
            [elapsed.as_nanos() as u64, 0, 0],
            std::array::from_fn(|i| phases_after[i].saturating_sub(phases_before[i])),
        );
        metrics.feed(
            started.elapsed(),
            bytes,
            growth_before,
            ring_growth(session),
            operations,
            std::array::from_fn(|i| phases_after[i].saturating_sub(phases_before[i])),
        );
    }
    (consumed, started)
}

/// Ring/grid allocation events belong to this feed's owner turn, not later report construction.
/// These counters exclude sparse text, reflow scratch and system allocator internals;
/// a zero delta rules out ordinary row/deque growth, not every allocation source.
#[cfg(any(debug_assertions, feature = "lock-metrics"))]
fn ring_growth(session: &Session) -> [u64; 3] {
    let terminal = session.terminal();
    let primary = terminal.primary.ring.storage_growth();
    let alternate = terminal.alternate.ring.storage_growth();
    // Lifetime counters from both screens retain growth across mode switches.
    [
        primary.rows + alternate.rows,
        primary.cell_bytes + alternate.cell_bytes,
        primary.deque_growths + alternate.deque_growths,
    ]
}

/// Enables fresh semantic counters before the same owner starts either feed clock.
#[cfg(any(debug_assertions, feature = "lock-metrics"))]
fn start_interval(session: &mut Session) -> [u64; 3] {
    // VT counters are opt-in. Resetting here prevents a disabled observer from
    // reporting fabricated zero operations and keeps the cluster maximum local.
    session.start_parse_operation_metrics();
    ring_growth(session)
}

/// Pool ownership and recording advance together after one bounded parser slice.
impl super::Owner {
    /// Returns false only when there is no borrowed pool slice to consume.
    pub(super) fn feed_next(&mut self) -> Result<bool, String> {
        let Some(bytes) = self.data.next_slice() else {
            return Ok(false);
        };
        // Pausing for decode returns the unconsumed suffix to the same pool slot.
        let (count, _) = feed(
            #[cfg(any(debug_assertions, feature = "lock-metrics"))]
            &self.pipeline,
            &mut self.session,
            bytes,
            self.frame.is_some(),
        );
        // Observation reflects accepted bytes, not bytes merely read from the PTY.
        self.parsed
            .store(self.session.parsed_bytes_total(), super::Ordering::Relaxed);
        // Releasing the slice permits the drain to reuse its fixed allocation.
        if count != 0 {
            self.data.consume(count)?;
        }
        // Zero retains the identical borrowed suffix behind a reply/work barrier.
        // The next owner iteration services controls before any continuation.
        // The private transcript uses the same accepted-byte boundary as snapshots.
        self.record_parse(count);
        // Host facts are queued only after the complete mutable parser turn.
        self.deliver();
        Ok(true)
    }
}
