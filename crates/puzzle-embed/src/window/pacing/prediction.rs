//! Predicted refresh reservations and FIFO calibration for Windows/Linux (D-35).
//!
//! macOS never calls these policies: its native link supplies physical clocks.
//! Injected tests retain the old timer contracts on every platform so replacing
//! one source cannot silently alter the other platforms' phase/settling rules.
//! A qualifying acquisition corrects the grid while preserving consumed indices.
//! Unknown phase reserves a full period between submission starts. Idle attempts
//! retry only after a fresh quiet opportunity; no-sample attempts invent no phase.
//! Timing constants here are the existing D-33/D-35 rules, unchanged by D-53.
use super::*;

/// Prediction owns no snapshots or surfaces; callers supply measured observations.
impl Pacer {
    /// Idle visible windows refresh their phase without a terminal snapshot.
    #[cfg(any(not(target_os = "macos"), test))]
    pub fn calibration_due(&self, now: Instant, visible: bool) -> Option<Instant> {
        // Calibration only reuses an already presented image. Active damage and
        // startup cannot lend that image to an extra idle calibration submission.
        // A host-driven surface already has authoritative phase and period;
        // probing its FIFO here would create an unsolicited second clock.
        if self.linked
            || !visible
            || self.first
            || self.due.is_some()
            || self.last_presented.is_none()
        {
            return None;
        }
        let idle = self
            .last_output
            .map_or(now, |last| last + Duration::from_millis(250));
        let retry = self.last_calibration.map_or(now, |last| {
            last + if self.calibration_sampled {
                Duration::from_secs(5)
            } else {
                // A whole visible/idle opportunity prevents spinning on an early
                // free FIFO, without withholding phase learning for five seconds.
                Duration::from_millis(250)
            }
        });
        let stale = self
            .phase_observed
            .map_or(now, |last| last + Duration::from_secs(5));
        Some(idle.max(retry).max(stale))
    }

    /// Learns qualified phase; a no-sample attempt retries at the next idle opportunity.
    #[cfg(any(not(target_os = "macos"), test))]
    pub fn calibrated(&mut self, now: Instant, sample: Option<Instant>) {
        // Record attempts separately from samples: an unblocked acquire must not
        // manufacture a phase or delay another opportunity by the five-second age.
        self.last_calibration = Some(now);
        self.calibration_sampled = sample.is_some();
        if let Some(phase) = sample {
            self.observe_phase(phase);
            self.phase_observed = Some(now);
        }
    }

    /// Corrects phase while preserving the identity of already consumed refreshes.
    #[cfg(any(not(target_os = "macos"), test))]
    pub fn observe_phase(&mut self, at: Instant) {
        if let Some(phase) = self.phase {
            // Acquisition-return jitter shifts the anchor by a fraction of a turn.
            // Round to the nearest old edge to keep the sequence's index identity.
            let elapsed = at.saturating_duration_since(phase);
            let turns = (elapsed + self.period / 2).as_nanos() / self.period.as_nanos();
            let turns = u32::try_from(turns).unwrap_or(u32::MAX);
            let predicted = phase + self.period.saturating_mul(turns);
            // A later observation corrects an early prediction. Earlier return
            // jitter cannot make the following target less than one full period
            // after its predecessor. Keep the anchor's index as well as its grid.
            self.phase = Some(phase + at.saturating_duration_since(predicted));
        } else {
            self.phase_index = self.last_refresh_index.unwrap_or(0);
            // The first qualified sample may precede its reserved target by a
            // small amount. Preserve that target as the first clock edge too.
            self.phase = Some(self.last_refresh_target.map_or(at, |last| at.max(last)));
        }
    }

    /// Computes one snapshot deadline and its stable predicted refresh index.
    pub fn next_due(&self, now: Instant) -> (Instant, Option<(u64, Instant)>) {
        // Settling constrains snapshot time; the lead relates that time to scanout.
        let earliest = self
            .settle
            .earliest()
            .filter(|_| !self.animation)
            .map_or(now, |elapsed| self.origin + elapsed)
            .max(now);
        let lead = self.lead();
        // Every clock mode reserves a refresh at least one period after the last.
        let minimum = self.last_refresh_target.map(|last| last + self.period);
        let Some(phase) = self.phase else {
            // Do not invent a refresh prediction. Admit requested work at most
            // once per period; FIFO backpressure provides the first phase sample.
            let due = earliest.max(minimum.unwrap_or(earliest));
            let index = self
                .last_refresh_index
                .map_or(0, |last| last.saturating_add(1));
            return (due, Some((index, due)));
        };
        let earliest = earliest.max(minimum.map_or(earliest, |at| at - lead));
        let elapsed = earliest.saturating_duration_since(phase);
        let turns = (elapsed + lead)
            .as_nanos()
            .div_ceil(self.period.as_nanos())
            .max(1);
        let candidate = self
            .phase_index
            .saturating_add(turns.min(u128::from(u64::MAX)) as u64);
        // Structural one-submission rule: even a smaller lead or corrected phase
        // cannot select the index already consumed by the previous snapshot.
        let index = candidate.max(
            self.last_refresh_index
                .map_or(0, |last| last.saturating_add(1)),
        );
        let turns = u32::try_from(index.saturating_sub(self.phase_index)).unwrap_or(u32::MAX);
        let target = phase + self.period.saturating_mul(turns);
        (target - lead, Some((index, target)))
    }
}
