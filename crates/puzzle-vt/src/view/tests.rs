//! Public buffer-read and VT replay properties from design §6.13.
//!
//! Random styled screens exercise the actual parser and formatter, not
//! a direct memory clone. A replay with identical cells, cursor and modes
//! proves the exported stream is usable by persistence and bug reports.
use super::*;
use proptest::prelude::*;

// VT export re-enters a fresh parser without losing random cell styles.
/// One generated write combines color, attributes, clusters and OSC 8.
///
/// The choices correspond to §6.5's SGR forms and §6.7's width model,
/// so VT export is exercised through the same parser as application data.
fn generated_write(term: &mut Terminal, spec: (usize, usize, u8, u8, u8, u16, u8, bool)) {
    let (row, col, glyph, fg, bg, flags, underline, link) = spec;
    let mut codes = String::from("0");
    for (bit, code) in [(0, 1), (1, 2), (2, 3), (3, 7), (4, 8), (5, 9), (6, 53)] {
        if flags & (1 << bit) != 0 {
            codes.push_str(&format!(";{code}"));
        }
    }
    if underline != 0 {
        codes.push_str(&format!(";4:{underline}"));
    }
    for (choice, selector) in [(fg, 38), (bg, 48)] {
        match choice % 3 {
            1 => codes.push_str(&format!(";{selector};5;{}", flags % 256)),
            2 => codes.push_str(&format!(";{selector};2;{};{};{}", flags % 256, glyph, 97)),
            _ => {}
        }
    }
    if underline != 0 && flags & 128 != 0 {
        codes.push_str(";58;2;123;45;67");
    }
    let text = ["A", "界", "e\u{0301}", "♥\u{fe0f}"][usize::from(glyph % 4)];
    let prefix = if link {
        "\x1b]8;;https://example.test/roundtrip\x1b\\"
    } else {
        ""
    };
    term.advance(
        format!(
            "\x1b[{};{}H\x1b[{codes}m{prefix}{text}\x1b]8;;\x1b\\",
            row + 1,
            col + 1
        )
        .as_bytes(),
    );
    crate::test_grid::check(term);
}

// Randomized SGR, clusters, history and modes must survive public VT replay.
proptest! {
    #[test]
    fn random_screen_vt_round_trip(
        writes in proptest::collection::vec((0usize..6, 0usize..12, any::<u8>(),
            0u8..3, 0u8..3, any::<u16>(), 0u8..6, any::<bool>()), 15..55),
        modes in any::<u8>(), cursor_row in 0usize..6, cursor_col in 0usize..12,
        style in 0u8..7,
    ) {
        let mut source = Terminal::new(12, 6, 8, 8192);
        source.advance(b"abcdefghijklmnop\r\nqrstuvwxyzABCD\r\n");
        for _ in 0..7 {
            source.advance(b"history-line\r\n");
        }
        source.advance(b"\x1b[?2027h");
        for write in writes {
            generated_write(&mut source, write);
        }
        for (bit, mode) in [1, 7, 25, 1004, 2004, 2027].into_iter().enumerate() {
            let action = if modes & (1 << bit) != 0 { 'h' } else { 'l' };
            source.advance(format!("\x1b[?{mode}{action}").as_bytes());
        }
        source.advance(format!("\x1b[{style} q\x1b[{};{}H", cursor_row + 1, cursor_col + 1).as_bytes());
        let Some(ids) = source.buffer().line_ids() else {
            prop_assert!(false, "source grid has rows");
            return Ok(());
        };
        let bytes = source.buffer().serialize(ids, Format::Vt);
        let mut restored = Terminal::new(12, 6, 8, 8192);
        restored.advance(bytes.as_bytes());
        crate::test_grid::check(&source);
        crate::test_grid::check(&restored);
        let original: Vec<_> = source.screen().ring.iter().collect();
        let replayed: Vec<_> = restored.screen().ring.iter().collect();
        prop_assert_eq!(original.len(), replayed.len());
        for (left, right) in original.iter().zip(&replayed) {
            prop_assert_eq!(&left.cells, &right.cells);
            prop_assert_eq!(&left.extras, &right.extras);
            prop_assert_eq!(left.flags, right.flags);
        }
        prop_assert_eq!(source.cursor, restored.cursor);
        prop_assert_eq!(source.cursor_style, restored.cursor_style);
        for mode in [1, 7, 25, 1004, 2004, 2027] {
            prop_assert_eq!(source.modes.get(true, mode), restored.modes.get(true, mode));
        }
    }
}

/// Soft wraps join, while block extraction preserves physical rows.
#[test]
fn extraction_and_bulk_cells_keep_clusters_and_wide_spacers() {
    let mut term = Terminal::new(4, 3, 0, 0);
    term.advance("abcde\u{0301}界".as_bytes());
    let view = term.buffer();
    let Some(ids) = view.line_ids() else {
        panic!("grid IDs exist")
    };
    let joined = view.text(ids.clone(), TextOptions::default());
    assert!(joined.starts_with("abcde\u{0301}界"));
    let block = view.text(
        ids.clone(),
        TextOptions {
            block: true,
            first_column: 0,
            end_column: Some(2),
            ..TextOptions::default()
        },
    );
    assert!(block.contains('\n'));
    let mut cells = CellBlock::default();
    view.cells_into(ids, &mut cells);
    assert_eq!(cells.code_points.len(), 12);
    assert!(cells.widths.contains(&0));
    assert!(cells.clusters.iter().any(|(_, text)| text == "e\u{0301}"));
}

/// Changes include new line identities even after a full-grid scroll.
#[test]
fn changes_since_scroll_report_new_rows() {
    let mut term = Terminal::new(4, 2, 2, 128);
    let token = term.buffer().change_token();
    term.advance(b"one\r\ntwo\r\nthree\r\n");
    let changed = term.buffer().changes_since(&token);
    assert!(changed.scroll_delta > 0);
    assert!(!changed.lines.is_empty());
}

