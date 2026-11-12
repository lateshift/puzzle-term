//! DCS cases from https://invisible-island.net/xterm/ctlseqs/ctlseqs.html.

use puzzle_vt::Terminal;

/// DECRQSS reports the live margin, cursor and SGR state.
#[test]
fn status_query_reports_current_state() {
    let mut term = Terminal::new(8, 4, 0, 0);
    term.advance(b"\x1b[2;3r\x1b[1;31m\x1bP$qr\x1b\\\x1bP$qm\x1b\\");
    assert_eq!(term.replies, b"\x1bP1$r2;3r\x1b\\\x1bP1$r0;1;38;5;1m\x1b\\");
}

/// XTGETTCAP uses hex for both names and values, unknown names fail.
#[test]
fn termcap_query_uses_hex_wire_format() {
    let mut term = Terminal::new(8, 2, 0, 0);
    term.advance(b"\x1bP+q436f\x1b\\\x1bP+qFFFF\x1b\\");
    assert_eq!(term.replies, b"\x1bP1+r436F=323536\x1b\\\x1bP0+r\x1b\\");
}

/// XTGETTCAP reflects the advertised TERM and the bundled extension list.
#[test]
fn termcap_query_uses_bundled_puzzle_capabilities() {
    let mut term = Terminal::new(8, 2, 0, 0);
    // xterm ctlseqs: DCS + q carries hex-encoded terminfo names.
    term.advance(b"\x1bP+q544e;4245;53796e63\x1b\\");
    assert_eq!(
        term.replies,
        b"\x1bP1+r544E=787465726D2D70757A7A6C65;4245=1B5B3F3230303468;53796E63=1B5B3F32303236253F257031257B317D252D25746C256568253B\x1b\\"
    );
    term.replies.clear();
    term.term_name = "xterm-256color";
    term.advance(b"\x1bP+q544e\x1b\\");
    assert_eq!(
        term.replies,
        b"\x1bP1+r544E=787465726D2D323536636F6C6F72\x1b\\"
    );
}
