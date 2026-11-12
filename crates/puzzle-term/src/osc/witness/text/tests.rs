//! Real source witnesses exercise bounded preflight, logical copy and cancellation.
//! No test installs authority from coordinates supplied by a program.
#![allow(
    clippy::expect_used,
    reason = "Fixtures assert that live source ownership exists."
)]
use super::*;
use crate::osc::{EndpointReceipt, anchors::Store};
use puzzle_vt::Terminal;

/// The fixture is the same exclusive owner borrow supplied to registered handlers.
fn context<'a>(term: &'a mut Terminal, store: &'a mut Store) -> OscContext<'a> {
    OscContext {
        terminal: term,
        endpoints: store,
        replies: None,
    }
}

/// A mark captures a current cursor gap without modifying output.
fn mark(term: &mut Terminal, store: &mut Store) -> EndpointReceipt {
    context(term, store)
        .capture_endpoint(512)
        .expect("fixture mark")
}

/// Completed source inspection installs an independent retained record watch.
fn witness(
    term: &mut Terminal,
    store: &mut Store,
    start: &EndpointReceipt,
    end: &EndpointReceipt,
) -> SourceWitness {
    let mut ctx = context(term, store);
    let mut scan = ctx.begin_source_scan(start, end).expect("fixture scan");
    loop {
        let mut credit = 4096;
        if ctx
            .prepare_source_witness(&mut scan, &mut credit)
            .expect("metadata")
            == ScanProgress::Complete
            && ctx
                .visit_source_rows(&mut scan, &mut credit, |row| {
                    row.contains_complete_units(&ctx)
                })
                .expect("source")
                == ScanProgress::Complete
        {
            break;
        }
    }
    ctx.capture_source_witness(&scan, 256)
        .expect("fixture witness")
}

/// Tiny turns force resumption even within an ordinary full-width row.
fn copy(
    ctx: &OscContext<'_>,
    source: &SourceWitness,
    trim: bool,
    limit: usize,
) -> Result<String, SourceReadError> {
    let mut scan = ctx.begin_source_text(source, trim, limit)?;
    for _ in 0..100_000 {
        let mut work = 3;
        if ctx.advance_source_text(source, &mut scan, &mut work)? == ScanProgress::Complete {
            return scan.take_text().ok_or(SourceReadError::Changed);
        }
        assert!(scan.take_text().is_none(), "no partial export");
    }
    panic!("source copy failed to make finite progress");
}

#[test]
fn source_text_matches_contents_after_mark_release_at_both_trim_policies() {
    for trim in [false, true] {
        let mut term = Terminal::new(12, 6, 64, 64 * 1024);
        let mut store = Store::new(&term);
        term.advance(b"prefix");
        let start = mark(&mut term, &mut store);
        term.advance("A一e\u{301}  wrapped continuation\r\nhard   \r\n".as_bytes());
        let end = mark(&mut term, &mut store);
        let expected = {
            let ctx = context(&mut term, &mut store);
            let a = ctx.query_endpoint(&start).expect("start");
            let b = ctx.query_endpoint(&end).expect("end");
            ctx.terminal
                .buffer()
                .text_span_options((a.line, a.column), (b.line, b.column), trim)
        };
        let source = witness(&mut term, &mut store, &start, &end);
        let mut ctx = context(&mut term, &mut store);
        assert!(ctx.release_endpoint(&start));
        assert!(ctx.release_endpoint(&end));
        assert_eq!(copy(&ctx, &source, trim, 65_536).expect("copy"), expected);
    }
}

#[test]
fn source_text_counts_before_allocating_and_refuses_whole_oversize() {
    let mut term = Terminal::new(282, 3, 64, 64 * 1024);
    let mut store = Store::new(&term);
    let start = mark(&mut term, &mut store);
    term.advance(&vec![b'a'; 600]);
    let end = mark(&mut term, &mut store);
    let source = witness(&mut term, &mut store, &start, &end);
    let ctx = context(&mut term, &mut store);
    let mut scan = ctx
        .begin_source_text(&source, true, 599)
        .expect("bounded scan");
    let allocations = allocation_counter::measure(|| {
        loop {
            let mut credit = 17;
            match ctx.advance_source_text(&source, &mut scan, &mut credit) {
                Ok(ScanProgress::Pending) => {
                    assert_eq!(scan.charged_bytes(), size_of::<SourceTextScan>())
                }
                Err(SourceReadError::Rejected) => break,
                other => panic!("unexpected preflight result: {other:?}"),
            }
        }
    });
    assert_eq!(allocations.count_total, 0);
    assert!(scan.take_text().is_none());
    assert_eq!(
        copy(&ctx, &source, true, 600).expect("exact fit"),
        "a".repeat(600)
    );
}

