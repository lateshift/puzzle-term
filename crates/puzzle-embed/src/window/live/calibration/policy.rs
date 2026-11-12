//! Bounded acquire policy for an unchanged-frame phase attempt (§10.4).
//! Stops at the first >1-ms wait and cannot run indefinitely when startup is unpaced.
use std::time::Duration;

/// One idle attempt consumes at most drawable count + 1 unchanged presents.
pub(super) struct Calibration {
    /// The maximum includes the acquisition after every drawable was filled.
    bound: u32,
    /// Successfully acquired unchanged images in this attempt.
    presents: u32,
    /// A wait strictly longer than one millisecond identifies a phase sample.
    qualified: bool,
}

/// Tracks only this attempt; the Pacer owns learned phase and retry timing.
impl Calibration {
    /// Uses the same configured latency as surface setup, without a second default.
    pub(super) fn new(latency: u32) -> Self {
        // The extra acquire follows filling every Metal drawable (latency + 1).
        Self {
            bound: latency.saturating_add(2),
            presents: 0,
            qualified: false,
        }
    }

    /// Allows another retained-image present only while this attempt needs a sample.
    pub(super) fn can_present(&self) -> bool {
        !self.qualified && self.presents < self.bound
    }

    /// Records one real acquire; a short wait cannot manufacture a phase.
    pub(super) fn acquired(&mut self, blocked: Duration) {
        // Count actual acquisitions, including the qualifying final one.
        self.presents = self.presents.saturating_add(1);
        self.qualified = blocked > Duration::from_millis(1);
    }

    /// Reports actual work for diagnostic evidence, never an inferred queue depth.
    pub(super) fn presents(&self) -> u32 {
        self.presents
    }

    /// Only a qualifying acquire may update the display phase.
    pub(super) fn qualified(&self) -> bool {
        self.qualified
    }
}
