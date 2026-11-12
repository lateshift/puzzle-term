//! D-61's native find contract, independently checked before any host component UI.
//! These criteria add no measurement rounds or native UI self-certification.
//!
//! Core tests exercise both the unchanged built-in default and host ownership,
//! including changing an open field without retaining invisible keyboard capture.
//! Their Cargo result names are checked so a renamed filter cannot pass vacuously.
//! The existing native C fixture supplies an independent consumer of puzzle.h:
//! its events arrive only through tick, after owner work and real presentation.
//! It proves the option, find intent, result counts, navigation and clear, as
//! well as selection extraction without a clipboard write. Header/export parity
//! stays a separate line, so successful Rust tests cannot hide C contract drift.
//! Platform component visuals are intentionally left as a named user review.
use crate::accept::report;

/// Test the product seam; the AppKit component remains a separate user review.
pub(super) fn run() -> bool {
    let mut passed = true;
    report(
        "M12.search host ownership, retained results and selection messages",
        core(),
        &mut passed,
    );
    report(
        "M12.search checked C header and calls",
        crate::ffi::verify(),
        &mut passed,
    );
    report(
        "M12.search C host requested/find/next/state/clear and selection",
        crate::ffi::host(),
        &mut passed,
    );
    println!("REVIEW M12.search native component: session 2; docs/embedding/search.md");
    passed
}

/// Named assertions prevent a successful empty Cargo filter from certifying the seam.
fn core() -> Result<(), String> {
    let text = crate::commands::capture(
        "cargo",
        &["test", "-p", "puzzle-term", "session::tests::search"],
    )?;
    // Require the built-in/default result too: a host-only fixture would miss
    // regressions for embedders that never opt in to native find controls.
    // Automatic selection must preserve its first choice across later slices;
    // the immediate-Enter witness also protects the deferred-navigation order.
    for name in [
        "host_find_contract_does_not_capture_text_or_change_keybinding_platform",
        "replacing_open_builtin_query_releases_input_but_preserves_matches",
        "search_actions_follow_platform_and_result_availability",
        "early_navigation_waits_for_matches_and_keeps_latest_direction",
        "initial_current_prefers_newest_visible_match_without_scrolling",
        "initial_offscreen_match_centres_once_and_later_slices_preserve_it",
        "immediate_enter_equals_enter_after_automatic_selection",
    ] {
        if !text.contains(&format!("::{name} ... ok")) {
            return Err(format!("missing search regression {name}: {text}"));
        }
    }
    Ok(())
}
