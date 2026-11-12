//! Incremental M5 acceptance while the reviewed work order is underway.
//!
//! Each implemented task contributes separate evidence lines. Later M5
//! tasks append their criteria here; the first STOP after M5.2 intentionally
//! leaves the remainder of §17 for a later run. `--exact` prevents an
//! accidentally renamed unit test from silently satisfying a criterion.
//! A cell-size check protects the memory model, while the warm-scroll test
//! measures heap requests rather than assuming reuse from pointer equality.
//! The tracked-position check covers both a retained history line and its
//! eventual eviction. The identity check then guards monotonically growing
//! IDs independently of the marker registry. These are separate failures
//! because selection can break even while the ring allocation stays sound.
//! Parser acceptance combines property tests with small reviewed snapshots.
//! The property test covers malformed bytes and chunk boundaries; fixed
//! snapshots make delayed wrap and wide-cell repairs easy to inspect.
//! Character-set and C0 checks name xterm control families independently.
//! Throughput uses release code on a warm, scrolling grid, not a parser
//! invocation that discards its callbacks. The timing sample is fresh each
//! run so an old numeric artifact cannot satisfy the current source tree.
//! M5.3 adds public-feed conformance tests, one source file per family.
//! The accept runner executes exact names rather than relying on the full
//! suite's existence: an accidentally deleted or renamed case must fail
//! its own criterion. Screen families are checked independently because
//! cursor placement can remain correct while BCE or scrollback breaks.
//! M5.4 reports SGR, OSC, DCS, replies and bounded-input behavior as
//! separate failures, so a working palette cannot hide a clipboard leak.
//! The public-feed tests intentionally use complete escape strings through
//! `Terminal::advance`. Calling a private handler directly would miss vte's
//! parameter splitting, terminator choice or truncation behavior. Exact
//! test names also ensure a removed conformance case becomes a FAIL line.
//!
//! Security limits are grouped by the resource they bound. Parser string
//! length, title length and stack depth are independent controls; both a
//! deterministic oversize fixture and a property test exercise the byte
//! surface. URI length, table occupancy and zone retention are checked
//! separately from glyph cluster size and REP's work limit. Later M5 tasks
//! add kitty stack and search-result ceilings when those features exist.
//!
//! R14 requires the runner to continue after a failure. These report
//! groups return no early error to `run`, and the shared mutable result
//! accumulates failures while leaving later evidence visible to reviewers.
//! The ASCII corpus runs on each acceptance pass because earlier control
//! branches can inadvertently slow the common printable path (§15.2).
//! Its measured throughput is not inferred from a prior test artifact.
use super::report;

/// M5.7 public buffer, selection, search and change observations.
mod contents;
/// M5.4 SGR, OSC, DCS and resource ceilings.
mod control;
/// M5.6 pure input protocols, with exact wire-table unit cases.
mod input;
/// M5.5b script-replay screenshots through the M4 text seam.
mod preview;
/// M5.5 Unicode, resize and reflow checks.
mod reflow;
/// M5.8 deterministic corpora, native rate floors and wasm size.
mod throughput;

/// Runs one named puzzle-vt unit test and requires evidence it actually ran.
fn vt_test(name: &str) -> Result<(), String> {
    let output = crate::commands::capture(
        "cargo",
        &["test", "-p", "puzzle-vt", "--lib", name, "--", "--exact"],
    )?;
    // Cargo returns success when the filter selects zero tests; inspect its
    // result line so deletion cannot make this acceptance criterion pass.
    if !output.contains(&format!("test {name} ... ok")) {
        return Err(format!("puzzle-vt test {name} did not run"));
    }
    Ok(())
}

/// Runs one conformance case through the public byte-feed integration test.
fn vt_conformance(name: &str) -> Result<(), String> {
    // The integration crate exercises Terminal::advance as an application
    // would; private helper methods cannot silently substitute for parsing.
    let output = crate::commands::capture(
        "cargo",
        &[
            "test",
            "-p",
            "puzzle-vt",
            "--test",
            "conformance",
            name,
            "--",
            "--exact",
        ],
    )?;
    // Cargo succeeds even when the filter selects no cases, so verify that
    // the named xterm behaviour actually executed before reporting PASS.
    if !output.contains(&format!("test {name} ... ok")) {
        return Err(format!("conformance test {name} did not run"));
    }
    Ok(())
}

