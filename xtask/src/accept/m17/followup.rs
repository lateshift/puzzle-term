//! Targeted closure receipts, authorized after the single full M17 acceptance.
//!
//! Every original failure remains in its immutable recording. These fresh
//! receipts apply the original input-based edges and unchanged warm CPU limits;
//! restart clocks are reported independently and never silently subtracted.
//! Both treatment runs of A/B/A/B must pass, rather than choosing the faster one.
//!
//! This selector does not run app-native tests, Chrome, or unaffected M16 gates.
//! The app criterion has its own native receipt after Phase B confirmation.
//! The measurements themselves run before this reader is compiled, avoiding
//! compiler load in the recorded native intervals. Full closure remains a
//! separate inventory and its original four failures remain historical facts.
use crate::accept::{
    m8::measurements::cadence::analysis, m16::throughput::budgets::frames, report,
};
use serde_json::{Value, json};
use std::path::Path;

/// Reads only the authorized affected-line recordings; launches no full sweep.
pub(super) fn run(passed: &mut bool) {
    // Separate directories preserve the failed originals and each rerun's load
    // witnesses. No mutable generic "latest scroll" artifact is read here.
    let root = Path::new("target/m17-close/followup");
    // The sibling control and single-window scroll are distinct failed lines;
    // neither receipt substitutes for the other, even on the same display.
    report(
        "M17.close two-window control rerun",
        control(&root.join("edges/control")),
        passed,
    );
    report(
        "M17.close LG edge rerun",
        scroll(&root.join("edges/lg-scroll")),
        passed,
    );
    for run in ["B1", "B2"] {
        // A1/A2 are historical-revision controls, not candidates for acceptance.
        // Testing both B runs prevents control variance from selecting a pass.
        report(
            &format!("M17.close warm Unicode {run}"),
            // The shared assessor selects full-row warm samples and applies
            // the original percentile and wake-plus-submit arithmetic.
            read(&root.join(format!("quiet-ab/{run}/native.json")))
                .and_then(|v| frames::assess(&v)),
            passed,
        );
    }
}

/// Missing or malformed evidence fails, rather than borrowing an old successful run.
fn read(path: &Path) -> Result<Value, String> {
    // Include the path in I/O failures so a missing rerun cannot resemble a
    // successful empty cohort. JSON errors also remain explicit failures.
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    serde_json::from_slice(&bytes).map_err(|e| e.to_string())
}

/// Both witnesses must place the recording on the actual LG, regardless of lid state.
fn display(directory: &Path) -> Result<(), String> {
    for when in ["before", "after"] {
        let v = read(&directory.join(format!("display-{when}.json")))?;
        // The user authorized the active external display specifically. A
        // visible built-in monitor, if present, cannot stand in for LG cadence.
        // Actual screen identity, active state, DPR and refresh all have to agree.
        if !v["screens"].as_array().is_some_and(|screens| {
            screens.iter().any(|s| {
                s["id"] == 3
                    && s["active"] == true
                    && s["backing_scale"] == 2
                    && s["maximum_fps"] == 60
            })
        }) {
            return Err("active LG CG 3 / 60 Hz / DPR 2 witness absent".into());
        }
    }
    Ok(())
}

/// The native control already enforces exact keys, both key limits and sibling cadence.
fn control(directory: &Path) -> Result<(), String> {
    display(directory)?;
    let v = read(&directory.join("report.json"))?;
    let sibling = &v["surfaces"][1];
    // Report native restart duration separately from all input-based edges.
    // Missing attribution data therefore never silently excuses a late frame.
    println!(
        "M17_METRIC control display_link={} fps={} keys={}",
        sibling["display_link"], sibling["fps"], v["child_key_ms"]
    );
    // Independently recompute every edge, count and interior bound from raw stamps.
    let assessed = analysis::evaluate(
        &json!({"elapsed_ms":2000.0,"submission_ms":sibling["submission_ms"]}),
        60.0,
        60.0,
    )?;
    println!(
        "M17_METRIC control frames={} expected={} max_gap_ms={:.6} max_edge_ms={:.6}",
        assessed.frames, assessed.expected, assessed.max_gap, assessed.max_edge
    );
    assessed.require_clean()?;
    // The native verdict also covers exact child bytes, writer enqueue and
    // child receipt latency. Cadence arithmetic alone cannot certify those.
    if v["pass"] != true || sibling["display"] != 3 || sibling["millihertz"] != 60000 {
        return Err(format!("native control failed: {}", v["failure"]));
    }
    Ok(())
}

/// Every presented frame must use its own publication on the witnessed display.
fn scroll(directory: &Path) -> Result<(), String> {
    display(directory)?;
    let v = read(&directory.join("scroll.json"))?;
    // Monitor inventory establishes physical identity; the in-window report
    // independently establishes the rate actually supplied to this probe.
    if v["monitor_millihertz"] != 60000 {
        return Err("wrong monitor rate".into());
    }
    let assessed = analysis::evaluate(&v, 60.0, 60.0)?;
    // Successful submissions have a second, independent chronology containing
    // the owner request identity. A compact timestamp list cannot prove age.
    let attempts = v["frame_gates"]["attempts"]
        .as_array()
        .ok_or("missing attempts")?;
    let mut count = 0;
    for a in attempts.iter().filter(|a| !a["submitted_ms"].is_null()) {
        count += 1;
        let c = &a["cadence"];
        let request = c["request_id"].as_u64().ok_or("missing request")?;
        // A fast old snapshot would pass cadence but violate D-50; source
        // freshness and the truthful late flag remain independent conditions.
        if c["source"]["id"].as_u64() != Some(request)
            || c["source"]["late"] != false
            || c["link"]["display"] != 3
        {
            return Err("old, late or wrong-display source".into());
        }
    }
    println!(
        "M17_METRIC LG display_link={} frames={} expected={} max_gap_ms={:.6} max_edge_ms={:.6}",
        v["display_link"], assessed.frames, assessed.expected, assessed.max_gap, assessed.max_edge
    );
    // Neither chronology may omit a submission counted by the other.
    if count != assessed.frames {
        return Err("submission/source counts differ".into());
    }
    assessed.require_clean()
}
