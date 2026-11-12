//! M8.7 browser criteria share one complete two-DPR suite, never empty filters.
//!
//! The wheel witness dispatches real cancelable DOM events across every routing
//! destination and hardware unit. It checks both default ownership and bytes.
//! Public API witnesses use served fonts/packs and the normal asynchronous create.
//! Pointer checks run through capture listeners, Session policy and copy bindings.
//! The clipboard service alone is replaced so the user's clipboard stays untouched.
//! Search and selection readbacks inspect actual submitted canvas pixels: every
//! expected cell needs authored paper and its own ink, and every outside pixel
//! must be identical. Neither a correct grid nor a fill-only frame can pass.
//! Neutral touch timing checks decay and idle cessation with exact injected time.
//! Stable command identity and top/centre navigation are tested natively as well.
//! Each browser label requires the exact successful witness twice, once per scale.
//! Counting successful names cannot certify visual feel; Chrome review stays pending.
//! A renamed or removed witness fails its label instead of running an empty filter.
//! A failed suite fails every browser label, even if one isolated test printed ok.
//! Full output is retained by the suite driver for assertion and protocol diagnosis.
//! The API checks cover disposed handles and invalid arguments alongside behavior.
//! Search progress is advanced by normal refreshes and read through its public state.
//! Scroll tests verify accepted position facts as well as direct navigation targets.
//! Touch input uses actual DOM timestamps; neutral tests separately inject exact time.
//! Selection gestures exercise the shared state machine rather than selecting by API.
//! The pixel witness uses public operations and never rebuilds an offscreen snapshot.
//! Trusted click actions supply OS/browser counts, with zero pointerdown detail.
//! The hidden-frame guard prepares new glyphs before skipping the real draw path,
//! then requires every cell's ink from the next presentation at both scales.
//! This protects cached placement/upload ownership during delayed presentation.
use super::{report, test};
/// Core additions exist before the JS facade can expose their behavior.
pub(super) fn core(passed: &mut bool) {
    report(
        "M8.7 one-finger kinetic decay, cancellation and idle",
        test(
            "puzzle-term",
            "touch::tests::drag_momentum_cancellation_and_idle",
        ),
        passed,
    );
    report(
        "M8.7 stable zone identity and top/centre line alignment",
        test(
            "puzzle-term",
            "session::tests::scrolling::zone_identity_and_line_alignment",
        ),
        passed,
    );
}
/// Verifies exact named tests at both DPRs; full-suite errors remain failures.
pub(super) fn browser(output: &Result<String, String>, passed: &mut bool) {
    for (label, name) in [
        (
            "wheel routing/default ownership and pixel/line/page units",
            "viewport::wheel_consumption_and_units_follow_shared_routing",
        ),
        (
            "all viewport APIs and scroll event",
            "viewport::viewport_api_members_and_scroll_event",
        ),
        (
            "touch drag, reduced motion, momentum and tap focus",
            "viewport::touch_drag_momentum_and_tap_focus",
        ),
        (
            "all selection APIs and selectionChange event",
            "selection_search::selection_api_members_and_selection_event",
        ),
        (
            "pointer gestures/tracking and copy inside keydown",
            "selection_search::pointer_selection_and_copy_in_key_handler",
        ),
        (
            "trusted double/triple presses select word and wrapped line",
            "trusted_clicks::trusted_double_and_triple_click_select_word_and_wrapped_line",
        ),
        (
            "all search APIs, live progress and searchRequested event",
            "selection_search::search_api_members_progress_and_requested_event",
        ),
        (
            "public served-assets per-cell selection/search fills and ink",
            "selection_search::public_selection_and_search_fill_exact_cells_with_served_assets",
        ),
        (
            "hidden prepared frame retains every glyph for the next presentation",
            "presentation::hidden_prepared_frame_keeps_every_glyph_for_next_presentation",
        ),
    ] {
        let witness = format!("test runtime::tests::{name} ... ok");
        let result = output.as_ref().map_err(Clone::clone).and_then(|text| {
            if text.matches(&witness).count() == 2 {
                Ok(())
            } else {
                Err(format!("missing two-DPR browser witness {name}"))
            }
        });
        report(&format!("M8.7 {label}"), result, passed);
    }
}