/// Reports cursor and cell-editing conformance independently.
fn report_cursor_edit(passed: &mut bool) {
    // A DECOM regression would put output on the wrong physical row even
    // though simple movement still appears bounded.
    report(
        "CSI cursor movement and DECOM-relative CUP",
        vt_conformance("cursor::decom_cup_is_relative_to_scroll_region")
            .and_then(|()| vt_conformance("cursor::relative_movement_clamps_at_edges"))
            .and_then(|()| vt_conformance("cursor::tab_stop_controls_share_one_table"))
            .and_then(|()| vt_conformance("cursor::horizontal_margins_require_declrmm")),
        passed,
    );
    // BCE and selective protection have separate input sequences but share
    // erase machinery; both are visible when full-screen apps repaint.
    report(
        "BCE erase and DEC selective protection",
        vt_conformance("editing::erased_cells_keep_current_background")
            .and_then(|()| vt_conformance("editing::selective_erase_keeps_protected_cells"))
            .and_then(|()| vt_conformance("editing::ed_three_clears_only_history"))
            .and_then(|()| vt_conformance("editing::ech_does_not_leave_a_wide_spacer")),
        passed,
    );
    // REP's cap is a §16 denial-of-service boundary; count parsing alone
    // is insufficient to prove its work stays bounded.
    report(
        "bounded REP and horizontal editing",
        vt_conformance("editing::repeat_cannot_run_beyond_grid_budget")
            .and_then(|()| vt_conformance("editing::insert_and_delete_characters")),
        passed,
    );
}

/// Reports scrolling, snapshots, modes and ESC controls for M5.3.
fn report_scroll_modes(passed: &mut bool) {
    // Only the top-origin, full-width primary path may retain history.
    // Region-local scrolling must leave the top status area untouched.
    report(
        "scroll region versus primary history",
        vt_conformance("scroll::partial_scroll_region_does_not_enter_scrollback")
            .and_then(|()| vt_conformance("scroll::top_region_scrolling_keeps_primary_history"))
            .and_then(|()| {
                vt_conformance("scroll::short_top_region_saves_history_without_moving_footer")
            })
            .and_then(|()| vt_conformance("scroll::horizontal_margins_preserve_outer_columns"))
            .and_then(|()| vt_conformance("scroll::insert_delete_lines_inside_region")),
        passed,
    );
    // Compact screen dumps expose both retained history count and cursor
    // placement for reviewer inspection at the next work-order STOP.
    // The snapshots are small enough to audit against xterm's margin rules
    // without blessing a whole terminal session as an opaque reference.
    report(
        "scroll region and DECOM screen snapshots",
        vt_test("terminal::tests::top_scroll_region_with_scrollback_snapshot")
            .and_then(|()| vt_test("terminal::tests::decom_relative_cup_snapshot")),
        passed,
    );
    // 1049 is a state switch with an independent saved cursor. The shell's
    // old text must reappear after a full-screen application exits.
    report(
        "alternate screen preserves primary grid",
        vt_conformance("scroll::alternate_screen_keeps_primary_grid"),
        passed,
    );
    // DECRQM distinguishes unsupported from supported-but-reset modes;
    // programs use that to feature-detect terminal extensions.
    report(
        "DECRQM listed and unknown mode table",
        vt_conformance("modes::decrqm_reports_supported_and_unknown_modes")
            .and_then(|()| vt_conformance("modes::mode_query_tracks_changes"))
            .and_then(|()| vt_test("modes::tests::all_advertised_modes_are_queryable")),
        passed,
    );
    // ESC has a separate parser callback and must not be accidentally
    // covered only by the CSI family tests above.
    report(
        "ESC save/restore, DECALN and RIS",
        vt_conformance("esc::saved_cursor_restores_graphic_state")
            .and_then(|()| vt_conformance("esc::alignment_and_ris"))
            .and_then(|()| vt_conformance("esc::cursor_style_and_soft_reset")),
        passed,
    );
}

