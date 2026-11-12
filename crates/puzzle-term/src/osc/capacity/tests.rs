//! Source growth refusal preserves retained bytes and previously issued handles.
#![allow(
    clippy::expect_used,
    reason = "source admission is the behavior under test"
)]
use super::*;
use crate::osc::{ScanProgress, anchors::Store};
use puzzle_vt::Terminal;

#[test]
fn refusal_does_not_grow_any_source_owner_or_consume_receipts() {
    let mut terminal = Terminal::new(12, 4, 64, 64 * 1024);
    let mut endpoints = Store::new(&terminal);
    let mut source = OscContext {
        replies: None,
        terminal: &mut terminal,
        endpoints: &mut endpoints,
    };
    assert_eq!(source.source_storage_bytes(), 0);
    let refused = allocation_counter::measure(|| {
        assert_eq!(
            source.reserve_source_storage(1, 0, 0),
            Err(EndpointError::Limit)
        );
    });
    assert_eq!(refused.count_total, 0);
    assert_eq!(source.source_storage_bytes(), 0);
    source
        .reserve_source_storage(1, 0, 1024 * 1024)
        .expect("source fixture");
    let start = source.capture_endpoint(2).expect("source fixture");
    // Safe trusted API misuse must be refusal rather than integer overflow.
    assert_eq!(
        source.reserve_source_storage(usize::MAX, 1, usize::MAX),
        Err(EndpointError::Limit)
    );
    assert!(
        !source
            .terminal
            .reserve_private_source(usize::MAX, 0, usize::MAX)
    );
    let previous = source.query_endpoint(&start);
    let bytes = source.source_storage_bytes();
    assert_eq!(
        source.reserve_source_storage(1000, 1000, bytes),
        Err(EndpointError::Limit)
    );
    assert_eq!(source.source_storage_bytes(), bytes);
    assert_eq!(source.query_endpoint(&start), previous);
}

#[test]
fn admitted_capacity_covers_mark_and_watch_without_capture_allocations() {
    let mut terminal = Terminal::new(12, 4, 64, 64 * 1024);
    let mut endpoints = Store::new(&terminal);
    let mut source = OscContext {
        replies: None,
        terminal: &mut terminal,
        endpoints: &mut endpoints,
    };
    source
        .reserve_source_storage(2, 1, 1024 * 1024)
        .expect("source fixture");
    // First mark creates the explicitly charged immutable Arc identity.
    let start = source.capture_endpoint(2).expect("source fixture");
    source.terminal.advance(b"visible");
    let before = source.source_storage_bytes();
    let mut end = None;
    let measured = allocation_counter::measure(|| {
        end = Some(source.capture_endpoint(2).expect("source fixture"));
    });
    assert_eq!(measured.count_total, 0);
    let mut scan = source
        .begin_source_scan(&start, end.as_ref().expect("source fixture"))
        .expect("source fixture");
    let mut visits = 4096;
    assert_eq!(
        source
            .prepare_source_witness(&mut scan, &mut visits)
            .expect("source fixture"),
        ScanProgress::Complete
    );
    assert_eq!(
        source
            .visit_source_rows(&mut scan, &mut visits, |_| true)
            .expect("source fixture"),
        ScanProgress::Complete
    );
    let measured = allocation_counter::measure(|| {
        let witness = source
            .capture_source_witness(&scan, 1)
            .expect("source fixture");
        assert!(
            !source
                .query_source_witness(&witness)
                .expect("source fixture")
                .changed
        );
        source.release_source_witness(witness);
    });
    assert_eq!(measured.count_total, 0);
    // Retiring the final watch frees its watch allocation; no uncharged growth.
    assert!(source.source_storage_bytes() <= before);
}
