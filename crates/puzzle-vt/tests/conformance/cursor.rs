//! Cursor addressing cases from https://invisible-island.net/xterm/ctlseqs/ctlseqs.html.

use puzzle_vt::Terminal;

/// CUP uses one-based coordinates and DECOM makes them region-relative.
#[test]
fn decom_cup_is_relative_to_scroll_region() {
    let mut term = Terminal::new(12, 8, 0, 0);
    term.advance(b"\x1b[3;6r\x1b[?6h\x1b[1;2H@");
    assert_eq!((term.cursor.row, term.cursor.column), (2, 2));
    assert_eq!(super::row(&term, 2).as_bytes()[1], b'@');
    term.advance(b"\x1b[?6l\x1b[1;2H#");
    assert_eq!(super::row(&term, 0).as_bytes()[1], b'#');
}

/// CUU/CUD/CUF/CUB clamp to the grid instead of wrapping around it.
#[test]
fn relative_movement_clamps_at_edges() {
    let mut term = Terminal::new(10, 4, 0, 0);
    term.advance(b"\x1b[999A\x1b[999D");
    assert_eq!((term.cursor.row, term.cursor.column), (0, 0));
    term.advance(b"\x1b[999B\x1b[999C");
    assert_eq!((term.cursor.row, term.cursor.column), (3, 9));
}

/// HTS/TBC mutate the stops used by HT, CHT and CBT.
#[test]
fn tab_stop_controls_share_one_table() {
    let mut term = Terminal::new(20, 2, 0, 0);
    term.advance(b"\x1b[3g\x1b[1;5H\x1bH\r\t");
    assert_eq!(term.cursor.column, 4);
    term.advance(b"\x1b[1;10H\x1b[1Z");
    assert_eq!(term.cursor.column, 4);
    term.advance(b"\x1b[0g\r\t");
    assert_eq!(term.cursor.column, 19);
}

/// DECSLRM is active only with mode 69; otherwise CSI s saves the cursor.
#[test]
fn horizontal_margins_require_declrmm() {
    let mut term = Terminal::new(12, 3, 0, 0);
    term.advance(b"\x1b[3;8s");
    assert_eq!((term.margins.left, term.margins.right), (0, 11));
    term.advance(b"\x1b[?69h\x1b[3;8s\x1b[?6h\x1b[1;1H@");
    assert_eq!(
        term.screen().row(0).expect("row").cells[2].ch,
        u32::from(b'@')
    );
}
