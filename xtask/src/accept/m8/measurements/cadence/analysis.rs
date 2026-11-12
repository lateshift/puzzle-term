//! Per-run cadence arithmetic, independent of files and deferral policy (D-36).
//!
//! Keep the original duration, count and edge checks. A one-and-a-half-period
//! allowance admits timestamp jitter, but rejects a complete missed refresh.
//! Missing refreshes count empty slots inside long gaps, not merely slow gaps.
//! Leading/trailing omissions remain separate failures; they are not silently
//! reclassified as interior misses. CPU present return is not physical scanout.
//! Evidence validity and cadence success are different results: malformed data
//! cannot give a rate, whereas a complete slow run remains useful D-36 evidence.
//! The caller decides whether cadence is deferred; this module always records
//! every failed bound so normal Chrome/60-Hz gates can reject the same assessment.
//! Independent hardware Hz is compared to the required Hz before any PASS.
//! The required rate defines the slot count, even when submissions are slower.
//! A count deficit catches regular slow pacing that fits the individual-gap bound.
//! An edge failure catches a truncated beginning/end even when the count passes.
//! An interior miss and an edge failure may occur in different repetitions;
//! aggregating them separately preserves the reviewer's two-stall observation.
//! No percentile or median removes a long interval or forgives a missing slot.
//! The nearest-period slot estimate is diagnostic; acceptance still uses the
//! original maximum-gap comparison, including its exact 12.5-ms 120-Hz ceiling.
use super::{number, numbers};
use serde_json::Value;

/// One complete interval, including diagnostics for bounds that remain unchanged.
pub(in crate::accept) struct Run {
    /// Successful submissions, including samples adjacent to a stall.
    pub(in crate::accept) frames: usize,
    /// Floor of duration times required Hz: nominal refresh opportunities.
    pub(in crate::accept) expected: usize,
    /// Empty interior refresh slots, rounded to the nearest monitor period.
    pub(in crate::accept) missed: usize,
    /// Largest interval between any pair of submissions, in milliseconds.
    pub(in crate::accept) max_gap: f64,
    /// Largest leading or trailing interval, in milliseconds.
    pub(in crate::accept) max_edge: f64,
    /// Whether either edge exceeds the existing jitter allowance.
    pub(in crate::accept) edge_failure: bool,
    /// Full recorded duration; no interval is shortened to hide a slow frame.
    pub(in crate::accept) elapsed: f64,
    /// All failed bounds; an empty list means the complete run passes.
    failures: Vec<String>,
}

/// Reading valid data does not establish that its cadence meets the budget.
impl Run {
    /// Produces every violated bound instead of stopping at the first slow gap.
    pub(in crate::accept) fn require_clean(&self) -> Result<(), String> {
        if self.failures.is_empty() {
            Ok(())
        } else {
            Err(self.failures.join("; "))
        }
    }
}

/// Validates evidence, then assesses all unchanged cadence bounds for this rate.
///
/// `actual_hz` comes from monitor metadata or rAF, never from submission rate.
/// Errors mean an incomplete/malformed recording, not a deferred cadence miss.
pub(in crate::accept) fn evaluate(
    value: &Value,
    actual_hz: f64,
    required_hz: f64,
) -> Result<Run, String> {
    let elapsed = number(value, "elapsed_ms")?;
    let stamps = numbers(value, "submission_ms")?;
    if !actual_hz.is_finite() || actual_hz <= 0.0 || !matches!(required_hz, 60.0 | 120.0) {
        return Err("invalid independent monitor/refresh rate".into());
    }
    // A short or unsorted interval cannot supply an honest missed-refresh rate.
    if elapsed < 2000.0 || stamps.len() < 2 {
        return Err("recording must contain at least two submissions over >=2000 ms".into());
    }
    if stamps[0] < 0.0 || stamps[stamps.len() - 1] > elapsed {
        return Err("submission outside recorded interval".into());
    }
    let period = 1000.0 / required_hz;
    let tolerance = 1.5 * period;
    let mut max_gap = 0.0_f64;
    let mut missed = 0;
    for pair in stamps.windows(2) {
        if pair[1] <= pair[0] {
            return Err("non-monotonic submission timestamps".into());
        }
        let gap = pair[1] - pair[0];
        max_gap = max_gap.max(gap);
        // Count two absent refreshes when one gap spans three monitor periods.
        if gap > tolerance {
            missed += (gap / period).round() as usize - 1;
        }
    }
    // Floor matches the original finite-interval check; one boundary frame is allowed.
    let expected = (elapsed * required_hz / 1000.0).floor() as usize;
    // Include both interval boundaries: an otherwise smooth trace may end early.
    let max_edge = stamps[0].max(elapsed - stamps[stamps.len() - 1]);
    let failures = bounds(
        actual_hz,
        required_hz,
        max_gap,
        max_edge,
        stamps.len(),
        expected,
    );
    Ok(Run {
        frames: stamps.len(),
        expected,
        missed,
        max_gap,
        max_edge,
        edge_failure: max_edge > tolerance,
        elapsed,
        failures,
    })
}

/// Applies the previous acceptance bounds without changing their jitter tolerance.
/// The 60-Hz criterion uses the same 1.5-period definition already used in the
/// two-display investigation; the 120-Hz ceiling remains exactly 12.5 ms.
fn bounds(
    actual_hz: f64,
    required_hz: f64,
    max_gap: f64,
    max_edge: f64,
    frames: usize,
    expected: usize,
) -> Vec<String> {
    let tolerance = 1.5 * 1000.0 / required_hz;
    let mut failures = Vec::new();
    // These are the previous gate's four independent bounds, applied to every run.
    if actual_hz < required_hz {
        failures.push(format!("actual refresh {actual_hz:.3} Hz < {required_hz}"));
    }
    if max_gap > tolerance {
        failures.push(format!("maximum gap {max_gap:.6} ms > {tolerance:.3}"));
    }
    if max_edge > tolerance {
        failures.push(format!("maximum edge {max_edge:.6} ms > {tolerance:.3}"));
    }
    if frames + 1 < expected {
        failures.push(format!("{} submissions + 1 < {expected} refreshes", frames));
    }
    failures
}
