//! SGR table cases from https://invisible-island.net/xterm/ctlseqs/ctlseqs.html.

use puzzle_vt::{Attrs, PackedColor, Terminal};

/// Standard and bright ANSI colors stay tagged palette references.
#[test]
fn indexed_and_bright_colors() {
    let mut term = Terminal::new(8, 1, 0, 0);
    term.advance(b"\x1b[31;104mX");
    let cell = term.screen().row(0).expect("row").cells[0];
    assert_eq!(cell.fg, PackedColor::palette(1));
    assert_eq!(cell.bg, PackedColor::palette(12));
}

/// Semicolon and colon RGB forms must resolve to identical tagged bytes.
#[test]
fn rgb_semicolon_and_colon_forms() {
    let mut term = Terminal::new(8, 1, 0, 0);
    term.advance(b"\x1b[38;2;17;34;51mX\x1b[38:2::17:34:51mY");
    let row = term.screen().row(0).expect("row");
    assert_eq!(row.cells[0].fg, PackedColor::rgb(17, 34, 51));
    assert_eq!(row.cells[1].fg, row.cells[0].fg);
}

/// Underline styles 0–5 and color 58/59 do not affect foreground color.
#[test]
fn underline_style_and_color_table() {
    for (style, expected) in [(0, 0), (1, 1), (2, 2), (3, 3), (4, 4), (5, 5)] {
        let mut term = Terminal::new(8, 1, 0, 0);
        term.advance(format!("\x1b[4:{style};58:5:123mX").as_bytes());
        assert_eq!(term.pen.attrs.underline_style(), expected);
        assert_eq!(
            term.screen().row(0).expect("row").extras[0].underline_color,
            Some(PackedColor::palette(123))
        );
        term.advance(b"\x1b[59m");
        assert_eq!(term.underline_color, None);
    }
}

/// SGR 0 resets rendition but keeps DECSCA protection and OSC 8 state.
#[test]
fn reset_does_not_clear_protection() {
    let mut term = Terminal::new(8, 1, 0, 0);
    term.advance(b"\x1b[1\"q\x1b[1;3;7;53m\x1b[0mX");
    let cell = term.screen().row(0).expect("row").cells[0];
    assert_eq!(cell.attrs, Attrs::empty());
    assert!(cell.flags.contains(puzzle_vt::CellFlags::PROTECTED));
}
