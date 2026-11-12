//! Warm full-screen Unicode CPU fits the approved 120-Hz refresh allowance.
//! The interval is 10–30 seconds after producer release, following cold discovery.
//! Only actual presented full-row builds contribute to this CPU distribution.
//! Raw attempts and observers remain in the original report; cadence evidence
//! never discards readback stalls or uses this CPU filter to inflate measured fps.
//! Upload/submit includes queue writes, encoding and present, excluding FIFO wait.
//! Its p95 is a conservative measured reserve beside build p95 and the D-33 margin.
use serde_json::Value;

/// Load only the fresh maximized complete-ink cohort from this acceptance run.
/// The window owner removes its labelled report before launching, so a closed
/// window cannot inherit a prior pass. Timing-only probes cannot satisfy ink.
pub(super) fn matched() -> Result<Value, String> {
    let bytes = std::fs::read("target/m16-2/bitmap-maximized.json").map_err(|e| e.to_string())?;
    let value: Value = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    if value["unicode_stress_pass"] != true || value["cadence_only"] == true {
        return Err("fresh maximized complete-ink cohort failed".into());
    }
    Ok(value)
}

/// Nearest-rank percentiles include every supplied finite nonnegative sample.
/// The caller chooses its workload, never individual fast or successful timings.
/// Empty/invalid data fails rather than supplying a zero-cost reserve.
pub(super) fn percentile(values: &mut [f64], fraction: f64) -> Result<f64, String> {
    if values.is_empty() || values.iter().any(|v| !v.is_finite() || *v < 0.0) {
        return Err("empty or invalid measurement distribution".into());
    }
    values.sort_by(f64::total_cmp);
    let index = (values.len() as f64 * fraction).ceil() as usize;
    Ok(values[index.saturating_sub(1).min(values.len() - 1)])
}

/// A full-screen cache miss rebuilds as many rows as the initial full redraw.
/// Require that count in the warm producing interval, so unchanged frames and
/// one-row cursor updates cannot stand in for the approved new-Unicode row.
pub(super) fn run() -> Result<(), String> {
    // The earlier maximized cohort establishes ink. GPU reads take ~470 ms
    // here and perturb later CPU caches, so timing gets its own ordinary cohort.
    matched()?;
    let value = super::super::super::complete::maximized_cadence()?;
    assess(&value)
}

/// Reuses the identical budget arithmetic for an explicitly authorized A/B receipt.
/// This checks CPU observations only; it cannot replace the complete-ink witness.
pub(in crate::accept) fn assess(value: &Value) -> Result<(), String> {
    if value["cadence_only"] != true || value["aborted_builds"] != 0 {
        return Err("ordinary cadence cohort absent or build aborted".into());
    }
    let attempts = value["frame_gates"]["attempts"]
        .as_array()
        .ok_or("frame chronology absent")?;
    // The initial cold redraw invalidates every visible row. Its maximum rebuild
    // count supplies this window's full height, avoiding a fixed monitor geometry.
    let full_rows = attempts
        .iter()
        .filter_map(|a| a["rebuilt_rows"].as_u64())
        .max()
        .filter(|rows| *rows > 0)
        .ok_or("full-row count absent")?;
    // Build and submit clocks belong to the same set of newly exposed screens.
    // This prevents cheaper cursor/blink presents from reducing either percentile.
    let mut builds = Vec::new();
    let mut submissions = Vec::new();
    for attempt in attempts {
        // Full chronology stays on disk, including surface retries and readback costs.
        if attempt["outcome"] == "presented"
            && attempt["rebuilt_rows"] == full_rows
            && attempt["start_ms"]
                .as_f64()
                .is_some_and(|t| (10_000.0..30_000.0).contains(&t))
        {
            builds.push(attempt["build_ms"].as_f64().ok_or("build CPU absent")?);
            submissions.push(
                attempt["upload_submit_ms"]
                    .as_f64()
                    .ok_or("upload/submit CPU absent")?,
            );
        }
    }
    // The diagnostic collects scalars during the run; sorting and allocation here
    // occur after the process closes and cannot influence a presentation interval.
    let count = builds.len();
    let median = percentile(&mut builds, 0.5)?;
    let p95 = percentile(&mut builds, 0.95)?;
    let submit_p95 = percentile(&mut submissions, 0.95)?;
    // 120 Hz gives 8⅓ ms; the existing D-33 wake margin reserves one ms.
    let period = 1000.0 / 120.0;
    // The threshold is derived from the approved refresh period and measured cost,
    // rather than a separately tuned Unicode p95 ceiling.
    let required = p95 + 1.0 + submit_p95;
    println!(
        "M16.5 warm full {full_rows}-row builds: n={count}, median={median:.6} ms, p95={p95:.6}; upload/submit p95={submit_p95:.6}; sum with D-33 margin={required:.6}/{period:.6} ms"
    );
    if median > 2.0 || required > period {
        return Err(format!(
            "median {median:.6} >2 ms or p95+margin+submit {required:.6} >{period:.6} ms"
        ));
    }
    Ok(())
}
