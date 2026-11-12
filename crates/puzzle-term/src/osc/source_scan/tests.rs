//! Bounded source segments, charged fallback visits and cancellation guarantees.
#![allow(
    clippy::expect_used,
    reason = "Fixture capture/continuation failures are regression assertions."
)]
use super::*;
use crate::osc::anchors::Store;

/// Borrow a real registration's private source namespace for one owner operation.
fn context<'a>(terminal: &'a mut Terminal, endpoints: &'a mut Store) -> OscContext<'a> {
    OscContext {
        replies: None,
        terminal,
        endpoints,
    }
}
/// Capture an ordered current gap; only trusted fixture code selects the cursor.
fn capture(term: &mut Terminal, store: &mut Store) -> EndpointReceipt {
    context(term, store).capture_endpoint(512).expect("capture")
}

#[test]
fn half_open_columns_and_every_physical_visit_survive_turn_boundaries() {
    let mut term = Terminal::new(12, 4, 64, 64 * 1024);
    let mut store = Store::new(&term);
    term.advance(b"ab");
    let start = capture(&mut term, &mut store);
    term.advance(b"cdefghijklmn");
    let end = capture(&mut term, &mut store);
    let source = context(&mut term, &mut store);
    let mut scan = source.begin_source_scan(&start, &end).expect("begin");
    let mut seen = Vec::new();
    for expected in [ScanProgress::Pending, ScanProgress::Complete] {
        let mut visits = 1;
        let result = source.visit_source_rows(&mut scan, &mut visits, |part| {
            seen.push((part.row.id, part.columns));
            true
        });
        assert_eq!(result, Ok(expected));
        assert_eq!(visits, 0);
    }
    assert_eq!(seen, [(LineId(0), 2..12), (LineId(1), 0..2)]);
    let mut visits = 5;
    assert_eq!(
        source.visit_source_rows(&mut scan, &mut visits, |_| panic!("no replay")),
        Ok(ScanProgress::Complete)
    );
    assert_eq!(visits, 5);
    assert_eq!(scan.charged_bytes(), size_of::<SourceScan>());
}

#[test]
fn long_reflow_capture_retains_bounded_physical_visits_with_constant_witness() {
    let mut term = Terminal::new(200, 50, 10_000, 32 * 1024 * 1024);
    let mut store = Store::new(&term);
    let start = capture(&mut term, &mut store);
    term.advance(&vec![b'A'; 8200]);
    let end = capture(&mut term, &mut store);
    term.resize(2, 50);
    let mut source = context(&mut term, &mut store);
    let mut scan = source
        .begin_source_scan(&start, &end)
        .expect("begin after transform");
    while source
        .prepare_source_witness(&mut scan, &mut 4096)
        .expect("prepare")
        != ScanProgress::Complete
    {}

    let mut visited = 0;
    let mut visit = |_part: SourceRow<'_>| {
        visited += 1;
        true
    };
    let mut allowance = 4096;
    assert_eq!(
        source.visit_source_rows(&mut scan, &mut allowance, &mut visit),
        Ok(ScanProgress::Pending)
    );
    assert_eq!(allowance, 0);
    allowance = 4096;
    assert_eq!(
        source.visit_source_rows(&mut scan, &mut allowance, &mut visit),
        Ok(ScanProgress::Complete)
    );
    assert_eq!(allowance, 4092);
    assert_eq!(visited, 4100);
    let witness = source
        .capture_source_witness(&scan, 256)
        .expect("constant witness");
    assert_eq!(witness.entries(), 3);
    assert_eq!(
        witness.charged_bytes(),
        size_of::<crate::osc::SourceWitness>() + Terminal::source_watch_entry_bytes()
    );
}

#[test]
fn region_reordering_has_one_verified_guess_then_counted_linear_fallback() {
    let mut term = Terminal::new(12, 4, 64, 64 * 1024);
    let mut store = Store::new(&term);
    term.advance(b"aa\r\nbb\r\ncc\r\ndd\x1b[2;1H");
    let start = capture(&mut term, &mut store);
    term.advance(b"\x1b[4;3H");
    let end = capture(&mut term, &mut store);
    term.advance(b"\x1b[2;3r\x1b[T");
    let source = context(&mut term, &mut store);
    let mut scan = source
        .begin_source_scan(&start, &end)
        .expect("retained endpoints");
    let mut rows = Vec::new();
    let mut total_visits = 0;
    loop {
        let mut allowance = 1;
        let state = source
            .visit_source_rows(&mut scan, &mut allowance, |part| {
                rows.push(part.row.id);
                true
            })
            .expect("bounded source order");
        total_visits += 1 - allowance;
        if state == ScanProgress::Complete {
            break;
        }
    }
    assert_eq!(rows, [LineId(1), LineId(3)]);
    assert_eq!(total_visits, 5);
}

