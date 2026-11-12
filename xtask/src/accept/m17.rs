//! M17's task receipts preserve budgets while avoiding intermediate full sweeps.
//!
//! Task acceptance deliberately runs optimized witnesses separately from check.
//! Debug tests retain the same bytes, row counts and lifecycle assertions, but
//! parallel debug execution cannot serve as a product latency measurement.
//! The existing 50-ms child receipt, 0.5-ms transport and snapshot limits remain
//! enforced here; replacing a debug assertion never removes its release gate.
//!
//! Each subprocess must report exactly one passing test. Cargo treats a filter
//! matching no tests as success, which would otherwise silently waive a budget
//! after a module move during embedding. Test names below are complete paths.
//! Measurement lines are forwarded after the test succeeds; failed subprocesses
//! retain their complete diagnostics through commands::capture and report.
//!
//! The OS-lock witnesses exercise a different concern: independent top-level
//! native and Chrome gates share one GPU and must exclude one another. Nested
//! build and harness helpers run under their parent's guard, so reacquiring it
//! would deadlock acceptance. Their test proves that selection explicitly.
//! Task selectors do not claim the unfinished full M17 is complete. The complete
//! inventory grows with embedding, extensions, transport and the C interface.
use super::report;
/// Checked native host capabilities and App-owned worker lifetime.
mod api;
/// Final inventory, shared-code regression gates and pending host ownership.
mod closure;
/// Borrowed host-layer contract and independent AppKit integration build.
mod demo;
/// C ABI, actual host executable and audited-pointer instrumentation.
mod ffi;
/// Reviewer-authorized affected-line reruns preserve the original full receipt.
mod followup;
/// Per-atlas policy, complete pressure frames and runtime GPU reload.
mod glyph_budget;
/// Generic parser extension ordering and optional draft 0.3 program status.
mod osc;
/// Exact fixture contracts and retained-failure regression witnesses.
mod probes;
/// Host-selected display callbacks retain the existing frame policy.
mod refresh;
/// Bounded host output and FIFO callbacks share the production Session pipeline.
mod transport;
/// Two actual native windows share one App while one child floods.
mod windows;

/// Selects an explicit task; an unfinished full milestone cannot report success.
pub(super) fn run(args: &[String]) -> bool {
    let mut passed = true;
    // A task receipt permits the ordered work without a full milestone sweep.
    match args {
        [flag, task] if flag == "--task" && task == "test-gates" => test_gates(&mut passed),
        [flag, task] if flag == "--task" && task == "probe-stability" => probes::run(&mut passed),
        // This receipt checks atlas policy without claiming the unfinished embedding API.
        [flag, task] if flag == "--task" && task == "glyph-budget" => {
            glyph_budget::run(&mut passed)
        }
        [flag, task] if flag == "--task" && task == "embed-api" => api::run(&mut passed),
        [flag, task] if flag == "--task" && task == "embed-resources" => api::resources(&mut passed),
        [flag, task] if flag == "--task" && task == "embed-windows" => windows::run(&mut passed),
        // Optional protocol checks are a task receipt, not full milestone acceptance.
        [flag, task] if flag == "--task" && task == "osc" => osc::run(&mut passed),
        [flag, task] if flag == "--task" && task == "transport" => transport::run(&mut passed),
        [flag, task] if flag == "--task" && task == "ffi" => ffi::run(&mut passed),
        [flag, task] if flag == "--task" && task == "demo" => demo::run(&mut passed),
        [flag, task] if flag == "--task" && task == "refresh-api" => refresh::api(&mut passed),
        [flag, task] if flag == "--task" && task == "close-followup" => followup::run(&mut passed),
        [] => closure::run(&mut passed),
        _ => report(
            "M17 arguments",
            Err(
                "usage: accept M17 [--task test-gates|probe-stability|glyph-budget|embed-api|embed-resources|embed-windows|osc|transport|ffi|demo|refresh-api|close-followup]"
                    .into(),
            ),
            &mut passed,
        ),
    }
    passed
}

/// Executes each optimized counterpart, rejecting empty Cargo test filters.
pub(super) fn test_gates(passed: &mut bool) {
    for (criterion, name) in [
        // A blocked render turn must not delay a key reaching the real child.
        (
            "M17.test-gates child key during held frame <=50 ms",
            "render_thread::tests::long_frame_key_budget",
        ),
        // Host updates and driver destruction retain the original one-second bound.
        (
            "M17.test-gates resize/close <1 s",
            "render_thread::tests::resize_close_budget",
        ),
        // A stalled ten-megabyte paste must preserve FIFO and permit immediate enqueue.
        (
            "M17.test-gates paste key enqueue <=50 ms",
            "pty::tests::paste_enqueue_budget",
        ),
        // Output and priority controls exercise the actual owner-channel race.
        (
            "M17.test-gates 100,000 control handoffs <=1 s",
            "session_thread::tests::pipeline::control_handoff_budget",
        ),
        // Full recapture uses all 80 rows and records paired wall/CPU percentiles.
        (
            "M17.test-gates full snapshot p99 <=0.5 ms",
            "session_thread::tests::capture::full_snapshot_copy_p99",
        ),
        // This endpoint is the real LocalPty writer queue, not a memory transport.
        (
            "M17.test-gates RawInput to queued PTY <=0.5 ms",
            "keyboard::tests::latency::key_to_queued_pty_budget",
        ),
    ] {
        // report records failures and continues through every remaining criterion.
        report(criterion, release_test(name), passed);
    }
    report(
        "M17.test-gates native/Chrome OS exclusion and release",
        super::m7::test(
            "xtask",
            "gate::tests::gate_releases_without_deleting_its_file",
        ),
        passed,
    );
    // Acceptance invokes these helpers itself; a second lock would wait forever.
    report(
        "M17.test-gates nested helpers cannot deadlock parent gate",
        super::m7::test("xtask", "gate::tests::nested_helpers_do_not_lock"),
        passed,
    );
}

/// Uses the sanctioned observer feature for paired clocks and the real PTY writer.
fn release_test(name: &str) -> Result<(), String> {
    // The observer feature exposes clocks; it does not alter product scheduling.
    // All ordered-pipeline witnesses now live with their embedding owners.
    // Keeping one package here also rejects accidental harness-only coverage.
    let output = crate::commands::capture(
        "cargo",
        &[
            "test",
            "--release",
            "-p",
            "puzzle-embed",
            "--features",
            "lock-metrics",
            name,
            "--",
            "--exact",
            "--nocapture",
        ],
    )?;
    // nocapture may insert a metric between the test name and its final `ok`.
    // Check the exact name and one-test summary independently, without accepting
    // an empty filter or a different witness merely because Cargo exited zero.
    if !output.contains(&format!("test {name} ..."))
        || !output.contains("test result: ok. 1 passed;")
    {
        return Err(format!("release witness {name} did not execute"));
    }
    // Forward only measurement records; routine compiler output stays in the log.
    for line in output.lines().filter(|line| {
        line.starts_with("M17_METRIC") || line.starts_with("snapshot_copy_distribution=")
    }) {
        println!("{line}");
    }
    Ok(())
}
