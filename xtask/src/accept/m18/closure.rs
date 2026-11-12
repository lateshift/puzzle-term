//! Final M18 regression and browser gates reuse earlier budget implementations.
//!
//! Protocol conformance alone cannot prove ordinary terminal performance. The
//! unchanged M16 measurements still cover parsing, snapshots, glyphs, full and
//! changed-row builds, floods and scrolling through the production embedding path.
//! M17's optimized witnesses retain the input and lifecycle limits; their debug
//! counterparts remain functional tests rather than scheduler-sensitive budgets.
//! These helpers report every failure and continue, so a fast parser cannot hide
//! missing ink, a slow key, or a later Chrome regression. Nothing is retried here.
//! Native work finishes before the browser and bridge gates take the shared GPU.
//! Full acceptance has no browser-skip flag; task selectors remain independent.
//! User-confirmed native host work is recorded separately from pending visual
//! references, prompt-framework behavior and the reviewer's comment spot check.
use crate::accept::{BrowserGate, bridge, m16, m17, report};

/// Runs the exact earlier mechanical contracts without redefining their thresholds.
pub(super) fn run(passed: &mut bool) {
    // The full definition-of-done gate includes native and shared wasm compilation.
    report(
        "M18.9 native and wasm check",
        crate::commands::check(),
        passed,
    );
    // Reuse the original release assertions and paired-clock snapshot distributions.
    m17::test_gates(passed);
    // Parser/frame workloads regenerate their measurement artifacts here.
    // Earlier approved refresh dispositions retain their original owners.
    m16::embedding_regressions(passed);
    // Loopback/PTYS and Chrome are serial after every native GPU workload ends.
    bridge::closure(BrowserGate::Full, passed);
    report(
        "M18.9 full Chrome regression suite at DPR 1 and 2",
        crate::browser::suite().map(|_| ()),
        passed,
    );
    // Source parsing and exact pixels cannot approve the user's real host workflow.
    println!("REVIEW M18.9 protocols in app and harness: docs/protocol-review.md; pending user");
    println!(
        "REVIEW M18.9 reference candidates and comments: docs/history/M18.md; README.md; pending reviewer"
    );
}
