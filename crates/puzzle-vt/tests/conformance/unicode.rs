//! Unicode width cases from design §6.7 and Unicode UAX #11/#29.

use puzzle_vt::{Terminal, cell::CellFlags};

/// Mode 2027 keeps a ZWJ sequence in one two-cell grapheme by default.
#[test]
fn mode_2027_joins_zwj_emoji() {
    let mut plain = Terminal::new(12, 2, 0, 0);
    plain.advance(b"\x1b[?2027l");
    plain.advance("👩\u{200d}💻".as_bytes());
    assert_eq!(plain.cursor.column, 4);
    let mut clustered = Terminal::new(12, 2, 0, 0);
    clustered.advance("👩\u{200d}💻".as_bytes());
    assert_eq!(clustered.cursor.column, 2);
    assert!(
        clustered.screen().row(0).expect("row").cells[0]
            .flags
            .contains(CellFlags::WIDE)
    );
}

/// Measured reference cursor reports pin the default clustered cell counts.
///
/// Source: reference terminal cursor-position reports (CPR), 2026-09-30.
/// The legacy column below is the sum of per-scalar `unicode-width` widths.
#[test]
fn grapheme_width_parity_table() {
    let cases = [
        ("❤️", 2),
        ("x\u{fe0f}\u{fe0f}", 1),
        ("⌚\u{fe0e}\u{fe0f}", 2),
        ("🏴\u{200d}☠️", 2),
        ("👨\u{200d}👩\u{200d}👧", 2),
        ("#️⃣", 2),
        ("1\u{20e3}", 1),
        ("👋🏿", 2),
        ("🇦🇧🇨", 4),
        ("ab", 2),
        ("\u{0301}\u{0302}", 0),
        ("✌️", 2),
        ("✌", 1),
    ];
    for (text, clustered) in cases {
        // Legacy mode deliberately counts scalar widths rather than using
        // The reference terminal's grapheme result as an unverified source for that column.
        let legacy: usize = text
            .chars()
            .map(|scalar| unicode_width::UnicodeWidthChar::width(scalar).unwrap_or(0))
            .sum();
        for (mode, cells) in [(true, clustered), (false, legacy)] {
            let mut term = Terminal::new(16, 1, 0, 0);
            term.modes.set(true, 2027, mode);
            term.advance(text.as_bytes());
            assert_eq!(term.cursor.column, cells, "{text:?}, mode 2027={mode}");
        }
    }
}

/// VS16 widens the prior pictograph without leaving a stale first scalar.
#[test]
fn default_grapheme_widening_replaces_spacer_and_preserves_following_text() {
    let mut term = Terminal::new(12, 1, 0, 0);
    term.advance("ab✌️cd|".as_bytes());
    let row = term.screen().row(0).expect("visible row");
    assert_eq!(row.extras[0].cluster.as_deref(), Some("✌️"));
    assert_eq!(row.cells[2].ch, puzzle_vt::Cell::CLUSTER);
    assert!(row.cells[3].flags.contains(CellFlags::WIDE_SPACER));
    assert_eq!(row.cells[4].ch, 'c' as u32);
    assert_eq!(row.cells[5].ch, 'd' as u32);
    assert_eq!(row.cells[6].ch, '|' as u32);
}

/// RIS and DECSTR restore the configured mode; DECRQM exposes its value.
#[test]
fn grapheme_default_is_restored_by_both_resets() {
    for configured in [true, false] {
        let mut term = Terminal::new(8, 2, 0, 0);
        term.set_grapheme_width_default(configured);
        term.advance(if configured {
            b"\x1b[?2027l"
        } else {
            b"\x1b[?2027h"
        });
        term.advance(b"\x1b[!p\x1b[?2027$p");
        let expected = if configured {
            b"\x1b[?2027;1$y"
        } else {
            b"\x1b[?2027;2$y"
        };
        assert_eq!(term.replies, expected);
        term.replies.clear();
        term.advance(if configured {
            b"\x1b[?2027l"
        } else {
            b"\x1b[?2027h"
        });
        term.advance(b"\x1bc\x1b[?2027$p");
        assert_eq!(term.replies, expected);
    }
}

/// The reference terminal reclaims the spacer when VS15 requests text presentation.
#[test]
fn vs15_can_narrow_a_previously_wide_base() {
    let mut term = Terminal::new(8, 1, 0, 0);
    term.advance("⌚︎X".as_bytes());
    assert_eq!(term.cursor.column, 2);
    let row = term.screen().row(0).expect("visible row");
    assert!(!row.cells[0].flags.contains(CellFlags::WIDE));
    assert_eq!(row.cells[1].ch, 'X' as u32);
}

/// A late VS16 moves a last-column base before adding its second cell.
#[test]
fn late_widening_at_right_margin_wraps_the_cluster() {
    let mut term = Terminal::new(4, 2, 0, 0);
    term.advance("abc✌️".as_bytes());
    assert_eq!((term.cursor.row, term.cursor.column), (1, 2));
    let first = term.screen().row(0).expect("first row");
    let second = term.screen().row(1).expect("second row");
    assert_eq!(first.cells[3].ch, 0);
    assert_eq!(second.extras[0].cluster.as_deref(), Some("✌️"));
    assert!(second.cells[0].flags.contains(CellFlags::WIDE));
}

/// VS16 and national flags become one visual cell pair under 2027.
#[test]
fn mode_2027_vs16_and_flag_widths() {
    let mut term = Terminal::new(12, 2, 0, 0);
    term.advance(b"\x1b[?2027h");
    term.advance("❤️🇩🇪".as_bytes());
    assert_eq!(term.cursor.column, 4);
    let row = term.screen().row(0).expect("row");
    assert!(row.cells[0].flags.contains(CellFlags::WIDE));
    assert!(row.cells[2].flags.contains(CellFlags::WIDE));
}

/// Ambiguous East Asian Width uses a configurable two-cell presentation.
#[test]
fn ambiguous_width_setting_changes_cell_count() {
    let mut term = Terminal::new(8, 1, 0, 0);
    term.advance("·".as_bytes());
    assert_eq!(term.cursor.column, 1);
    term.ambiguous_width_two = true;
    term.advance("·".as_bytes());
    assert_eq!(term.cursor.column, 3);
}
