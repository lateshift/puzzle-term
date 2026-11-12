//! Compact screen snapshots and hostile-byte parser checks for M5.2.
use super::*;
use crate::test_grid::check as check_grid;
use crate::{cell::CellFlags, row::RowFlags};
use proptest::prelude::*;

/// Displays one small grid with cursor and soft-wrap metadata for review.
fn dump(term: &Terminal) -> String {
    check_grid(term);
    let (columns, rows) = term.size();
    let mut out = format!(
        "cursor=({},{}), pending={}\n",
        term.cursor.row, term.cursor.column, term.cursor.pending_wrap
    );
    for row_index in 0..rows {
        let Some(row) = term.screen().row(row_index) else {
            continue;
        };
        for cell in row.cells.iter().take(columns) {
            let ch = if cell.flags.contains(CellFlags::WIDE_SPACER) {
                '~'
            } else if cell.ch == 0 {
                '·'
            } else {
                char::from_u32(cell.ch).unwrap_or('�')
            };
            out.push(ch);
        }
        if row.flags.contains(RowFlags::WRAPPED) {
            out.push('↪');
        }
        out.push('\n');
    }
    out
}

/// xterm ctlseqs: CR, LF and BS cancel DECAWM's delayed wrap.
#[test]
fn pending_wrap_after_controls_and_printable() {
    let mut snapshots = String::new();
    for (label, suffix) in [
        ("CR", b"\rX".as_slice()),
        ("LF", b"\nX"),
        ("BS", b"\x08X"),
        ("printable", b"X"),
    ] {
        let mut term = Terminal::new(4, 2, 0, 0);
        term.advance(b"ABCD");
        term.advance(suffix);
        snapshots.push_str(label);
        snapshots.push('\n');
        snapshots.push_str(&dump(&term));
    }
    insta::assert_snapshot!("pending_wrap_controls", snapshots);
}

/// xterm ctlseqs: a wide glyph at the edge wraps before drawing or drops.
#[test]
fn wide_at_right_margin_with_autowrap_on_and_off() {
    let mut on = Terminal::new(4, 2, 0, 0);
    on.advance("ABC界".as_bytes());
    let mut off = Terminal::new(4, 2, 0, 0);
    off.advance(b"\x1b[?7lABC");
    off.advance("界".as_bytes());
    insta::assert_snapshot!(
        "wide_at_margin",
        format!("on\n{}off\n{}", dump(&on), dump(&off))
    );
}

/// Replacing either half must not leave a dangling wide spacer (design §6.2).
#[test]
fn overwriting_both_halves_of_wide_character() {
    let mut first = Terminal::new(4, 1, 0, 0);
    first.advance("界\rA".as_bytes());
    let mut second = Terminal::new(4, 1, 0, 0);
    second.advance("界\x08A".as_bytes());
    insta::assert_snapshot!(
        "wide_overwrite",
        format!("first\n{}second\n{}", dump(&first), dump(&second))
    );
}

/// xterm ctlseqs: a short top region keeps history and a fixed footer.
#[test]
fn top_scroll_region_with_scrollback_snapshot() {
    let mut term = Terminal::new(6, 4, 4, 6 * 16 * 4);
    term.advance(b"one\r\ntwo\r\ntri\r\nend\x1b[1;3r\x1b[3;1H\n");
    insta::assert_snapshot!(
        "top_region_history",
        format!(
            "history={}\n{}",
            term.primary.ring.scrollback_len(),
            dump(&term)
        )
    );
}

/// A tmux status row outside DECSTBM must retain its frame cache on SU/SD.
#[test]
fn region_scroll_damage_excludes_footer() {
    for sequence in [b"\x1b[S".as_slice(), b"\x1b[T"] {
        let mut term = Terminal::new(8, 4, 8, 8 * 16 * 8);
        term.advance(b"one\r\ntwo\r\ntri\r\nstatus\x1b[1;3r");
        term.screen_mut().damage.clear();
        term.advance(sequence);
        let damage = &term.screen().damage;
        assert!(!damage.full, "partial scroll must not flush every cache");
        assert!((0..3).all(|row| damage.is_dirty(row)));
        assert!(!damage.is_dirty(3), "the status row did not move");
        assert_eq!(dump(&term).lines().nth(4), Some("status··"));
    }
}

