//! Primary height changes preserve the cursor instead of manufacturing prompt history.
use super::*;
use crate::row::RowFlags;

/// Keep hard/soft endings in the assertion, not merely the visible text.
fn rows(ring: &crate::ring::RowRing) -> Vec<(String, bool)> {
    ring.iter()
        .map(|row| {
            (
                row.cells
                    .iter()
                    .map(|c| char::from_u32(c.ch).unwrap_or(' '))
                    .collect::<String>()
                    .trim_end_matches('\0')
                    .to_owned(),
                row.flags.contains(RowFlags::WRAPPED),
            )
        })
        .collect()
}

/// The reported fresh two-line prompt must never enter history on a height-only resize.
#[test]
fn fresh_two_line_prompt_survives_vertical_resize_without_history() {
    let mut term = Terminal::new(40, 12, 100, 1 << 20);
    term.advance(b"directory\r\n> ");
    for _ in 0..10 {
        term.resize(40, 4);
        assert_eq!(term.primary.ring.scrollback_len(), 0);
        assert_eq!((term.cursor.row, term.cursor.column), (1, 2));
        term.resize(40, 12);
        assert_eq!(term.primary.ring.scrollback_len(), 0);
        assert_eq!((term.cursor.row, term.cursor.column), (1, 2));
    }
}

/// Cursor placement, not retained history length or width, determines the shrink.
#[test]
fn primary_height_sequences_preserve_rows_history_and_cursor() {
    for history_limit in [0, 100] {
        for history in [false, true] {
            for cursor in [0, 2, 5] {
                for wrapped in [false, true] {
                    let mut term = Terminal::new(8, 6, history_limit, 1 << 20);
                    if history {
                        term.advance(b"H0\r\nH1\r\nH2\x1b[6;1H\n\n\n");
                    }
                    for row in 0..=cursor {
                        term.advance(format!("\x1b[{};1HR{row}", row + 1).as_bytes());
                    }
                    if wrapped && cursor > 0 {
                        // Two physical rows of one logical line must keep their soft ending.
                        term.advance(format!("\x1b[{cursor};1Habcdefghijk").as_bytes());
                    }
                    term.advance(format!("\x1b[{};4H", cursor + 1).as_bytes());
                    let before = rows(&term.primary.ring);
                    let old_history = term.primary.ring.scrollback_len();
                    let cursor_id = term.cursor_line();
                    let shifted = (cursor + 1usize).saturating_sub(3);
                    let discarded = 3 - shifted;
                    let mut expected = before[..before.len() - discarded].to_vec();
                    let mut shrunk_history = old_history + shifted;
                    if history_limit == 0 {
                        expected.drain(..shrunk_history);
                        shrunk_history = 0;
                    }
                    term.resize(8, 3);
                    assert_eq!(rows(&term.primary.ring), expected);
                    assert_eq!(term.primary.ring.scrollback_len(), shrunk_history);
                    assert_eq!((term.cursor.row, term.cursor.column), (cursor - shifted, 3));
                    assert_eq!(term.cursor_line(), cursor_id);
                    let pulled = shrunk_history.min(3);
                    expected.extend(std::iter::repeat_n((String::new(), false), 3 - pulled));
                    term.resize(8, 6);
                    assert_eq!(rows(&term.primary.ring), expected);
                    assert_eq!(term.primary.ring.scrollback_len(), shrunk_history - pulled);
                    assert_eq!(
                        (term.cursor.row, term.cursor.column),
                        (cursor - shifted + pulled, 3)
                    );
                    assert_eq!(term.cursor_line(), cursor_id);
                }
            }
        }
    }
}

/// xterm truncates even written content below a cursor that already fits.
#[test]
fn primary_shrink_does_not_scroll_fitting_cursor_for_content_below_it() {
    let mut term = Terminal::new(8, 6, 100, 1 << 20);
    term.advance(b"A\r\nB\r\nC\r\nD\r\nE\r\nF\x1b[2;2H");
    term.resize(8, 3);
    assert_eq!(term.primary.ring.scrollback_len(), 0);
    assert_eq!((term.cursor.row, term.cursor.column), (1, 1));
    assert_eq!(
        rows(&term.primary.ring)
            .iter()
            .map(|row| row.0.as_str())
            .collect::<Vec<_>>(),
        ["A", "B", "C"]
    );
}

/// Width reflow maps the logical cursor before discarding unused bottom space.
#[test]
fn primary_width_and_height_resize_preserves_wrapped_prompt() {
    let mut term = Terminal::new(8, 10, 100, 1 << 20);
    term.advance(b"abcdefghijk\r\n> ");
    term.resize(4, 5);
    assert_eq!(term.primary.ring.scrollback_len(), 0);
    assert_eq!((term.cursor.row, term.cursor.column), (3, 2));
    let narrow = rows(&term.primary.ring);
    assert_eq!(
        &narrow[..4],
        &[
            ("abcd".into(), true),
            ("efgh".into(), true),
            ("ijk".into(), false),
            ("> ".into(), false)
        ]
    );
    term.resize(8, 10);
    assert_eq!(term.primary.ring.scrollback_len(), 0);
    assert_eq!((term.cursor.row, term.cursor.column), (2, 2));
    assert_eq!(
        &rows(&term.primary.ring)[..3],
        &[
            ("abcdefgh".into(), true),
            ("ijk".into(), false),
            ("> ".into(), false)
        ]
    );
}

/// An alternate application's discarded footer must not reappear on growth.
#[test]
fn alternate_resize_still_drops_bottom_rows_and_restores_hidden_primary_cursor() {
    let mut term = Terminal::new(8, 6, 100, 1 << 20);
    term.advance(b"shell\r\n> \x1b[?1049hA\r\nB\r\nC\r\nD\r\nE\r\nF");
    term.resize(8, 3);
    assert_eq!(
        rows(&term.alternate.ring)
            .iter()
            .map(|row| row.0.as_str())
            .collect::<Vec<_>>(),
        ["A", "B", "C"]
    );
    assert_eq!(term.primary.ring.scrollback_len(), 0);
    term.resize(8, 6);
    term.advance(b"\x1b[?1049l");
    assert_eq!((term.cursor.row, term.cursor.column), (1, 2));
    assert_eq!(
        &rows(&term.primary.ring)[..2],
        &[("shell".into(), false), ("> ".into(), false)]
    );
}
