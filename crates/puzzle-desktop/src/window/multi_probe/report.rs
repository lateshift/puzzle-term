//! Raw two-window evidence and unchanged native cadence/input limits.
//!
//! A receipt is paired by exact byte order; a missing or extra key fails rather
//! than shortening the measured latency list. The journal uses the system wall
//! clock shared by the two processes, while presentation intervals use Instant.
//! Hardware, geometry and every interval are written even when a criterion fails.
//! The secondary retains the established LG 58-fps baseline and one-refresh edge
//! allowance; the reusable-glyph flood keeps its existing thirty-fps floor.
//! Leading and trailing edges matter as well as interior intervals: a stream
//! can have regular spacing while starting one refresh late or ending early.
//! Sample count has its own floor, so gradual drift cannot hide behind jitter.
//! Frames after the interval edge never improve any of these measurements.
//! Pipeline counters are read after the last key, without borrowing a screen.
//! Child timestamps include the kernel writer path after the queue endpoint.
use super::*;
use serde_json::{Value, json};

impl Probe {
    /// Saves all observations before returning a budget failure to the caller.
    pub(super) fn finish(&self) -> Result<(), String> {
        // Receipt errors are exact-byte failures, not an empty latency distribution.
        // Both raw journals remain on disk even if their counts disagree.
        let latencies = self.receipts()?;
        let mut reports = Vec::new();
        for (index, window) in self.windows.iter().enumerate() {
            let sample = self.samples[index]
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            // Guard-drain presentations are retained by the observer but excluded here.
            // The measured denominator stays two seconds for both windows and controls.
            let frames: Vec<_> = sample
                .presents
                .iter()
                .copied()
                .filter(|ms| *ms <= 2000.0)
                .collect();
            // These reads inspect fixed atomic observations, never mutable Session data.
            // Owner CPU samples distinguish actual work from OS descheduling.
            let pipeline = sample.pipeline.as_ref().map(|p| p.stage_report());
            let info = host_surface::info(&window.window);
            // Retain the final hardware witness alongside the pre-interval validation.
            // A requested monitor alone would not prove the OS placement succeeded.
            let visibility = host_surface::visibility(&window.window);
            reports.push(json!({"surface":index, "display":info.display,
                "monitor":visibility.monitor_name, "millihertz":info.millihertz,
                "scale":info.scale, "extent":[info.extent.width,info.extent.height],
                "submission_ms":frames, "fps":frames.len() as f64/2.0,
                "display_link":sample.start.map(|origin| sample.restart.json(origin)),
                "pipeline":pipeline}));
        }
        // Evaluate only after copying raw observations; reporting cannot delay a frame.
        // Every failed criterion is preserved in the report before returning nonzero.
        let verdict = evaluate(&reports, &latencies, self.flood);
        let report = json!({"flood":self.flood,"surfaces":reports,
            "key_send_unix_ns":self.keys.iter().map(u128::to_string).collect::<Vec<_>>(),
            "child_key_ms":latencies,"pass":verdict.is_ok(),"failure":verdict.as_ref().err()});
        std::fs::write(self.out.join("report.json"), report.to_string() + "\n")
            .map_err(|e| e.to_string())?;
        verdict
    }
    /// Exact journals distinguish child delivery from successful writer enqueue.
    fn receipts(&self) -> Result<Vec<f64>, String> {
        let file =
            std::fs::read_to_string(self.out.join("sibling.keys")).map_err(|e| e.to_string())?;
        // A receipt line is one byte at the actual child read endpoint.
        // Extra terminal replies or human input must fail this exact journal.
        let mut result = Vec::new();
        for (index, line) in file.lines().enumerate() {
            let (byte, received) = line.split_once(' ').ok_or("malformed child receipt")?;
            if byte != "120" {
                return Err(format!("unexpected child byte {byte}"));
            }
            let sent = self.keys.get(index).ok_or("extra child key receipt")?;
            let received = received.parse::<u128>().map_err(|e| e.to_string())?;
            // A backwards clock is invalid evidence rather than a negative latency.
            // Order, byte value and count are checked independently of timing.
            let elapsed = received
                .checked_sub(*sent)
                .ok_or("system clock moved backwards")?;
            result.push(elapsed as f64 / 1e6);
        }
        if result.len() != self.keys.len() || result.is_empty() {
            return Err(format!(
                "exact child keys: {} received / {} sent",
                result.len(),
                self.keys.len()
            ));
        }
        Ok(result)
    }
}
/// A measured failure remains a failure even if another surface met its own floor.
fn evaluate(reports: &[Value], child: &[f64], flood: bool) -> Result<(), String> {
    // Flood and sibling have independent acceptance; neither hides the other.
    // This is the recorded LG baseline rather than the unavailable 120-Hz criterion.
    let sibling = &reports[1];
    let fps = sibling["fps"].as_f64().ok_or("missing sibling fps")?;
    if fps < 58.0 {
        return Err(format!("sibling cadence {fps} fps < LG baseline 58 fps"));
    }
    let frames = sibling["submission_ms"]
        .as_array()
        .ok_or("missing frame times")?
        .iter()
        .map(|v| v.as_f64().ok_or("invalid frame time"))
        .collect::<Result<Vec<_>, _>>()?;
    // Half-period jitter tolerance is the established cadence witness, not a new margin.
    if frames.first().is_none_or(|v| *v > 25.0)
        || frames.last().is_none_or(|v| 2000.0 - v > 25.0)
        || frames.windows(2).any(|p| p[1] - p[0] > 25.0)
    {
        return Err("sibling missed a 60-Hz refresh; raw edge/interior times retained".into());
    }
    // Writer enqueue and child delivery measure two different existing budgets.
    // A zero-sample writer distribution must never be interpreted as a pass.
    let keys = &sibling["pipeline"]["controls_and_keys"];
    if keys["key_to_writer_max_ms"]
        .as_f64()
        .is_none_or(|v| v > 0.5)
        || keys["queued_keys"].as_u64() != Some(child.len() as u64)
    {
        return Err(format!("sibling writer budget/count failed: {keys}"));
    }
    if child.iter().any(|v| *v > 50.0) {
        return Err("child key receipt exceeds 50 ms".into());
    }
    // The control has no flood floor because its first surface is deliberately idle.
    // Treatment uses a reusable glyph set, so the ordinary flood row applies.
    if flood && reports[0]["fps"].as_f64().is_none_or(|v| v < 30.0) {
        return Err("reusable-glyph flood below 30 fps".into());
    }
    Ok(())
}