/// Region-local editing changes IDs without moving the full viewport.
#[test]
fn changes_since_region_edit_has_no_full_scroll_delta() {
    let mut term = Terminal::new(5, 3, 3, 1024);
    let token = term.buffer().change_token();
    term.advance(b"\x1b[2;3r\x1b[2;1H\x1b[L");
    let changes = term.buffer().changes_since(&token);
    assert_eq!(changes.scroll_delta, 0);
    assert!(!changes.lines.is_empty());
}

/// Sealed frames keep one bounded log entry instead of copying history.
#[test]
fn changes_since_merges_sealed_frame_damage() {
    let mut term = Terminal::new(4, 2, 2, 128);
    term.seal_frame();
    let token = term.buffer().change_token();
    term.advance(b"new");
    term.seal_frame();
    let changed = term.buffer().changes_since(&token);
    assert_eq!(changed.scroll_delta, 0);
    assert_eq!(changed.lines.len(), 1);
    assert_eq!(term.change_log.entries_len(), 2);
    for _ in 0..100 {
        term.seal_frame();
    }
    assert_eq!(term.change_log.entries_len(), 64);
    assert!(!term.buffer().changes_since(&token).lines.is_empty());
}

/// HTML escapes application-controlled markup before embedding it.
#[test]
fn html_serialization_escapes_untrusted_text() {
    let mut term = Terminal::new(8, 1, 0, 0);
    term.advance(b"<b&>");
    let Some(ids) = term.buffer().line_ids() else {
        panic!("grid IDs exist")
    };
    let html = term.buffer().serialize(ids, Format::Html);
    assert!(html.contains("&lt;"));
    assert!(html.contains("&amp;"));
    assert!(!html.contains("<b&>"));
}

/// VT export includes retained history before the visible tail (§6.13).
#[test]
fn vt_serialize_replays_scrollback_in_order() {
    let mut source = Terminal::new(4, 2, 4, 1024);
    source.advance(b"aaaa\r\nbbbb\r\ncccc\r\ndddd");
    let Some(ids) = source.buffer().line_ids() else {
        panic!("retained lines")
    };
    let bytes = source.buffer().serialize(ids, Format::Vt);
    let mut restored = Terminal::new(4, 2, 4, 1024);
    restored.advance(bytes.as_bytes());
    let original: Vec<_> = source.screen().ring.iter().map(|row| &row.cells).collect();
    let replayed: Vec<_> = restored
        .screen()
        .ring
        .iter()
        .map(|row| &row.cells)
        .collect();
    assert_eq!(original, replayed);
    assert_eq!(source.cursor, restored.cursor);
}

/// Existing cells adopt OSC palette changes without a row rewrite.
#[test]
fn cell_view_resolves_raw_color_against_live_palette() {
    let mut term = Terminal::new(4, 1, 0, 0);
    term.advance(b"\x1b[31mR");
    term.colors.set(
        crate::color_table::ColorSlot::Palette(1),
        crate::color_table::ColorValue::Rgb([12, 34, 56]),
    );
    let Some(line) = term.screen().row(0).map(|row| row.id) else {
        panic!("row")
    };
    let Some(cell) = term.buffer().line(line).and_then(|view| view.cell(0)) else {
        panic!("cell")
    };
    assert_eq!(cell.foreground(), PackedColor::palette(1));
    assert_eq!(
        cell.resolved_foreground([1, 2, 3], &[[0; 3]; 256]),
        [12, 34, 56]
    );
}

/// Recycled frame journal entries request no heap storage in steady state.
#[test]
fn warm_frame_change_log_allocates_nothing() {
    let mut term = Terminal::new(4, 2, 0, 0);
    for _ in 0..super::CHANGE_LOG_FRAMES {
        term.seal_frame();
    }
    let counts = allocation_counter::measure(|| {
        for _ in 0..16 {
            term.seal_frame();
        }
    });
    assert_eq!(counts.count_total, 0);
}

/// Public marker registration rejects invalid cells and releases handles.
#[test]
fn marker_lifecycle_rejects_stale_or_outside_positions() {
    let mut term = Terminal::new(4, 2, 0, 0);
    let Some(line) = term.screen().row(0).map(|row| row.id) else {
        panic!("row")
    };
    assert!(term.add_marker(line, 4).is_none());
    assert!(term.add_marker(LineId(u64::MAX), 0).is_none());
    let Some(marker) = term.add_marker(line, 1) else {
        panic!("valid marker")
    };
    assert_eq!(
        term.tracked.get(marker),
        crate::tracked::PositionState::Present { line, column: 1 }
    );
    term.remove_marker(marker);
    assert_eq!(
        term.tracked.get(marker),
        crate::tracked::PositionState::Gone
    );
}

/// Replaying colored historical ink must not tint untouched cells of the next row.
#[test]
fn serialized_history_starts_new_rows_with_default_bce() {
    let mut source = Terminal::new(12, 2, 4, 8192);
    for _ in 0..5 {
        source.advance(b"\x1b[11Ga\x1b[40mZ\x1b[0m\r\nb\r\n");
    }
    let Some(ids) = source.buffer().line_ids() else {
        panic!("buffer absent");
    };
    let encoded = source.buffer().serialize(ids, Format::Vt);
    let mut restored = Terminal::new(12, 2, 4, 8192);
    restored.advance(encoded.as_bytes());
    for (left, right) in source
        .screen()
        .ring
        .iter()
        .zip(restored.screen().ring.iter())
    {
        assert_eq!(left.cells, right.cells);
    }
}
