//! Closure runs each owned criterion once, followed by the full Chrome gates.
//!
//! Phase B and the live host review were confirmed by the user on 2026-10-10.
//! The native scripts therefore run normally; historical held/failed receipts
//! remain in M17 evidence alongside the authorized targeted replacements.
use super::{api, demo, ffi, glyph_budget, osc, probes, refresh, transport, windows};
use crate::accept::{BrowserGate, bridge, m16, report};
/// Retained physical runs use the original cadence bounds and all samples.
mod recordings;

/// R14 collects every failure; no failed cohort is fixed or retried here.
pub(super) fn run(passed: &mut bool) {
    report("native and wasm check", crate::commands::check(), passed);
    super::test_gates(passed);
    probes::run(passed);
    glyph_budget::run(passed);
    api::run(passed);
    api::resources(passed);
    // The actual sibling-window cohort protects D-41 sharing under flood.
    windows::run(passed);
    osc::run(passed);
    transport::run(passed);
    // Pointer auditing is exercised by native C, ASan and applicable Miri tests.
    ffi::run(passed);
    demo::run(passed);
    refresh::api(passed);
    // Physical recordings include transferred misses, not a chosen passing subset.
    recordings::run(passed);
    // Migration cannot replace earlier end-to-end contracts with API unit tests.
    m16::embedding_regressions(passed);
    // Native work finishes before Chrome owns the GPU. No task skip is accepted.
    bridge::closure(BrowserGate::Full, passed);
    report(
        "M17 full Chrome regression suite at DPR 1 and 2",
        crate::browser::suite().map(|_| ()),
        passed,
    );
    // Human review is separate from both a green subprocess and a valid recording.
    println!("REVIEW M17 comment quality: confirmed by reviewer 2026-10-10; docs/history/M17.md");
    println!(
        "REVIEW M17.demo AppKit live shell/input/IME/resize/clipboard/links: confirmed by user 2026-10-10, Phase B 9a8be55; apps/macos/API-REQUESTS.md; docs/history/M17.md"
    );
    println!(
        "REVIEW M17.refresh built-in FIFO misses: user moved to M12.refresh after M12.layer on 2026-10-10, criterion unchanged; all ten runs retained in target/m17-refresh/; docs/history/M17.md"
    );
}
