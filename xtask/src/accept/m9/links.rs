//! A successful test command must include its exact witness, not an empty filter.
//! Screenshot readback is separate from human approval of the visible appearance.
//! Focused task receipts do not replace the complete milestone closure gate.
//! Link acceptance checks actual Session gestures, policy and GPU ink at both DPRs.
//! Only explicit hover queries detect URLs; the parser does not scan for links.
//! Named tests prove that source changes cannot redirect a click to unseen text.
//! Readback catches missing submitted annotations independently from model data.
use super::super::{m7::test, report};
/// Runs the public link contracts without waiving any historical acceptance line.
pub(super) fn run(passed: &mut bool) {
    for name in [
        "link_group_delay_and_presented_click_policy",
        "link_open_rechecks_rewritten_uri_and_web_file_policy",
    ] {
        // These witnesses pass through real pointer routing and normal command policy.
        report(
            &format!("M9.5 {name}"),
            test(
                "puzzle-term",
                &format!("session::tests::builtin_links::{name}"),
            ),
            passed,
        );
    }
    // Both images use OSC output and a presented hit table, rather than fabricated ink.
    report(
        "M9.5 grouped hyperlink GPU ink DPR 1/2",
        crate::commands::quiet(
            "cargo",
            &["xtask", "harness", "assets/scenarios/overlay-links.pzs"],
        ),
        passed,
    );
    // Human eyes still judge URL readability and pointer behavior in the live shells.
    println!("REVIEW M9.5 links: pending; target/harness/overlay-links; README.md");
}
