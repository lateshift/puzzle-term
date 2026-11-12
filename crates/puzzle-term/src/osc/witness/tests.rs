//! Reviewer-required constant storage, bounded visits and ownership proofs.
//! Large fixture history retains the complete interval; no product defaults change.
#![allow(
    clippy::expect_used,
    reason = "Missing fixture source is the regression being asserted."
)]
use super::*;
use crate::osc::{ScanProgress, anchors::Store};
use puzzle_vt::Terminal;

/// Fixture borrows the same private registration namespace as a real handler.
fn context<'a>(term: &'a mut Terminal, store: &'a mut Store) -> OscContext<'a> {
    OscContext {
        replies: None,
        terminal: term,
        endpoints: store,
    }
}

/// Source capture is an actual cursor gap, never a producer-provided coordinate.
fn mark(term: &mut Terminal, store: &mut Store) -> EndpointReceipt {
    context(term, store)
        .capture_endpoint(512)
        .expect("retained fixture source")
}

/// Both metadata preparation and inspection spend the unchanged shared visit cap.
fn witness(
    term: &mut Terminal,
    store: &mut Store,
    start: &EndpointReceipt,
    end: &EndpointReceipt,
) -> SourceWitness {
    let mut ctx = context(term, store);
    let mut scan = ctx
        .begin_source_scan(start, end)
        .expect("retained fixture source");
    loop {
        let mut visits = 4096;
        let ready = ctx
            .prepare_source_witness(&mut scan, &mut visits)
            .expect("retained fixture source");
        if ready == ScanProgress::Complete
            && ctx
                .visit_source_rows(&mut scan, &mut visits, |_| true)
                .expect("retained fixture source")
                == ScanProgress::Complete
        {
            break;
        }
    }
    ctx.capture_source_witness(&scan, 256)
        .expect("retained fixture source")
}

/// Fixed entry bytes and actual shared table capacity must not scale with rows.
fn proof(term: &mut Terminal, store: &mut Store, witness: &SourceWitness, rows: usize) {
    let ctx = context(term, store);
    let state = ctx
        .query_source_witness(witness)
        .expect("retained fixture source");
    assert!(!state.changed);
    let ring = &ctx.terminal.screen().ring;
    let a = ring
        .index_of(state.start.0)
        .expect("retained fixture source");
    let b = ring.index_of(state.end.0).expect("retained fixture source");
    assert_eq!(b - a + 1, rows);
    assert_eq!(ctx.source_witness_order(witness), Some((a, b)));
    assert_eq!(witness.entries(), 3);
    assert_eq!(
        witness.charged_bytes(),
        size_of::<SourceWitness>() + Terminal::source_watch_entry_bytes()
    );
    // The original constant VT pools still occupy 608 bytes. The registration
    // now stores a fold bit beside each watch; allocator slack (four entries)
    // must be billed too, independently of 41, 4,100 or 60,000 source rows.
    assert_eq!(ctx.terminal.source_watch_storage_bytes(), 608);
    assert_eq!(ctx.endpoints.watches.capacity(), 4);
    assert_eq!(
        ctx.source_witness_storage_bytes(),
        608 + 4 * size_of::<Watch>()
    );
}

#[test]
fn forty_one_rows_reflow_to_four_thousand_one_hundred_with_constant_witness() {
    let mut term = Terminal::new(200, 50, 100_000, 128 * 1024 * 1024);
    let mut store = Store::new(&term);
    let start = mark(&mut term, &mut store);
    term.advance(&vec![b'A'; 8200]);
    let end = mark(&mut term, &mut store);
    let source = witness(&mut term, &mut store, &start, &end);
    proof(&mut term, &mut store, &source, 41);
    term.resize(2, 50);
    proof(&mut term, &mut store, &source, 4100);
}

/// Whole and varying-length hard lines use the real VT owner, not manufactured versions.
fn output(term: &mut Terminal, variable: bool) {
    for index in 0..20_000 {
        let length = if variable {
            1 + (73 * index % 120)
        } else {
            120
        };
        term.advance(&vec![b'A'; length]);
        if index != 19_999 {
            term.advance(b"\r\n");
        }
    }
}