/// Reports the four independent M5.1 grid and history guarantees.
fn report_grid(passed: &mut bool) {
    // The representation is a wire-independent render/storage contract.
    report(
        "16-byte cell layout",
        vt_test("cell::tests::cell_has_sixteen_bytes"),
        passed,
    );
    report(
        "scrollback eviction reuses row allocation without heap requests",
        // The ring is warmed before measurement, matching the P1 promise.
        vt_test("ring::tests::warm_scroll_reuses_the_oldest_cell_allocation"),
        passed,
    );
    report(
        "tracked positions survive scrolling and become Gone on eviction",
        // A stale handle must never point to a recycled, unrelated row.
        vt_test("ring::tests::tracked_position_survives_then_becomes_gone"),
        passed,
    );
    report(
        "LineId is monotonic and never reused",
        // A retired line ID cannot reappear when its allocation is recycled.
        vt_test("ring::tests::line_ids_never_repeat"),
        passed,
    );
}

/// Checks the bounded parser, width rules and native plain-text budget.
fn report_parser(passed: &mut bool) {
    // Random byte chunks include incomplete UTF-8 and escape strings, which
    // are the paths most likely to expose an external-input panic (R8).
    report(
        "arbitrary bytes never panic and cursor remains in bounds",
        vt_test("terminal::tests::arbitrary_bytes_never_panic_and_cursor_stays_bounded"),
        passed,
    );
    report(
        "pending-wrap CR LF BS and printable snapshots",
        // A premature wrap changes cursor placement even when final text
        // looks similar; the snapshot includes cursor and WRAPPED state.
        vt_test("terminal::tests::pending_wrap_after_controls_and_printable"),
        passed,
    );
    report(
        "wide character right-margin autowrap snapshots",
        // DECAWM on and off intentionally differ at the final column.
        vt_test("terminal::tests::wide_at_right_margin_with_autowrap_on_and_off"),
        passed,
    );
    report(
        "overwriting either wide half snapshot",
        // Repairing only the first half leaves an invisible stale spacer.
        vt_test("terminal::tests::overwriting_both_halves_of_wide_character"),
        passed,
    );
    report(
        "DEC Special Graphics maps lqkx and shifted sets",
        // Designation and invocation are separate ISO-2022 operations.
        vt_test("terminal::tests::dec_special_graphics_maps_lqkx"),
        passed,
    );
    report(
        "C0 table and bounded zero-width clusters",
        // C0 moves and a combining run both touch cursor state, but only
        // the latter consumes sparse row metadata under the §16 byte cap.
        vt_test("terminal::tests::c0_control_table").and_then(|()| {
            vt_test("terminal::tests::zero_width_joins_previous_cell_with_bounded_cluster")
        }),
        passed,
    );
    report(
        "steady print LF and scrolling make zero allocations",
        // P1 covers the complete callback-to-grid path, not only RowRing.
        vt_test("terminal::tests::steady_print_line_feed_and_scroll_allocate_nothing"),
        passed,
    );
    report(
        "plain ASCII parse >= 250 MB/s native",
        // The work order makes the §15.2 floor a mechanical M5.2 gate.
        ascii_budget(),
        passed,
    );
}

/// Compares a fresh release corpus result with the approved §15.2 floor.
fn ascii_budget() -> Result<(), String> {
    let rate = crate::corpus::ascii_rate()?;
    if rate >= 250.0 {
        Ok(())
    } else {
        Err(format!("{rate:.3} MB/s below 250 MB/s"))
    }
}

/// Executes every implemented M5 criterion and keeps review separate.
pub(super) fn run() -> bool {
    let mut passed = true;
    report(
        "native and wasm check",
        crate::commands::check(),
        &mut passed,
    );
    report_grid(&mut passed);
    report_parser(&mut passed);
    report_cursor_edit(&mut passed);
    report_scroll_modes(&mut passed);
    control::report_control_strings(&mut passed);
    control::report_control_limits(&mut passed);
    reflow::report_reflow(&mut passed);
    preview::report_preview(&mut passed);
    input::report_input(&mut passed);
    contents::report_contents(&mut passed);
    throughput::report_close(&mut passed);
    // R15 still requires a human spot check of the new parser's comments.
    println!("REVIEW M5 comment quality: crates/puzzle-vt/src/; README.md steps");
    passed
}
