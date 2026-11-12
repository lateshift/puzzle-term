//! Independent C ABI acceptance lines preserve failure diagnostics (R14).
//!
//! Header drift is detectable without a logged-in desktop or native GPU startup.
//! Export misuse tests then cover nulls, generations and thread affinity directly.
//! The real C caller supplies AppKit views and exercises successful operations,
//! which a collection of rejected-handle unit tests could not prove.
//! ASan instruments both languages, including close while a PTY is producing output.
//! Miri handles pure pointer/ID logic and deliberately excludes native foreign calls.
//! Each result is reported even after an earlier failure; no successful sanitizer
//! can mask a missing declaration, a native host failure or another sanitizer error.
use crate::accept::report;
/// No sanitizer or caller failure can hide the remaining independent results.
pub(super) fn run(passed: &mut bool) {
    report(
        "M17.ffi header/export/C caller consistency",
        crate::ffi::verify(),
        passed,
    );
    report(
        "M17.ffi exported calls, generations, affinity and marshalling",
        crate::commands::quiet("cargo", &["test", "-p", "puzzle-ffi", "--lib"]),
        passed,
    );
    report("M17.ffi real native C host", crate::ffi::host(), passed);
    run_extra(passed);
    report(
        "M17.ffi ASan all FFI tests and native C host",
        crate::ffi::asan(),
        passed,
    );
    report(
        "M17.ffi Miri pure registry and marshalling",
        crate::ffi::miri(),
        passed,
    );
}

// Optional variants are omitted with the extra C declarations.
include!("ffi/optional_items.rs");