#[test]
fn source_text_yields_inside_wide_rows_and_source_changes_cancel_completed_text() {
    let mut term = Terminal::new(282, 3, 64, 64 * 1024);
    let mut store = Store::new(&term);
    let start = mark(&mut term, &mut store);
    term.advance(b"abc");
    let end = mark(&mut term, &mut store);
    let source = witness(&mut term, &mut store, &start, &end);
    let mut scan = context(&mut term, &mut store)
        .begin_source_text(&source, true, 3)
        .expect("scan");
    let mut credit = 1;
    assert_eq!(
        context(&mut term, &mut store).advance_source_text(&source, &mut scan, &mut credit),
        Ok(ScanProgress::Pending)
    );
    assert_eq!(credit, 0);
    // Continuing on the same row beyond the captured end is not a source rewrite.
    term.advance(b"outside");
    loop {
        let mut credit = 2;
        if context(&mut term, &mut store)
            .advance_source_text(&source, &mut scan, &mut credit)
            .expect("outside write preserves source")
            == ScanProgress::Complete
        {
            break;
        }
    }
    term.advance(b"\x1b[1;2Hb");
    let mut credit = 4096;
    assert_eq!(
        context(&mut term, &mut store).advance_source_text(&source, &mut scan, &mut credit),
        Err(SourceReadError::Changed)
    );
    assert!(scan.take_text().is_none());
}

#[test]
fn source_text_scaled_rectangles_match_copy_without_lower_row_padding() {
    for trim in [false, true] {
        let mut term = Terminal::new(20, 8, 64, 64 * 1024);
        let mut store = Store::new(&term);
        let start = mark(&mut term, &mut store);
        term.advance("\x1b]66;s=2;A一é\x1b\\\r\n\r\nordinary\r\n".as_bytes());
        let end = mark(&mut term, &mut store);
        let a = context(&mut term, &mut store)
            .query_endpoint(&start)
            .expect("start");
        let b = context(&mut term, &mut store)
            .query_endpoint(&end)
            .expect("end");
        let expected =
            term.buffer()
                .text_span_options((a.line, a.column), (b.line, b.column), trim);
        let source = witness(&mut term, &mut store, &start, &end);
        assert_eq!(
            copy(&context(&mut term, &mut store), &source, trim, 65_536).expect("scaled copy"),
            expected
        );
    }
}

#[test]
fn source_text_uses_retained_order_and_refuses_foreign_or_reflowed_source() {
    let mut term = Terminal::new(12, 4, 64, 64 * 1024);
    term.advance(b"one\r\ntwo\r\nthree\r\nfour\x1b[2;4r\x1b[2;1H\x1b[L");
    let mut store = Store::new(&term);
    let start = mark(&mut term, &mut store);
    term.advance(b"new\x1b[3;4H");
    let end = mark(&mut term, &mut store);
    let source = witness(&mut term, &mut store, &start, &end);
    assert_eq!(
        copy(&context(&mut term, &mut store), &source, true, 64).expect("source order"),
        "new\ntwo"
    );
    let mut foreign = Store::new(&term);
    assert!(matches!(
        context(&mut term, &mut foreign).begin_source_text(&source, true, 64),
        Err(SourceReadError::Gone)
    ));
    let mut scan = context(&mut term, &mut store)
        .begin_source_text(&source, true, 64)
        .expect("source");
    term.resize(5, 4);
    assert_eq!(
        context(&mut term, &mut store).advance_source_text(&source, &mut scan, &mut 64),
        Err(SourceReadError::Changed)
    );
}

#[test]
fn source_text_completed_export_moves_once_and_partial_cancel_never_escapes() {
    let mut term = Terminal::new(12, 4, 64, 64 * 1024);
    let mut store = Store::new(&term);
    let start = mark(&mut term, &mut store);
    term.advance(b"abc");
    let end = mark(&mut term, &mut store);
    let source = witness(&mut term, &mut store, &start, &end);
    let mut scan = context(&mut term, &mut store)
        .begin_source_text(&source, true, 3)
        .expect("copy");
    assert_eq!(
        context(&mut term, &mut store).advance_source_text(&source, &mut scan, &mut 64),
        Ok(ScanProgress::Complete)
    );
    assert_eq!(scan.take_text().as_deref(), Some("abc"));
    assert!(scan.take_text().is_none());
    let mut pending = context(&mut term, &mut store)
        .begin_source_text(&source, true, 3)
        .expect("copy");
    assert_eq!(
        context(&mut term, &mut store).advance_source_text(&source, &mut pending, &mut 5),
        Ok(ScanProgress::Pending)
    );
    term.advance(b"\x1b[1;1Ha");
    assert_eq!(
        context(&mut term, &mut store).advance_source_text(&source, &mut pending, &mut 64),
        Err(SourceReadError::Changed)
    );
    assert!(pending.take_text().is_none());
}
