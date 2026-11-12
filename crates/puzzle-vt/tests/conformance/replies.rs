//! Exact report bytes from puzzle design §6.12 and xterm ctlseqs.

use puzzle_vt::Terminal;

/// Device identity, DSR and cursor reports have stable byte formats.
#[test]
fn identity_and_cursor_reports() {
    let mut term = Terminal::new(10, 4, 0, 0);
    term.advance(b"\x1b[c\x1b[=c\x1b[5n\x1b[2;3H\x1b[6n\x1b[?6n");
    assert_eq!(
        term.replies,
        b"\x1b[?62;22c\x1bP!|00000000\x1b\\\x1b[0n\x1b[2;3R\x1b[?2;3;1R"
    );
}

/// Size reports expose cell and text dimensions, never the title.
#[test]
fn window_and_appearance_reports_exclude_title() {
    let mut term = Terminal::new(10, 4, 0, 0);
    term.advance(b"\x1b[14t\x1b[16t\x1b[18t\x1b[21t\x1b[?996n");
    assert_eq!(
        term.replies,
        b"\x1b[4;64;80t\x1b[6;16;8t\x1b[8;4;10t\x1b[?997;1n"
    );
}
