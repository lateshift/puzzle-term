//! HUD acceptance distinguishes observation from drawing and scheduling.
//! The hidden branch must take no clocks, produce no statistics and allocate
//! nothing while idle. The visible branch receives completed presentation facts
//! without creating a self-sustaining loop merely to update its own counters.
//! Readback proves actual glyph ink, while appearance remains a human review.
//! These focused witnesses also run as part of the complete milestone gate.
use super::super::{m7::test, report};
/// Checks observation policy and the same public painter at both densities.
pub(super) fn run(passed: &mut bool) {
    for name in [
        "hidden_hud_has_no_frame_statistics_or_idle_work",
        "shown_hud_observes_real_frame_and_never_self_schedules",
    ] {
        // Exact names prevent a successful empty Cargo filter from accepting the seam.
        report(
            &format!("M9.5 {name}"),
            test(
                "puzzle-term",
                &format!("session::tests::builtin_hud::{name}"),
            ),
            passed,
        );
    }
    // Two real presentations paint the previous frame's measured statistics.
    report(
        "M9.5 performance HUD GPU ink DPR 1/2",
        crate::commands::quiet(
            "cargo",
            &["xtask", "harness", "assets/scenarios/overlay-hud.pzs"],
        ),
        passed,
    );
    // Pixel assertions establish ink only; panel readability still needs human review.
    println!("REVIEW M9.5 HUD appearance/comments: pending; target/harness/overlay-hud");
}
