//! Termination regressions use an operation bound, never a wall-clock budget.
#![allow(
    clippy::expect_used,
    reason = "regression fixtures require retained cells and valid watch admission"
)]
use super::*;

/// Run the full caller chunk with a hard LF budget derived from its small grid.
fn feed(term: &mut Terminal, bytes: &[u8]) -> usize {
    let (columns, rows) = term.size();
    let calls =
        crate::scroll_operations::bounded_line_feeds(columns * rows, || term.advance(bytes));
    crate::test_grid::check(term);
    #[cfg(debug_assertions)]
    for screen in [&term.primary, &term.alternate] {
        assert_eq!(screen.ring.check_multicells(), Ok(()));
    }
    calls
}

/// Every physical half must disappear when its rectangle is overwritten.
fn assert_repaired(term: &Terminal) {
    let row = term.screen().row(7).expect("last physical row");
    assert_eq!(row.cells[14].ch, u32::from('a'));
    assert!(!row.flags.contains(RowFlags::WRAPPED));
    assert!(!row.cells[15].flags.contains(CellFlags::MULTICELL));
    let root = term.screen().row(6).expect("root row");
    assert!(!root.cells[14].flags.contains(CellFlags::MULTICELL));
    assert_eq!((term.cursor.row, term.cursor.column), (7, 15));
    assert_eq!(
        term.screen()
            .ring
            .multicells
            .as_deref()
            .map_or(0, |store| store.iter().count()),
        7
    );
}

#[test]
fn ordinary_print_below_scroll_region_cannot_cycle_through_lower_fragments() {
    let mut term = Terminal::new(16, 8, 10_000, 32 * 1024 * 1024);
    // Reviewer reproduction: one 28-byte caller chunk, no injected fuzz bytes.
    let calls = feed(&mut term, b"\x1b[;5r\x1b[6e\x1b]66;s=2;H-inaing\x07a");
    assert_eq!(calls, 1);
    assert_repaired(&term);
}

#[test]
fn split_chunks_and_both_print_modes_preserve_the_blocked_wrap_repair() {
    let bytes = b"\x1b[;5r\x1b[6e\x1b]66;s=2;H-inaing\x07a";
    for watched in [false, true] {
        for split in 0..=bytes.len() {
            let mut term = Terminal::new(16, 8, 10_000, 32 * 1024 * 1024);
            if watched {
                let line = term.cursor_line().expect("initial row");
                term.watch_source((line, 0), (line, 1), 1)
                    .expect("valid watch");
            }
            let calls = feed(&mut term, &bytes[..split]) + feed(&mut term, &bytes[split..]);
            assert_eq!(calls, 1);
            assert_repaired(&term);
        }
    }
}

#[test]
fn lower_reservations_skip_independently_of_decawm_and_erase_when_stuck() {
    let mut term = Terminal::new(4, 4, 0, 0);
    assert_eq!(feed(&mut term, b"\x1b]66;s=2;AB\x07\x1b[2;1H\x1b[?7la"), 1);
    assert_eq!(
        term.screen().row(2).expect("next row").cells[0].ch,
        u32::from('a')
    );
    assert_eq!(
        term.screen()
            .ring
            .multicells
            .as_deref()
            .expect("blocks")
            .iter()
            .count(),
        2
    );
    let mut term = Terminal::new(4, 4, 0, 0);
    assert_eq!(
        feed(
            &mut term,
            b"\x1b[;2r\x1b[3;1H\x1b]66;s=2;AB\x07\x1b[4;1H\x1b[?7la"
        ),
        0
    );
    assert_eq!(
        term.screen().row(3).expect("last row").cells[2].ch,
        u32::from('a')
    );
    assert!(
        !term.screen().row(2).expect("root").cells[2]
            .flags
            .contains(CellFlags::MULTICELL)
    );
}

