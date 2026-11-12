//! Allocation-free timestamp mailbox for native callbacks and restart stragglers.
//!
//! A generation gates detached subscriptions. A sequence counter seals the three
//! timestamp scalars as one value; the render thread never observes a mixed tick.
//! Both pointers used by CoreVideo lead only to process-lifetime storage (D-53).
//! Writers claim the sequence with compare-exchange rather than taking a lock.
//! A concurrent writer never waits for a preempted callback to resume; it skips
//! that publication. This keeps stop/restart overlap safe without blocking the
//! native link thread or exposing a partially replaced timestamp to rendering.
use super::{Refresh, Wake};
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

/// Stable callback context, deliberately retained once per distinct display.
pub(super) struct Signal {
    /// All scalar timestamps are offsets from this monotonic origin.
    pub(super) origin: Instant,
    /// Immutable fanout snapshot; control allocates replacements before locking.
    /// The callback clones this Arc, then releases the guard before waking actors.
    wakes: Mutex<Arc<[Wake]>>,
    /// Zero means detached; every resubscription has a new generation.
    active: AtomicU64,
    /// Even values seal ticks; odd values mean the native producer is writing.
    sequence: AtomicU64,
    /// Native output host timestamp identifies a physical refresh across restarts.
    index: AtomicU64,
    /// Subscription incarnation that produced the sealed tick.
    generation: AtomicU64,
    /// CoreVideo's next scanout time, in nanoseconds after origin.
    output: AtomicU64,
    /// Callback timestamp distinguishes CPU/queue delay from the target period.
    callback: AtomicU64,
    /// First valid callback of this generation survives later coalesced ticks.
    first_callback: AtomicU64,
    /// Actual video refresh period, in nanoseconds.
    period: AtomicU64,
}

