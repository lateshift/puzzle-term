//! Platform-neutral output-burst settling before a refresh snapshot (D-33).
//!
//! Hosts provide monotonic times and refresh ticks. This rule never reads a
//! clock itself, so desktop deadlines and browser rAF ticks make the same
//! decision about transient updates from an otherwise idle program.
use std::time::Duration;

/// Remembers the first output of a burst until one frame is presented.
pub struct SettleWindow {
    /// Required time between first output and the snapshot that shows it.
    settle: Duration,
    /// First output after one refresh period without application output.
    burst: Option<Duration>,
    /// Last application output distinguishes isolated keys from animation.
    last_output: Option<Duration>,
}

impl SettleWindow {
    /// Creates one decision rule from validated settings or a temporary host default.
    pub fn new(settle: Duration) -> Self {
        Self {
            settle,
            burst: None,
            last_output: None,
        }
    }

    /// Records damage; later writes in the same burst cannot postpone it.
    pub fn request(&mut self, now: Duration, period: Duration) {
        if self.burst.is_none() {
            // Auto-repeat may arrive soon after a frame even though its last
            // output was a full refresh ago. Output silence, rather than
            // presentation silence, identifies a new editor command burst.
            let idle = self
                .last_output
                .is_none_or(|last| now.saturating_sub(last) >= period);
            self.burst = Some(if idle {
                now
            } else {
                // Backdating by settle makes earliest() equal now during
                // continuous output, so an animation waits only for refresh.
                now.saturating_sub(self.settle)
            });
        }
        // Every write closes the preceding silence interval, including an
        // erase sent a few milliseconds after the mark in the same burst.
        self.last_output = Some(now);
    }

    /// Supplies preceding output that was coalesced without a host wake.
    /// This only records silence; it neither starts nor postpones a pending burst.
    pub fn observe_output(&mut self, now: Duration) {
        self.last_output = Some(now);
    }

    /// Earliest legal snapshot time; hosts choose a refresh tick at or after it.
    pub fn earliest(&self) -> Option<Duration> {
        self.burst.map(|start| start.saturating_add(self.settle))
    }

    /// Animation already owns this refresh; output silence cannot delay it.
    pub fn may_present(&self, tick: Duration, animation: bool) -> bool {
        animation || self.may_snapshot(tick)
    }

    /// Checks a supplied desktop deadline or browser rAF tick.
    pub fn may_snapshot(&self, tick: Duration) -> bool {
        self.earliest().is_some_and(|earliest| tick >= earliest)
    }

    /// A successful presentation closes the burst without resetting output silence.
    pub fn presented(&mut self, _now: Duration) {
        self.burst = None;
    }
}

#[cfg(test)]
mod tests;
