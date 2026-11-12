//! M5.7 contents and search checks exercised through parser-created rows.
//!
//! The VT round trip is a randomized screen property, so a serializer
//! that merely works on one hand-picked style cannot satisfy persistence.
//! Extraction, search and changes are separate failures because each feeds
//! a different later caller: clipboard, search overlay and JS observers.
use super::{report, vt_test};

/// Executes an exact-name group, failing if any required case disappeared.
fn tests(names: &[&str]) -> Result<(), String> {
    // Cargo treats a filter selecting zero tests as success. `vt_test`
    // checks the exact executed name, so deleting a case is observable.
    for &name in names {
        vt_test(name)?;
    }
    Ok(())
}

/// Reports persistence and clipboard behavior on retained screen rows.
fn report_copy(passed: &mut bool) {
    // Persistence replays ordinary VT bytes through a fresh parser. Its
    // property case randomizes both cells and mode flags so private grid
    // cloning cannot masquerade as a valid terminal export.
    report(
        "VT serialize proptest round trip",
        tests(&[
            "view::tests::random_screen_vt_round_trip",
            "view::tests::vt_serialize_replays_scrollback_in_order",
        ]),
        passed,
    );
    // Clipboard text has two boundary policies: logical-line joining and
    // rectangular row preservation. Wide placeholder columns and cluster
    // extras must not become duplicate or missing Unicode output.
    report(
        "copy extraction, block rows, wide spacers and clusters",
        tests(&[
            "view::tests::extraction_and_bulk_cells_keep_clusters_and_wide_spacers",
            "selection::tests::block_copy_preserves_rows_and_simple_copy_joins_wrap",
            "selection::tests::row_version_rewrite_invalidates_selection",
            "selection::tests::selection_survives_primary_reflow_without_rewrite",
            "selection::tests::selected_row_rewrite_after_reflow_is_rejected",
            "selection::tests::word_and_line_selection_expand_from_parser_rows",
            "selection::tests::semantic_selection_uses_shell_boundary_columns",
        ]),
        passed,
    );
}

/// Reports every M5.7 work-order acceptance criterion independently.
pub(super) fn report_contents(passed: &mut bool) {
    report_copy(passed);
    // Search results cross the physical wrap boundary, and repeated host
    // queries release their position handles. The registry test protects
    // against old handles reading a newly reused slot.
    report(
        "literal search crosses soft wraps and reuses tracked results",
        tests(&[
            "search::tests::literal_search_crosses_soft_wrap_with_smart_case",
            "search::tests::repeated_search_reuses_tracked_slots",
            "search::tests::first_next_and_previous_wrap_to_expected_matches",
            "search::tests::search_result_cap_is_ten_thousand",
            "tracked::tests::released_slot_is_reused_without_resurrecting_old_handle",
        ]),
        passed,
    );
    // Views resolve live palette state, detect plain URLs only on demand,
    // and expose generation-checked markers without copying the grid.
    report(
        "BufferView colors, links, zones and marker lifecycle",
        // Theme/OSC recoloring and inferred URLs are read-time views;
        // neither should rewrite a row or enter the hot parser path.
        tests(&[
            "view::tests::cell_view_resolves_raw_color_against_live_palette",
            "view::links::tests::url_detection_crosses_wrap_and_trims_punctuation",
            "view::tests::marker_lifecycle_rejects_stale_or_outside_positions",
        ]),
        passed,
    );
    // A new line identity after full-grid scroll must be visible to the
    // observer even when previously retained rows have unchanged versions.
    report(
        "changes_since reports scrolling lines",
        // The bounded frame journal and unsealed damage must agree on
        // which identities changed without treating a region edit as LF.
        tests(&[
            "view::tests::changes_since_scroll_report_new_rows",
            "view::tests::changes_since_region_edit_has_no_full_scroll_delta",
            "view::tests::changes_since_merges_sealed_frame_damage",
            "view::tests::warm_frame_change_log_allocates_nothing",
        ]),
        passed,
    );
}
