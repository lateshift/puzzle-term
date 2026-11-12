//! Exact mutation semantics, source-order movement and zero-watch work proofs.
#![allow(
    clippy::expect_used,
    reason = "Missing fixture source is the regression being asserted."
)]
use super::*;

/// Watch only real retained source columns, independent of cursor positioning.
fn watch(term: &mut Terminal, start: (usize, usize), end: (usize, usize)) -> RangeWatch {
    let first = term
        .screen()
        .row(start.0)
        .expect("retained fixture source")
        .id;
    let last = term
        .screen()
        .row(end.0)
        .expect("retained fixture source")
        .id;
    term.watch_source((first, start.1), (last, end.1), 256)
        .expect("retained fixture source")
}

/// Missing source is inert just like an observed rewrite.
fn changed(term: &Terminal, watch: RangeWatch) -> bool {
    term.source_watch(watch).is_none_or(|state| state.changed)
}

#[test]
fn indistinguishable_final_cells_and_versions_still_have_distinct_write_witnesses() {
    let mut inside = Terminal::new(12, 4, 64, 64 * 1024);
    let mut outside = Terminal::new(12, 4, 64, 64 * 1024);
    inside.advance(b"abcdefgh");
    outside.advance(b"abcdefgh");
    let a = watch(&mut inside, (0, 2), (0, 4));
    let b = watch(&mut outside, (0, 2), (0, 4));
    inside.advance(b"\x1b[1;3Hc\x1b[1;9HX\x1b[1;1H");
    outside.advance(b"\x1b[1;1Ha\x1b[1;9HX\x1b[1;1H");
    assert_eq!(inside.cursor, outside.cursor);
    assert_eq!(inside.write_clock(), outside.write_clock());
    for (a, b) in inside
        .screen()
        .ring
        .iter()
        .zip(outside.screen().ring.iter())
    {
        assert_eq!(
            a.cells.iter().collect::<Vec<_>>(),
            b.cells.iter().collect::<Vec<_>>()
        );
        assert_eq!(a.version, b.version);
    }
    assert!(changed(&inside, a));
    assert!(!changed(&outside, b));
}

#[test]
fn first_last_and_single_row_columns_observe_half_open_boundaries() {
    for (start, end) in [((0, 2), (0, 4)), ((0, 2), (2, 4))] {
        for (row, column, expected) in [
            (0, 0, false),
            (0, 2, true),
            (end.0, 3, true),
            (end.0, 4, false),
        ] {
            let mut term = Terminal::new(12, 4, 64, 64 * 1024);
            term.advance(b"abcdefgh\r\nijklmnop\r\nqrstuvwx");
            let handle = watch(&mut term, start, end);
            term.advance(format!("\x1b[{};{}HX", row + 1, column + 1).as_bytes());
            assert_eq!(
                changed(&term, handle),
                expected,
                "{start:?}..{end:?}, {row}:{column}"
            );
        }
    }
}

#[test]
fn interior_writes_need_no_column_test_and_redraws_cannot_resurrect_a_watch() {
    let mut term = Terminal::new(12, 4, 64, 64 * 1024);
    term.advance(b"abcdefgh\r\nijklmnop\r\nqrstuvwx");
    let handle = watch(&mut term, (0, 2), (2, 4));
    term.advance(b"\x1b[2;1Hi");
    assert!(changed(&term, handle));
    assert_eq!(term.source_watch_counts().column_tests, 0);
    term.advance(b"\x1b[1;1Habcdefgh\r\nijklmnop\r\nqrstuvwx");
    assert!(changed(&term, handle));
}

#[test]
fn editing_and_selective_erasure_report_only_actual_intersecting_writes() {
    for (bytes, expected) in [
        (b"\x1b[1;7H\x1b[P".as_slice(), false),
        (b"\x1b[1;3H\x1b[P", true),
        (b"\x1b[1;7H\x1b[@", false),
        (b"\x1b[1;3H\x1b[@", true),
        (b"\x1b[1;7H\x1b[X", false),
        (b"\x1b[1;3H\x1b[X", true),
        (b"\x1b[1;1H\x1b[1K", false),
        (b"\x1b[1;3H\x1b[K", true),
        (b"\x1b[1;1H\x1b[1J", false),
        (b"\x1b[1;3H\x1b[J", true),
        (b"\x1b[1;1H\x1b[?2K", false),
    ] {
        let mut term = Terminal::new(12, 4, 64, 64 * 1024);
        term.advance(b"\x1b[1\"qabcdefgh\x1b[0\"q");
        let handle = watch(&mut term, (0, 2), (0, 4));
        term.advance(bytes);
        assert_eq!(changed(&term, handle), expected, "{bytes:?}");
    }
}

