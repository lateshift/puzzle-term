//! M5.5 Unicode, resizing and reflow acceptance.
//!
//! Opt-in grapheme width and primary reflow are independent of the ASCII
//! parser path. The native budget runs on a fresh release example artifact
//! so a prior result cannot satisfy the current source tree.
//!
//! The text-preservation proptest checks hard and soft line boundaries as
//! the width narrows and widens again. Separate cases exercise cursor and
//! tracked-marker remapping, colored erased cells, wide pairs and the
//! alternate screen's deliberate lack of reflow. A compact before/after
//! grid dump remains reviewable beside those mechanical properties.
//! The timing criterion uses the §15.2 10,000-line × 200-column fixture
//! and the approved 20 ms ceiling; it never relaxes that ceiling (R14).

use super::{report, vt_conformance};

/// Reports M5.5 Unicode, resize and native reflow criteria separately.
pub(super) fn report_reflow(passed: &mut bool) {
    // UAX #29 clustering is opt-in; the default wcwidth model must retain
    // four columns for a two-emoji ZWJ sequence until mode 2027 is set.
    report(
        "mode 2027 VS16, flag and ZWJ width table; ambiguous-width option",
        vt_conformance("unicode::mode_2027_joins_zwj_emoji")
            .and_then(|()| vt_conformance("unicode::mode_2027_vs16_and_flag_widths"))
            .and_then(|()| vt_conformance("unicode::ambiguous_width_setting_changes_cell_count")),
        passed,
    );
    report(
        "proptest reflow narrow/wide preserves logical-line text",
        vt_conformance("reflow::narrowing_and_widening_preserve_text"),
        passed,
    );
    report(
        "cursor and tracked positions remap across primary reflow",
        vt_conformance("reflow::cursor_and_marker_remap_to_reflowed_text")
            .and_then(|()| vt_conformance("reflow::resize_resets_geometry_and_reports_mode_2048")),
        passed,
    );
    report(
        "wide pairs and background cells survive; alternate grid never reflows",
        vt_conformance("reflow::wide_character_remains_whole_after_reflow")
            .and_then(|()| vt_conformance("reflow::background_cells_survive_reflow"))
            .and_then(|()| vt_conformance("reflow::alternate_screen_does_not_reflow")),
        passed,
    );
    report(
        "small before/after reflow grid snapshots",
        vt_conformance("reflow::small_reflow_snapshots"),
        passed,
    );
    // The release example measures only resize of a prepared 10k×200 grid.
    // Five trials' median must meet the unchanged §15.2 native ceiling.
    let result = crate::corpus::reflow_ms().and_then(|ms| {
        if ms <= 20.0 {
            Ok(())
        } else {
            Err(format!("{ms:.3} ms above 20 ms"))
        }
    });
    report("10,000×200 reflow <= 20 ms native", result, passed);
}
