//! Notification acceptance crosses streaming grammar, owner policy and native ABI.
//!
//! The protocol parser preserves exact semicolons and split base64 symbols.
//! Session tests exercise focus occasions, shared legacy rate limits and generations.
//! Toast input goes through the last presented region table at both device scales.
//! A clean toast source is shared rather than copied into every frame.
//! Idle drawing asserts zero allocations with the provider still installed.
//! The C fixture delays feedback while ordinary terminal queries continue to reply.
//! Both NSView and a borrowed CAMetalLayer use the same callback schema.
//! Strict header compilation, ASan and Miri remain independent acceptance lines.
//! None of these mechanical checks certifies Notification Center or visual quality.
//! The Swift host integration and candidate images remain pending human review.
//! A targeted task gate does not waive later M18 protocol/design obligations.
//!
//! Parser failure cannot be hidden by a successful C host, or vice versa.
//! The loop runs only the newly affected tests rather than a milestone-wide sweep.
//! Release timings preserve the existing 0.5-ms overlay budget without asserting
//! wall time in parallel debug tests. Every sample remains in the evidence CSV.
//! Header verification compares exports, declarations and independent C calls.
//! The real native fixture exercises posting while feedback is still outstanding.
//! Its subsequent terminal query must reply before the host returns completion.
//! ASan instruments the same C fixture and Rust allocation/copy boundary.
//! Miri excludes foreign GPU setup and checks malformed pointer/length handling.
//! The authored scene adds mechanical ink checks without blessing visual quality.
//! Full M18 acceptance still reports future ordered work as incomplete.
use super::report;

/// Reports independent parser, lifecycle, view and foreign-interface criteria.
pub(super) fn run(passed: &mut bool) {
    for (name, package, filter) in [
        (
            "M18.4 bounded OSC 99 chunks, base64, query and reset",
            "puzzle-vt",
            "protocols::kitty::notifications::",
        ),
        (
            "M18.4 shared policy, report isolation and presented toast input",
            "puzzle-term",
            "session::tests::notifications::",
        ),
        (
            "M18.4 malformed C notification feedback",
            "puzzle-ffi",
            "notification_feedback_rejects",
        ),
    ] {
        // Functional assertions run in debug; the separately measured view budget is optimized.
        report(
            name,
            crate::commands::quiet("cargo", &["test", "-p", package, filter]),
            passed,
        );
    }
    // Typical visible toasts must fit the unchanged overlay CPU allowance.
    report(
        "M18.4 toast overlay build within 0.5 ms at both DPRs",
        crate::commands::quiet(
            "cargo",
            &[
                "test",
                "--release",
                "-p",
                "puzzle-term",
                "toast_overlay_build_budget",
                "--",
                "--include-ignored",
            ],
        ),
        passed,
    );
    native(passed);
}

/// Checks the foreign boundary independently of parser and overlay timings.
fn native(passed: &mut bool) {
    // Export/header equality and strict clang compile catch contract drift.
    report(
        "M18.4 checked header and notification callback calls",
        crate::ffi::verify(),
        passed,
    );
    // Actual foreign callbacks run after tick, on both native attachment kinds.
    report(
        "M18.4 asynchronous notification reports through both C native hosts",
        crate::ffi::host(),
        passed,
    );
    sanitizers(passed);
    // A screenshot also carries independently authored ink assertions; no reference is blessed.
    report(
        "M18.4 harness toast screenshot and urgency ink",
        crate::commands::quiet(
            "cargo",
            &[
                "xtask",
                "harness",
                "assets/scenarios/notification-toasts.pzs",
            ],
        ),
        passed,
    );
    println!(
        "REVIEW M18.4 toasts and native Notification Center: docs/protocol-review.md; session 2 host work in README.md"
    );
}

/// Repeats only instrumented criteria when the reviewer invalidates their environment.
/// The full task and milestone call these same functions with identical assertions.
pub(super) fn sanitizers(passed: &mut bool) {
    // A valid C transcript does not prove its borrowed buffers were safe.
    report(
        "M18.4 notification buffers under ASan",
        crate::ffi::asan(),
        passed,
    );
    // Pure range checks must also hold under the interpreter's memory model.
    report(
        "M18.4 notification argument validation under Miri",
        crate::ffi::miri(),
        passed,
    );
}
