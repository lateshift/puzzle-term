//! Ordered optional-work continuation without threads, timers or extra queues.
//! The fixture spends one callback credit per host turn to force deferral; real
//! protocols choose their approved budgets. Input and VT keep production paths.
use super::*;
use crate::osc::{EndpointReceipt, OscContext, OscHandler, OscProgress, OscRequest};

/// Only observations leave the callback; source positions are captured by VT.
#[derive(Default)]
struct Trace {
    /// Actual terminal gaps prove following text never overtakes a deferred mark.
    columns: Vec<usize>,
    /// Original selectors distinguish numeric dispatch from authenticated bytes.
    selectors: Vec<Vec<u8>>,
    /// A quantum begins/ends exactly once, even for the borrowed synchronized API.
    turns: [usize; 2],
}

/// One-credit handler forces several wire boundaries into separate host turns.
struct Limited {
    /// Owner-local credit, replenished only by begin_turn.
    available: bool,
    /// Shared test observations are not a shared product Session.
    trace: Arc<Mutex<Trace>>,
    /// Receipts are retained until reset/exit, as a real mark-name table would do.
    receipts: Vec<EndpointReceipt>,
}

impl OscHandler for Limited {
    fn handle(&mut self, _: &OscRequest, _: &mut Vec<u8>, _: &mut Bus<Command, Event>) {
        panic!("source-aware fixture uses the resumable callback");
    }
    fn begin_turn(&mut self) {
        self.available = true;
        self.trace.lock().expect("trace").turns[0] += 1;
    }
    fn end_turn(&mut self) {
        self.trace.lock().expect("trace").turns[1] += 1;
    }
    fn try_handle_with_context(
        &mut self,
        request: &OscRequest,
        source: &mut OscContext<'_>,
        replies: &mut Vec<u8>,
        _: &mut Bus<Command, Event>,
    ) -> OscProgress {
        if !self.available {
            return OscProgress::Deferred;
        }
        self.available = false;
        let receipt = source.capture_endpoint(8).expect("fixture endpoint cap");
        let column = source.query_endpoint(&receipt).expect("live gap").column;
        let mut trace = self.trace.lock().expect("trace");
        trace.columns.push(column);
        trace.selectors.push(request.selector.clone());
        self.receipts.push(receipt);
        replies.extend_from_slice(&request.payload);
        OscProgress::Complete
    }
}

/// All tests enter through real Session registration and production parsing.
fn registered(images: bool) -> (TestHost, Writes, Arc<Mutex<Trace>>) {
    let (mut session, writes, _) = fixture();
    let trace = Arc::new(Mutex::new(Trace::default()));
    if images {
        session.enable_images(1024 * 1024);
    }
    session
        .register_osc_handler(
            7779,
            Box::new(Limited {
                available: false,
                trace: trace.clone(),
                receipts: Vec::new(),
            }),
        )
        .expect("registration");
    (session, writes, trace)
}

#[test]
fn deferred_marks_capture_before_following_text_and_queries() {
    for images in [false, true] {
        let (mut session, writes, trace) = registered(images);
        let bytes = b"A\x1b]7779;one\x1b\\B\x1b]7779;two\x1b\\C\x1b[5n";
        let first = session.feed(bytes, Instant::now());
        assert!(first.pending_bytes > 0);
        assert_eq!(session.terminal.cursor.column, 2);
        assert_eq!(trace.lock().expect("trace").columns, [1]);
        assert_eq!(writes.lock().expect("writes").concat(), b"one");
        // Controls are independent of the paused application stream (D-50).
        session.submit(Command::Text("key".into()), Origin::User);
        session.pump(Instant::now());
        assert_eq!(writes.lock().expect("writes").concat(), b"onekey");
        // A frame request seals the current prefix without consuming C or
        // replenishing this protocol's exhausted parsing credits.
        assert!(
            session
                .publish(Instant::now() + Duration::from_millis(20))
                .is_some()
        );
        let row = session.snapshot.retained_row(0).expect("published prefix");
        assert_eq!(row.cells[0].ch, u32::from('A'));
        assert_eq!(row.cells[1].ch, u32::from('B'));
        assert_eq!(row.cells[2].ch, 0);
        assert_eq!(trace.lock().expect("trace").turns, [1, 1]);
        let second = session.feed(&[], Instant::now());
        assert_eq!(second.pending_bytes, 0);
        assert_eq!(session.terminal.cursor.column, 3);
        assert_eq!(trace.lock().expect("trace").columns, [1, 2]);
        assert_eq!(writes.lock().expect("writes").concat(), b"onekeytwo\x1b[0n");
        assert_eq!(trace.lock().expect("trace").turns, [2, 2]);
    }
}