#[test]
fn reflow_preserves_outside_writes_and_detects_inside_writes_before_and_after() {
    for inside in [false, true] {
        let mut term = Terminal::new(12, 4, 64, 64 * 1024);
        term.advance(b"abcdefghijkl");
        let handle = watch(&mut term, (0, 2), (0, 8));
        term.advance(if inside { b"\x1b[1;3Hc" } else { b"\x1b[1;1Ha" });
        term.resize(5, 4);
        assert_eq!(changed(&term, handle), inside);
        term.resize(17, 4);
        assert_eq!(changed(&term, handle), inside);
        if !inside {
            let state = term.source_watch(handle).expect("retained fixture source");
            let y = term
                .screen()
                .ring
                .index_of(state.start.0)
                .expect("retained fixture source")
                - term.screen().ring.scrollback_len();
            term.advance(format!("\x1b[{};{}Hc", y + 1, state.start.1 + 1).as_bytes());
            assert!(changed(&term, handle));
        }
    }
}

#[test]
fn row_handle_moves_keep_source_order_even_when_line_ids_are_reordered() {
    let mut term = Terminal::new(12, 6, 64, 64 * 1024);
    term.advance(b"aa\r\nbb\r\ncc\r\ndd\r\nee\r\nff");
    let handle = watch(&mut term, (2, 1), (3, 1));
    // Insert above the watched text: the new high LineId precedes older rows.
    term.advance(b"\x1b[2;5r\x1b[2;1H\x1b[L");
    assert!(!changed(&term, handle));
    term.advance(b"\x1b[4;2Hc");
    assert!(changed(&term, handle));
}

#[test]
fn history_scroll_preserves_survivors_but_eviction_reset_and_screens_revoke() {
    let mut term = Terminal::new(12, 4, 64, 64 * 1024);
    term.advance(b"text");
    let handle = watch(&mut term, (0, 0), (0, 4));
    term.advance(b"\r\nnext\r\nnext\r\nnext\r\nnext");
    assert!(!changed(&term, handle));
    for command in [b"\x1bc".as_slice(), b"\x1b[?47h", b"\x1b[2J", b"\x1b[3J"] {
        let mut term = Terminal::new(12, 4, 64, 64 * 1024);
        term.advance(b"text");
        let handle = watch(&mut term, (0, 0), (0, 4));
        term.advance(command);
        assert!(changed(&term, handle));
    }
    term.advance(&b"next\r\n".repeat(100));
    assert!(changed(&term, handle));
}

#[test]
fn no_watch_and_watched_printing_have_exact_operation_and_optional_work_counts() {
    for enabled in [false, true] {
        let mut term = Terminal::new(12, 4, 64, 64 * 1024);
        term.advance(b"abcdefgh\r\nijklmnop\r\nqrstuvwx");
        if enabled {
            watch(&mut term, (0, 2), (2, 4));
        }
        term.start_operation_metrics();
        term.advance(b"\x1b[1;1Ha\x1b[2;1Hi");
        assert_eq!(term.operation_metrics(), Some([0; 10]));
        let counts = term.source_watch_counts();
        assert_eq!(counts.row_tests, if enabled { 2 } else { 0 });
        assert_eq!(counts.interval_tests, if enabled { 2 } else { 0 });
        assert_eq!(counts.column_tests, if enabled { 1 } else { 0 });
        assert_eq!(counts.invalidations, u64::from(enabled));
    }
}

#[test]
fn terminal_clock_is_unique_across_rows_screens_and_ris_without_wrapping() {
    let mut term = Terminal::new(12, 4, 64, 64 * 1024);
    term.register_source_observer();
    let mut previous = 0;
    for input in [b"A".as_slice(), b"\r\nB", b"\x1b[?47hC", b"\x1bcD"] {
        term.advance(input);
        let row = term
            .screen()
            .row(term.cursor.row)
            .expect("retained fixture source");
        assert!(row.version > previous);
        assert_eq!(row.version, term.write_clock());
        previous = row.version;
    }
}

