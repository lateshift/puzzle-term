//! Validates every raw submission in each retained scrolling cohort (D-36).
//!
//! Native 60 Hz and Chrome 120 Hz retain normal all-run PASS/FAIL criteria.
//! Native 120 Hz reports its complete missed-refresh count/rate as DEFERRED
//! to M12. Evidence validity is checked independently of cadence deferral.
//! Directory discovery never chooses only the last or cleanest recording.
//! Current-build/probe cohorts retain failures; historical comparison runs
//! stay in their original directories and are not substituted for fresh data.
//! Missing, stale or malformed records fail instead of borrowing old PASSes.
//! Artifact booleans and precomputed gaps are ignored: xtask recomputes them.
//! Duration, count and both interval edges prevent truncated or slow evidence.
//! A half-period jitter allowance still rejects a full missed monitor refresh.
//! Native metadata and actual Chrome rAF establish the independent target rate.
//! Submission return times do not establish GPU completion or physical scanout.
//! Human scrolling feel remains a separately confirmed review (R15).
//! Source freshness names the exact input requiring a new hardware recording.
//! Ignored target evidence remains local development data, never packaged.
//! Finite validation precedes comparisons so NaN cannot evade a timing bound.
//! Freshness walks only the runtime crates and their manifests, plus the web
//! dev scripts that own wheel timing. Unrelated TypeScript edits and build-cache
//! churn cannot invalidate a hardware measurement of the unchanged runtime.
//! Rebuilding or editing an exercised source requires recording that build again.
//! A full interval starts with the first scrolling request and ends at the
//! recorded stop time. Omitting either boundary could hide a missed first/last
//! refresh despite regular interior gaps. The count also rejects uniform 100-fps
//! pacing on a 120-Hz display even when each individual gap is below 12.5 ms.
//! Search CPU measurements are independent: fast assembly cannot certify that
//! the surface scheduler presented at every requested monitor refresh.
use serde_json::Value;
use std::{fs, path::Path, time::SystemTime};
/// Pure per-run arithmetic keeps bounds identical for every retained sample.
pub(in crate::accept) mod analysis;
/// Directory cohorts prevent the last clean run from hiding earlier stalls.
mod runs;
/// Gate regressions reject sparse, truncated and malformed recordings.
#[cfg(test)]
mod tests;

/// Reads fresh source-backed evidence; newer source requires a new recording.
fn recording(path: &str, browser: bool) -> Result<Value, String> {
    let recorded = fs::metadata(path)
        .and_then(|m| m.modified())
        .map_err(|e| format!("{path}: {e}"))?;
    // Creation before this source revision would make the numbers unverifiable here.
    // These crates define the actual input, scheduler, snapshot, fonts and executor.
    // Ignore target builds and unrelated TypeScript rather than timing repository churn.
    for package in [
        "puzzle-term",
        "puzzle-vt",
        "puzzle-text",
        "puzzle-render",
        "puzzle-frame",
        "puzzle-slug",
    ] {
        fresh(Path::new(&format!("crates/{package}/src")), recorded)?;
        fresh(Path::new(&format!("crates/{package}/Cargo.toml")), recorded)?;
    }
    let host = if browser {
        "crates/puzzle-web"
    } else {
        "crates/puzzle-desktop"
    };
    fresh(Path::new(&format!("{host}/src")), recorded)?;
    fresh(Path::new(&format!("{host}/Cargo.toml")), recorded)?;
    if browser {
        // The page scripts drive wheel timing and own the recorded interval's bounds.
        fresh(Path::new("dev"), recorded)?;
    }
    serde_json::from_str(&fs::read_to_string(path).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())
}

/// Walks only source inputs; generated recordings and compiler caches stay excluded.
fn fresh(path: &Path, recorded: SystemTime) -> Result<(), String> {
    if path.is_dir() {
        // Recursion starts at source-only roots, excluding generated dist bindings.
        for entry in fs::read_dir(path).map_err(|e| e.to_string())? {
            fresh(&entry.map_err(|e| e.to_string())?.path(), recorded)?;
        }
    } else if fs::metadata(path)
        .and_then(|m| m.modified())
        .map_err(|e| e.to_string())?
        > recorded
    {
        return Err(format!(
            "recording predates {}; record this build again",
            path.display()
        ));
    }
    Ok(())
}

/// Rejects absent/non-finite values before doing any acceptance arithmetic.
fn number(value: &Value, name: &str) -> Result<f64, String> {
    value[name]
        .as_f64()
        .filter(|n| n.is_finite())
        .ok_or_else(|| format!("missing/invalid {name}"))
}

/// Numeric arrays reject null/strings rather than silently dropping bad samples.
fn numbers(value: &Value, name: &str) -> Result<Vec<f64>, String> {
    value[name]
        .as_array()
        .ok_or_else(|| format!("missing {name}"))?
        .iter()
        .map(|v| {
            v.as_f64()
                .filter(|n| n.is_finite())
                .ok_or_else(|| format!("invalid {name} time"))
        })
        .collect()
}

/// Recomputes monotonicity, interior gaps, total sample count and both edge gaps.
#[cfg(test)]
fn submissions(value: &Value, hz: f64) -> Result<(), String> {
    analysis::evaluate(value, hz, 120.0)?.require_clean()
}

/// D-36 defers cadence, while malformed or missing evidence still fails its own check.
pub(super) fn desktop_120(passed: &mut bool) {
    let result = runs::native("target/m8-d35-native", 120.0, 5);
    super::super::super::report(
        "M8.8 all retained native 120 Hz recordings are valid",
        result.as_ref().map(|_| ()).map_err(Clone::clone),
        passed,
    );
    // This cadence obligation is neither a PASS nor a FAIL in M8 (D-36).
    let evidence = result.map_or_else(|error| error, |summary| summary.describe());
    println!("DEFERRED M8.8 native 120 Hz scrolling to M12 (D-36): {evidence}");
}

/// Native 60 Hz keeps the same one-and-a-half-period gap and edge allowance.
pub(super) fn desktop_60() -> Result<(), String> {
    runs::native("target/m8-d36-native-60", 60.0, 5)?.require_clean()
}

/// Chrome's current-probe cohort includes every run, not its latest artifact alone.
pub(super) fn chrome() -> Result<(), String> {
    runs::chrome("target/m8-d36-chrome-120")?.require_clean()
}

/// Chrome evidence independently derives refresh cadence from every actual rAF tick.
fn chrome_rate(value: &Value) -> Result<f64, String> {
    if !value["userAgent"]
        .as_str()
        .is_some_and(|s| s.contains("Chrome/"))
    {
        return Err("recording is not identified as Chrome".into());
    }
    let scroll = &value["m8"]["scroll"];
    let ticks = numbers(scroll, "raf_ms")?;
    let mut gaps = Vec::new();
    for pair in ticks.windows(2) {
        if pair[1] <= pair[0] {
            return Err("non-monotonic rAF timestamps".into());
        }
        gaps.push(pair[1] - pair[0]);
    }
    if gaps.is_empty() {
        return Err("empty refresh recording".into());
    }
    gaps.sort_by(f64::total_cmp);
    // Median estimates the refresh clock only; submission acceptance uses every gap.
    Ok(1000.0 / gaps[gaps.len() / 2])
}
