//! Rectangular retention, copy and persistence share the public VT view.
use super::*;

#[test]
fn rectangular_reflow_preserves_payload_and_fragment_anchors() {
    let mut term = Terminal::new(8, 5, 20, 65536);
    feed(&mut term, b"\x1b]66;s=2;A\x07BC");
    let root = term.screen().row(0).expect("root").id;
    let lower = term.screen().row(1).expect("lower").id;
    let marker = term.tracked.track(lower, 1);
    assert_eq!(term.buffer().text(root..=lower, Default::default()), "ABC");
    for width in [5, 8, 5, 8] {
        term.resize(width, 5);
        check(&term);
        let block = block(&term, 0, 0);
        let root = block.fragments[0].line;
        let lower = block.fragments[1].line;
        assert_eq!(term.buffer().text(root..=lower, Default::default()), "ABC");
        assert_eq!(
            term.tracked.get(marker),
            puzzle_vt::tracked::PositionState::Present {
                line: lower,
                column: 1
            }
        );
    }
    // An explicit six-column rectangle cannot be partially flattened at width five.
    feed(&mut term, b"\x1b[3;1H\x1b]66;s=2:w=3;XYZ\x07");
    term.resize(5, 5);
    check(&term);
    assert!(
        term.screen()
            .ring
            .multicells
            .as_ref()
            .expect("store")
            .iter()
            .all(|(_, b)| b.text != "XYZ")
    );
}

#[test]
fn lower_fragment_copy_and_search_emit_one_logical_payload() {
    let mut term = Terminal::new(8, 4, 0, 0);
    feed(&mut term, b"\x1b]66;s=2:w=2;heading\x07");
    let lower = term.screen().row(1).expect("lower").id;
    let options = puzzle_vt::view::TextOptions {
        block: true,
        first_column: 1,
        end_column: Some(3),
        ..Default::default()
    };
    assert_eq!(term.buffer().text(lower..=lower, options), "heading\n");
    // Evict the original root; remaining fragments retain text and ownership.
    feed(&mut term, b"\x1b[4;1H\n");
    assert_eq!(block(&term, 0, 0).fragments[0].offset, 1);
    let line = term.screen().row(0).expect("retained lower").id;
    assert_eq!(
        term.buffer().text(line..=line, Default::default()),
        "heading"
    );
    assert_eq!(term.buffer().find("heading", Default::default()).count(), 1);
}

/// Compares semantic block geometry without depending on recycled private keys.
fn same_document(source: &Terminal, replay: &Terminal) {
    let a: Vec<_> = source.screen().ring.iter().collect();
    let b: Vec<_> = replay.screen().ring.iter().collect();
    assert_eq!(a.len(), b.len(), "retained rows");
    for (a, b) in a.into_iter().zip(b) {
        assert_eq!(a.flags, b.flags, "row flags");
        for x in 0..a.cells.len() {
            let mut ac = a.cells[x];
            let mut bc = b.cells[x];
            if ac.flags.contains(CellFlags::MULTICELL) {
                let ar = source.screen().ring.multicell(ac.ch).expect("source block");
                let br = replay.screen().ring.multicell(bc.ch).expect("replay block");
                assert_eq!(
                    (&ar.text, ar.sizing, ar.column, ar.columns),
                    (&br.text, br.sizing, br.column, br.columns)
                );
                let ao = ar
                    .fragments
                    .iter()
                    .find(|f| f.line == a.id)
                    .expect("source fragment")
                    .offset;
                let bo = br
                    .fragments
                    .iter()
                    .find(|f| f.line == b.id)
                    .expect("replay fragment")
                    .offset;
                assert_eq!(ao, bo, "original offset");
                ac.ch = 0;
                bc.ch = 0;
            }
            assert_eq!(ac, bc, "cell {x}");
        }
    }
    assert_eq!(source.modes.get(true, 6), replay.modes.get(true, 6));
    assert_eq!(
        (
            source.cursor.row,
            source.cursor.column,
            source.cursor.pending_wrap
        ),
        (
            replay.cursor.row,
            replay.cursor.column,
            replay.cursor.pending_wrap
        )
    );
}

