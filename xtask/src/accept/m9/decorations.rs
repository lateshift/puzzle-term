//! M9.3 checks declaration lifetime and overlay-first pointer ownership (§11.3–11.4).
//!
//! Session-owned model routing uses hit regions returned by presented render views.
//! Its witness runs with and without application tracking at both device scales.
//! Chrome's actual DOM witness is checked in the preceding full two-DPR suite.
//! Public registry tests independently cover clipped regions and drag capture.
//! Source-lifetime tests include identical rewrites and valid content-preserving reflow.
//! An invalid SVG update must leave the old declaration and caller markers intact.
//! Bundled references are compared by the shared golden command before this task.
//! These mechanical checks do not certify the pending native/browser appearance review.
//! Each lifetime check uses a separate source mutation so scrolling cannot mask rewriting.
//! The eviction case trims actual retained VT rows rather than deleting a provider directly.
//! The pointer case permits parser output between source capture and callback delivery.
//! Its empty transport and selection observations protect overlay-first ownership.
//! The integration witness removes a captured provider before releasing the button.
//! Stable registration IDs must keep that release from reaching a replacement provider.
//! Region clipping uses the visible nested scissor, not only its untrimmed bounds.
//! Pending review lines remain visible even when all exact mechanical witnesses pass.
use super::super::{m7::test, report};

/// Checks distinct public guarantees; a removed exact test cannot become an empty pass.
pub(super) fn run(passed: &mut bool) {
    for (criterion, name) in [
        (
            "M9.3 auto-remove on rewrite/eviction, stable across reflow",
            "auto_remove_preserves_reflow_but_rejects_rewrites_and_eviction",
        ),
        (
            "M9.3 detached shell pointer ownership and cursor at DPR 1/2",
            "detached_pointer_consumes_before_selection_and_tracking_at_two_dprs",
        ),
        (
            "M9.3 atomic validation and caller-owned source markers",
            "failed_decoration_update_preserves_old_definition",
        ),
    ] {
        // Session's public controller and detached begin/finish handoff supply the observations.
        report(
            criterion,
            test(
                "puzzle-term",
                &format!("session::tests::decorations::{name}"),
            ),
            passed,
        );
    }
    report(
        "M9.3 nested region clipping, enter/leave and captured release",
        pointer(),
        passed,
    );
    println!(
        "REVIEW M9.3 declarations and pointer routing: pending; assets/scenarios/overlay-decorations; README.md"
    );
}

/// Runs the independent public registry integration witness and requires its exact success line.
fn pointer() -> Result<(), String> {
    let name = "clips_hover_and_captured_release_share_one_router";
    let output = crate::commands::capture(
        "cargo",
        &[
            "test",
            "-p",
            "puzzle-overlay",
            "--test",
            "pointer",
            name,
            "--",
            "--exact",
        ],
    )?;
    // A successful Cargo process with zero tests cannot establish ownership or clipping.
    if output.contains(&format!("test {name} ... ok")) {
        Ok(())
    } else {
        Err("overlay pointer witness did not execute".into())
    }
}