/// DECLRMM copies only its rectangle and dirties only its vertical span.
#[test]
fn rectangular_scroll_damage_excludes_outer_rows() {
    let mut term = Terminal::new(8, 4, 0, 0);
    term.advance(b"top\r\none\r\ntwo\r\nfoot\x1b[?69h\x1b[2;3r\x1b[2;5s");
    term.screen_mut().damage.clear();
    term.advance(b"\x1b[S");
    let damage = &term.screen().damage;
    assert!(!damage.full);
    assert!(!damage.is_dirty(0));
    assert!(damage.is_dirty(1) && damage.is_dirty(2));
    assert!(!damage.is_dirty(3));
}

/// xterm ctlseqs: DECOM makes CUP one-based within DECSTBM margins.
#[test]
fn decom_relative_cup_snapshot() {
    let mut term = Terminal::new(6, 4, 0, 0);
    term.advance(b"\x1b[2;3r\x1b[?6h\x1b[1;2H@");
    insta::assert_snapshot!("decom_cup", dump(&term));
}

/// xterm ctlseqs' DEC SCS table maps line-drawing GL characters.
#[test]
fn dec_special_graphics_maps_lqkx() {
    let mut term = Terminal::new(8, 1, 0, 0);
    term.advance(b"\x1b(0lqkx");
    assert_eq!(dump(&term).lines().nth(1), Some("┌─┐│····"));
    // G1 and one-character G2 shifts use the same designated table.
    let mut shifted = Terminal::new(8, 1, 0, 0);
    shifted.advance(b"\x1b)0\x0elq\x0fA\x1b*0\x1bNqB");
    assert_eq!(dump(&shifted).lines().nth(1), Some("┌─A─B···"));
}

/// Combining marks occupy the previous cell, with a 32-byte cluster cap.
#[test]
fn zero_width_joins_previous_cell_with_bounded_cluster() {
    let mut term = Terminal::new(4, 1, 0, 0);
    term.advance("A\u{0301}".as_bytes());
    let row = term.screen().row(0);
    assert_eq!(
        row.and_then(|row| row.extras.first())
            .and_then(|extra| extra.cluster.as_deref()),
        Some("A\u{0301}")
    );
    term.advance("\u{0301}".repeat(32).as_bytes());
    assert!(term.screen().row(0).is_some_and(|row| {
        row.extras[0]
            .cluster
            .as_ref()
            .is_some_and(|text| text.len() <= 32)
    }));
    let mut wide = Terminal::new(4, 1, 0, 0);
    wide.advance("界\u{0301}".as_bytes());
    assert_eq!(
        wide.screen()
            .row(0)
            .and_then(|row| row.extras.first())
            .and_then(|extra| extra.cluster.as_deref()),
        Some("界\u{0301}")
    );
}

/// xterm ctlseqs: BEL emits a fact, and LNM adds CR to LF/VT/FF.
#[test]
fn c0_bell_and_newline_mode() {
    let mut term = Terminal::new(8, 2, 0, 0);
    term.advance(b"A\x07\x1b[20h\nB");
    assert_eq!(
        term.events,
        vec![TerminalEvent::Bell {
            line: Some(crate::tracked::LineId(0))
        }]
    );
    assert_eq!(
        term.cursor,
        Cursor {
            row: 1,
            column: 1,
            pending_wrap: false
        }
    );
    assert_eq!(dump(&term).lines().nth(2), Some("B·······"));
}