#[test]
fn warmed_boundary_observation_and_lookup_allocate_nothing() {
    let mut term = Terminal::new(12, 4, 64, 64 * 1024);
    term.advance(b"abcdefgh");
    let handle = watch(&mut term, (0, 2), (0, 4));
    let measured = allocation_counter::measure(|| {
        for _ in 0..100 {
            term.advance(b"\x1b[1;1Ha");
            assert!(!changed(&term, handle));
        }
    });
    assert_eq!(measured.count_total, 0);
    assert_eq!(term.source_watch_counts().row_tests, 100);
    assert_eq!(term.source_watch_counts().column_tests, 100);
}

#[test]
fn explicit_multicell_joins_invalidate_source_cells_and_not_just_line_breaks() {
    let mut term = Terminal::new(12, 4, 64, 64 * 1024);
    term.advance(b"\x1b]66;s=2:w=1;A\x1b\\");
    let handle = watch(&mut term, (0, 0), (0, 2));
    term.advance("\u{301}".as_bytes());
    assert!(changed(&term, handle));
}

#[test]
fn inactive_screen_order_metadata_cannot_invalidate_current_screen_source() {
    let mut term = Terminal::new(12, 4, 64, 64 * 1024);
    term.advance(b"\x1b]66;s=2:w=1;A\x1b\\");
    watch(&mut term, (0, 0), (0, 2));
    term.advance(b"\x1b[?47habcd");
    let alternate = watch(&mut term, (0, 0), (0, 4));
    term.resize(12, 1);
    assert!(!changed(&term, alternate));
}

#[test]
fn a_new_row_inserted_inside_the_source_cannot_be_erased_by_a_reflow_rebase() {
    let mut term = Terminal::new(12, 4, 64, 64 * 1024);
    term.advance(b"abcd\r\nefgh\r\nijkl\r\nmnop");
    let source = watch(&mut term, (0, 0), (2, 2));
    // The discarded bottom row lies outside the source, while the new blank
    // enters between its live endpoints. Both endpoints themselves survive.
    term.advance(b"\x1b[2;4r\x1b[2;1H\x1b[L");
    term.resize(5, 4);
    assert!(changed(&term, source));
}

#[test]
fn dropping_an_interior_rectangle_during_reflow_invalidates_surviving_endpoints() {
    let mut term = Terminal::new(12, 6, 64, 64 * 1024);
    term.advance(b"pre\r\n\x1b]66;s=2:w=4;A\x1b\\\r\n\r\npost");
    let source = watch(&mut term, (0, 0), (3, 4));
    term.resize(5, 6);
    assert!(changed(&term, source));
}

#[test]
fn reflow_destinations_inherit_only_the_maximum_of_contributing_source_rows() {
    let mut term = Terminal::new(6, 4, 64, 64 * 1024);
    term.advance(b"abcdefghijkl");
    let first = term.screen().row(0).expect("first source").version;
    let second = term.screen().row(1).expect("second source").version;
    term.resize(12, 4);
    assert_eq!(
        term.screen().row(0).expect("merged").version,
        first.max(second)
    );
    term.resize(3, 4);
    for row in term.screen().ring.iter().take(4) {
        assert_eq!(row.version, first.max(second));
    }
}

#[test]
fn repeated_interior_writes_classify_membership_once_at_row_granularity() {
    let mut term = Terminal::new(120, 4, 64, 64 * 1024);
    term.advance(b"first\r\nmiddle\r\nlast");
    let source = watch(&mut term, (0, 2), (2, 4));
    term.advance(b"\x1b[2;1H");
    term.advance(&[b'X'; 100]);
    assert!(changed(&term, source));
    let counts = term.source_watch_counts();
    assert_eq!(counts.row_tests, 100);
    assert_eq!(counts.row_cache_hits, 99);
    assert_eq!(counts.interval_tests, 1);
    assert_eq!(counts.column_tests, 0);
    assert_eq!(counts.invalidations, 1);
}

#[test]
fn circular_order_coordinates_preserve_identity_and_detect_interior_writes() {
    let mut term = Terminal::new(12, 4, 64, 64 * 1024);
    term.advance(b"first\r\nmiddle\r\nlast");
    let source = watch(&mut term, (0, 2), (2, 4));
    let (screen, clock) = term.screen_clock_mut();
    clock.begin_move();
    for (index, row) in screen.ring.iter().enumerate() {
        clock.moving_boundary(row.id, u32::MAX.wrapping_add(index as u32));
    }
    for (index, row) in screen.ring.rows_mut().enumerate() {
        clock.moved(row, u32::MAX.wrapping_add(index as u32));
    }
    assert!(!changed(&term, source));
    term.advance(b"\x1b[2;1Hm");
    assert!(changed(&term, source));
}
