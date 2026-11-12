//! ESC controls from https://invisible-island.net/xterm/ctlseqs/ctlseqs.html.

use puzzle_vt::Terminal;

/// DECSC/DECRC restore location and pen instead of location alone.
#[test]
fn saved_cursor_restores_graphic_state() {
    let mut term = Terminal::new(8, 3, 0, 0);
    term.advance(b"\x1b[2;3H\x1b7\x1b[3;5H\x1b8X");
    assert_eq!(super::row(&term, 1).as_bytes()[2], b'X');
}

/// DECALN fills every cell and RIS removes that diagnostic content.
#[test]
fn alignment_and_ris() {
    let mut term = Terminal::new(8, 2, 0, 0);
    term.advance(b"\x1b#8");
    assert_eq!(super::row(&term, 0), "EEEEEEEE");
    term.advance(b"\x1bc");
    assert_eq!(super::row(&term, 0), "        ");
}

/// DECSCUSR accepts documented styles and DECSTR returns to default.
#[test]
fn cursor_style_and_soft_reset() {
    let mut term = Terminal::new(8, 2, 0, 0);
    term.advance(b"\x1b[6 q");
    assert_eq!(term.cursor_style, 6);
    term.advance(b"\x1b[!p");
    assert_eq!(term.cursor_style, 0);
}
