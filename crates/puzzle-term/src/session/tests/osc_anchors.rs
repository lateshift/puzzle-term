//! M19.2 source-order, owner isolation, bounded ownership and unused-path proof.
use super::*;
use crate::osc::{Endpoint, EndpointError, EndpointReceipt, OscContext, OscHandler, OscRequest};

/// Test observer stores only owned receipts and small query results.
#[derive(Default)]
struct Trace {
    /// Caller-held receipts survive clears only as inert values.
    receipts: Vec<EndpointReceipt>,
    /// Resolution happens in the real registered callback, never through Terminal access.
    queries: Vec<Option<Endpoint>>,
    /// Capture errors prove storage admission is atomic.
    errors: Vec<EndpointError>,
    /// Destructive source callbacks record the new epoch.
    epochs: Vec<u64>,
}

/// A source-sensitive plugin shares observations only with the test harness.
struct Anchors(Arc<Mutex<Trace>>);
impl OscHandler for Anchors {
    fn handle(&mut self, _: &OscRequest, _: &mut Vec<u8>, _: &mut Bus<Command, Event>) {
        panic!("source-sensitive callback must receive its context");
    }
    fn handle_with_context(
        &mut self,
        request: &OscRequest,
        source: &mut OscContext<'_>,
        replies: &mut Vec<u8>,
        _: &mut Bus<Command, Event>,
    ) {
        let mut trace = self.0.lock().expect("trace");
        match request.payload.as_slice() {
            b"mark" => match source.capture_endpoint(2) {
                Ok(receipt) => trace.receipts.push(receipt),
                Err(error) => trace.errors.push(error),
            },
            b"query" => {
                let result = source.query_endpoint(trace.receipts.last().expect("receipt"));
                trace.queries.push(result);
            }
            b"release" => {
                let receipt = trace.receipts.last().expect("receipt");
                assert!(source.release_endpoint(receipt));
                assert!(!source.release_endpoint(receipt));
                assert!(source.query_endpoint(receipt).is_none());
            }
            _ => panic!("fixture operation"),
        }
        replies.extend_from_slice(b"receipt");
    }
    fn on_source_changed(&mut self, source: &mut OscContext<'_>, _: &mut Bus<Command, Event>) {
        self.0.lock().expect("trace").epochs.push(source.epoch());
    }
}

/// Register one optional owner through the production Session surface.
fn registered() -> (TestHost, Arc<Mutex<Trace>>) {
    let (mut session, _, _) = fixture();
    let trace = Arc::new(Mutex::new(Trace::default()));
    session
        .register_osc_handler(7779, Box::new(Anchors(trace.clone())))
        .expect("register");
    (session, trace)
}

#[test]
fn endpoint_gap_is_captured_before_following_text_at_every_wire_split() {
    let bytes = b"abc\x1b]7779;mark\x1b\\DEF\x1b]7779;query\x1b\\";
    for split in 0..=bytes.len() {
        let (mut session, trace) = registered();
        session.feed(&bytes[..split], Instant::now());
        session.feed(&bytes[split..], Instant::now());
        let trace = trace.lock().expect("trace");
        assert_eq!(trace.queries[0].expect("gap").column, 3);
        assert_eq!(session.terminal.cursor.column, 6);
    }
}

#[test]
fn pending_wrap_gap_survives_narrow_wide_reflow_without_mutating_cursor() {
    let (mut session, trace) = registered();
    session.terminal.resize(8, 4);
    session.feed(b"abcdefgh\x1b]7779;mark\x1b\\", Instant::now());
    assert!(session.terminal.cursor.pending_wrap);
    assert_eq!(session.terminal.cursor.column, 7);
    session.terminal.resize(4, 4);
    session.feed(b"\x1b]7779;query\x1b\\", Instant::now());
    assert_eq!(
        trace
            .lock()
            .expect("trace")
            .queries
            .last()
            .expect("query")
            .expect("gap")
            .column,
        4
    );
    session.terminal.resize(8, 4);
    session.feed(b"\x1b]7779;query\x1b\\", Instant::now());
    assert_eq!(
        trace
            .lock()
            .expect("trace")
            .queries
            .last()
            .expect("query")
            .expect("gap")
            .column,
        8
    );
}