#[test]
fn twenty_thousand_full_width_and_varying_lines_stay_constant_after_reflow() {
    for variable in [false, true] {
        let mut term = Terminal::new(120, 50, 100_000, 128 * 1024 * 1024);
        let mut store = Store::new(&term);
        let start = mark(&mut term, &mut store);
        output(&mut term, variable);
        let end = mark(&mut term, &mut store);
        let source = witness(&mut term, &mut store, &start, &end);
        proof(&mut term, &mut store, &source, 20_000);
        term.resize(40, 50);
        proof(
            &mut term,
            &mut store,
            &source,
            if variable { 39_999 } else { 60_000 },
        );
    }
}

#[test]
fn scattered_overwrites_do_not_expand_the_representation() {
    let mut term = Terminal::new(120, 20_000, 100_000, 128 * 1024 * 1024);
    let mut store = Store::new(&term);
    let start = mark(&mut term, &mut store);
    output(&mut term, false);
    for row in (0..20_000).step_by(100) {
        term.advance(format!("\x1b[{};1HX", row + 1).as_bytes());
    }
    term.advance(b"\x1b[20000;120H");
    term.cursor.pending_wrap = true;
    let end = mark(&mut term, &mut store);
    let source = witness(&mut term, &mut store, &start, &end);
    proof(&mut term, &mut store, &source, 20_000);
}

#[test]
fn named_release_does_not_release_record_source_and_foreign_owners_are_refused() {
    let mut term = Terminal::new(12, 4, 64, 64 * 1024);
    let mut store = Store::new(&term);
    let mut other = Store::new(&term);
    let start = mark(&mut term, &mut store);
    term.advance(b"visible");
    let end = mark(&mut term, &mut store);
    let source = witness(&mut term, &mut store, &start, &end);
    context(&mut term, &mut store).release_endpoint(&start);
    context(&mut term, &mut store).release_endpoint(&end);
    assert!(
        context(&mut term, &mut store)
            .query_source_witness(&source)
            .is_some()
    );
    assert!(
        context(&mut term, &mut other)
            .query_source_witness(&source)
            .is_none()
    );
    assert!(context(&mut term, &mut store).release_source_witness(source));
    assert_eq!(term.source_watch_storage_bytes(), 0);
}

#[test]
fn bounded_preparation_and_source_verification_never_hide_an_unbounded_visit() {
    let mut term = Terminal::new(120, 50, 100_000, 128 * 1024 * 1024);
    let mut store = Store::new(&term);
    let start = mark(&mut term, &mut store);
    output(&mut term, false);
    let end = mark(&mut term, &mut store);
    term.resize(40, 50);
    let mut ctx = context(&mut term, &mut store);
    let mut scan = ctx
        .begin_source_scan(&start, &end)
        .expect("retained fixture source");
    let mut turns = Vec::new();
    let mut seen = 0;
    loop {
        let mut credit = 4096;
        let ready = ctx
            .prepare_source_witness(&mut scan, &mut credit)
            .expect("retained fixture source");
        let inspected = if ready == ScanProgress::Complete {
            ctx.visit_source_rows(&mut scan, &mut credit, |_| {
                seen += 1;
                true
            })
            .expect("retained fixture source")
        } else {
            ScanProgress::Pending
        };
        turns.push(4096 - credit);
        if inspected == ScanProgress::Complete {
            break;
        }
    }
    assert_eq!(seen, 60_000);
    assert_eq!(turns.iter().sum::<usize>(), 120_000);
    assert_eq!(turns.len(), 30);
    assert!(turns.iter().all(|visits| *visits <= 4096));
    let source = ctx
        .capture_source_witness(&scan, 256)
        .expect("retained fixture source");
    let allocations = allocation_counter::measure(|| {
        for _ in 0..100 {
            assert!(
                !ctx.query_source_witness(&source)
                    .expect("retained fixture source")
                    .changed
            );
        }
    });
    assert_eq!(allocations.count_total, 0);
}

