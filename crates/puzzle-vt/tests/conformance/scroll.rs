//! Scrolling and history cases from https://invisible-island.net/xterm/ctlseqs/ctlseqs.html.

use puzzle_vt::Terminal;

/// A partial top margin must discard rows rather than adding history.
#[test]
fn partial_scroll_region_does_not_enter_scrollback() {
    let mut term = Terminal::new(8, 4, 12, 4096);
    term.advance(b"A\r\nB\r\nC\r\nD\x1b[2;3r\x1b[3;1H\n");
    assert_eq!(term.primary.ring.scrollback_len(), 0);
    assert_eq!(super::row(&term, 0).chars().next(), Some('A'));
}

/// Full-width top-margin scroll adds history only on the primary screen.
#[test]
fn top_region_scrolling_keeps_primary_history() {
    let mut term = Terminal::new(8, 3, 12, 4096);
    term.advance(b"A\r\nB\r\nC\n");
    assert_eq!(term.primary.ring.scrollback_len(), 1);
    term.advance(b"\x1b[3J");
    assert_eq!(term.primary.ring.scrollback_len(), 0);
}

/// A full-width region beginning at row zero keeps history even if shorter.
#[test]
fn short_top_region_saves_history_without_moving_footer() {
    let mut term = Terminal::new(8, 4, 12, 4096);
    term.advance(b"A\r\nB\r\nC\r\nD\x1b[1;3r\x1b[3;1H\n");
    assert_eq!(term.primary.ring.scrollback_len(), 1);
    assert_eq!(super::row(&term, 3).chars().next(), Some('D'));
    assert_eq!(super::row(&term, 0).chars().next(), Some('B'));
}

/// DECLRMM confines SU to its column rectangle and produces no history.
#[test]
fn horizontal_margins_preserve_outer_columns() {
    let mut term = Terminal::new(8, 3, 12, 4096);
    term.advance(b"abcdefgh\r\nABCDEFGH\r\n12345678\x1b[?69h\x1b[3;6s\x1b[1S");
    assert_eq!(super::row(&term, 0), "abCDEFgh");
    assert_eq!(super::row(&term, 1), "AB3456GH");
    assert_eq!(term.primary.ring.scrollback_len(), 0);
}

/// DEC 1049 returns to the previous primary content after alternate output.
#[test]
fn alternate_screen_keeps_primary_grid() {
    let mut term = Terminal::new(8, 2, 0, 0);
    term.advance(b"hello\x1b[?1049hother\x1b[?1049l");
    assert!(super::row(&term, 0).starts_with("hello"));
}

/// IL/DL affect rows at and below the cursor but not a header above it.
#[test]
fn insert_delete_lines_inside_region() {
    let mut term = Terminal::new(8, 4, 0, 0);
    term.advance(b"A\r\nB\r\nC\r\nD\x1b[2;4r\x1b[3;1H\x1b[1L");
    assert_eq!(super::row(&term, 0).chars().next(), Some('A'));
    assert_eq!(super::row(&term, 2).chars().next(), Some(' '));
    assert_eq!(super::row(&term, 3).chars().next(), Some('C'));
    term.advance(b"\x1b[1M");
    assert_eq!(super::row(&term, 2).chars().next(), Some('C'));
}
