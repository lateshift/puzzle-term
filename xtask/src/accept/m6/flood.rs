//! Acceptance of the continuous-output budget after the DOOM-fire freeze.
//!
//! A fresh release harness run measures the real Session and GPU executor
//! under a fake PTY that repeats the 80×40 half-block truecolor corpus.
//! Each metric is reported separately under R14, including when the run
//! itself fails. Human review of the OS window remains a separate R15 item.
//!
//! DOOM-fire rewrites every cell with truecolor half blocks each turn.
//! A normal line-oriented corpus would leave the parser mostly idle and
//! could pass while this application still starves the winit event loop.
//! The headless producer deliberately outruns parsing, exercising actual
//! backpressure rather than timing a single already-buffered frame.
//!
//! The report separates throughput from responsiveness. High average fps
//! can conceal a long pause during a resize, so the longest frame gap is
//! checked with it. The key timer ends at Transport::write, not at a
//! screenshot: input must reach the program without waiting for painting.
//! Pending bytes include the channel and Session's parser suffix because
//! either allocation can grow beyond the four-MiB policy limit.
//! A clean close measures the path that previously left an unresponsive
//! black window requiring an external kill.
use super::report;
use std::{collections::BTreeMap, fs, path::Path};

/// Structured measurements emitted by the headless flood scenario.
/// These are copied from a fresh artifact for one release run; acceptance
/// never trusts an old file without rerunning the producer first.
struct FloodMetrics {
    /// Submitted frame rate over the complete active run.
    fps: f64,
    /// Longest observed interval between GPU submissions.
    /// This catches short stalls that an average frame rate hides.
    max_frame_gap_ms: f64,
    /// OS-like key injection to transport write.
    key_ms: f64,
    /// OS-like resize injection to Session resize.
    /// Resize must stay schedulable even when PTY output never stops.
    resize_ms: f64,
    /// Close injection to loop exit and reader join.
    close_ms: f64,
    /// Channel plus Session bytes waiting for later parser slices.
    /// Counting both places prevents moving an unbounded queue downstream.
    max_pending_bytes: usize,
}

/// Runs the fixed corpus through the native release harness once.
/// The harness writes measurements only after its key, resize and close
/// events have completed, so a hung event loop cannot leave a valid report.
fn measure() -> Result<FloodMetrics, String> {
    // Debug compilation would charge instrumentation overhead against a
    // production frame budget, so this probe uses the release profile.
    crate::commands::capture(
        "cargo",
        &[
            "run",
            "--release",
            "-p",
            "puzzle-desktop",
            "--",
            "harness",
            "assets/scenarios/flood.pzs",
        ],
    )?;
    let path = Path::new("target/harness/flood/flood.txt");
    let report = fs::read_to_string(path).map_err(|error| error.to_string())?;
    // Names are stable across harness and xtask; using fields rather than
    // positional lines keeps a new diagnostic from shifting budget values.
    let fields = report
        .lines()
        .filter_map(|line| line.split_once('='))
        .collect::<BTreeMap<_, _>>();
    // Every missing or malformed field fails the associated acceptance
    // report; Cargo's exit code alone would not prove a budget was measured.
    Ok(FloodMetrics {
        fps: number(&fields, "fps")?,
        max_frame_gap_ms: number(&fields, "max_frame_gap_ms")?,
        key_ms: number(&fields, "key_ms")?,
        resize_ms: number(&fields, "resize_ms")?,
        close_ms: number(&fields, "close_ms")?,
        max_pending_bytes: integer(&fields, "max_pending_bytes")?,
    })
}

/// Requires a finite decimal, rejecting NaN as a false budget PASS.
/// A malformed timing must fail the criterion instead of comparing as if
/// it were zero; NaN comparisons have particularly misleading behavior.
fn number(fields: &BTreeMap<&str, &str>, key: &str) -> Result<f64, String> {
    let value = fields
        .get(key)
        .ok_or_else(|| format!("missing {key} in flood report"))?
        .parse::<f64>()
        .map_err(|error| format!("invalid {key}: {error}"))?;
    value
        .is_finite()
        .then_some(value)
        .ok_or_else(|| format!("non-finite {key}"))
}

/// Reads the exact pending-byte count without floating-point rounding.
/// Four MiB is a byte cap, so accepting a rounded decimal at its edge
/// could incorrectly pass a queue that exceeded the bound by one buffer.
fn integer(fields: &BTreeMap<&str, &str>, key: &str) -> Result<usize, String> {
    fields
        .get(key)
        .ok_or_else(|| format!("missing {key} in flood report"))?
        .parse()
        .map_err(|error| format!("invalid {key}: {error}"))
}

/// Reports every independent §15.2 flood criterion after one fresh run.
/// Independent lines allow a reviewer to distinguish render starvation,
/// input latency, excess buffering and shutdown failure in one command.
pub(super) fn report_flood(passed: &mut bool) {
    let result = measure();
    // Average rate and longest gap jointly reject a burst of frames
    // followed by a long black-screen pause.
    report(
        "M6 flood GPU frame interval and at least 30 fps",
        result.as_ref().map_err(Clone::clone).and_then(|m| {
            (m.fps >= 30.0 && m.max_frame_gap_ms <= 33.334)
                .then_some(())
                .ok_or_else(|| format!("fps={:.3}, max gap={:.3} ms", m.fps, m.max_frame_gap_ms))
        }),
        passed,
    );
    report(
        "M6 flood key reaches PTY within 50 ms",
        // The PTY write is the input contract; a frame may legitimately
        // arrive later when output has dirtied most of the terminal.
        result.as_ref().map_err(Clone::clone).and_then(|m| {
            (m.key_ms <= 50.0)
                .then_some(())
                .ok_or_else(|| format!("key latency {:.3} ms", m.key_ms))
        }),
        passed,
    );
    report(
        "M6 flood pending bytes stay within 4 MiB",
        // This cap bounds the user's memory footprint during an endless
        // producer, independent of how fast the GPU draws its frames.
        result.as_ref().map_err(Clone::clone).and_then(|m| {
            (m.max_pending_bytes <= 4 * 1024 * 1024)
                .then_some(())
                .ok_or_else(|| format!("{} pending bytes", m.max_pending_bytes))
        }),
        passed,
    );
    report(
        "M6 flood resize and close stay responsive",
        // A flooded terminal must still process OS lifecycle events;
        // otherwise the user cannot recover without killing the process.
        result.as_ref().map_err(Clone::clone).and_then(|m| {
            (m.resize_ms <= 50.0 && m.close_ms <= 50.0)
                .then_some(())
                .ok_or_else(|| format!("resize {:.3} ms, close {:.3} ms", m.resize_ms, m.close_ms))
        }),
        passed,
    );
}
