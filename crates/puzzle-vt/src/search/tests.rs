//! Literal search regressions for soft wraps and incremental result handles.
//!
//! These tests use terminal bytes so the WRAPPED bit comes from the real
//! parser rather than a hand-built row fixture.
use super::*;

/// A query may begin before a soft wrap and finish on the next row.
#[test]
fn literal_search_crosses_soft_wrap_with_smart_case() {
    let mut term = Terminal::new(4, 3, 0, 0);
    term.advance(b"abcdEf");
    let hits: Vec<_> = term.buffer().find("dE", FindOptions::default()).collect();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].start.column, 3);
    assert_eq!(hits[0].end.column, 0);
    // An uppercase query uses exact case; the source has lowercase d.
    assert_eq!(term.buffer().find("DE", FindOptions::default()).count(), 0);
}

/// Incremental queries release their registry slots when replaced.
#[test]
fn repeated_search_reuses_tracked_slots() {
    let mut term = Terminal::new(8, 2, 0, 0);
    term.advance(b"abc abc");
    let mut search = Search::new("abc", FindOptions::default());
    search.advance(&mut term);
    assert_eq!(search.count(), 2);
    let first = search.results[0].start;
    search.clear(&mut term);
    assert_eq!(term.tracked.get(first), PositionState::Gone);
    let mut again = Search::new("abc", FindOptions::default());
    again.advance(&mut term);
    assert_eq!(again.count(), 2);
    assert_eq!(term.tracked.positions_len(), 4);
}

/// Navigation begins at the first hit and wraps after the final hit.
#[test]
fn first_next_and_previous_wrap_to_expected_matches() {
    let mut term = Terminal::new(12, 2, 0, 0);
    term.advance(b"abc abc");
    let mut search = Search::new("abc", FindOptions::default());
    search.advance(&mut term);
    assert_eq!(search.next(&term).map(|hit| hit.start.column), Some(0));
    assert_eq!(search.next(&term).map(|hit| hit.start.column), Some(4));
    assert_eq!(search.next(&term).map(|hit| hit.start.column), Some(0));
    assert_eq!(search.prev(&term).map(|hit| hit.start.column), Some(4));
    search.clear(&mut term);
}

/// A hostile broad query cannot register more than §16's 10,000 hits.
#[test]
fn search_result_cap_is_ten_thousand() {
    let mut term = Terminal::new(200, 2, 200, 1_000_000);
    term.advance(&vec![b'a'; 20_000]);
    let mut search = Search::new("a", FindOptions::default());
    search.advance(&mut term);
    assert_eq!(search.count(), 10_000);
    assert!(search.is_done());
    search.clear(&mut term);
}

/// A giant soft-wrapped logical line must not defer all scanning to one frame.
#[test]
fn giant_soft_wrap_has_bounded_scratch_and_rewind() {
    // The fixture must retain the same 5,000-row logical line, including boxed headers.
    let mut term = Terminal::new(4, 2, 6_000, 2_000_000);
    term.advance(&vec![b'a'; 20_004]);
    let mut search = Search::new("ab", FindOptions::default());
    while !search.is_done() {
        search.advance(&mut term);
        assert!(search.visited() <= 2_000);
        assert!(search.pending.len() < 2);
    }
    // Rewriting the live tail requires walking the preceding logical line.
    term.advance(b"\x1b[1;1Hb");
    search.reconcile(&mut term);
    let mut slices = 0;
    while !search.is_done() {
        search.advance(&mut term);
        assert!(search.visited() <= 2_000);
        slices += 1;
        assert!(slices < 10);
    }
    assert!(slices >= 5, "backward discovery must yield too");
    assert_eq!(search.count(), 1);
}

/// Evicting the scan frontier must also discard its incomplete text overlap.
#[test]
fn evicted_frontier_cannot_invent_a_cross_boundary_match() {
    let mut term = Terminal::new(4, 2, 3_000, 1_000_000);
    term.advance(&vec![b'a'; 12_004]);
    let mut search = Search::new("ab", FindOptions::default());
    search.advance(&mut term);
    assert!(!search.is_done());
    // Remove every scanned row before the next refresh; the retained text is b.
    term.advance(&vec![b'b'; 12_008]);
    search.reconcile(&mut term);
    while !search.is_done() {
        search.advance(&mut term);
        assert!(search.visited() <= 2_000);
    }
    assert_eq!(search.count(), 0);
}
