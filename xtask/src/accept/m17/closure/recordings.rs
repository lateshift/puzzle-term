//! All M17.refresh physical recordings remain authoritative, including misses.
//!
//! A valid built-in run is not a cadence PASS. Its unchanged bounds are evaluated
//! and printed before the explicit user disposition transfers them to M12.
//! LG still must satisfy every original bound. Both sets must prove that each
//! presented frame used its own request, independently of compositor cadence.
//!
//! These files are local witnesses, not shipped assets. Missing artifacts fail
//! closure rather than borrowing a historical run from another milestone.
//! The user authorized transfer only for built-in FIFO misses: source freshness,
//! valid display identity, LG cadence and old-display cancellation still bind.
//! In particular, fast build CPU cannot conceal a slow present or a lost edge.
//! The submission clock measures CPU return, not physical display scanout.
//! All output labels retain that distinction; no GPU timestamp is inferred.
//! The frame-attempt list carries request metadata absent from the compact list
//! of successful submissions. Requiring both lists prevents an empty source
//! series from certifying an otherwise plausible cadence recording.
//! Display switches are checked against host observations, not the renderer's
//! eventual geometry update: that delay was precisely the old-acquire defect.
//! A wait already entered before a switch remains visible if it ends afterward.
//! No parser, GPU or window behavior is changed by this evidence reader.
use crate::accept::{m8::measurements::cadence::analysis, report};
use serde_json::Value;
use std::{fs, path::Path};

/// Validate retained evidence without launching another hardware experiment.
pub(super) fn run(passed: &mut bool) {
    // CG identities are part of the witnessed machine state, not selected by fps.
    for (name, hz, display) in [("builtin", 120.0, 1), ("lg", 60.0, 3)] {
        // Separate reports prevent one malformed run from hiding the other four.
        for ordinal in 1..=5 {
            let directory = format!("target/m17-refresh/{name}-scroll-{ordinal}");
            report(
                &format!(
                    "M17.refresh {name} run {ordinal}: display, full interval and same-refresh source"
                ),
                scroll(Path::new(&directory), hz, display),
                passed,
            );
        }
    }
    // Both starting displays are included, so one direction cannot hide the other.
    report(
        "M17.refresh twelve crossings have zero old-display waits",
        crossings(),
        passed,
    );
    // Deferral names its owner and authority; it never emits a cadence PASS.
    println!(
        "DEFERRED M17.refresh built-in five-of-five cadence to M12.refresh/M12.layer by user decision 2026-10-10; unchanged bounds, every failed bound printed above"
    );
}

/// Read errors and malformed JSON are setup failures, never empty evidence.
fn json(path: &Path) -> Result<Value, String> {
    // Keep the path on both I/O and syntax errors so missing evidence is actionable.
    let data = fs::read(path).map_err(|error| format!("{}: {error}", path.display()))?;
    serde_json::from_slice(&data).map_err(|error| format!("{}: {error}", path.display()))
}

/// The observed monitor identity/rate and lid state are independent of frame rate.
fn display_state(path: &Path, display: u64, hz: f64) -> Result<(), String> {
    let value = json(path)?;
    // Read the independent NSScreen inventory taken outside the frame probe.
    let screens = value["screens"]
        .as_array()
        .ok_or("missing screen inventory")?;
    // Presence alone is insufficient: rate and DPR must match the intended cell.
    let screen = screens
        .iter()
        .find(|s| s["id"] == display)
        .ok_or("target display absent")?;
    // Both active displays are required for this same-state paired comparison.
    if value["clamshell"] != false
        || screens.len() != 2
        || screens
            .iter()
            .any(|s| s["active"] != true || s["backing_scale"] != 2)
        || screen["maximum_fps"].as_f64() != Some(hz)
    {
        return Err(format!(
            "{}: display state differs from witnessed pair",
            path.display()
        ));
    }
    // A closed lid invalidates the paired display-state comparison.
    Ok(())
}

/// Recompute cadence from every timestamp; do not trust the probe's 120-Hz boolean.
///
/// LG reports have `pass=false` because the producer always requests 120 Hz.
/// Assessing their independently observed 60 Hz avoids treating that producer
/// label as either a waiver or proof of failure at the LG's actual rate.
fn scroll(directory: &Path, hz: f64, display: u64) -> Result<(), String> {
    // Placement is witnessed at both ends, guarding accidental display movement.
    for file in ["display-before.json", "display-after.json"] {
        display_state(&directory.join(file), display, hz)?;
    }
    // This is the immutable per-run report, not the latest generic probe output.
    let value = json(&directory.join("scroll.json"))?;
    if value["monitor_millihertz"].as_f64() != Some(hz * 1000.0) {
        return Err("window was measured on the wrong refresh rate".into());
    }
    // Reuse M8's duration, monotonicity, count, leading and trailing checks exactly.
    let assessed = analysis::evaluate(&value, hz, hz)?;
    let presentations = sources(&value, display)?;
    // No submitted frame may escape the source-identity validation above.
    if presentations != assessed.frames {
        return Err("submission/source counts differ".into());
    }
    // Preserve all failed bounds in the log, including transferred built-in misses.
    let bounds = assessed.require_clean();
    println!(
        "M17_METRIC {} frames={} expected={} max_gap_ms={:.6} max_edge_ms={:.6} cadence={}",
        directory.display(),
        assessed.frames,
        assessed.expected,
        assessed.max_gap,
        assessed.max_edge,
        bounds
            .as_ref()
            .map_or_else(|error| error.as_str(), |()| "clean")
    );
    // Only the user's explicit built-in transfer permits a recorded cadence miss.
    if hz == 60.0 {
        bounds?;
    }
    Ok(())
}

