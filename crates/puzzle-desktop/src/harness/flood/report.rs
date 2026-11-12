//! Machine-readable flood and resize-storm measurements.
//!
//! Acceptance reads a fresh report written only after the producer and
//! controls join. Separate fields let it name a failed latency, memory or
//! presentation budget without masking later criteria.
//!
//! Throughput is based on bytes accepted by VT, not bytes enqueued by the
//! producer; an already full channel could otherwise advertise work the
//! terminal has not done. Frame rate covers the whole active interval,
//! including resize work. The frame percentile uses submitted GPU frames
//! because an event-loop wake is not evidence that anything was painted.
//! Steady and storm phases are kept in one file so their units and
//! denominator stay identical for the T4 90-percent comparison.
use super::Metrics;
use std::{fs, path::Path, time::Duration};

/// Writes paired baseline metrics without changing the existing flood report.
pub(super) fn write_storm_metrics(
    output: &Path,
    steady: (Duration, &Metrics),
    storm: (Duration, &Metrics),
) -> Result<(), String> {
    let mut lines = String::new();
    for (name, elapsed, metrics) in [("steady", steady.0, steady.1), ("storm", storm.0, storm.1)] {
        let seconds = elapsed.as_secs_f64();
        let mut intervals = metrics.frame_intervals_ms.clone();
        intervals.sort_by(f64::total_cmp);
        // The high percentile exposes resize stalls hidden by average fps.
        let index = (intervals.len() * 99 / 100).min(intervals.len().saturating_sub(1));
        let p99 = intervals.get(index).copied().unwrap_or(0.0);
        lines.push_str(&format!(
            "{name}_throughput_mb_s={:.3}\n{name}_fps={:.3}\n{name}_frame_p99_ms={p99:.3}\n{name}_key_ms={:.3}\n{name}_pending_bytes={}\n{name}_resize_ms={:.3}\n{name}_resize_events={}\n",
            metrics.parsed_bytes as f64 / 1_000_000.0 / seconds,
            metrics.frames as f64 / seconds,
            metrics.key_ms.ok_or("key write missing")?,
            metrics.max_pending,
            metrics.resize_ms.unwrap_or(0.0),
            metrics.storm_resizes,
        ));
    }
    fs::write(output.join("before.txt"), lines).map_err(|error| error.to_string())
}

/// Persists values separately so xtask can report every flood criterion.
pub(super) fn write_metrics(
    output: &Path,
    elapsed: Duration,
    metrics: &Metrics,
) -> Result<(), String> {
    // Use the complete active run, including control handling and GPU
    // submission, rather than just the parser's faster inner loop.
    let rate = metrics.frames as f64 / elapsed.as_secs_f64();
    let report = format!(
        // Distinct fields let acceptance report every failing flood budget
        // instead of hiding the later results behind the first failure.
        "fps={rate:.3}\nmax_frame_gap_ms={:.3}\nkey_ms={:.3}\nresize_ms={:.3}\nclose_ms={:.3}\nmax_pending_bytes={}\nslices={}\n",
        metrics.max_frame_gap_ms,
        metrics.key_ms.ok_or("flood key was not written")?,
        metrics.resize_ms.ok_or("flood resize was not handled")?,
        metrics.close_ms.ok_or("flood close was not handled")?,
        metrics.max_pending,
        metrics.slices
    );
    fs::write(output.join("flood.txt"), report).map_err(|error| error.to_string())
}
