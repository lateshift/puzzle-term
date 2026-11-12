//! D-29 acceptance begins with a measured main-thread baseline.
//!
//! The storm uses a producer that is always backpressured, and injects
//! physical resizes from an independent control thread at sixty hertz.
//! Re-running the scenario during acceptance prevents a stale report from
//! making later changes appear to have preserved responsiveness.
//!
//! T0 is observational. Its pass condition is a complete, fresh report,
//! not the T4 performance threshold: imposing that threshold now would
//! rewrite the baseline and obscure whether the threading change helped.
//! The two phases create separate sessions and GPU executors, then use the
//! same flood corpus and parser path. Their clocks include event dispatch,
//! parsing and drawing. Resize injection is counted independently from
//! scheduling so a stalled or lost control thread cannot masquerade as a
//! fast resize path. Every numeric field must be finite because NaN would
//! make later before/after comparisons meaningless.
//!
//! T1's copied-row timing is separate from the existing full frame CPU
//! probe. The session thread seals copied rows before publishing a slot; the
//! render thread then shapes, updates atlases and assembles GPU instances.
//! A fast frame can still conceal slow publication that delays control turns,
//! so one-row and full-grid copies have independent acceptance lines.
//! A release example reports native medians; a dev-only counter checks
//! warm allocation behavior. The named test must appear in Cargo's output
//! because a zero-selected test also exits successfully. Terminal-frame
//! goldens protect actual pixels: timing alone would pass an incomplete
//! copy of clusters, cursor modes or color overrides.
//!
//! The headless storm measures the same VT resize and grid arithmetic as
//! the live window, but macOS can route interactive dragging through a
//! modal AppKit loop that this harness cannot create. T0 therefore records
//! a reproducible synthetic baseline without claiming to reproduce the
//! user's `mpv` drops. T5 adds live-window counters and a user-operated
//! before/after drag to test that remaining platform-specific risk.
use super::report;
use std::{collections::BTreeMap, fs, path::Path};
/// T4's detached frame hold and threaded resize-storm gates.
mod main;
/// T3's release publication probe and real-PTY reply-order gates.
pub(in crate::accept) mod parse;
/// Exposes T3's separate report lines to the milestone runner.
pub(super) fn report_parse(passed: &mut bool) {
    parse::report_parse(passed);
}

/// Exposes independent main-thread and PTY resize criteria.
pub(super) fn report_main(passed: &mut bool) {
    main::report_main(passed);
}

/// Runs both baseline phases and requires all promised measurements.
fn baseline() -> Result<(), String> {
    // Release mode reflects the native timing budget; debug compilation
    // would mostly measure checks and instrumentation in the parser.
    crate::commands::capture(
        "cargo",
        &[
            "run",
            "--release",
            "-p",
            "puzzle-desktop",
            "--",
            "harness",
            "assets/scenarios/resize-storm.pzs",
        ],
    )?;
    let report = fs::read_to_string(Path::new("target/harness/resize-storm/before.txt"))
        .map_err(|error| error.to_string())?;
    let fields = report
        .lines()
        .filter_map(|line| line.split_once('='))
        .collect::<BTreeMap<_, _>>();
    // Both phases must report all six metrics, even when one is zero. A
    // missing field is not a successful measurement of inactivity.
    for phase in ["steady", "storm"] {
        for metric in [
            "throughput_mb_s",
            "fps",
            "frame_p99_ms",
            "key_ms",
            "pending_bytes",
            "resize_ms",
        ] {
            let name = format!("{phase}_{metric}");
            let value = fields
                .get(name.as_str())
                .ok_or_else(|| format!("missing {name}"))?
                .parse::<f64>()
                .map_err(|error| format!("invalid {name}: {error}"))?;
            if !value.is_finite() {
                return Err(format!("non-finite {name}"));
            }
        }
    }
    // Exactly 180 applied resizes distinguishes a real three-second storm
    // from a steady run whose control thread exited without exercising VT.
    if fields.get("steady_resize_events") != Some(&"0")
        || fields.get("storm_resize_events") != Some(&"180")
    {
        return Err("resize storm did not apply exactly 180 events".into());
    }
    Ok(())
}

/// T0 records the before state without imposing T4's future limits on it.
pub(super) fn report_baseline(passed: &mut bool) {
    report(
        "M6-T0 main-thread resize-storm baseline",
        baseline(),
        passed,
    );
}

