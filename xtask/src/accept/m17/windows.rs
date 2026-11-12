//! Real two-window App acceptance on the explicitly witnessed LG display.
//!
//! One surface floods a reusable glyph set while its sibling scrolls real PTY
//! history and sends keys to its own child. Both share one App and render actor.
//! The no-flood control has identical geometry, input schedule and source policy.
//! The harness writes every interval and paired owner/writer clock, including on
//! a budget failure. A failed subprocess is reported without retrying its cohort.
//! This task selector does not run the unfinished full M17 milestone inventory.
//! Native host availability is a setup requirement, never silently substituted.
//! The user currently has the laptop lid closed, so no 120-Hz result is claimed.
//! The control's idle first view has no flood fps requirement; its sibling still
//! has every cadence and key check. Treatment adds the ordinary thirty-fps floor.
//! A line cannot pass from an empty frame or key list: the binary checks counts.
//! Requested display placement is verified again before recording begins.
//! The report keeps display identity, nominal rate, DPR and backing extent.
//! Native input or closing an owned window invalidates a run instead of trimming it.
//! No Chrome process runs concurrently with this native GPU gate (M17.test-gates).
use crate::accept::report;
use std::{path::PathBuf, process::Command};

/// Build once and run each condition exactly once, keeping separate raw directories.
pub(super) fn run(passed: &mut bool) {
    // Instrumentation supplies observations; normal product scheduling is unchanged.
    let build = crate::commands::quiet(
        "cargo",
        &[
            "build",
            "--release",
            "-p",
            "puzzle-desktop",
            "--features",
            "lock-metrics",
        ],
    );
    if let Err(error) = build {
        report("two-window integration build", Err(error), passed);
        return;
    }
    // Time-named directories keep previous failures and passing controls immutable.
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |time| time.as_nanos());
    let root = PathBuf::from(format!("target/m17-two-windows/accept-{stamp}"));
    for condition in ["control", "flood"] {
        let out = root.join(condition);
        // report continues to the other criterion, but never retries a failed run.
        report(
            &format!("two-window shared App {condition}: cadence and key budgets"),
            cohort(condition, &out),
            passed,
        );
    }
}

/// Actual native windows use the supported Surface calls; counters only observe them.
fn cohort(condition: &str, out: &std::path::Path) -> Result<(), String> {
    std::fs::create_dir_all(out).map_err(|error| error.to_string())?;
    // Private fixture settings avoid dependence on a user's running shell or profile.
    // The font matches the user's M16 comparison while both contexts remain identical.
    let config = out.join("config.json");
    std::fs::write(&config, "{\"font\":{\"size\":15}}\n").map_err(|e| e.to_string())?;
    let output = Command::new("target/release/puzzle-desktop")
        .args(["run", "--config"])
        .arg(&config)
        .env("PUZZLE_EMBED_PROBE", condition)
        .env("PUZZLE_EMBED_PROBE_OUT", out)
        .output()
        .map_err(|error| error.to_string())?;
    // Preserve logs before checking status, including interrupted or invalid placements.
    let mut log = output.stdout;
    log.extend_from_slice(&output.stderr);
    std::fs::write(out.join("run.log"), &log).map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err(format!(
            "{}: {}",
            out.display(),
            String::from_utf8_lossy(&log)
        ));
    }
    // The executable enforces exact keys and budgets; also require its actual receipt.
    let bytes = std::fs::read(out.join("report.json")).map_err(|e| e.to_string())?;
    let value: serde_json::Value = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    if value["pass"] != true {
        return Err(format!("{}: {}", out.display(), value["failure"]));
    }
    // Concise metrics link the passing line to every raw interval and paired clock.
    let sibling = &value["surfaces"][1];
    println!(
        "M17_METRIC condition={condition} sibling_fps={} writer_max_ms={} evidence={}",
        sibling["fps"],
        sibling["pipeline"]["controls_and_keys"]["key_to_writer_max_ms"],
        out.display()
    );
    Ok(())
}
