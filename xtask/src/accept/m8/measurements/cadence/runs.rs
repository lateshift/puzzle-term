//! Retained recording cohorts: every scroll-*.json participates (D-36, R14).
//!
//! A cohort names one runtime/probe revision and target display. Historic A/B
//! experiments and superseded probe revisions remain elsewhere in target/.
//! No passing-run filter or last-artifact shortcut is allowed. Bad recordings
//! fail evidence validation; valid slow recordings still contribute statistics.
//! Native 120 Hz deliberately returns valid measurements with cadence failures:
//! marking them as data errors would convert the user's deferral back into FAIL.
//! Missing, stale or malformed data instead remains an evidence error, because
//! it cannot support the missed-refresh rate that D-36 requires M8 to report.
//! All paths are discovered before validation; content never influences inclusion.
//! Stable sorting makes logs reviewable without choosing the last clean record.
//! Nominal opportunities, rather than successful submissions, form the rate's
//! denominator: a dropped frame must not also disappear from the expected count.
//! Summing per-run floor counts preserves the existing finite-interval convention.
//! Edge failures have their own count, because a long trailing interval is not
//! a measured pair of submissions and is absent from the interior-miss metric.
//! The maximum edge and failed-run count keep those omissions visible nonetheless.
//! Current cohorts have explicit directories so older experimental settings or
//! earlier probe revisions cannot accidentally enter the adopted-build result.
//! Historic files stay retained in their original directories for investigation.
//! Every future repeat must be saved as another scroll JSON in the same cohort.
//! The local freshness reader checks the runtime inputs each record exercised;
//! edits to this reporting code do not pretend that runtime observations changed.
//! Chrome's rAF clock and the native monitor clock are independent provenance:
//! neither is guessed from a submission sequence that may itself be dropping.
//! Five native repeats are required even for deferral; Chrome includes every
//! result from the current corrected probe, with at least one complete recording.
//! A malformed record does not stop the loop from assessing later valid records.
//! The final error preserves their summary as well as every invalid filename.
/// Discovery guards ensure every repeat is retained before cadence arithmetic.
#[cfg(test)]
mod tests;

use super::{
    analysis::{self, Run},
    chrome_rate, number, recording,
};
use std::{fs, path::PathBuf};

/// Retained paths and their complete assessments, in stable filename order.
#[derive(Default)]
pub(super) struct Summary {
    /// Names let a failure identify every affected repeat instead of one example.
    pub(super) runs: Vec<(String, Run)>,
}

/// Aggregation describes evidence; the caller separately chooses D-36 deferral.
impl Summary {
    /// Counts misses over all nominal refresh opportunities, including clean runs.
    pub(super) fn describe(&self) -> String {
        let frames: usize = self.runs.iter().map(|(_, r)| r.frames).sum();
        let expected: usize = self.runs.iter().map(|(_, r)| r.expected).sum();
        let missed: usize = self.runs.iter().map(|(_, r)| r.missed).sum();
        let failed = self
            .runs
            .iter()
            .filter(|(_, r)| r.require_clean().is_err())
            .count();
        let edges = self.runs.iter().filter(|(_, r)| r.edge_failure).count();
        let max_gap = self.runs.iter().map(|(_, r)| r.max_gap).fold(0.0, f64::max);
        let max_edge = self
            .runs
            .iter()
            .map(|(_, r)| r.max_edge)
            .fold(0.0, f64::max);
        let elapsed: f64 = self.runs.iter().map(|(_, r)| r.elapsed).sum();
        // The denominator is the sum of per-run floor(duration * required Hz).
        // Edge/count failures remain explicit even when no interior slot is missing.
        let rate = if expected == 0 {
            0.0
        } else {
            100.0 * missed as f64 / expected as f64
        };
        format!(
            "runs={} frames={frames} expected_refreshes={expected} missed_refreshes={missed} missed_rate_pct={rate:.6} failed_runs={failed} edge_failures={edges} max_gap_ms={max_gap:.6} max_edge_ms={max_edge:.6} elapsed_ms={elapsed:.6}",
            self.runs.len()
        )
    }