/// Each M5.2 C0 action follows the xterm ctlseqs table in design §6.5.
#[test]
fn c0_control_table() {
    let mut term = Terminal::new(16, 4, 0, 0);
    term.advance(b"A\x05\x07\tB\rC\x08D\x0bE\x0cF");
    assert_eq!(
        term.events,
        vec![TerminalEvent::Bell {
            line: Some(crate::tracked::LineId(0))
        }]
    );
    // ENQ adds no reply; HT reaches zero-based column 8, and CR returns.
    assert_eq!(
        term.screen().row(0).map(|row| row.cells[8].ch),
        Some('B' as u32)
    );
    // VT and FF each index once while retaining the column with LNM off.
    assert_eq!(term.cursor.row, 2);
    assert_eq!(term.cursor.column, 3);
    assert_eq!(
        term.screen().row(2).map(|row| row.cells[2].ch),
        Some('F' as u32)
    );
}

/// Warm ASCII printing, LF and full-width scrolling have no heap requests.
#[test]
fn steady_print_line_feed_and_scroll_allocate_nothing() {
    let mut term = Terminal::new(80, 4, 2, 80 * 16 * 2);
    for _ in 0..8 {
        term.advance(b"ASCII\n");
    }
    let measured = allocation_counter::measure(|| {
        for _ in 0..32 {
            term.advance(b"ASCII\n");
        }
    });
    assert_eq!(measured.count_total, 0);
}

proptest! {
    /// Any bytes, including interrupted UTF-8/OSC/CSI, preserve cursor bounds.
    #[test]
    fn arbitrary_bytes_never_panic_and_cursor_stays_bounded(bytes in proptest::collection::vec(any::<u8>(), 0..4096)) {
        let mut term = Terminal::new(13, 7, 12, 13 * 16 * 12);
        // Seed the extended parser path before arbitrary interruptions and edits.
        term.advance(b"\x1b]66;s=2:w=2:n=1:d=2:v=2:h=2;AB\x07");
        for chunk in bytes.chunks(31) {
            term.advance(chunk);
            check_grid(&term);
            let (columns, rows) = term.size();
            prop_assert!(term.cursor.column < columns);
            prop_assert!(term.cursor.row < rows);
            #[cfg(debug_assertions)]
            prop_assert!(term.screen().ring.check_multicells().is_ok());
        }
    }
}

/// Independent Unicode scalars must not allocate a speculative grapheme string (P1).
#[test]
fn separate_unicode_scalars_do_not_allocate_grapheme_candidates() {
    let mut terminal = Terminal::new(282, 63, 0, 0);
    // Mixed CJK, Hangul and symbols exercise boundaries without joining clusters.
    let bytes = "漢字한글𝄞ΩЖ".repeat(500);
    terminal.advance(bytes.as_bytes());
    let measured = allocation_counter::measure(|| terminal.advance(bytes.as_bytes()));
    assert_eq!(measured.count_total, 0);
    assert_eq!(measured.bytes_total, 0);
}

/// Reset and clear-history retire text without reallocating warmed cluster stores.
#[test]
fn repeated_reset_and_clear_reuse_clustered_rows() {
    let mut term = Terminal::new(120, 40, 100, 32 * 1024 * 1024);
    let output = "e\u{301}\r\n".repeat(500);
    for _ in 0..3 {
        term.advance(b"\x1bc");
        term.advance(output.as_bytes());
    }
    let allocations = allocation_counter::measure(|| {
        for clear in [b"\x1bc".as_slice(), b"\x1b[H\x1b[2J\x1b[3J"] {
            term.advance(clear);
            assert_eq!(term.primary.ring.scrollback_len(), 0);
            term.advance(output.as_bytes());
        }
    });
    assert_eq!(allocations.count_total, 0);
    assert!(term.primary.ring.allocated_history_bytes() <= 32 * 1024 * 1024);
}

/// Feature-free printing has no hidden sparse text work.
#[path = "tests/text_sizing_work.rs"]
mod text_sizing_work;

/// Deterministic no-watch operation counts and reply-callback admission boundaries.
#[path = "tests/unused_path.rs"]
mod unused_path;
