//! Margin moves must not copy half of an indivisible glyph into another row.
#![allow(clippy::expect_used, reason = "fixtures require retained grid rows")]
use super::*;
use proptest::prelude::*;

/// Test every routed scroll operation with the same two split horizontal edges.
const SCROLLS: [(&str, usize); 8] = [
    ("\x1b[S", 0),
    ("\x1b[T", 0),
    ("\x1b[8;8H\x1bD", 0),
    ("\x1b[8;8H\n", 0),
    ("\x1b[8;8H\x1bE", 0),
    ("\x1b[1;8H\x1bM", 0),
    ("\x1b[3;8H\x1b[L", 2),
    ("\x1b[3;8H\x1b[M", 2),
];

/// Both crossing pairs and one intact interior pair have non-BCE original backgrounds.
fn populated() -> Terminal {
    let mut term = Terminal::new(16, 8, 64, 64 * 1024);
    term.advance(b"\x1b[41m");
    for y in 1..=8 {
        term.advance(format!("\x1b[{y};1HABCDEF一G界J界KL").as_bytes());
    }
    term.advance(b"\x1b[?69h\x1b[8;13s\x1b[42m");
    term
}

/// Assert all stable invariants, including sparse rectangular ownership after edits.
fn check(term: &Terminal) {
    crate::test_grid::check(term);
    #[cfg(debug_assertions)]
    for screen in [&term.primary, &term.alternate] {
        assert_eq!(screen.ring.check_multicells(), Ok(()));
    }
}

#[test]
fn left_margin_scroll_clears_both_wide_halves_before_moving_cells() {
    let mut term = Terminal::new(16, 8, 10_000, 32 * 1024 * 1024);
    // Reviewer reproduction is one ordinary chunk, not campaign input.
    term.advance("\x1b[?69hhaGgrn一\x1b[8s\n\n\n\n\n\n\n\n".as_bytes());
    crate::test_grid::check(&term);
    let row = term.screen().row(0).expect("retained first row");
    assert_eq!(row.cells[5].ch, u32::from('n'));
    assert_eq!(row.cells[6].ch, 0);
    assert_eq!(row.cells[7].ch, 0);
}

#[test]
fn every_scroll_route_prepares_both_source_and_destination_margin_edges() {
    for (operation, first) in SCROLLS {
        let mut term = populated();
        term.advance(operation.as_bytes());
        check(&term);
        for y in first..8 {
            let row = term.screen().row(y).expect("affected row");
            for x in [6, 7, 12, 13] {
                assert_eq!(
                    row.cells[x],
                    Cell::erased(term.pen.bg),
                    "{operation:?}: {y},{x}"
                );
            }
            // Clearing the outside halves must not touch unrelated outer text.
            assert_eq!(row.cells[5].ch, u32::from('F'));
            assert_eq!(row.cells[14].ch, u32::from('K'));
        }
        for y in 0..first {
            assert!(
                term.screen().row(y).expect("untouched row").cells[6]
                    .flags
                    .contains(CellFlags::WIDE)
            );
        }
        // A complete wide pair inside the rectangle moves, rather than being erased.
        assert!((0..8).any(|y| {
            term.screen().row(y).expect("row").cells[9]
                .flags
                .contains(CellFlags::WIDE)
        }));
    }
}

#[test]
fn ich_dch_prepare_outer_edges_and_preserve_complete_shifted_pairs() {
    for (operation, lead) in [("\x1b[@", 10), ("\x1b[P", 8)] {
        let mut term = populated();
        term.advance(b"\x1b[1;8H");
        term.advance(operation.as_bytes());
        check(&term);
        let row = term.screen().row(0).expect("edited row");
        assert_eq!(row.cells[6], Cell::erased(term.pen.bg));
        assert_eq!(row.cells[13], Cell::erased(term.pen.bg));
        assert!(row.cells[lead].flags.contains(CellFlags::WIDE));
        assert!(row.cells[lead + 1].flags.contains(CellFlags::WIDE_SPACER));
    }
}

#[test]
fn ich_dch_inner_discard_edge_clears_the_whole_pair_with_current_bce() {
    for (operation, blank_at) in [("\x1b[5@", 7), ("\x1b[3P", 0)] {
        let mut term = Terminal::new(8, 2, 0, 0);
        term.advance("\x1b[41mAB界CD界\x1b[42m\x1b[H".as_bytes());
        term.advance(operation.as_bytes());
        check(&term);
        assert_eq!(
            term.screen().row(0).expect("row").cells[blank_at],
            Cell::erased(term.pen.bg)
        );
    }
}

#[test]
fn ech_el_ed_share_atomic_edges_in_both_directions() {
    for (cursor, operation, pair) in [
        (8, "\x1b[X", [6, 7]),
        (13, "\x1b[X", [12, 13]),
        (8, "\x1b[K", [6, 7]),
        (13, "\x1b[1K", [12, 13]),
        (8, "\x1b[J", [6, 7]),
        (13, "\x1b[1J", [12, 13]),
    ] {
        let mut term = populated();
        term.advance(format!("\x1b[1;{cursor}H{operation}").as_bytes());
        check(&term);
        let row = term.screen().row(0).expect("row");
        for x in pair {
            assert_eq!(row.cells[x], Cell::erased(term.pen.bg));
        }
    }
}

