//! Host UI exclusions share placement/search policy without changing text.
//!
//! The owner regression builds actual search/HUD primitives at both DPRs. It
//! checks the selected cell's geometry as well as unchanged grid dimensions.
//! Clearing coverage restores the default corner, so a stale retained list
//! cannot satisfy the test. Native C separately proves validation and clear
//! through the checked header, without calling a private Session method.
//!
//! The C path also guards the new export/header/caller catalogue against drift.
//! Neither the task nor the owner test changes any scrolling or pixel tolerance.
use crate::accept::report;

/// Keep core placement and the independent C caller as separate criteria.
pub(super) fn run() -> bool {
    let mut passed = true;
    report(
        "M12.occlusion overlay/control placement and match centring, DPR 1/2",
        core(),
        &mut passed,
    );
    report(
        "M12.occlusion checked C list, validation and clear",
        crate::ffi::host(),
        &mut passed,
    );
    passed
}

/// Require the regression name as well as a successful Cargo exit.
fn core() -> Result<(), String> {
    let output = crate::commands::capture("cargo", &["test", "-p", "puzzle-term", "occlusion"])?;
    // Empty test filters must not stand in for actual floating-view builds.
    for name in [
        "host_top_right_ui_moves_overlays_and_centres_matches_outside_it",
        "copy_control_slides_left_then_below_or_disappears_with_its_hit_target",
        "occlusion_removes_active_link_target_without_moving_its_underline",
        "host_occlusion_elsewhere_preserves_immediate_modifier_click",
    ] {
        if !output.contains(&format!("::{name} ... ok")) {
            return Err(format!(
                "missing host occlusion regression {name}: {output}"
            ));
        }
    }
    Ok(())
}