#[test]
fn zero_credit_and_visitor_refusal_cannot_authorize_partial_source() {
    let mut term = Terminal::new(12, 4, 64, 64 * 1024);
    let mut store = Store::new(&term);
    let start = capture(&mut term, &mut store);
    term.advance(b"text");
    let end = capture(&mut term, &mut store);
    let source = context(&mut term, &mut store);
    let mut scan = source.begin_source_scan(&start, &end).expect("begin");
    assert_eq!(
        source.visit_source_rows(&mut scan, &mut 0, |_| panic!("no credit")),
        Ok(ScanProgress::Pending)
    );
    let mut credit = 3;
    assert_eq!(
        source.visit_source_rows(&mut scan, &mut credit, |_| false),
        Err(SourceReadError::Rejected)
    );
    assert_eq!(credit, 2);
    assert_eq!(
        source.visit_source_rows(&mut scan, &mut credit, |_| panic!("failed capture")),
        Err(SourceReadError::Changed)
    );
    assert_eq!(credit, 2);
}

#[test]
fn foreign_released_and_empty_endpoints_are_refused_before_inspection() {
    let mut term = Terminal::new(12, 4, 64, 64 * 1024);
    let mut store = Store::new(&term);
    let start = capture(&mut term, &mut store);
    assert!(matches!(
        context(&mut term, &mut store).begin_source_scan(&start, &start),
        Err(SourceReadError::Invalid)
    ));
    term.advance(b"text");
    let end = capture(&mut term, &mut store);
    let mut other = Store::new(&term);
    assert!(matches!(
        context(&mut term, &mut other).begin_source_scan(&start, &end),
        Err(SourceReadError::Gone)
    ));
    let mut source = context(&mut term, &mut store);
    assert!(source.release_endpoint(&start));
    assert!(matches!(
        source.begin_source_scan(&start, &end),
        Err(SourceReadError::Gone)
    ));
}

#[test]
fn reflow_or_clear_between_turns_cancels_without_more_visits() {
    for clear in [false, true] {
        let mut term = Terminal::new(12, 4, 64, 64 * 1024);
        let mut store = Store::new(&term);
        let start = capture(&mut term, &mut store);
        term.advance(b"abcdefghijklmnop");
        let end = capture(&mut term, &mut store);
        let mut scan = context(&mut term, &mut store)
            .begin_source_scan(&start, &end)
            .expect("begin");
        assert_eq!(
            context(&mut term, &mut store).visit_source_rows(&mut scan, &mut 1, |_| true),
            Ok(ScanProgress::Pending)
        );
        if clear {
            term.advance(b"\x1b[2J");
            store.sync(&mut term);
        } else {
            term.resize(5, 4);
        }
        let mut credit = 4096;
        assert_eq!(
            context(&mut term, &mut store)
                .visit_source_rows(&mut scan, &mut credit, |_| panic!("stale source")),
            Err(SourceReadError::Changed)
        );
        assert_eq!(credit, 4096);
    }
}

#[test]
fn repeated_source_scans_and_visits_allocate_nothing_after_receipts_exist() {
    let mut term = Terminal::new(12, 4, 64, 64 * 1024);
    let mut store = Store::new(&term);
    let start = capture(&mut term, &mut store);
    term.advance(b"visible text");
    let end = capture(&mut term, &mut store);
    let source = context(&mut term, &mut store);
    let measured = allocation_counter::measure(|| {
        for _ in 0..100 {
            let mut scan = source.begin_source_scan(&start, &end).expect("begin");
            assert_eq!(
                source.visit_source_rows(&mut scan, &mut 4096, |_| true),
                Ok(ScanProgress::Complete)
            );
        }
    });
    assert_eq!(measured.count_total, 0);
}

#[test]
fn trailing_hard_line_gap_consumes_a_visit_but_exports_no_end_row_cells() {
    let mut term = Terminal::new(12, 4, 64, 64 * 1024);
    let mut store = Store::new(&term);
    let start = capture(&mut term, &mut store);
    term.advance(b"text\r\n");
    let end = capture(&mut term, &mut store);
    let source = context(&mut term, &mut store);
    let mut scan = source.begin_source_scan(&start, &end).expect("begin");
    let mut visits = 4096;
    let mut rows = Vec::new();
    assert_eq!(
        source.visit_source_rows(&mut scan, &mut visits, |part| {
            rows.push((part.row.id, part.columns));
            true
        }),
        Ok(ScanProgress::Complete)
    );
    assert_eq!(visits, 4094);
    assert_eq!(rows, [(LineId(0), 0..12)]);
}

#[test]
fn reversed_source_order_never_completes_or_commits_a_partial_capture() {
    let mut term = Terminal::new(12, 4, 64, 64 * 1024);
    let mut store = Store::new(&term);
    term.advance(b"first\r\nlast");
    let start = capture(&mut term, &mut store);
    term.advance(b"\x1b[1;1H");
    let end = capture(&mut term, &mut store);
    let source = context(&mut term, &mut store);
    let mut scan = source
        .begin_source_scan(&start, &end)
        .expect("different rows require traversal");
    let mut visits = 4096;
    assert_eq!(
        source.visit_source_rows(&mut scan, &mut visits, |_| true),
        Err(SourceReadError::Invalid)
    );
    // A consumer commits only on Complete; a second attempt remains inert.
    assert_eq!(
        source.visit_source_rows(&mut scan, &mut visits, |_| panic!("failed scan")),
        Err(SourceReadError::Changed)
    );
}
