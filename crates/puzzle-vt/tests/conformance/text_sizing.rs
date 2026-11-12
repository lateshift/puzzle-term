//! OSC 66 conformance follows kitty's text-sizing protocol, not font metrics.
//! Each mutation checks both directions of the sparse cell/fragment relationship.
use proptest::prelude::*;
use puzzle_vt::{CellFlags, Terminal, multicell::Multicell};

/// Every test turn checks the whole retained document, including alternate state.
fn check(term: &Terminal) {
    super::check_grid(term);
    #[cfg(debug_assertions)]
    {
        term.primary
            .ring
            .check_multicells()
            .expect("text-sizing invariant");
        term.alternate
            .ring
            .check_multicells()
            .expect("text-sizing invariant");
    }
    assert_eq!(size_of::<puzzle_vt::Cell>(), 16);
}
/// A complete operation is the test's invariant boundary.
fn feed(term: &mut Terminal, bytes: &[u8]) {
    term.advance(bytes);
    check(term);
}
/// The addressed physical cell must resolve through the sparse store.
fn block(term: &Terminal, y: usize, x: usize) -> &Multicell {
    let cell = term.screen().row(y).expect("text-sizing invariant").cells[x];
    assert!(cell.flags.contains(CellFlags::MULTICELL));
    term.screen()
        .ring
        .multicell(cell.ch)
        .expect("text-sizing invariant")
}
#[test]
fn both_terminators_every_split_and_semicolons_are_exact() {
    for end in ["\x07", "\x1b\\"] {
        let bytes = format!("\x1b]66;s=2:w=3:n=1:d=2:v=1:h=2;A;B{end}");
        for split in 0..=bytes.len() {
            let mut term = Terminal::new(12, 7, 0, 0);
            feed(&mut term, &bytes.as_bytes()[..split]);
            feed(&mut term, &bytes.as_bytes()[split..]);
            let record = block(&term, 0, 0);
            assert_eq!(record.text, "A;B");
            assert_eq!(record.columns, 6);
            assert_eq!(record.fragments.len(), 2);
            assert_eq!(
                (
                    record.sizing.numerator,
                    record.sizing.denominator,
                    record.sizing.vertical,
                    record.sizing.horizontal
                ),
                (1, 2, 1, 2)
            );
            assert_eq!((term.cursor.row, term.cursor.column), (0, 6));
        }
    }
}
#[test]
fn invalid_pairs_are_independent_and_control_text_is_discarded() {
    let mut term = Terminal::new(20, 7, 0, 0);
    feed(
        &mut term,
        b"\x1b]66;s=2:s=0:s=8:w=3:w=9:n=16:d=15:h=3:v=2:unknown=2;ok\x07",
    );
    let value = block(&term, 0, 0);
    assert_eq!(
        (
            value.sizing.scale,
            value.sizing.width,
            value.sizing.numerator,
            value.sizing.denominator,
            value.sizing.horizontal,
            value.sizing.vertical
        ),
        (2, 3, 0, 15, 0, 2)
    );
    for text in [
        b"a\nb".as_slice(),
        b"a\x00b",
        b"\xff",
        "a\u{85}b".as_bytes(),
    ] {
        let mut term = Terminal::new(20, 7, 0, 0);
        let mut bytes = b"\x1b]66;s=2;".to_vec();
        bytes.extend_from_slice(text);
        bytes.extend_from_slice(b"\x07Z");
        feed(&mut term, &bytes);
        assert!(term.screen().ring.multicells.is_none());
        assert_eq!(
            term.screen().row(0).expect("text-sizing invariant").cells[0].ch,
            u32::from('Z')
        );
    }
    let mut term = Terminal::new(20, 7, 0, 0);
    feed(
        &mut term,
        format!("\x1b]66;w=1;{}\x07Z", "x".repeat(4097)).as_bytes(),
    );
    assert!(term.screen().ring.multicells.is_none());
}
#[test]
fn explicit_width_survives_selectors_and_cpr_uses_physical_cells() {
    let mut term = Terminal::new(20, 7, 0, 0);
    feed(&mut term, "\x1b]66;s=2:w=1;❤\x07\u{fe0f}\x1b[6n".as_bytes());
    assert_eq!(block(&term, 0, 0).text, "❤️");
    assert_eq!(block(&term, 0, 0).columns, 2);
    assert_eq!(term.replies, b"\x1b[1;3R");
    feed(&mut term, b"\x1b[2;1Hx");
    assert_eq!(
        term.screen().row(1).expect("text-sizing invariant").cells[2].ch,
        u32::from('x')
    );
    feed(&mut term, b"\x1b[1;2HY");
    assert!(
        term.screen()
            .ring
            .multicells
            .as_ref()
            .expect("text-sizing invariant")
            .iter()
            .next()
            .is_none()
    );
    assert_eq!(
        term.screen().row(0).expect("text-sizing invariant").cells[1].ch,
        u32::from('Y')
    );
    assert_eq!(
        term.screen().row(1).expect("text-sizing invariant").cells[0].ch,
        0
    );
}
#[test]
fn wraps_backs_up_and_scrolls_whole_rectangles() {
    let mut term = Terminal::new(8, 5, 8, 8192);
    feed(&mut term, b"\x1b[1;8H\x1b]66;s=2;A\x07");
    assert_eq!(block(&term, 1, 0).text, "A");
    feed(&mut term, b"\x1b[?7l\x1b[4;8H\x1b]66;s=2;B\x07");
    assert_eq!(block(&term, 3, 6).text, "B");
    feed(&mut term, b"\x1b[?7h\x1b[5;1H\x1b]66;s=3;C\x07");
    assert_eq!(block(&term, 2, 0).text, "C");
    assert!(term.primary.ring.scrollback_len() >= 2);
    // Rectangles larger than the screen cannot leave a partial record.
    feed(&mut term, b"\x1b]66;s=7:w=7;oversized\x07");
    assert_eq!(block(&term, 2, 0).text, "C");
}
#[test]
fn erase_protection_and_shift_boundaries_are_atomic() {
    let mut term = Terminal::new(12, 7, 0, 0);
    feed(&mut term, b"\x1b[1\"q\x1b]66;s=2;A\x07\x1b[2;2H\x1b[?2K");
    assert_eq!(block(&term, 0, 0).text, "A");
    feed(&mut term, b"\x1b[X");
    assert!(
        term.screen()
            .ring
            .multicells
            .as_ref()
            .expect("text-sizing invariant")
            .iter()
            .next()
            .is_none()
    );
    feed(
        &mut term,
        b"\x1b[0\"q\x1b[1;3H\x1b]66;w=2;BC\x07\x1b[1;1H\x1b[2@",
    );
    assert_eq!(block(&term, 0, 4).text, "BC");
    feed(&mut term, b"\x1b[1;1H\x1b[2P");
    assert_eq!(block(&term, 0, 2).text, "BC");
    feed(&mut term, b"\x1b[1;4H\x1b[P");
    assert!(
        term.screen()
            .ring
            .multicells
            .as_ref()
            .expect("text-sizing invariant")
            .iter()
            .next()
            .is_none()
    );
}
proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]
    /// Every edit class shares the same atomicity oracle, including retained fragments.
    #[test]
    fn random_placement_editing_scroll_and_reset(ops in prop::collection::vec((0u8..21, 0u8..20, 0u8..7), 1..180)) {
        let mut term = Terminal::new(20, 7, 5, 4096);
        for (op, x, y) in ops {
            if op == 18 {
                term.resize(2+usize::from(x), 1+usize::from(y));
                check(&term);
                continue;
            }
            if op == 20 {
                let row = usize::from(y) % term.size().1;
                let line = term.screen().row(row).expect("selection row").id;
                let point = puzzle_vt::selection::SelectPoint { line, column: usize::from(x) % term.size().0 };
                if let Some(selection) = puzzle_vt::selection::Selection::new(&mut term, puzzle_vt::selection::SelectionKind::Simple, point, point) {
                    let _ = selection.text(&term, "");
                    selection.clear(&mut term);
                }
                check(&term);
                continue;
            }
            let bytes = match op {
                0..=2 => format!("\x1b]66;s={}:w={}:n=1:d=2:h={}:v={};A{}\x07", 1+y%7, x%8, y%3, x%3, if op == 1 { "\u{301}" } else { "B" }),
                3 => "abc漢\r\n".into(),
                4 => format!("\x1b[{};{}H", 1+y, 1+x),
                5..=11 => format!("\x1b[{}{}", 1+x%4, ["@", "P", "X", "K", "J", "L", "M"][usize::from(op-5)]),
                12 => "\x1b[2;6r".into(), 13 => "\x1b[r".into(),
                14 => "\x1b[2S".into(), 15 => "\x1b[2T".into(),
                16 => "\x1b[?1049h\x1b]66;s=2;alt\x07\x1b[?1049l".into(),
                _ => "\x1bc".into(),
            };
            feed(&mut term, bytes.as_bytes());
        }
    }
}

