//! CSI erase/edit cases from https://invisible-island.net/xterm/ctlseqs/ctlseqs.html.

use puzzle_vt::{PackedColor, Terminal};

/// Every edit boundary validates both screens before a protocol assertion.
fn feed(term: &mut Terminal, bytes: &[u8]) {
    term.advance(bytes);
    super::check_grid(term);
}

/// EL and ECH use the current SGR background (xterm-256color `bce`).
#[test]
fn erased_cells_keep_current_background() {
    let mut term = Terminal::new(8, 2, 0, 0);
    term.pen.bg = PackedColor::rgb(17, 34, 51);
    feed(&mut term, b"abcdef\x1b[1;3H\x1b[0K");
    let row = term.screen().row(0).expect("row");
    assert_eq!(row.cells[0].ch, u32::from(b'a'));
    assert_eq!(row.cells[2].ch, 0);
    assert_eq!(row.cells[2].bg, PackedColor::rgb(17, 34, 51));
}

/// DECSED skips cells printed under DECSCA protection.
#[test]
fn selective_erase_keeps_protected_cells() {
    let mut term = Terminal::new(8, 2, 0, 0);
    feed(&mut term, b"A\x1b[1\"qB\x1b[0\"qC\x1b[?2J");
    assert_eq!(super::row(&term, 0).chars().nth(1), Some('B'));
    assert_eq!(term.screen().row(0).expect("row").cells[0].ch, 0);
}

/// REP is bounded to the grid even when the CSI count is maximal.
#[test]
fn repeat_cannot_run_beyond_grid_budget() {
    let mut term = Terminal::new(8, 2, 0, 0);
    feed(&mut term, b"X\x1b[65535b");
    assert!(term.cursor.row < 2 && term.cursor.column < 8);
}

/// ICH/DCH shift text only as far as the right margin.
#[test]
fn insert_and_delete_characters() {
    let mut term = Terminal::new(8, 2, 0, 0);
    feed(&mut term, b"abcdef\x1b[1;3H\x1b[2@");
    assert_eq!(super::row(&term, 0), "ab  cdef");
    feed(&mut term, b"\x1b[1;3H\x1b[2P");
    assert_eq!(super::row(&term, 0), "abcdef  ");
}

/// ED 3 removes history without clearing the current visible grid.
#[test]
fn ed_three_clears_only_history() {
    let mut term = Terminal::new(8, 2, 12, 4096);
    feed(&mut term, b"A\r\nB\r\nC");
    assert_eq!(term.primary.ring.scrollback_len(), 1);
    feed(&mut term, b"\x1b[3J");
    assert_eq!(term.primary.ring.scrollback_len(), 0);
    assert_eq!(super::row(&term, 1).chars().next(), Some('C'));
}

/// Erasing one half of a wide glyph also clears its partner (§6.2).
#[test]
fn ech_does_not_leave_a_wide_spacer() {
    let mut term = Terminal::new(8, 2, 0, 0);
    feed(&mut term, "界".as_bytes());
    feed(&mut term, b"\x1b[1;2H\x1b[1X");
    let row = term.screen().row(0).expect("row");
    assert_eq!(row.cells[0].ch, 0);
    assert_eq!(row.cells[1].ch, 0);
}

/// A splice through either boundary clears the wide partner outside that span (§6.2).
#[test]
fn character_splices_repair_wide_partners_outside_edit_boundaries() {
    for operation in *b"@P" {
        // Editing the spacer must also clear the lead immediately before the cursor.
        let mut term = Terminal::new(6, 1, 0, 0);
        feed(&mut term, "界\x1b[1;2H".as_bytes());
        feed(&mut term, &[0x1b, b'[', operation]);
        let row = term.screen().row(0).expect("left splice boundary");
        assert_eq!(
            row.cells[0].ch, 0,
            "left partner, CSI {}",
            operation as char
        );
        assert!(row.cells.iter().all(|cell| {
            !cell
                .flags
                .intersects(puzzle_vt::CellFlags::WIDE | puzzle_vt::CellFlags::WIDE_SPACER)
        }));

        // A right margin through a lead can strand its spacer outside the region.
        let mut term = Terminal::new(6, 1, 0, 0);
        feed(&mut term, "ab界\x1b[?69h\x1b[1;3s\x1b[1;1H".as_bytes());
        feed(&mut term, &[0x1b, b'[', operation]);
        let row = term.screen().row(0).expect("right splice boundary");
        assert_eq!(
            row.cells[3].ch, 0,
            "right partner, CSI {}",
            operation as char
        );
        assert!(row.cells.iter().all(|cell| {
            !cell
                .flags
                .intersects(puzzle_vt::CellFlags::WIDE | puzzle_vt::CellFlags::WIDE_SPACER)
        }));
    }
}
