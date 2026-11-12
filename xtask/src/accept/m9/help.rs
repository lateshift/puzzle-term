//! Effective shortcut help is accepted through matcher, Session and native GPU witnesses.
//!
//! Source-layer and filtering tests run through accepted commands rather than calling
//! the painter with manufactured labels. Both scenes check submitted pixels at two
//! device ratios. Their appearance remains a pending human review under R15.
//! Exact test names prevent a removed witness becoming a successful empty Cargo run.
//! The same lines run at task checkpoints and final acceptance; no prior line is waived.
use super::super::{m7::test, report};

/// Checks leader grammar, query ownership and actual compiled annotation ink.
pub(super) fn run(passed: &mut bool) {
    for name in [
        "leader_hint_lists_exact_continuations_and_disappears_on_completion",
        "cheat_sheet_shows_sources_and_owns_only_its_filter_input",
    ] {
        // These tests use real matching and normal policy-controlled input dispatch.
        report(
            &format!("M9.5 {name}"),
            test(
                "puzzle-term",
                &format!("session::tests::builtin_help::{name}"),
            ),
            passed,
        );
    }
    for scene in ["overlay-leader", "overlay-help"] {
        // A GPU readback verifies that source state actually reaches rendered ink.
        // Each scenario changes DPR in place, catching stale scale-dependent geometry.
        report(
            &format!("M9.5 {scene} DPR 1/2"),
            crate::commands::quiet(
                "cargo",
                &["xtask", "harness", &format!("assets/scenarios/{scene}.pzs")],
            ),
            passed,
        );
    }
    // Mechanical ink proves visibility; only the reviewer can approve its appearance.
    println!(
        "REVIEW M9.5 shortcut hints/filter: pending; target/harness/overlay-leader and overlay-help; README.md"
    );
}