/// The callback producer alone changes clock facts; the owner gates subscriptions.
impl Signal {
    /// Allocates context at source creation, never during a refresh callback.
    pub(super) fn empty() -> Self {
        // The origin is initialized before CoreVideo can receive this context.
        // Keeping offsets rather than Instant objects makes each clock atomic.
        Self {
            origin: Instant::now(),
            wakes: Mutex::new(Arc::from([])),
            // Registration starts detached, so setup cannot emit an early wake.
            active: AtomicU64::new(0),
            // Sequence zero has no publication; readers ignore these empty facts.
            sequence: AtomicU64::new(0),
            generation: AtomicU64::new(0),
            index: AtomicU64::new(0),
            output: AtomicU64::new(0),
            callback: AtomicU64::new(0),
            first_callback: AtomicU64::new(0),
            period: AtomicU64::new(0),
        }
    }
    /// Scalar-mailbox tests attach a single wake without native registry setup.
    #[cfg(test)]
    pub(super) fn new(wake: Wake) -> Self {
        let signal = Self::empty();
        signal.set_wakes(Arc::from([wake]));
        signal
    }
    /// Publish a complete replacement set without holding a lock across actor code.
    pub(super) fn set_wakes(&self, wakes: Arc<[Wake]>) {
        // Building the snapshot is the registry's cold work. Only an Arc move
        // occurs under this guard, never allocation, native start/stop or a wake.
        let old = std::mem::replace(
            &mut *self
                .wakes
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
            wakes,
        );
        drop(old);
    }
    /// A newly joining actor starts with its own cursor, not another reader's.
    pub(super) fn cursor(&self) -> u64 {
        self.sequence.load(Ordering::SeqCst)
    }
    /// Activates a new subscription generation or detaches every surface.
    pub(super) fn activate(&self, generation: u64) {
        // Detaching invalidates all older facts even when their callback returns
        // after stop. Reattaching does not consume that stale sealed timestamp.
        self.active.store(generation, Ordering::SeqCst);
    }
    /// Publishes only clock facts; inactive or invalidated callbacks do no work.
    pub(super) fn publish(&self, index: u64, output: Instant, period: Duration, callback: Instant) {
        let generation = self.active.load(Ordering::SeqCst);
        if generation == 0 || period.is_zero() {
            return;
        }
        // SeqCst keeps readers from accepting new fields under an old sequence.
        // A restart may overlap a straggler from stop. Claim without spinning:
        // a competing producer leaves the existing complete publication intact.
        let sequence = self.sequence.load(Ordering::SeqCst);
        if !sequence.is_multiple_of(2)
            || self
                .sequence
                .compare_exchange(
                    sequence,
                    sequence.wrapping_add(1),
                    Ordering::SeqCst,
                    Ordering::SeqCst,
                )
                .is_err()
        {
            return;
        }
        self.index.store(index, Ordering::SeqCst);
        self.output.store(
            nanos(output.saturating_duration_since(self.origin)),
            Ordering::SeqCst,
        );
        self.period.store(nanos(period), Ordering::SeqCst);
        self.callback.store(
            nanos(callback.saturating_duration_since(self.origin)),
            Ordering::SeqCst,
        );
        // Retain the first publication even if several ticks arrive before receipt.
        // This field is sealed by the same sequence/generation as the clock tuple.
        if self.generation.load(Ordering::SeqCst) != generation {
            self.first_callback.store(
                nanos(callback.saturating_duration_since(self.origin)),
                Ordering::SeqCst,
            );
        }
        self.generation.store(generation, Ordering::SeqCst);
        self.sequence
            .store(sequence.wrapping_add(2), Ordering::SeqCst);
        // A detach during timestamp storage must suppress the obsolete wake.
        if self.active.load(Ordering::SeqCst) == generation {
            self.wake();
        }
    }
    /// Every member of the captured set gets a wake; contention never drops one.
    fn wake(&self) {
        // This short guard clones one immutable Arc. Control replaces that Arc
        // only, and releases it before CoreVideo calls, so stop cannot wait on a
        // callback blocked behind a lock held by stop itself. No try-lock skip.
        let wakes = Arc::clone(
            &self
                .wakes
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        );
        // A concurrent detach may leave an in-flight snapshot. Its closures own
        // only render inboxes, never windows; closed actors safely ignore tokens.
        for wake in wakes.iter() {
            wake();
        }
    }
    /// A concurrent callback is retried on its next wake, never spun on by rendering.
    pub(super) fn take(&self, seen: &mut u64) -> Option<Refresh> {
        let sequence = self.sequence.load(Ordering::SeqCst);
        if sequence == *seen || !sequence.is_multiple_of(2) {
            return None;
        }
        let index = self.index.load(Ordering::SeqCst);
        let output = self.output.load(Ordering::SeqCst);
        let period = self.period.load(Ordering::SeqCst);
        let callback = self.callback.load(Ordering::SeqCst);
        let generation = self.generation.load(Ordering::SeqCst);
        if sequence != self.sequence.load(Ordering::SeqCst)
            || generation == 0
            || generation != self.active.load(Ordering::SeqCst)
        {
            return None;
        }
        // Commit consumption only after validating all facts and the incarnation.
        // A callback racing this read remains pending for the next actor turn.
        *seen = sequence;
        // Duration offsets reconstruct the output on the render thread; the
        // callback does not need to allocate or transfer an Instant payload.
        Some(Refresh {
            restart: None,
            index,
            output: self.origin.checked_add(Duration::from_nanos(output))?,
            period: Duration::from_nanos(period),
            callback: self.origin.checked_add(Duration::from_nanos(callback))?,
        })
    }

    /// Reconstructs a sealed first-arrival observation on the owner thread only.
    pub(super) fn first_callback(&self) -> Option<Instant> {
        // Only the source owner reads this after take validated the current
        // generation. A later tick in that generation cannot replace this field;
        // reactivation also belongs to that owner and cannot interleave its read.
        self.origin.checked_add(Duration::from_nanos(
            self.first_callback.load(Ordering::SeqCst),
        ))
    }
}

/// Saturation handles an impossible-in-practice centuries-long process safely.
fn nanos(duration: Duration) -> u64 {
    duration.as_nanos().min(u64::MAX as u128) as u64
}