#[test]
fn selective_edges_keep_protected_pairs_and_clear_unprotected_pairs_atomically() {
    for protected in [false, true] {
        for (cursor, operation) in [(4, "\x1b[?K"), (3, "\x1b[?1J")] {
            let mut term = Terminal::new(8, 2, 0, 0);
            if protected {
                term.advance(b"\x1b[1\"q");
            }
            term.advance("AB界CD\x1b[0\"q\x1b[42m".as_bytes());
            term.advance(format!("\x1b[1;{cursor}H{operation}").as_bytes());
            check(&term);
            let row = term.screen().row(0).expect("row");
            assert_eq!(row.cells[2].flags.contains(CellFlags::WIDE), protected);
            assert_eq!(
                row.cells[3].flags.contains(CellFlags::WIDE_SPACER),
                protected
            );
            if !protected {
                assert_eq!(row.cells[2], Cell::erased(term.pen.bg));
            }
        }
    }
}

#[test]
fn partial_scroll_removes_cut_blocks_but_preserves_blocks_outside_the_columns() {
    for (operation, first) in SCROLLS {
        let mut term = Terminal::new(16, 8, 64, 64 * 1024);
        let y = first + 1;
        for column in [1, 7, 10, 13, 15] {
            term.advance(format!("\x1b[{y};{column}H\x1b]66;s=2;A\x1b\\").as_bytes());
        }
        term.advance(b"\x1b[?69h\x1b[8;13s\x1b[42m");
        term.advance(operation.as_bytes());
        check(&term);
        let store = term
            .screen()
            .ring
            .multicells
            .as_deref()
            .expect("outside blocks survive");
        assert_eq!(store.iter().count(), 2);
        assert!(
            store
                .iter()
                .all(|(_, block)| [0, 14].contains(&block.column))
        );
    }
}

#[test]
fn splice_and_erase_cut_multicell_boundaries_without_leaving_fragments() {
    for operation in ["\x1b[@", "\x1b[P", "\x1b[X", "\x1b[K", "\x1b[J"] {
        let mut term = Terminal::new(12, 4, 64, 64 * 1024);
        term.advance(b"\x1b[1;3H\x1b]66;s=2;A\x1b\\\x1b[2;4H\x1b[42m");
        term.advance(operation.as_bytes());
        check(&term);
        assert!(
            term.screen()
                .ring
                .multicells
                .as_deref()
                .expect("store")
                .is_empty()
        );
        for y in 0..2 {
            let row = term.screen().row(y).expect("old fragment");
            assert_eq!(row.cells[2], Cell::erased(term.pen.bg));
        }
    }
}

#[test]
fn plain_truncation_uses_the_atomic_cut_and_retires_cluster_extras() {
    for alternate in [false, true] {
        let mut term = Terminal::new(8, 3, 64, 64 * 1024);
        if alternate {
            term.advance(b"\x1b[?1049h");
        } else {
            term.reflow_enabled = false;
        }
        term.advance("AB一\u{e0102}CD".as_bytes());
        term.resize(3, 3);
        check(&term);
        let row = term.screen().row(0).expect("truncated row");
        assert_eq!(row.cells[2], Cell::default());
        assert!(!row.extras.iter().any(|extra| extra.column == 2));
    }
}

#[test]
fn boundary_cleanup_reports_outside_writes_and_warm_margin_edits_allocate_nothing() {
    let mut term = populated();
    let line = term.screen().row(0).expect("row").id;
    let watch = term
        .watch_source((line, 6), (line, 7), 1)
        .expect("outside lead watch");
    term.advance(b"\x1b[S");
    assert!(
        term.source_watch(watch)
            .expect("outside source remains")
            .changed
    );
    let measured = allocation_counter::measure(|| {
        for _ in 0..100 {
            term.advance(b"\x1b[S\x1b[T");
        }
    });
    assert_eq!(measured.count_total, 0);
    check(&term);
}

#[test]
fn unsupported_column_and_dec_rectangle_controls_remain_inert() {
    let mut term = populated();
    let before: Vec<_> = (0..8)
        .map(|y| {
            let row = term.screen().row(y).expect("row");
            (row.cells.iter().copied().collect::<Vec<_>>(), row.version)
        })
        .collect();
    // DECIC/DECDC and DECFRA/DECERA/DECSERA/DECCRA have no dispatch route.
    // Testing their inert behavior prevents this repair silently adding features.
    for bytes in [
        b"\x1b[1'}".as_slice(),
        b"\x1b[1'~",
        b"\x1b[65;1;1;8;16$x",
        b"\x1b[1;1;8;16$z",
        b"\x1b[1;1;8;16${",
        b"\x1b[1;1;8;16;1;1;1;1$v",
    ] {
        term.advance(bytes);
        check(&term);
        for (y, (cells, version)) in before.iter().enumerate() {
            let row = term.screen().row(y).expect("row");
            assert_eq!(row.cells.iter().copied().collect::<Vec<_>>(), *cells);
            assert_eq!(row.version, *version);
        }
    }
}

proptest! {
    #[test]
    fn arbitrary_margin_cuts_never_leave_orphaned_pairs(
        columns in 4usize..40, offset in 0usize..40, upward in any::<bool>(),
        alternate in any::<bool>(), count in 1usize..12,
    ) {
        let mut term = Terminal::new(columns, 8, 64, 64 * 1024);
        if alternate { term.advance(b"\x1b[?1049h"); }
        let lead = offset % (columns - 2);
        term.advance(format!("\x1b[1;{}H界", lead + 1).as_bytes());
        check(&term);
        term.advance(format!("\x1b[?69h\x1b[{}s", lead + 2).as_bytes());
        check(&term);
        let final_byte = if upward { 'S' } else { 'T' };
        for _ in 0..3 {
            term.advance(format!("\x1b[{count}{final_byte}").as_bytes());
            check(&term);
        }
    }
}
