//! Event-turn counters for the P2 idle contract, with no logging or timer.
//!
//! These count completed setup and actual selected events, not future polls.
//! Tests observe a one-second interval from outside the runtime; an idle
//! session and listener must not enter another loop turn during that interval.
use std::sync::atomic::{AtomicU64, Ordering};

/// Process-local observations; neither counters nor secrets enter the wire.
#[derive(Default)]
pub struct Metrics {
    /// Accept-loop entries reveal listener polling as well as session polling.
    pub(crate) admissions: AtomicU64,
    /// Session loop entries include its initial wait and each actual event.
    pub(crate) turns: AtomicU64,
    /// Accepted payload bytes prove a stall witness actually spent its grant.
    pub(crate) input_bytes: AtomicU64,
    /// Valid ACK controls applied independently of a stalled stdin writer.
    pub(crate) acks: AtomicU64,
    /// Completed native resizes, observed before a non-reading child resumes.
    pub(crate) resizes: AtomicU64,
}

/// Relaxed reads suffice: counters are evidence, never synchronization gates.
impl Metrics {
    /// Counts listener loop entries, including the initial wait.
    pub fn admission_turns(&self) -> u64 {
        self.admissions.load(Ordering::Relaxed)
    }
    /// Counts session loop entries; an idle interval adds zero.
    pub fn session_turns(&self) -> u64 {
        self.turns.load(Ordering::Relaxed)
    }
    /// Total credited input enqueued, including bytes still awaiting OS writes.
    pub fn input_bytes(&self) -> u64 {
        self.input_bytes.load(Ordering::Relaxed)
    }
    /// Number of valid output acknowledgments processed by the coordinator.
    pub fn acks(&self) -> u64 {
        self.acks.load(Ordering::Relaxed)
    }
    /// Number of kernel PTY resize operations completed.
    pub fn resizes(&self) -> u64 {
        self.resizes.load(Ordering::Relaxed)
    }
}
