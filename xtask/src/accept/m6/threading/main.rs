//! T4 measures the live-thread seams under a finite truecolor resize storm.
//!
//! The headless runner has an independent 60 Hz control producer, the same
//! fixed PTY pool and parser worker as the window, and real offscreen GPU
//! submissions. It reports steady and storm phases from separate sessions.
//! A release-only full-frame probe measures owner-side snapshot publication,
//! preserving the original full-copy ceiling after the shared mutex is removed.
//! Each criterion has its own line so a good frame rate cannot hide a PTY
//! resize storm, and a correct final grid cannot hide poor throughput.
//!
//! The runner's report is regenerated on each acceptance invocation.
//! Reading an old `after.txt` without running its scenario would let a
//! successful earlier commit hide a broken current implementation.
//! The `capture` call therefore runs before opening the report file.
//! A child exit failure is propagated to every dependent T4 criterion.
//! The parser and GPU phase still produce their own diagnostic log under
//! `target/harness/thread-storm` for a reviewer to inspect afterward.
//!
//! Throughput compares two phases of the same executable and machine.
//! The steady phase has continuous truecolor output without resizing;
//! the storm phase adds 180 independent physical resize notifications.
//! This ratio reduces sensitivity to workstation speed and makes the
//! 90-percent requirement about contention caused by the resize path.
//! We require a positive steady rate so a dead parser cannot satisfy a
//! ratio by reporting zero in both phases. Decimal MB/s is the same unit
//! used by the T0 baseline and is not rounded before comparison.
//!
//! FPS counts completed GPU submissions. Thirty is the flood floor from
//! design §15.2. The separate flood acceptance still checks frame gap,
//! input latency and backlog size; those tests remain in force here.
//! This T4 line adds simultaneous resize work rather than replacing any
//! earlier flood criterion. A high parse rate with no submitted frames
//! must fail this line even when the PTY remained responsive.
//!
//! The final size check includes event delivery. If the independent
//! controller silently exits early, a matching grid and PTY size would
//! not demonstrate operation during a real three-second resize storm.
//! Exactly 180 events means all control notifications reached the UI.
//! After the final event, both the visible grid and fake PTY report are
//! sampled. This checks the last-size-wins path and the final flush.
//! Width and height are compared separately to catch one-axis mistakes.
//!
//! The PTY transport log counts OS resize calls, not VT grid reflows.
//! A continuous 60 Hz drag must notify throughout its first-event 25-ms
//! throttle windows, rather than waiting for silence. Count the three-second
//! window bound plus the fixture's final flush, and reject one signal per event.
//! Grid reflow stays independent: before notifying, the newest grid is applied
//! so the child and the next image agree on rows and columns.
//!
//! The full-grid copy test creates a 300-by-80 Session with real geometry and
//! content in every visible cell. Failed-build recovery invalidates each of
//! 101 publications, so every sample copies all rows rather than an idle slot.
//! Its exact microsecond p99 retains the existing 500-µs ceiling; shaping and
//! rendering remain outside this interval. Release mode measures native work.
//!
//! Cargo treats a test filter matching no tests as a success. This gate
//! checks the named test's `ok` line and its labelled p99 output, so a
//! future rename cannot leave a vacuous acceptance result. The test
//! itself asserts the numeric budget; xtask repeats the comparison for
//! a useful failure reason in the milestone report.
//!
//! All report fields are parsed explicitly. Missing fields, malformed
//! integers and non-finite floating-point values fail the applicable
//! criteria. A stale or truncated report therefore cannot fabricate a
//! passing ratio or a correct final size. Independent report lines keep
//! running even when one measurement is unavailable, per design §2.4.
use super::super::report;
use std::{collections::BTreeMap, fs};

/// Typed fields from the runner's fresh report.
struct Storm {
    /// Decimal MB/s before the independent resize producer starts.
    steady_mb_s: f64,
    /// Decimal MB/s while 180 physical size events arrive.
    storm_mb_s: f64,
    /// GPU submissions per elapsed storm second.
    storm_fps: f64,
    /// All control-thread events must reach the UI queue.
    received: u32,
    /// PTY notifications during first-event 25-ms throttle windows.
    pty_calls: u32,
    /// Final visible grid width in cells.
    grid_cols: u32,
    /// Final visible grid height in cells.
    grid_rows: u32,
    /// Final PTY width reported to the child.
    pty_cols: u32,
    /// Final PTY height reported to the child.
    pty_rows: u32,
}