/// Reads one release probe so row and full-grid limits share the same run.
fn snapshot_times() -> Result<(f64, f64), String> {
    // The example reads no fonts and opens no GPU; its timed region is
    // exactly the row copy performed by the desktop session owner.
    let output = crate::commands::capture(
        "cargo",
        &[
            "run",
            "--release",
            "-p",
            "puzzle-term",
            "--example",
            "snapshot_bench",
        ],
    )?;
    let fields = output
        .split_whitespace()
        .filter_map(|token| token.split_once('='))
        .collect::<BTreeMap<_, _>>();
    // Labelled fields prevent row and full values being reversed if the
    // probe later emits another diagnostic number.
    let read = |key| -> Result<f64, String> {
        let value = fields
            .get(key)
            .ok_or_else(|| format!("missing {key} in snapshot probe"))?
            .parse::<f64>()
            .map_err(|error| format!("invalid {key}: {error}"))?;
        if !value.is_finite() {
            return Err(format!("non-finite {key}"));
        }
        Ok(value)
    };
    Ok((read("snapshot_row_us")?, read("snapshot_full_us")?))
}

/// Tests that a warmed full row copy makes no heap request.
fn snapshot_allocations() -> Result<(), String> {
    // Allocation counting is test-only, absent from shipped wasm and
    // desktop builds. The exact name catches a deleted or renamed test.
    let output = crate::commands::capture(
        "cargo",
        &[
            "test",
            "-p",
            "puzzle-term",
            "--lib",
            "frame_snapshot::tests::warm_full_snapshot_allocates_nothing",
            "--",
            "--exact",
        ],
    )?;
    output
        .contains("test frame_snapshot::tests::warm_full_snapshot_allocates_nothing ... ok")
        .then_some(())
        .ok_or_else(|| "snapshot allocation test did not run".into())
}

/// Rejects a missing or over-budget timing without masking the next line.
fn within(value: Result<f64, String>, limit: f64) -> Result<(), String> {
    let measured = value?;
    (measured <= limit)
        .then_some(())
        .ok_or_else(|| format!("measured {measured:.3} us, limit {limit:.3} us"))
}

/// Adds distinct T1 visual, copy-time and allocation criteria.
pub(super) fn report_snapshot(passed: &mut bool) {
    // References were produced before T1; the new snapshot path cannot
    // make its own output expected by blessing these files automatically.
    report(
        "M6-T1 snapshot terminal-frame pixels match references",
        crate::golden::run_terminal(false, false),
        passed,
    );
    let times = snapshot_times();
    // Both budgets come from the same fresh release process. A failure
    // reading that process is reported on each independent criterion.
    let row = times.as_ref().map(|values| values.0).map_err(Clone::clone);
    let full = times.as_ref().map(|values| values.1).map_err(Clone::clone);
    report(
        "M6-T1 one changed row snapshot <= 50 us",
        within(row, 50.0),
        passed,
    );
    report(
        "M6-T1 full 240x70 snapshot <= 0.5 ms",
        within(full, 500.0),
        passed,
    );
    report(
        "M6-T1 warm snapshot zero allocations",
        snapshot_allocations(),
        passed,
    );
}

/// Runs one exact desktop pool test so a renamed test cannot pass vacuously.
fn pool_test(name: &str) -> Result<(), String> {
    // M17 moves the fixed pool and its exact witnesses into puzzle-embed;
    // platform threads and atomics still never enter the core or wasm.
    let full_name = format!("pty_pool::tests::{name}");
    desktop_test(&full_name)
}

/// Requires the moved engine witness to execute rather than match no tests.
fn desktop_test(name: &str) -> Result<(), String> {
    let output = crate::commands::capture(
        "cargo",
        &["test", "-p", "puzzle-embed", name, "--", "--exact"],
    )?;
    output
        .contains(&format!("test {name} ... ok"))
        .then_some(())
        .ok_or_else(|| format!("desktop test {name} did not run"))
}

/// Reports the fixed-pool guarantees separately from frame timing.
pub(super) fn report_pool(passed: &mut bool) {
    // An exhausted pool tests producer throttling; the checksum tests
    // ordering under many handoffs; allocation counting protects the
    // sustained path after the fixed buffers have been created.
    // Independent lines identify which guarantee failed during a flood.
    report(
        "M6-T2 producer backpressure <= 4 MiB",
        pool_test("producer_waits_for_an_empty_buffer"),
        passed,
    );
    report(
        "M6-T2 byte-exact 100 MB handoff",
        pool_test("hundred_mb_checksum_and_bound"),
        passed,
    );
    report(
        "M6-T2 warm drain and handoff zero allocations",
        pool_test("warm_handoff_allocates_nothing"),
        passed,
    );
}
