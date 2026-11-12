//! OSC 72 acceptance keeps protocol coverage independent of native toolkit limits.
//! Synthetic commands cover the complete product flow, not merely file drops.
//! The C fixture repeats that flow through NSView and borrowed CAMetalLayer hosts.
//! Hover permission, binary replies, lifecycle and geometry have separate tests.
//! Invalid C ranges run without native GPU calls, including under Miri.
//! No acceptance line claims OS acceptance feedback that winit cannot provide.
//! Native drag/drop in the Swift app remains session 2 work and a human review.
//! The existing function export/header equality gate protects the published ABI.
//! ASan checks call-scoped MIME copies, while Miri checks pure range validation.
//! All criteria continue independently so one successful layer cannot hide another.
//! Task acceptance adds no dependency, screenshot tolerance or performance exemption.
//! Later M18 tasks remain explicitly unfinished in the full milestone gate.
use super::report;

/// Reports wire, owner, toolkit subset and foreign-boundary criteria independently.
pub(super) fn run(passed: &mut bool) {
    // Wire tests preserve exact OSC terminators and client IDs across chunk assembly.
    // Owner tests add policy interception and geometry from the last presented frame.
    // The native adapter checks filename encoding and rejection without opening files.
    // Argument tests reject malformed C input before any GPU or Session is required.
    for (name, package, filter) in [
        (
            "M18.5 OSC 72 registration, bounds, no unused cost and ordered binary replies",
            "puzzle-vt",
            "protocols::kitty::dnd::",
        ),
        (
            "M18.5 owner geometry, lifecycle and policy isolation",
            "puzzle-term",
            "session::tests::dnd::",
        ),
        (
            "M18.5 winit file URI/plain path subset",
            "puzzle-desktop",
            "window::ui::dnd::tests::",
        ),
        (
            "M18.5 invalid C drag ranges and kinds",
            "puzzle-ffi",
            "drop_rejects_invalid",
        ),
    ] {
        // The name filter avoids unrelated native GPU or real-PTY scenarios.
        // Every assertion below is functional; debug wall time is no speed budget.
        report(
            name,
            crate::commands::quiet("cargo", &["test", "-p", package, filter]),
            passed,
        );
    }
    // Synthetic hosts can exercise MIME negotiation despite winit's OS restrictions.
    // The authored script checks every reply byte and all four gesture phases.
    // Its independent recording transport also verifies reply order against a DSR.
    report(
        "M18.5 full synthetic drag enter/move/drop/leave scenario",
        crate::commands::quiet(
            "cargo",
            &[
                "xtask",
                "harness",
                "assets/scenarios/drag-drop.pzs",
                "--check",
            ],
        ),
        passed,
    );
    foreign(passed);
}

/// Native attachment, exported declarations and memory safety are distinct obligations.
fn foreign(passed: &mut bool) {
    // An exported function without an independent C call fails header verification.
    report(
        "M18.5 checked drag header and C call inventory",
        crate::ffi::verify(),
        passed,
    );
    // Both native host kinds receive actual asynchronous registration/acceptance callbacks.
    report(
        "M18.5 complete drag contract through both C native hosts",
        crate::ffi::host(),
        passed,
    );
    sanitizers(passed);
    println!(
        "REVIEW M18.5 native drop and harness file-only subset: docs/protocol-review.md; session 2 host work in README.md"
    );
}

/// Repeats only instrumented criteria when the reviewer invalidates their environment.
/// The full task and milestone call these same functions with identical assertions.
pub(super) fn sanitizers(passed: &mut bool) {
    // Borrowed names and binary data must be copied before their C lifetimes end.
    report("M18.5 drag buffers under ASan", crate::ffi::asan(), passed);
    // Miri covers pure argument rejection rather than unsupported foreign GPU execution.
    report(
        "M18.5 drag argument validation under Miri",
        crate::ffi::miri(),
        passed,
    );
}
