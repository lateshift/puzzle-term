//! Embedder input clocks are checked independently of one-shot hardware receipts.
//!
//! Deterministic tests assert clock-domain validation and exact stage boundaries;
//! no debug wall-time budget is inferred from a parallel functional test. The
//! native C fixture proves both timed exports preserve the original byte stream.
//! Hardware samples are recorded once under M12 evidence, never retried here.
use crate::accept::report;

/// Task-local API checks do not repeat the user's bounded measurement cohorts.
/// Passing these functional checks makes no claim about hardware latency budgets.
pub(super) fn run() -> bool {
    // Report independently: a failed clock-domain test must not hide whether
    // the separately compiled C client still obeys the public input contract.
    let mut passed = true;
    // The first case rejects fabricated fast samples from invalid host clocks.
    // The second protects stage partitioning and consumed-key stamp cleanup.
    // The third uses real App/Surface ownership and the host-transport FIFO.
    for (name, filter, expected) in [
        (
            "clock-domain validation",
            "input_timing::tests",
            "host_epoch_is_arbitrary_but_invalid_pairs_never_become_fast_samples",
        ),
        (
            "stage clocks and consumed input",
            "pipeline_metrics::control::tests",
            "timed_input_preserves_each_clock_boundary_and_consumed_keys_clear_it",
        ),
        (
            "Rust timed-input FIFO and policy",
            "timed_host_inputs_preserve_bytes_and_session_policy",
            "timed_host_inputs_preserve_bytes_and_session_policy",
        ),
    ] {
        // Cargo's successful empty filter is not evidence that a regression ran.
        report(
            &format!("M12.input-latency {name}"),
            test(filter, expected),
            &mut passed,
        );
    }
    // A C process cannot install a Rust subscriber. Its explicit file sink
    // must arm the same target that captures the runtime input stamp.
    report(
        "M12.input-latency C-host diagnostic sink",
        c_sink(),
        &mut passed,
    );
    // A Rust-only test cannot detect a C signature or Key layout mismatch.
    // The independent fixture requires exact bytes from both timed exports.
    report(
        "M12.input-latency C timed key/text bytes and invalid clocks",
        crate::ffi::host(),
        &mut passed,
    );
    println!(
        "REVIEW M12.input-latency: one-shot stage/child/resize receipts in docs/history/M12.md; AppKit OS timestamps supplied by session 2"
    );
    passed
}

/// Require a named functional result rather than a timing-sensitive debug assertion.
fn test(filter: &str, expected: &str) -> Result<(), String> {
    let output = crate::commands::capture("cargo", &["test", "-p", "puzzle-embed", filter])?;
    if !output.contains(&format!("::{expected} ... ok")) {
        return Err(format!("missing input regression {expected}: {output}"));
    }
    Ok(())
}

/// Default C builds must expose input observations without a Rust logger bridge.
/// This checks the opt-in sink separately from FIFO delivery: a working key
/// path alone would not prove that a C embedder can retrieve its measurements.
fn c_sink() -> Result<(), String> {
    let name = "c_host_sink_enables_private_input_and_excludes_program_output";
    let output = crate::commands::capture("cargo", &["test", "-p", "puzzle-ffi", name])?;
    // A disabled diagnostic would make correctly carried timestamps inaccessible.
    if !output.contains(&format!("::{name} ... ok")) {
        return Err(format!("missing C subscriber regression: {output}"));
    }
    Ok(())
}