#[test]
fn endpoints_are_bounded_released_and_isolated_across_handlers_and_sessions() {
    let (mut first, trace) = registered();
    first.feed(
        b"\x1b]7779;mark\x1b\\\x1b]7779;mark\x1b\\\x1b]7779;mark\x1b\\",
        Instant::now(),
    );
    assert_eq!(trace.lock().expect("trace").errors, [EndpointError::Limit]);
    // The same observation shares a receipt across owner boundaries deliberately.
    first
        .register_osc_handler(7780, Box::new(Anchors(trace.clone())))
        .expect("sibling");
    first.feed(b"\x1b]7780;query\x1b\\", Instant::now());
    let (mut second, _, _) = fixture();
    second
        .register_osc_handler(7779, Box::new(Anchors(trace.clone())))
        .expect("other Session");
    second.feed(b"\x1b]7779;query\x1b\\", Instant::now());
    assert_eq!(trace.lock().expect("trace").queries, [None, None]);
    first.feed(
        b"\x1b]7779;release\x1b\\\x1b]7779;mark\x1b\\",
        Instant::now(),
    );
    assert_eq!(trace.lock().expect("trace").receipts.len(), 3);
}

#[test]
fn destructive_source_transitions_and_exit_retire_receipts() {
    for clear in [
        b"\x1b[2J".as_slice(),
        b"\x1b[3J",
        b"\x1b#8",
        b"\x1b[?1049h\x1b[?1049l",
        b"\x1bc",
    ] {
        let (mut session, trace) = registered();
        session.feed(b"abc\x1b]7779;mark\x1b\\", Instant::now());
        session.feed(clear, Instant::now());
        session.feed(b"\x1b]7779;query\x1b\\", Instant::now());
        let trace = trace.lock().expect("trace");
        assert_eq!(trace.queries, [None], "{clear:?}");
        assert_eq!(trace.epochs, [2], "{clear:?}");
    }
    let (mut session, trace) = registered();
    session.feed(b"abc\x1b]7779;mark\x1b\\", Instant::now());
    session.submit(
        Command::ChildExited(ExitInfo {
            code: Some(0),
            signal: None,
            runtime_ms: 1000,
        }),
        Origin::Platform,
    );
    session.pump(Instant::now());
    // Held final text cannot keep endpoint references alive after session end.
    assert!(session.osc.endpoint_count() == 0);
    assert_eq!(trace.lock().expect("trace").receipts.len(), 1);
}

#[test]
fn no_handler_preserves_operation_counts_and_warm_zero_allocations() {
    let (mut session, _, _) = fixture();
    let mut direct = puzzle_vt::Terminal::new(
        session.terminal.size().0,
        session.terminal.size().1,
        10_000,
        32 * 1024 * 1024,
    );
    // Compare actual VT operation counts, not wall time or a surrogate fast path.
    let corpus = b"hello\r\n\x1b[2;4r\x1b[4;1Hworld\r\n\x1b[2K\x1b[r";
    session.feed(corpus, Instant::now());
    direct.advance(corpus);
    session.terminal.start_operation_metrics();
    direct.start_operation_metrics();
    session.feed(corpus, Instant::now());
    direct.advance(corpus);
    assert_eq!(
        session.terminal.operation_metrics(),
        direct.operation_metrics()
    );
    let measured = allocation_counter::measure(|| {
        session.feed(corpus, Instant::now());
    });
    assert_eq!(measured.count_total, 0);
    assert_eq!(session.osc.endpoint_count(), 0);
}

#[test]
fn scrolling_preserves_retained_gaps_and_eviction_does_not_resurrect_them() {
    let (mut session, trace) = registered();
    session.terminal = puzzle_vt::Terminal::new(8, 2, 1, 1024 * 1024);
    session.feed(
        b"wide\x1b]7779;mark\x1b\\\r\nx\r\ny\x1b]7779;query\x1b\\",
        Instant::now(),
    );
    assert_eq!(
        trace.lock().expect("trace").queries[0]
            .expect("history gap")
            .column,
        4
    );
    session.feed(b"\r\nz\x1b]7779;query\x1b\\", Instant::now());
    assert!(trace.lock().expect("trace").queries[1].is_none());
    session.terminal.resize(16, 4);
    session.feed(b"\x1b]7779;query\x1b\\", Instant::now());
    assert!(trace.lock().expect("trace").queries[2].is_none());
}

#[test]
fn soft_reset_preserves_gaps_but_host_clear_invalidates_them() {
    let (mut session, trace) = registered();
    session.feed(
        "一\x1b]7779;mark\x1b\\\x1b[!p\x1b]7779;query\x1b\\".as_bytes(),
        Instant::now(),
    );
    assert_eq!(
        trace.lock().expect("trace").queries[0]
            .expect("wide gap")
            .column,
        2
    );
    session.terminal.clear_history();
    session.feed(b"\x1b]7779;query\x1b\\", Instant::now());
    assert!(trace.lock().expect("trace").queries[1].is_none());
}
