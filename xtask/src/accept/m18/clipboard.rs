//! OSC 5522 acceptance crosses wire, Session policy and both native C surfaces.
//! Host callbacks complete asynchronously; the C fixture proves unrelated output
//! proceeds while a write is pending. Grant tests cover each security expiry.
//! Sanitizers exercise the same checked arrays and exported operations, without
//! substituting an offscreen or Rust-only caller for the installed header.
//! The task selector is a partial milestone gate, not a declaration of M18 closure.
//! The full milestone command still reports later unimplemented tasks as failures.
//! Parser tests cover arbitrary stream splits, strict base64 and decoded-size bounds.
//! Session tests cover policy denial, pending-write backpressure and reply ordering.
//! Grant tests exercise cross-session/location refusal and every expiry condition.
//! PNG tests inspect binary pixels without replacing the user's clipboard contents.
//! Recorder tests check that split credentials never enter the diagnostic file.
//! FFI unit tests reject malformed arrays without constructing a native window.
//! The strict C compile proves the installed header matches independent call sites.
//! The linked C fixture crosses actual AppKit surfaces through both native host kinds.
//! It posts host answers later, proving that pending clipboard work cannot stall output.
//! ASan checks foreign buffer lifetimes; Miri checks platform-free validation paths.
//! Each criterion reports independently, retaining earlier failures in the final result.
//! Human clipboard behavior and comment quality remain explicit pending reviews.
//! A partial gate cannot confirm the later notification/drop/context milestones.
use super::report;

/// Independent criteria continue after a failed parser, permission or host line.
pub(super) fn run(passed: &mut bool) {
    for (name, package, filter) in [
        (
            "M18.3 strict bounded OSC 5522 streaming and replies",
            "puzzle-vt",
            "clipboard",
        ),
        (
            "M18.3 asynchronous completion, policy and single-use paste grants",
            "puzzle-term",
            "clipboard",
        ),
        (
            "M18.3 harness bounded PNG clipboard conversion",
            "puzzle-desktop",
            "clipboard::",
        ),
        (
            "M18.3 recordings redact split clipboard credentials",
            "puzzle-embed",
            "recording::tests::",
        ),
        ("M18.3 C argument validation", "puzzle-ffi", "tests::"),
    ] {
        // Tests assert protocol behavior and privacy, not private implementation shapes.
        report(
            name,
            crate::commands::quiet("cargo", &["test", "-p", package, filter]),
            passed,
        );
    }
    report(
        "M18.3 checked header and C MIME calls",
        crate::ffi::verify(),
        passed,
    );
    report(
        "M18.3 MIME completion and paste grants through both C native hosts",
        crate::ffi::host(),
        passed,
    );
    sanitizers(passed);
    println!(
        "REVIEW M18.3 clipboard: docs/protocol-review.md; Swift MIME/changeCount host work in README.md"
    );
}

/// Repeats only instrumented criteria when the reviewer invalidates their environment.
/// The full task and milestone call these same functions with identical assertions.
pub(super) fn sanitizers(passed: &mut bool) {
    report(
        "M18.3 C and Rust clipboard buffers under ASan",
        crate::ffi::asan(),
        passed,
    );
    report(
        "M18.3 clipboard pointer validation under Miri",
        crate::ffi::miri(),
        passed,
    );
}
