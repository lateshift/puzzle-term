//! Puzzle-side D-56 receipts; AppKit source ownership stays with its maintainer.
//!
//! The C fixture supplies both NSView and CAMetalLayer targets, verifies gravity
//! and drawable dimensions, and destroys native objects immediately after free.
//! The app command proves the real opted-in linkage and runs the host's scripts.
use crate::accept::report;

/// Reports layer and app build criteria independently, preserving either failure.
pub(super) fn run(passed: &mut bool) {
    puzzle_side(passed);
    report(
        "M17.demo enabled AppKit build and host test scripts",
        crate::app::run(&["macos".into()]),
        passed,
    );
}

/// The native receipt requires the actual borrowed layer and close contracts through C.
fn puzzle_side(passed: &mut bool) {
    report(
        "M17.demo host CAMetalLayer, drawable size, gravity and synchronous close",
        crate::ffi::host(),
        passed,
    );
    // ASan covers the new raw layer target's lifetime, including close under output.
    report("M17.demo CAMetalLayer ASan", crate::ffi::asan(), passed);
}
