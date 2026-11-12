//! M18.2 tests wire negotiation separately from user policy and host routing.
//!
//! Terminal conformance feeds actual CSI bytes, including invalid forms and
//! RIS/DECSTR. An allocation counter protects the no-cost negotiation path.
//! Settings tests exercise canonical storage, both old spellings, precedence,
//! export, reset and invalid values through the public admission surface.
//! Session tests exhaust every configured policy and program request against
//! tracking and encoding modes, then check actual bytes and local selection.
//! Host InputState updates independently of a frame, so native coalescing and
//! DOM default ownership observe the same decision as command dispatch.
use super::report;

/// Reports every independent Shift-capture obligation without hiding failures.
pub(super) fn run(passed: &mut bool) {
    for (criterion, package, target, filter) in [
        (
            "M18.2 XTSHIFTESCAPE grammar, resets and zero allocations",
            "puzzle-vt",
            "conformance",
            "shift_capture::",
        ),
        (
            "M18.2 legacy Shift settings, layers and validation",
            "puzzle-term",
            "lib",
            "settings::store::shift_tests::",
        ),
        (
            "M18.2 every shared Shift policy, host event and pointer route",
            "puzzle-term",
            "lib",
            "session::tests::shift_capture::",
        ),
    ] {
        // Each test has behavioral assertions; selectors never substitute file existence.
        let mut args = vec!["test", "-p", package];
        if target == "lib" {
            args.push("--lib");
        } else {
            args.extend(["--test", target]);
        }
        args.push(filter);
        // A failed family must still allow the remaining independent families to run.
        report(criterion, crate::commands::quiet("cargo", &args), passed);
    }
}
