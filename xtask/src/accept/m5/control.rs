//! M5.4 control-string and §16 acceptance through public byte input.
//!
//! Protocol families report independently so a working color query cannot
//! hide a clipboard or size-report regression. Exact named tests prevent
//! an accidentally deleted case from passing with a zero-test Cargo filter.
//!
//! SGR tests cover each parameter syntax because vte presents colon groups
//! differently from semicolon groups. OSC tests include both legal string
//! terminators; choosing BEL unconditionally would break programs that
//! expect ST after a color query. OSC 52 is a separate security criterion:
//! a successful palette report must not conceal a clipboard read leak.
//! DCS tests compare exact bytes for both recognized query families.
//!
//! §16 limits bind different resources. The parser has a 64 KiB OSC cap;
//! a title has a Unicode-scalar cap and the title stack has an entry cap.
//! Link URI and table counts are checked separately from shell-zone count.
//! Existing M5.2/M5.3 checks enforce cluster and REP work ceilings. The
//! later kitty and search limits join acceptance when those tasks exist.
//! Every line names observable behavior instead of merely confirming that
//! a source file or test executable exists (project rule R12, R14).

use super::{report, vt_conformance, vt_test};

/// Reports M5.4's control-string semantics through the public byte parser.
pub(super) fn report_control_strings(passed: &mut bool) {
    // Colon colors, sparse underline and independent reset semantics all
    // affect the cell style that a later frame builder will consume.
    report(
        "SGR semicolon/colon colors and underline style table",
        vt_conformance("sgr::indexed_and_bright_colors")
            .and_then(|()| vt_conformance("sgr::rgb_semicolon_and_colon_forms"))
            .and_then(|()| vt_conformance("sgr::underline_style_and_color_table"))
            .and_then(|()| vt_conformance("sgr::reset_does_not_clear_protection")),
        passed,
    );
    // BEL and ST are both common OSC terminators; replies must mirror the
    // request rather than choosing one fixed encoding.
    report(
        "OSC title stack and color query round trips with BEL/ST",
        vt_conformance("osc::title_stack_restores_previous_text")
            .and_then(|()| vt_conformance("osc::color_queries_preserve_request_terminator")),
        passed,
    );
    // OSC 52 is an application request, not permission to read host data.
    report(
        "OSC 52 clipboard reads denied and writes remain policy events",
        vt_conformance("osc::clipboard_read_denied_write_is_event"),
        passed,
    );
    report(
        "OSC 8 links and 133/633 zones retain bounded identities",
        vt_conformance("osc::hyperlink_open_close_keeps_existing_cell")
            .and_then(|()| vt_conformance("osc::shell_mark_aliases_and_progress")),
        passed,
    );
    report(
        "DCS DECRQSS and XTGETTCAP exact wire replies",
        vt_conformance("dcs::status_query_reports_current_state")
            .and_then(|()| vt_conformance("dcs::termcap_query_uses_hex_wire_format")),
        passed,
    );
    report(
        "DA/DSR/CPR/window/appearance exact reply bytes; title query disabled",
        vt_conformance("replies::identity_and_cursor_reports")
            .and_then(|()| vt_conformance("replies::window_and_appearance_reports_exclude_title")),
        passed,
    );
}

/// Reports the §16 ceilings owned by M5.4, including hostile byte strings.
pub(super) fn report_control_limits(passed: &mut bool) {
    // vte's OSC cap is tested through an oversized public-feed string, not
    // inferred from its const generic argument alone.
    report(
        "§16 OSC 64 KiB, title 1024 and title-stack 10 limits",
        vt_conformance("limits::title_and_title_stack_caps")
            .and_then(|()| vt_conformance("limits::oversized_control_strings_are_safe")),
        passed,
    );
    report(
        "§16 truncated OSC 8/52 payloads rejected",
        vt_conformance("limits::truncated_link_and_clipboard_are_rejected"),
        passed,
    );
    report(
        "§16 URI 2 KiB, link-table 4096 and zone 10000 limits",
        vt_conformance("limits::link_and_zone_count_caps")
            .and_then(|()| vt_conformance("limits::uri_and_dcs_caps"))
            .and_then(|()| vt_test("links::tests::stale_generation_does_not_resolve")),
        passed,
    );
    report(
        "§16 grapheme 32-byte, REP grid and scrollback line/byte limits",
        vt_test("terminal::tests::zero_width_joins_previous_cell_with_bounded_cluster")
            .and_then(|()| vt_conformance("editing::repeat_cannot_run_beyond_grid_budget"))
            .and_then(|()| {
                vt_test("ring::tests::byte_limit_restricts_history_independently_of_line_limit")
            }),
        passed,
    );
}
