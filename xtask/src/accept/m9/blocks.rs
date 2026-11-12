//! A successful test command must include its exact witness, not an empty filter.
//! Screenshot readback is separate from human approval of the visible appearance.
//! Focused task receipts do not replace the complete milestone closure gate.
//! Command-block acceptance covers parsed boundaries, output copying and submitted ink.
//! Tests go through Session pointer routing and the ordinary clipboard interceptor chain.
//! The GPU scenario uses the same public model/view API offered to extensions.
use super::super::{m7::test, report};
/// Checks explicit command identity, clock ownership and both display densities.
pub(super) fn run(passed: &mut bool) {
    for name in [
        "command_block_duration_hover_copy_and_policy",
        "explicit_empty_output_and_evicted_zone_are_inert",
    ] {
        // Exact filters cannot silently pass after a witness is renamed or removed.
        report(
            &format!("M9.5 {name}"),
            test(
                "puzzle-term",
                &format!("session::tests::builtin_blocks::{name}"),
            ),
            passed,
        );
    }
    // Readback exercises theme colors, owner-provided durations and presented hover regions.
    report(
        "M9.5 command-block GPU ink DPR 1/2",
        crate::commands::quiet(
            "cargo",
            &["xtask", "harness", "assets/scenarios/overlay-blocks.pzs"],
        ),
        passed,
    );
    // Visible quality and live clipboard behavior remain human review obligations.
    println!("REVIEW M9.5 command blocks: pending; target/harness/overlay-blocks; README.md");
}