#[test]
fn final_deferred_request_is_counted_and_replied_to_at_eof() {
    let (mut session, writes, trace) = registered(false);
    let request = b"\x1b]7779;last\x1b\\";
    let mut bytes = b"\x1b]7779;first\x1b\\".to_vec();
    bytes.extend_from_slice(request);
    let first = session.feed(&bytes, Instant::now());
    assert_eq!(first.parsed_bytes, bytes.len());
    assert_eq!(first.pending_bytes, request.len());
    assert_eq!(session.pending_bytes(), request.len());
    assert_eq!(session.feed(&[], Instant::now()).pending_bytes, 0);
    assert_eq!(writes.lock().expect("writes").concat(), b"firstlast");
    assert_eq!(trace.lock().expect("trace").turns, [2, 2]);
}

#[test]
fn synchronized_borrowed_suffix_does_not_replenish_at_internal_drain() {
    let (mut session, writes, trace) = registered(false);
    session.feed(b"A\x1b]7779;one\x1b\\B\x1b]7779;two\x1b\\", Instant::now());
    let bytes = b"C\x1b]7779;three\x1b\\D\x1b[5n";
    let used = session.feed_to_sync_end(bytes, Instant::now());
    // Resuming two spent this turn's only credit. Three must remain ahead of D.
    assert_eq!(&bytes[used..], b"D\x1b[5n");
    assert_eq!(session.terminal.cursor.column, 3);
    assert_eq!(trace.lock().expect("trace").turns, [2, 2]);
    assert_eq!(writes.lock().expect("writes").concat(), b"onetwo");
    let tail = &bytes[used..];
    assert_eq!(session.feed_to_sync_end(tail, Instant::now()), tail.len());
    assert_eq!(
        writes.lock().expect("writes").concat(),
        b"onetwothree\x1b[0n"
    );
    assert_eq!(trace.lock().expect("trace").turns, [3, 3]);
}

#[test]
fn exact_selector_and_order_survive_every_transport_split() {
    let bytes = b"A\x1b]07779;one\x1b\\B\x1b]7779;two\x1b\\C\x1b]7779;three\x1b\\D";
    for split in 0..=bytes.len() {
        let (mut session, writes, trace) = registered(false);
        session.feed(&bytes[..split], Instant::now());
        session.feed(&bytes[split..], Instant::now());
        // A finite fixture has at most three deferred operations; a lost
        // continuation or credit refill at each OSC makes these assertions fail.
        for _ in 0..3 {
            if session.pending_bytes() != 0 {
                session.feed(&[], Instant::now());
            }
        }
        assert_eq!(session.pending_bytes(), 0);
        assert_eq!(session.terminal.cursor.column, 4);
        let trace = trace.lock().expect("trace");
        assert_eq!(trace.columns, [1, 2, 3]);
        assert_eq!(
            trace.selectors,
            [b"07779".to_vec(), b"7779".to_vec(), b"7779".to_vec()]
        );
        assert_eq!(writes.lock().expect("writes").concat(), b"onetwothree");
    }
}

#[test]
fn disconnect_discards_paused_work_and_keeps_final_screen_inert() {
    let (mut session, writes, _) = registered(false);
    session.feed(b"\x1b]7779;one\x1b\\\x1b]7779;two\x1b\\", Instant::now());
    session.submit(
        Command::ChildExited(ExitInfo {
            code: Some(0),
            signal: None,
            runtime_ms: 1000,
        }),
        Origin::Platform,
    );
    session.pump(Instant::now());
    assert_eq!(session.pending_bytes(), 0);
    assert_eq!(session.feed(&[], Instant::now()).pending_bytes, 0);
    assert_eq!(writes.lock().expect("writes").concat(), b"one");
}

/// Allocation proof has no allocating observation sink or simulated writer reply.
struct Quiet(bool);
impl OscHandler for Quiet {
    fn handle(&mut self, _: &OscRequest, _: &mut Vec<u8>, _: &mut Bus<Command, Event>) {}
    fn begin_turn(&mut self) {
        self.0 = true;
    }
    fn try_handle_with_context(
        &mut self,
        _: &OscRequest,
        _: &mut OscContext<'_>,
        _: &mut Vec<u8>,
        _: &mut Bus<Command, Event>,
    ) -> OscProgress {
        if std::mem::take(&mut self.0) {
            OscProgress::Complete
        } else {
            OscProgress::Deferred
        }
    }
}

#[test]
fn warmed_deferred_wire_and_pending_suffix_reuse_all_storage() {
    let (mut session, _, _) = fixture();
    session
        .register_osc_handler(7779, Box::new(Quiet(false)))
        .expect("register");
    let bytes = b"\x1b]7779;a\x1b\\\x1b]7779;b\x1b\\\x1b]7779;c\x1b\\";
    // Warm both the selector/payload buffers and the retained ordinary suffix.
    session.feed(bytes, Instant::now());
    session.feed(&[], Instant::now());
    session.feed(&[], Instant::now());
    let measured = allocation_counter::measure(|| {
        session.feed(bytes, Instant::now());
        session.feed(&[], Instant::now());
        session.feed(&[], Instant::now());
    });
    assert_eq!(session.pending_bytes(), 0);
    assert_eq!(measured.count_total, 0);
}