#[test]
fn wide_print_after_lower_reservation_wraps_without_splitting() {
    let mut term = Terminal::new(5, 4, 0, 0);
    feed(&mut term, b"\x1b]66;s=2:w=2;AB\x07\x1b[2;1H");
    feed(&mut term, "漢".as_bytes());
    assert_eq!(term.cursor.row, 2);
    assert_eq!(term.cursor.column, 2);
    assert_eq!(
        term.screen().row(2).expect("row").cells[0].ch,
        u32::from('漢')
    );
    assert_eq!(block(&term, 0, 0).text, "AB");
}
#[test]
fn implicit_width_tracks_d31_selectors_but_explicit_width_is_fixed() {
    let mut term = Terminal::new(12, 4, 0, 0);
    feed(&mut term, "\x1b]66;s=2;❤\x07\u{fe0f}".as_bytes());
    assert_eq!(block(&term, 0, 0).columns, 4);
    assert_eq!(term.cursor.column, 4);
    feed(&mut term, "\u{fe0e}".as_bytes());
    assert_eq!(block(&term, 0, 0).columns, 2);
    assert_eq!(term.cursor.column, 2);
    assert_eq!(term.screen().row(1).expect("row").cells[3].ch, 0);
}

#[test]
fn ordinary_late_widening_retires_every_intersected_fragment() {
    let mut term = Terminal::new(12, 4, 0, 0);
    feed(&mut term, b"\x1b[1;2H\x1b]66;s=2;A\x07\x1b[1;1H");
    feed(&mut term, "❤".as_bytes());
    // A late selector claims the next column, which was a block's top fragment.
    feed(&mut term, "\u{fe0f}".as_bytes());
    assert!(
        term.screen().row(0).expect("row").cells[0]
            .flags
            .contains(CellFlags::WIDE)
    );
    assert!(
        !term.screen().row(1).expect("row").cells[1]
            .flags
            .contains(CellFlags::MULTICELL)
    );
    // The same rule applies when late widening relocates across the right edge.
    feed(&mut term, b"\x1b[2;1H\x1b]66;s=2;B\x07\x1b[1;12H");
    feed(&mut term, "❤\u{fe0f}".as_bytes());
    assert_eq!(term.cursor.row, 1);
    assert!(
        !term.screen().row(2).expect("row").cells[0]
            .flags
            .contains(CellFlags::MULTICELL)
    );
}

/// Read-side contracts and reflow are independent of placement conformance.
#[path = "text_sizing/contents.rs"]
mod contents;

/// Heap ownership survives joined text, edits and rectangular reflow.
#[path = "text_sizing/accounting.rs"]
mod accounting;
