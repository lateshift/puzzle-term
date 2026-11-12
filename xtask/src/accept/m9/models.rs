//! M9.4 acceptance pins model geometry, timed visibility and query ownership.
//!
//! Each exact test must execute, rather than passing an empty Cargo filter.
//! Geometry is checked against M8's own track/thumb data at both device scales.
//! Injected time covers reveal, 800-ms hold, fade endpoint and reduced motion.
//! Pointer tests use the shared detached route and exact page/drag operations.
//! Source markers come from parsed prompts, a bell, a search and a selection.
//! Native mode tests use typed/IME/paste input and an inspectable transport.
//! Harness pixels exercise the production primitive compiler and GPU renderer.
//! Images remain pending review under R15, independent of mechanical status.
//! Release-only dragging covers hosts that cancel a queued move before rAF.
//! Query reopening retains text while restoring matches cleared on close.
//! Same-line navigation checks field invalidation independently of scrolling.
//! Configured paste retains clipboard policy while the query suppresses app actions.
//! The clipboard witness compares accepted query text and an untouched terminal cell.
use super::super::{m7::test, report};

/// Runs every model obligation, preserving other criteria after a failed witness.
pub(super) fn run(passed: &mut bool) {
    // Each source-driven behavior has its own failure line in milestone output.
    // Both device scales run within the witnesses rather than one selected screenshot.
    // The exact-test helper checks the positive runner line, guarding renamed tests.
    for (criterion, name) in [
        (
            "scrollbar exact geometry, finite fade and idle at DPR 1/2",
            "scrollbar_geometry_and_finite_auto_hide_at_two_dprs",
        ),
        (
            "scrollbar drag/page and pill follow through shared pointer input",
            "scrollbar_drag_page_hover_and_pill_use_m8_operations",
        ),
        (
            "all marker sources and query/count sampled from M8",
            "every_marker_source_and_search_panel_are_drawn",
        ),
        (
            "native query typed/IME input never reaches PTY; Escape restores input",
            "query::native_query_consumes_keys_and_ime_and_restores_input",
        ),
        (
            "thumb release applies final position without queued motion",
            "scrollbar_release_applies_final_position_without_a_motion_frame",
        ),
        (
            "query reopen restores matches and same-row navigation updates counter",
            "query::reopened_search_restores_retained_query_and_same_line_current_counter",
        ),
        (
            "configured native paste edits the query through clipboard policy",
            "query::native_query_keeps_configured_clipboard_paste_without_pty_bytes",
        ),
    ] {
        // Named witnesses compare public state and submitted ink, not synthetic expected models.
        report(
            &format!("M9.4 {criterion}"),
            test(
                "puzzle-term",
                &format!("session::tests::model_overlays::{name}"),
            ),
            passed,
        );
    }
    // Custom tokens must survive terminal-color derivation and sparse child themes.
    // The thumb additionally retains its authored alpha through fade multiplication.
    // Invalid values are rejected by registration before a provider can observe them.
    report(
        "M9.4 authored UI colors, thumb alpha and sparse inheritance",
        test(
            "puzzle-term",
            "theme::tests::overlay_ui_tokens_survive_derivation_and_child_inheritance",
        ),
        passed,
    );
    // The golden runner rerenders the model scene before its fixed pixel comparison.
    report(
        "M9.4 scrollbar/markers/pill/search DPR references",
        crate::golden::overlays::run(false, false),
        passed,
    );
    println!(
        "REVIEW M9.4 desktop/Chrome appearance, auto-hide and clicks: pending; assets/scenarios/overlay-models; README.md; remaining visual review at M9.7 STOP"
    );
}