#[test]
fn live_relations_use_source_order_and_half_open_gaps() {
    use puzzle_vt::RangeRelation;
    let mut term = Terminal::new(20, 4, 64, 64 * 1024);
    let line = term.screen().row(0).expect("live range fixture").id;
    term.advance(b"abcdefghijklmnop");
    let a = term
        .watch_source((line, 2), (line, 10), 6)
        .expect("live range fixture");
    let equal = term
        .watch_source((line, 2), (line, 10), 6)
        .expect("live range fixture");
    let inner = term
        .watch_source((line, 4), (line, 8), 6)
        .expect("live range fixture");
    let crossing = term
        .watch_source((line, 8), (line, 12), 6)
        .expect("live range fixture");
    let touching = term
        .watch_source((line, 10), (line, 14), 6)
        .expect("live range fixture");
    assert_eq!(
        term.source_range_relation(a, equal),
        Some(RangeRelation::Equal)
    );
    assert_eq!(
        term.source_range_relation(a, inner),
        Some(RangeRelation::Contains)
    );
    assert_eq!(
        term.source_range_relation(inner, a),
        Some(RangeRelation::Within)
    );
    assert_eq!(
        term.source_range_relation(a, crossing),
        Some(RangeRelation::Crossing)
    );
    assert_eq!(
        term.source_range_relation(a, touching),
        Some(RangeRelation::Disjoint)
    );
    // Content-preserving resize changes physical rows, not containment.
    term.resize(6, 4);
    assert_eq!(
        term.source_range_relation(a, inner),
        Some(RangeRelation::Contains)
    );
    assert_eq!(
        term.source_range_relation(a, touching),
        Some(RangeRelation::Disjoint)
    );
    term.advance(b"\x1b[1;3Hc");
    assert_eq!(term.source_range_relation(a, inner), None);
}

#[test]
fn containment_does_not_sort_reordered_line_identities() {
    use puzzle_vt::RangeRelation;
    let mut term = Terminal::new(20, 4, 64, 64 * 1024);
    term.advance(b"first\r\nsecond\r\nthird\r\nfourth\x1b[2;4r\x1b[2;1H\x1b[L");
    let newer = term.screen().row(1).expect("inserted row").id;
    let older = term.screen().row(2).expect("moved row").id;
    assert!(
        newer > older,
        "fixture must contradict numeric source order"
    );
    let outer = term.watch_source((newer, 2), (older, 8), 2).expect("outer");
    let inner = term.watch_source((newer, 4), (older, 6), 2).expect("inner");
    assert_eq!(
        term.source_range_relation(outer, inner),
        Some(RangeRelation::Contains)
    );
    assert_eq!(
        term.source_range_relation(inner, outer),
        Some(RangeRelation::Within)
    );
}

#[test]
fn source_span_clips_half_open_boundaries_without_history_scans() {
    let mut term = Terminal::new(12, 4, 64, 64 * 1024);
    let mut store = Store::new(&term);
    term.advance(b"before");
    let start = mark(&mut term, &mut store);
    term.advance(b"\r\none\r\ntwo\r\nthree");
    let end = mark(&mut term, &mut store);
    let source = witness(&mut term, &mut store, &start, &end);
    let ctx = context(&mut term, &mut store);
    let clipped = ctx
        .source_span(&source, crate::osc::SourceWindow { top: 1, rows: 2 })
        .expect("visible span");
    assert_eq!(clipped.rows, (1, 3));
    assert_eq!(clipped.start.1, 0);
    assert_eq!(clipped.end.1, 12);
    assert_eq!(
        clipped.start.0,
        ctx.terminal.screen().ring.retained(1).expect("row").id
    );
    assert_eq!(
        clipped.end.0,
        ctx.terminal.screen().ring.retained(2).expect("row").id
    );
    assert!(
        ctx.source_span(
            &source,
            crate::osc::SourceWindow {
                top: usize::MAX,
                rows: usize::MAX
            }
        )
        .is_none()
    );
    assert!(
        ctx.source_span(&source, crate::osc::SourceWindow { top: 0, rows: 0 })
            .is_none()
    );
    ctx.terminal.advance(b"\x1b[1;7HX");
    assert!(ctx.source_witness_order(&source).is_none());
}
