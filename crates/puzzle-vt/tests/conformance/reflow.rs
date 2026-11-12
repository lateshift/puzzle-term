//! Resize and logical-line preservation from design §6.6.

use proptest::prelude::*;
use puzzle_vt::{Terminal, cell::PackedColor, row::RowFlags, tracked::PositionState};

/// Extracts occupied ASCII cells while joining rows marked WRAPPED.
fn logical_text(term: &Terminal) -> Vec<String> {
    super::check_grid(term);
    let mut lines = Vec::new();
    let mut current = String::new();
    for row in term.primary.ring.iter() {
        for cell in &row.cells {
            if cell.ch != 0 && cell.ch != 0xffff_ffff {
                current.push(char::from_u32(cell.ch).unwrap_or('\u{fffd}'));
            }
        }
        if !row.flags.contains(RowFlags::WRAPPED) && !current.is_empty() {
            lines.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        lines.push(current);
    }
    lines
}

/// Dumps a small retained grid with wrap flags for reviewer inspection.
fn grid_dump(term: &Terminal) -> String {
    super::check_grid(term);
    let mut dump = String::new();
    for row in term.primary.ring.iter() {
        let text: String = row
            .cells
            .iter()
            .map(|cell| {
                if cell.ch == 0 {
                    '·'
                } else {
                    char::from_u32(cell.ch).unwrap_or('·')
                }
            })
            .collect();
        dump.push_str(&format!(
            "{} |{text}|\n",
            if row.flags.contains(RowFlags::WRAPPED) {
                'W'
            } else {
                '-'
            }
        ));
    }
    dump
}

/// A compact before/after specimen exposes wide pairing and soft wraps.
#[test]
fn small_reflow_snapshots() {
    let mut term = Terminal::new(8, 3, 10, 10_000);
    term.advance("abcd界gh\r\n12345678".as_bytes());
    insta::assert_snapshot!("before_reflow", grid_dump(&term));
    term.resize(5, 3);
    // The extra wrapped row uses the unused bottom blank before adding history:
    // four content rows leave one historical row and the cursor on the last row.
    assert_eq!(term.primary.ring.scrollback_len(), 1);
    assert_eq!((term.cursor.row, term.cursor.column), (2, 2));
    insta::assert_snapshot!("after_reflow", grid_dump(&term));
}

proptest! {
    /// Narrowing then widening must keep each logical line's text.
    #[test]
    fn narrowing_and_widening_preserve_text(text in "[A-Za-z0-9 ]{1,80}") {
        let mut term = Terminal::new(20, 5, 100, 100_000);
        term.advance(text.as_bytes());
        let before = logical_text(&term);
        term.resize(9, 5);
        super::check_grid(&term);
        term.resize(20, 5);
        super::check_grid(&term);
        prop_assert_eq!(logical_text(&term), before);
    }
}

/// Cursor and a tracked marker retain their logical cell after rewrap.
#[test]
fn cursor_and_marker_remap_to_reflowed_text() {
    let mut term = Terminal::new(8, 3, 10, 10_000);
    term.advance(b"abcdefghi");
    let line = term.cursor_line().expect("cursor line");
    let marker = term.tracked.track(line, term.cursor.column);
    term.resize(4, 3);
    assert_eq!(
        term.tracked.get(marker),
        PositionState::Present {
            line: term.cursor_line().expect("new cursor line"),
            column: term.cursor.column,
        }
    );
}

/// Background-colored erased cells are content when reflow trims tails.
#[test]
fn background_cells_survive_reflow() {
    let mut term = Terminal::new(8, 2, 10, 10_000);
    term.advance(b"AB\x1b[41m\x1b[4X");
    term.resize(4, 2);
    let colored = term
        .primary
        .ring
        .iter()
        .flat_map(|row| row.cells.iter())
        .filter(|cell| cell.bg != PackedColor::DEFAULT)
        .count();
    assert_eq!(colored, 4);
}

/// A two-cell character never leaves its spacer at a resized row edge.
#[test]
fn wide_character_remains_whole_after_reflow() {
    let mut term = Terminal::new(8, 2, 10, 10_000);
    term.advance("abc界def".as_bytes());
    term.resize(4, 2);
    for row in term.primary.ring.iter() {
        let last = row.cells.last().expect("nonempty row");
        assert!(!last.flags.contains(puzzle_vt::cell::CellFlags::WIDE));
    }
}

/// Alternate-screen rows are clipped or padded without soft rewrap.
#[test]
fn alternate_screen_does_not_reflow() {
    let mut term = Terminal::new(8, 2, 10, 10_000);
    term.advance(b"\x1b[?1049hABCDEFG");
    term.resize(4, 2);
    assert_eq!(term.alternate.row(0).expect("row").cells.len(), 4);
    assert_eq!(
        term.alternate.row(0).expect("row").cells[3].ch,
        u32::from(b'D')
    );
}

/// Resize clears stale margins, extends tabs and reports mode 2048 geometry.
#[test]
fn resize_resets_geometry_and_reports_mode_2048() {
    let mut term = Terminal::new(8, 2, 10, 10_000);
    term.advance(b"\x1b[?2048h\x1b[2;2r");
    term.resize(16, 3);
    assert_eq!(term.margins.top, 0);
    assert_eq!(term.margins.bottom, 2);
    assert!(term.tabs[8]);
    assert_eq!(term.replies, b"\x1b[48;3;16;48;128t");
}
