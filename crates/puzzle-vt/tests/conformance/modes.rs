//! DECRQM wire replies from https://invisible-island.net/xterm/ctlseqs/ctlseqs.html.

use puzzle_vt::Terminal;

/// Every listed mode answers 1/2, while an unknown mode answers zero.
#[test]
fn decrqm_reports_supported_and_unknown_modes() {
    let mut term = Terminal::new(8, 2, 0, 0);
    term.advance(b"\x1b[?7$p\x1b[?2027$p\x1b[?9999$p\x1b[4$p");
    assert_eq!(
        term.replies,
        b"\x1b[?7;1$y\x1b[?2027;1$y\x1b[?9999;0$y\x1b[4;2$y"
    );
}

/// DECSET and DECRST are reflected in subsequent DECRQM answers.
#[test]
fn mode_query_tracks_changes() {
    let mut term = Terminal::new(8, 2, 0, 0);
    term.advance(b"\x1b[?6h\x1b[?6$p\x1b[?6l\x1b[?6$p");
    assert_eq!(term.replies, b"\x1b[?6;1$y\x1b[?6;2$y");
}