#[test]
fn scrolling_into_a_blank_row_is_progress_without_a_numeric_cursor_move() {
    let mut term = Terminal::new(4, 4, 8, 32_768);
    let calls = feed(&mut term, b"\x1b[3;1H\x1b]66;s=2;AB\x07a");
    assert_eq!(calls, 2);
    assert_eq!(term.primary.ring.scrollback_len(), 1);
    assert_eq!(
        term.screen().row(3).expect("new row").cells[0].ch,
        u32::from('a')
    );
    assert_eq!((term.cursor.row, term.cursor.column), (3, 1));
    assert_eq!(
        term.screen()
            .ring
            .multicells
            .as_deref()
            .expect("blocks")
            .iter()
            .count(),
        2
    );
}

#[test]
fn wide_text_skips_finite_tall_lower_fragments_until_the_pair_fits() {
    for scale in 2..=7 {
        let mut term = Terminal::new(8, 8, 0, 0);
        let bytes = format!("\x1b]66;s={scale};A\x07\x1b[2;1H漢");
        let calls = feed(&mut term, bytes.as_bytes());
        let (y, x, expected_calls) = if scale == 7 { (7, 0, 6) } else { (1, scale, 0) };
        assert_eq!(calls, expected_calls);
        let row = term.screen().row(y).expect("destination");
        assert_eq!(row.cells[x].ch, u32::from('漢'));
        assert!(row.cells[x].flags.contains(CellFlags::WIDE));
        assert!(row.cells[x + 1].flags.contains(CellFlags::WIDE_SPACER));
        let root = term.screen().row(0).expect("root").cells[0];
        assert_eq!(
            term.screen()
                .ring
                .multicell(root.ch)
                .expect("preserved block")
                .fragments
                .len(),
            scale
        );
    }
}

#[test]
fn sized_text_discards_impossible_fits_without_scrolling_an_unrelated_band() {
    let mut term = Terminal::new(4, 4, 0, 0);
    feed(&mut term, b"OK\x1b[;2r\x1b[4;1H");
    let clock = term.write_clock();
    assert_eq!(feed(&mut term, b"\x1b]66;s=2;A\x07"), 0);
    assert_eq!(term.write_clock(), clock);
    assert_eq!((term.cursor.row, term.cursor.column), (3, 0));
    assert_eq!(
        term.screen().row(0).expect("outside band").cells[0].ch,
        u32::from('O')
    );
    // A required horizontal wrap below the band cannot manufacture a new row.
    feed(&mut term, b"\x1b[4;4H");
    let clock = term.write_clock();
    assert_eq!(feed(&mut term, b"\x1b]66;s=2;A\x07"), 0);
    assert_eq!(term.write_clock(), clock);
    assert_eq!((term.cursor.row, term.cursor.column), (3, 3));
    assert!(term.screen().ring.multicells.is_none());
}

#[test]
fn short_scroll_band_discards_a_tall_block_before_wrap_or_scroll() {
    let mut term = Terminal::new(8, 6, 0, 0);
    feed(&mut term, b"OK\x1b[3;4r\x1b[4;8H");
    let clock = term.write_clock();
    assert_eq!(feed(&mut term, b"\x1b]66;s=3;A\x07"), 0);
    assert_eq!(term.write_clock(), clock);
    assert_eq!((term.cursor.row, term.cursor.column), (3, 7));
    assert!(term.screen().ring.multicells.is_none());
}

#[test]
fn height_fitting_consumes_exact_overflow_in_one_region_scroll() {
    let mut term = Terminal::new(8, 6, 0, 0);
    feed(&mut term, b"TOP\x1b[6;1HBOTTOM\x1b[2;5r\x1b[5;1H");
    term.start_operation_metrics();
    assert_eq!(feed(&mut term, b"\x1b]66;s=3;A\x07"), 0);
    assert_eq!(term.operation_metrics().expect("count observer")[0], 2);
    assert_eq!((term.cursor.row, term.cursor.column), (2, 3));
    assert_eq!(
        term.screen().row(0).expect("above band").cells[0].ch,
        u32::from('T')
    );
    assert_eq!(
        term.screen().row(5).expect("below band").cells[0].ch,
        u32::from('B')
    );
    let root = term.screen().row(2).expect("block root").cells[0];
    assert_eq!(
        term.screen()
            .ring
            .multicell(root.ch)
            .expect("whole block")
            .fragments
            .len(),
        3
    );
}