    /// Normal criteria require every run to satisfy every original bound.
    /// Reports the aggregate even on failure, then names each failed repetition.
    /// Unlike the native-120 deferral, normal acceptance cannot discard a failed
    /// earlier run simply because later samples demonstrate a clean interval.
    pub(super) fn require_clean(&self) -> Result<(), String> {
        println!("M8_METRIC cadence {}", self.describe());
        let failures: Vec<_> = self
            .runs
            .iter()
            .filter_map(|(path, run)| {
                run.require_clean()
                    .err()
                    .map(|reason| format!("{path}: {reason}"))
            })
            .collect();
        if failures.is_empty() && !self.runs.is_empty() {
            Ok(())
        } else {
            Err(format!("{}; {}", self.describe(), failures.join("; ")))
        }
    }
}

/// Discovers the complete cohort; unrelated reports/preflights are not scroll runs.
/// Missing directories, bad directory entries and insufficient repeats are errors.
fn paths(directory: &str, minimum: usize) -> Result<Vec<PathBuf>, String> {
    let mut result = Vec::new();
    for entry in fs::read_dir(directory).map_err(|e| format!("{directory}: {e}"))? {
        let path = entry.map_err(|e| e.to_string())?.path();
        if path
            .file_name()
            .and_then(|s| s.to_str())
            .is_some_and(|s| s.starts_with("scroll-") && s.ends_with(".json"))
        {
            result.push(path);
        }
    }
    result.sort();
    // Count all discovered records, never only those that later pass arithmetic.
    if result.len() < minimum {
        return Err(format!(
            "{directory}: {} runs < required {minimum}",
            result.len()
        ));
    }
    Ok(result)
}

/// Loads every record even after a malformed one, preserving the complete inventory.
/// `directory` scopes one build/display; `minimum` prevents missing repeats.
/// `browser` selects both source inputs and the independent rAF clock reader.
/// Evidence errors accumulate separately from each valid run's cadence failures.
fn load(directory: &str, hz: f64, minimum: usize, browser: bool) -> Result<Summary, String> {
    let mut summary = Summary::default();
    let mut errors = Vec::new();
    for path in paths(directory, minimum)? {
        let name = path.to_string_lossy().into_owned();
        match assess(&name, hz, browser) {
            Ok(run) => {
                // Include slow runs in all aggregates; this diagnostic is not a PASS.
                println!(
                    "M8_METRIC cadence_run path={name} frames={} missed_refreshes={} max_gap_ms={:.6} max_edge_ms={:.6} clean={}",
                    run.frames,
                    run.missed,
                    run.max_gap,
                    run.max_edge,
                    run.require_clean().is_ok()
                );
                summary.runs.push((name, run));
            }
            Err(error) => errors.push(format!("{name}: {error}")),
        }
    }
    if errors.is_empty() {
        Ok(summary)
    } else {
        // Report valid observations too, but never accept a partially loaded cohort.
        Err(format!(
            "{}; invalid recordings: {}",
            summary.describe(),
            errors.join("; ")
        ))
    }
}

/// Hardware and source provenance are checked before assessing timestamp bounds.
/// Returned Run may violate timing bounds; only invalid provenance/data is Err.
/// This distinction lets D-36 report honest measurements without certifying them.
fn assess(path: &str, hz: f64, browser: bool) -> Result<Run, String> {
    let value = recording(path, browser)?;
    let actual = if browser {
        chrome_rate(&value)?
    } else {
        let actual = number(&value, "monitor_millihertz")? / 1000.0;
        // A different display belongs in a separate cohort, not the 60/120-Hz total.
        if actual != hz {
            return Err(format!("monitor {actual} Hz does not match cohort {hz} Hz"));
        }
        actual
    };
    let scroll = if browser {
        &value["m8"]["scroll"]
    } else {
        &value
    };
    analysis::evaluate(scroll, actual, hz)
}

/// Native evidence requires the complete five-repeat cohort for its named display.
pub(super) fn native(directory: &str, hz: f64, minimum: usize) -> Result<Summary, String> {
    load(directory, hz, minimum, false)
}

/// Chrome uses every retained current-probe result and independently measured rAF.
pub(super) fn chrome(directory: &str) -> Result<Summary, String> {
    load(directory, 120.0, 1, true)
}