#[test]
fn text_sizing_vt_roundtrip_mixed_wrap_history_and_clipped_roots() {
    use puzzle_vt::view::Format;
    for (history, bytes) in [
        (20, "\x1b]66;s=2:w=4;edge\x07"),
        (20, "\x1b[?6h\x1b]66;s=2:w=4;edge\x07"),
        (20, "\x1b[3;1H\x1b]66;s=2:w=4;bottom\x07"),
        (20, "\x1b]66;s=2;A\x07BC\r\n\x1b[3;1Htail"),
        (20, "abcdef\x1b]66;s=2;A\x07BCDEF\r\n\r\n\r\n\r\nend"),
        (0, "\x1b]66;s=2:w=2:n=1:d=2;heading\x07\x1b[4;1H\n"),
        (20, "\x1b]66;s=2:w=4;edge\x07\x1b[2;8HX\r\n\r\n\r\nend"),
    ] {
        let mut source = Terminal::new(8, 4, history, 65536);
        feed(&mut source, bytes.as_bytes());
        let ids = source.buffer().line_ids().expect("retained rows");
        let stream = source.buffer().serialize(ids, Format::Vt);
        assert_eq!(
            stream.matches("]66;").count(),
            source
                .screen()
                .ring
                .multicells
                .as_ref()
                .expect("store")
                .iter()
                .count(),
            "one operation per owner"
        );
        let mut replay = Terminal::new(8, 4, history, 65536);
        feed(&mut replay, stream.as_bytes());
        same_document(&source, &replay);
    }
}

#[test]
fn text_sizing_selection_bulk_and_local_clear_keep_one_payload() {
    use puzzle_vt::{
        selection::{SelectPoint, Selection, SelectionKind},
        view::CellBlock,
    };
    let mut term = Terminal::new(8, 5, 10, 65536);
    feed(&mut term, b"startup\r\n\x1b]66;s=2:w=2;heading\x07BC");
    let lower = term.screen().row(2).expect("lower").id;
    let point = SelectPoint {
        line: lower,
        column: 1,
    };
    let selection =
        Selection::new(&mut term, SelectionKind::Simple, point, point).expect("selection");
    assert_eq!(selection.text(&term, " ").as_deref(), Some("heading"));
    term.resize(6, 5);
    check(&term);
    assert_eq!(selection.text(&term, " ").as_deref(), Some("heading"));
    let mut cells = CellBlock {
        multicells: Some(Vec::new()),
        ..Default::default()
    };
    term.buffer()
        .cells_into(term.buffer().line_ids().expect("retained rows"), &mut cells);
    assert_eq!(cells.multicells.as_ref().expect("requested table").len(), 1);
    assert_eq!(
        cells
            .clusters
            .iter()
            .filter(|(_, text)| text == "heading")
            .count(),
        1
    );
    // Cmd+K preserves all prompt fragments rather than retaining stale slot keys.
    term.clear_preserving_line();
    check(&term);
    assert_eq!(block(&term, 0, 0).text, "heading");
}

#[test]
fn text_sizing_storage_retires_payloads_and_shrinks_metadata() {
    let mut term = Terminal::new(8, 4, 100, 8192);
    let payload = "x".repeat(4096);
    for _ in 0..100 {
        feed(
            &mut term,
            format!("\x1b]66;w=1;{payload}\x07\r\n").as_bytes(),
        );
        assert!(term.primary.ring.allocated_history_bytes() <= 8192);
    }
    // Exact payload capacity is assigned to one row even across root eviction.
    let payloads: usize = term
        .primary
        .ring
        .multicells
        .as_ref()
        .expect("store")
        .iter()
        .map(|(_, b)| b.text.capacity())
        .sum();
    assert!(payloads <= 6 * 4096, "history cap plus active four rows");
    feed(&mut term, b"\x1b[2J\x1b[3J");
    term.resize(2, 1);
    check(&term);
    assert!(
        term.primary.ring.multicell_storage_bytes()
            < size_of::<puzzle_vt::multicell::MulticellStore>() + 1024
    );
}

