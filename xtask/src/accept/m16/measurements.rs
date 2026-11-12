//! M16.3 exposed-row budget defects have exact allocation and release CPU witnesses.
//!
//! A cached, unchanged viewport cannot satisfy these checks. Each witness exposes
//! history with a warmed glyph set, including the first miss and fractional edge
//! changes. The release example verifies one rebuilt row on every sample at both
//! DPRs and retains all timings. The existing M8 0.2-ms median check is reused
//! without changing its threshold or output parser. Raw maximums remain evidence.
//! These checks do not certify five clean hardware runs or user scrolling feel.
use super::super::{m7::test, m8, report};

/// Exposed-row scratch and the unchanged CPU ceiling report independently (R14).
pub(super) fn run(passed: &mut bool) {
    for (label, name) in [
        (
            "M16.3 first exposed history row reuses warmed storage",
            "frame_builder::tests::scrolling::first_exposed_history_row_reuses_warmed_frame_storage",
        ),
        (
            "M16.3 fractional edge changes retain warmed row storage",
            "frame_builder::tests::scrolling::exposed_history_row_retains_capacity_across_edge_changes",
        ),
    ] {
        // An exact test name cannot pass from a zero-test cargo filter.
        report(label, test("puzzle-term", name), passed);
    }
    // The M8 example now keeps both DPRs, every exposed row and raw worst samples.
    report(
        "M16.3 newly exposed scroll row CPU ≤0.2 ms native",
        m8::scroll_budget(),
        passed,
    );
}