/// Runs the threaded scenario before reading its generated evidence.
fn storm() -> Result<Storm, String> {
    crate::commands::capture(
        "cargo",
        &[
            "run",
            "--release",
            "-p",
            "puzzle-desktop",
            "--",
            "harness",
            "assets/scenarios/thread-storm.pzs",
        ],
    )?;
    let report = fs::read_to_string("target/harness/thread-storm/after.txt")
        .map_err(|error| error.to_string())?;
    let fields = report
        .lines()
        .filter_map(|line| line.split_once('='))
        .collect::<BTreeMap<_, _>>();
    let number = |key: &str| -> Result<f64, String> {
        let value = fields
            .get(key)
            .ok_or_else(|| format!("missing {key}"))?
            .parse::<f64>()
            .map_err(|error| format!("invalid {key}: {error}"))?;
        value
            .is_finite()
            .then_some(value)
            .ok_or_else(|| format!("non-finite {key}"))
    };
    let count = |key: &str| -> Result<u32, String> {
        fields
            .get(key)
            .ok_or_else(|| format!("missing {key}"))?
            .parse::<u32>()
            .map_err(|error| format!("invalid {key}: {error}"))
    };
    Ok(Storm {
        steady_mb_s: number("steady_throughput_mb_s")?,
        storm_mb_s: number("storm_throughput_mb_s")?,
        storm_fps: number("storm_fps")?,
        received: count("storm_resize_events")?,
        pty_calls: count("storm_pty_calls")?,
        grid_cols: count("storm_grid_cols")?,
        grid_rows: count("storm_grid_rows")?,
        pty_cols: count("storm_pty_cols")?,
        pty_rows: count("storm_pty_rows")?,
    })
}

/// The release test emits its p99 after 101 forced full-grid captures.
fn snapshot_copy() -> Result<(), String> {
    // A debug build times bounds checks and slow shaping rather than the
    // optimized publication used by the interactive run profile.
    let output = crate::commands::capture(
        "cargo",
        &[
            "test",
            "--release",
            "-p",
            "puzzle-embed",
            "--features",
            "lock-metrics",
            "session_thread::tests::capture::full_snapshot_copy_p99",
            "--",
            "--exact",
            "--nocapture",
        ],
    )?;
    if !output.contains("test session_thread::tests::capture::full_snapshot_copy_p99 ... ok") {
        return Err("full-frame copy test did not run".into());
    }
    // The test itself asserts the bound. Requiring the labelled number
    // ensures a renamed or skipped timing path cannot pass silently.
    let value = output
        .split_whitespace()
        .find_map(|word| word.strip_prefix("snapshot_copy_p99_us="))
        .ok_or("snapshot copy p99 missing")?
        .parse::<u64>()
        .map_err(|error| format!("snapshot copy p99: {error}"))?;
    (value <= 500)
        .then_some(())
        .ok_or_else(|| format!("{value} us exceeds 500 us"))
}

/// Reports snapshot copying, throughput, presentation and resize guarantees.
pub(super) fn report_main(passed: &mut bool) {
    report(
        "M6-T4 full 300x80 snapshot copy p99 <= 0.5 ms",
        snapshot_copy(),
        passed,
    );
    let observed = storm();
    let throughput = observed.as_ref().map_err(Clone::clone).and_then(|value| {
        (value.steady_mb_s > 0.0 && value.storm_mb_s >= 0.9 * value.steady_mb_s)
            .then_some(())
            .ok_or_else(|| {
                format!(
                    "steady {:.3}, storm {:.3} MB/s",
                    value.steady_mb_s, value.storm_mb_s
                )
            })
    });
    report(
        "M6-T4 resize-storm parser >= 90% steady",
        throughput,
        passed,
    );
    let frames = observed.as_ref().map_err(Clone::clone).and_then(|value| {
        (value.storm_fps >= 30.0)
            .then_some(())
            .ok_or_else(|| format!("storm {:.3} fps", value.storm_fps))
    });
    report("M6-T4 resize-storm presents >= 30 fps", frames, passed);
    let pty = observed.as_ref().map_err(Clone::clone).and_then(|value| {
        (value.received == 180
            && value.grid_cols == value.pty_cols
            && value.grid_rows == value.pty_rows)
            .then_some(())
            .ok_or_else(|| {
                format!(
                    "events {}, grid {}x{}, PTY {}x{}",
                    value.received,
                    value.grid_cols,
                    value.grid_rows,
                    value.pty_cols,
                    value.pty_rows
                )
            })
    });
    report("M6-T4 final PTY equals visible grid", pty, passed);
    let signals = observed.as_ref().map_err(Clone::clone).and_then(|value| {
        // D-29 now throttles throughout the three-second drag. The maximum is
        // one notification per 25-ms window plus the fixture's final flush;
        // more than one proves the stream is not deferred until drag completion.
        (value.pty_calls > 1 && value.pty_calls <= 3_000 / 25 + 1)
            .then_some(())
            .ok_or_else(|| format!("{} PTY resizes during 180-event drag", value.pty_calls))
    });
    report(
        "M6-T4 PTY resize throttle, no SIGWINCH storm",
        signals,
        passed,
    );
}