#[test]
fn text_sizing_rich_rectangle_emits_lower_intersection_once() {
    use puzzle_vt::view::{CellRange, Format};
    let mut term = Terminal::new(8, 4, 0, 0);
    feed(&mut term, b"\x1b]66;s=2:w=2;<heading>\x07");
    let line = term.screen().row(1).expect("lower").id;
    let range = CellRange {
        start: (line, 1),
        end: (line, 3),
        block: true,
        trim_trailing: true,
        final_newline: false,
    };
    let html = term.buffer().serialize_cells(range, Format::Html);
    assert_eq!(html.matches("&lt;heading&gt;").count(), 1);
    let vt = term.buffer().serialize_cells(range, Format::Vt);
    assert_eq!(vt.matches("]66;").count(), 1);
    assert!(vt.contains(";<heading>\x1b\\"));
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]
    /// An independent append-only text oracle survives varying sizes and reflow.
    #[test]
    fn text_sizing_logical_copy_oracle(items in prop::collection::vec((1u8..4, any::<bool>(), 0u8..3, 0u8..3), 1..7)) {
        let mut term = Terminal::new(64, 7, 20, 65536);
        let mut expected = String::new();
        for (scale, explicit, h, v) in items {
            let text = if explicit { "AB" } else { "é" };
            feed(&mut term, format!("\x1b]66;s={scale}:w={}:n=1:d=2:h={h}:v={v};{text}\x07", u8::from(explicit)).as_bytes());
            expected.push_str(text);
            for width in [20, 64] {
                term.resize(width, 7);
                check(&term);
                let copied = term.buffer().text(term.buffer().line_ids().expect("rows"), Default::default());
                prop_assert_eq!(copied.trim_end_matches('\n'), expected.as_str());
            }
        }
    }
}

#[test]
fn text_sizing_alternate_truncates_and_primary_growth_keeps_clipped_root() {
    let mut term = Terminal::new(8, 7, 20, 65536);
    feed(&mut term, b"\x1b[?1049h\x1b[5;1H\x1b]66;s=3:w=2;alt\x07");
    term.resize(8, 6);
    check(&term);
    assert!(
        term.screen()
            .ring
            .multicells
            .as_ref()
            .expect("store")
            .iter()
            .next()
            .is_none()
    );
    feed(
        &mut term,
        b"\x1b[?1049l\x1b]66;s=3:w=2;primary\x07\x1b[7;1H\n",
    );
    term.resize(8, 8);
    check(&term);
    assert_eq!(term.buffer().find("primary", Default::default()).count(), 1);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]
    /// Persistence uses wire operations, with no private slots in the saved stream.
    #[test]
    fn text_sizing_reflow_serialization_oracle(items in prop::collection::vec((1u8..4, any::<bool>(), 0u8..3, 0u8..3), 1..7)) {
        use puzzle_vt::view::Format;
        let mut source = Terminal::new(64, 7, 20, 65536);
        for (s, explicit, h, v) in items {
            feed(&mut source, format!("\x1b]66;s={s}:w={}:n=1:d=2:h={h}:v={v};AB\x07x", u8::from(explicit)).as_bytes());
        }
        for width in [20, 64] {
            source.resize(width, 7);
            check(&source);
            let stream = source.buffer().serialize(source.buffer().line_ids().expect("rows"), Format::Vt);
            let mut replay = Terminal::new(width, 7, 20, 65536);
            feed(&mut replay, stream.as_bytes());
            same_document(&source, &replay);
        }
    }
}