/// Verify source age for every submitted attempt independently of cadence.
///
/// The compact submission list has no owner identity; this second representation
/// establishes which publication actually reached each successfully drawn frame.
fn sources(value: &Value, display: u64) -> Result<usize, String> {
    // Only attempts expose the owner publication and physical refresh identities.
    let frames = value["frame_gates"]["attempts"]
        .as_array()
        .ok_or("missing frame attempts")?;
    let mut presentations = 0;
    for frame in frames {
        // Attempts with no submission do not claim a displayed source.
        if frame["submitted_ms"].is_null() {
            continue;
        }
        // Count successful attempts independently of the compact timestamp vector.
        presentations += 1;
        let cadence = &frame["cadence"];
        // A missing request is invalid, even if two missing JSON fields compare equal.
        let request = cadence["request_id"]
            .as_u64()
            .ok_or("missing request identity")?;
        // Same-refresh means this request's own source, not merely any recent image.
        // A truthful late flag is checked separately from matching numeric IDs.
        if cadence["source"]["id"].as_u64() != Some(request)
            || cadence["source"]["late"] != false
            || cadence["link"]["display"].as_u64() != Some(display)
        {
            return Err("presented source is old, late or from another display".into());
        }
    }
    Ok(presentations)
}

/// Parse a timestamp from the raw UTC log, in seconds since midnight.
fn timestamp(line: &str) -> Result<f64, String> {
    // Traces in these short cohorts stay within one UTC day; retain microseconds.
    let time = line
        .split_once('T')
        .and_then(|(_, rest)| rest.split_once('Z'))
        .map(|(time, _)| time)
        .ok_or("missing UTC timestamp")?;
    // Invalid clock syntax cannot silently skip an acquisition in the chronology.
    let fields = time
        .split(':')
        .map(str::parse::<f64>)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| "invalid UTC timestamp")?;
    if fields.len() != 3
        || fields
            .iter()
            .any(|field| !field.is_finite() || *field < 0.0)
    {
        return Err("invalid UTC clock fields".into());
    }
    // Convert the mixed hours/minutes/seconds representation before ordering events.
    Ok(fields[0] * 3600.0 + fields[1] * 60.0 + fields[2])
}

/// Only geometry records contain this debug field; the final occurrence is new info.
fn display(line: &str) -> Result<u64, String> {
    // Geometry logs include both old and new SurfaceInfo; choose the new one.
    line.rsplit_once("display: ")
        .and_then(|(_, rest)| rest.split_once(','))
        .ok_or("missing geometry display")?
        .0
        .parse()
        .map_err(|_| "invalid display".into())
}

/// Inspect raw host/acquisition clocks rather than trusting a derived CSV count.
fn crossings() -> Result<(), String> {
    // Each run has six switches and its own independent before/after screen witness.
    for initial in [0, 1] {
        let root = format!("target/m17-refresh/after-handoff-{initial}");
        display_state(&Path::new(&root).join("display-before.json"), 1, 120.0)?;
        display_state(&Path::new(&root).join("display-after.json"), 3, 60.0)?;
        // Raw logs keep the event order; derived CSV is supplemental evidence only.
        let log = fs::read_to_string(format!("{root}/window.log")).map_err(|e| e.to_string())?;
        crossing_trace(&log)?;
    }
    Ok(())
}

/// Every crossing must retain acquisitions, but none may wait on the old display.
///
/// A render-geometry update may trail the host message by an entire frame build.
/// Keeping both identities here preserves that race in the acceptance evidence.
fn crossing_trace(log: &str) -> Result<(), String> {
    // No geometry is assumed before the first corresponding observation.
    let (mut host, mut rendered, mut moved) = (None, None, 0.0);
    let (mut crossings, mut acquisitions) = (0, 0);
    for line in log.lines() {
        if line.contains("event=\"desktop.host_moved\"") {
            let next = display(line)?;
            // An unexpected destination cannot stand in for either witnessed screen.
            if !matches!(next, 1 | 3) {
                return Err("crossing targets an unwitnessed display".into());
            }
            // Size-only moves do not change display ownership or start a new crossing.
            if host.is_some_and(|old| old != next) {
                crossings += 1;
                moved = timestamp(line)?;
            }
            // The latest host fact is authoritative even before the actor consumes it.
            host = Some(next);
        } else if line.contains("event=\"desktop.render_geometry\"") {
            // The renderer's last adopted geometry identifies the acquisition target.
            rendered = Some(display(line)?);
        } else if line.contains("stage=\"drawable_acquire\"") {
            acquisitions += 1;
            // A wait that began before the move but finished after it counts too.
            if host.is_some() && rendered.is_some() && host != rendered && timestamp(line)? > moved
            {
                return Err(format!("old-display acquisition after crossing: {line}"));
            }
        }
    }
    // A truncated log must not pass merely because it contains no offending wait.
    if crossings != 6 || acquisitions == 0 {
        return Err("six crossings with acquisitions absent".into());
    }
    Ok(())
}

/// Raw trace checks reject waits even when acquisition began before the move.
#[cfg(test)]
mod tests;
