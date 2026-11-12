//! UAPI context acceptance separates parsing, lifetime, replay and C delivery.
//! No render output changes: contexts are descriptive metadata for later host UI.
//! Tests exercise real OSC bytes, bounded depth, RIS preservation and owner EOF.
//! Ordinary printing is checked with allocation counters; no frame scans are added.
//! Callback tests use both native attachment kinds and the existing generic event ABI.
//! Foreign pointers have not changed, so the exact checked header remains the contract.
//! Parsing and model tests do not need a GPU or native window.
//! The C witness separately proves delivery through the host's tick boundary.
//! Wire replay is checked independently of the parser's metadata construction.
//! Overflow tests update/end existing entries after the new child is refused.
//! Cancellation tests retain following printable output and ordinary replies.
//! Lifecycle tests preserve contexts in RIS and clear them on held child exit.
//! Human comment review remains pending regardless of these mechanical results.
use super::report;

/// Report every context obligation independently and retain human comment review.
pub(super) fn run(passed: &mut bool) {
    for (criterion, package, target, filter) in [
        (
            "M18.6 UAPI grammar, hierarchy, RIS, overflow and unused allocations",
            "puzzle-vt",
            "conformance",
            "context::",
        ),
        (
            "M18.6 contents and host facts agree; session end clears contexts",
            "puzzle-term",
            "lib",
            "session::tests::context::",
        ),
        (
            "M18.6 context event exact wire roundtrip and malformed replay",
            "puzzle-term",
            "lib",
            "messages::context_wire::tests::",
        ),
    ] {
        // Selectors run behavioral assertions, not folder or symbol existence checks.
        let mut args = vec!["test", "-p", package];
        if target == "lib" {
            args.push("--lib");
        } else {
            args.extend(["--test", target]);
        }
        args.push(filter);
        report(criterion, crate::commands::quiet("cargo", &args), passed);
    }
    // Context metadata uses the existing generic fact callback, not new C pointers.
    report(
        "M18.6 context facts through NSView and CAMetalLayer C hosts",
        crate::ffi::host(),
        passed,
    );
    println!(
        "REVIEW M18.6 context model and host facts: docs/protocol-review.md; no core visual change; pending M18.9"
    );
}
