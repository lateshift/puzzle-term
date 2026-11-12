//! Captured gaps admit complete wide/cluster/block units without expanding them.
#![allow(
    clippy::expect_used,
    reason = "fixture failures identify source-boundary defects"
)]
use super::*;
use crate::osc::{ScanProgress, anchors::Store};
use puzzle_vt::Terminal;

/// Move the fixture cursor to a real ordered gap, then capture through the owner.
fn mark(
    term: &mut Terminal,
    store: &mut Store,
    row: usize,
    column: usize,
) -> super::super::EndpointReceipt {
    term.advance(format!("\x1b[{};{}H", row + 1, column + 1).as_bytes());
    OscContext {
        replies: None,
        terminal: term,
        endpoints: store,
    }
    .capture_endpoint(8)
    .expect("mark")
}

/// Every physical visit is still charged, even when the visitor rejects a unit.
fn admits(
    term: &mut Terminal,
    store: &mut Store,
    start: (usize, usize),
    end: (usize, usize),
) -> bool {
    let first = mark(term, store, start.0, start.1);
    let last = mark(term, store, end.0, end.1);
    let source = OscContext {
        replies: None,
        terminal: term,
        endpoints: store,
    };
    let mut scan = source
        .begin_source_scan(&first, &last)
        .expect("source interval");
    let mut visits = 4096;
    let mut outcome = Ok(ScanProgress::Pending);
    let measured = allocation_counter::measure(|| {
        outcome = source.visit_source_rows(&mut scan, &mut visits, |row| {
            row.contains_complete_units(&source)
        });
    });
    assert_eq!(measured.count_total, 0);
    outcome == Ok(ScanProgress::Complete)
}

#[test]
fn wide_pairs_and_full_clusters_have_indivisible_boundaries() {
    for (start, end, expected) in [
        ((0, 0), (0, 3), true),
        ((0, 0), (0, 1), false),
        ((0, 1), (0, 3), false),
        ((0, 2), (0, 3), true),
    ] {
        let mut term = Terminal::new(12, 4, 64, 64 * 1024);
        let mut store = Store::new(&term);
        term.advance("一e\u{301}".as_bytes());
        assert_eq!(admits(&mut term, &mut store, start, end), expected);
    }
}

#[test]
fn all_horizontal_and_vertical_rectangle_cuts_are_refused() {
    for (start, end, expected) in [
        ((0, 0), (1, 2), true),
        ((0, 0), (2, 0), true),
        ((0, 0), (0, 2), false),
        ((1, 0), (1, 2), false),
        ((0, 1), (1, 2), false),
        ((0, 0), (1, 1), false),
    ] {
        let mut term = Terminal::new(12, 4, 64, 64 * 1024);
        let mut store = Store::new(&term);
        term.advance(b"\x1b]66;s=2;H\x1b\\");
        assert_eq!(
            admits(&mut term, &mut store, start, end),
            expected,
            "{start:?}..{end:?}"
        );
    }
}

#[test]
fn surviving_lower_fragment_cannot_authorize_a_fresh_whole_unit() {
    let mut term = Terminal::new(12, 2, 0, 64 * 1024);
    let mut store = Store::new(&term);
    term.advance(b"\x1b]66;s=2;H\x1b\\\x1b[2;1H\n");
    assert!(
        !term
            .screen()
            .row(0)
            .expect("remaining fragment")
            .blocks
            .is_empty()
    );
    assert!(!admits(&mut term, &mut store, (0, 0), (0, 2)));
}
