//! Registered handlers defer before sequence consumption when prepaid storage is busy.
use super::*;
use crate::{
    osc::{OscContext, OscHandler, OscProgress, OscRequest},
    platform::reply::*,
};
use std::collections::VecDeque;

/// The test peer retains bytes and completes only when explicitly told to consume them.
struct Peer(Arc<Mutex<VecDeque<ReplyWrite>>>, Writes);
impl Transport for Peer {
    fn write(&mut self, bytes: &[u8]) -> Result<(), String> {
        self.1.lock().expect("input").push(bytes.to_vec());
        Ok(())
    }
    fn deferred_replies(&self) -> bool {
        true
    }
    fn write_reply(&mut self, reply: ReplyWrite) -> Result<ReplyDelivery, String> {
        self.0.lock().expect("peer").push_back(reply);
        Ok(ReplyDelivery::Pending)
    }
    fn resize(&mut self, _: PtySize) -> Result<(), String> {
        Ok(())
    }
    fn close(&mut self) -> Result<(), String> {
        Ok(())
    }
}

/// One precharged packet exercises the full-table error path's minimum storage.
struct Scoped {
    /// Allocated before table admission and shared without copying while queued.
    buffer: ReusableReply,
    /// Mutation count stands at the same commit point as authenticated last+1 state.
    consumed: Arc<Mutex<Vec<Vec<u8>>>>,
}
impl OscHandler for Scoped {
    fn handle(&mut self, _: &OscRequest, _: &mut Vec<u8>, _: &mut Bus<Command, Event>) {
        panic!("scoped fixture needs owner context");
    }
    fn try_handle_with_context(
        &mut self,
        request: &OscRequest,
        source: &mut OscContext<'_>,
        _: &mut Vec<u8>,
        _: &mut Bus<Command, Event>,
    ) -> OscProgress {
        if source
            .reply_from_buffer(&mut self.buffer, &request.payload)
            .is_none()
        {
            return OscProgress::AwaitReply;
        }
        self.consumed
            .lock()
            .expect("consumption")
            .push(request.payload.clone());
        OscProgress::Complete
    }
}

#[test]
fn reply_completion_and_callback_release_resume_exactly_one_sequence() {
    assert_reply_sequence(false);
    assert_reply_sequence(true);
}

/// Native graphics opt-in must not change registered-handler receipt ordering.
fn assert_reply_sequence(images: bool) {
    let peer = Arc::new(Mutex::new(VecDeque::new()));
    let input = Arc::new(Mutex::new(Vec::new()));
    let consumed = Arc::new(Mutex::new(Vec::new()));
    let mut session = Session::new(
        SessionConfig::default(),
        Services::new(
            Box::new(Peer(peer.clone(), input.clone())),
            Box::new(ManualWaker(Arc::new(AtomicUsize::new(0)))),
        ),
    );
    if images {
        session.enable_images(4096);
    }
    let readiness = install(&mut session, consumed.clone());
    let bytes = b"\x1b[5nA\x1b]7779;one\x1b\\B\x1b]7779;two\x1b\\C\x1b[5n";
    session.feed(bytes, Instant::now());
    assert_eq!(*consumed.lock().expect("consumed"), [b"one".to_vec()]);
    assert_eq!(session.terminal().cursor.column, 2);
    assert!(session.waiting_reply_credit());
    session.submit(Command::Text("K".into()), Origin::User);
    session.pump(Instant::now());
    assert_eq!(*input.lock().expect("input"), [b"K".to_vec()]);
    // The older DSR ticket precedes the scoped ticket, even though it was staged.
    let first = peer.lock().expect("peer").pop_front().expect("DSR");
    assert_eq!(&*first.bytes, b"\x1b[0n");
    session
        .reply_completed(first.ticket.id())
        .expect("older completion");
    drop(first);
    session.feed(&[], Instant::now());
    assert_eq!(consumed.lock().expect("consumed").len(), 1);
    let scoped = peer.lock().expect("peer").pop_front().expect("scoped");
    let callback = scoped.bytes.lease().expect("immutable callback");
    let id = scoped.ticket.id();
    drop(scoped);
    session.reply_completed(id).expect("writer completion");
    session.feed(&[], Instant::now());
    assert_eq!(
        consumed.lock().expect("consumed").len(),
        1,
        "callback is still live"
    );
    drop(callback);
    for event in readiness.try_iter() {
        if matches!(event, ReplyCompletion::Ready) {
            session.reply_storage_ready();
        }
    }
    session.feed(&[], Instant::now());
    assert_eq!(
        *consumed.lock().expect("consumed"),
        [b"one".to_vec(), b"two".to_vec()]
    );
    assert_eq!(session.terminal().cursor.column, 3);
    assert_eq!(session.pending_bytes(), 0);
}

/// The reply-aware parser adds no grid work or warmed allocations without queries.
#[test]
fn unregistered_reply_lane_preserves_bulk_vt_counts_and_allocations() {
    let (mut session, _, _) = fixture();
    let mut vt = puzzle_vt::Terminal::new(80, 24, 10_000, 32 * 1024 * 1024);
    let bytes = "abc一é\u{301}\r\n\x1b[2;20r\x1b[2S\x1b[3;5H\x1b[2K".as_bytes();
    session.start_parse_operation_metrics();
    vt.start_operation_metrics();
    session.feed(bytes, Instant::now());
    vt.advance(bytes);
    assert_eq!(
        &session.parse_operation_metrics()[..10],
        &vt.operation_metrics().expect("metrics")
    );
    for row in 0..24 {
        assert_eq!(
            session.terminal().screen().row(row).expect("row").cells,
            vt.screen().row(row).expect("row").cells
        );
    }
    session.feed(b"x", Instant::now());
    let allocations = allocation_counter::measure(|| {
        session.feed(b"x", Instant::now());
    });
    assert_eq!(allocations.count_total, 0);
    assert_eq!(session.pending_reply_bytes(), 0);
}

/// Registration and platform completion wakeup belong to the same Session owner.
fn install(
    session: &mut Session,
    consumed: Arc<Mutex<Vec<Vec<u8>>>>,
) -> std::sync::mpsc::Receiver<ReplyCompletion> {
    let (ready, readiness) = std::sync::mpsc::channel();
    session.set_reply_completion(Box::new(move |event| {
        let _ = ready.send(event);
    }));
    session
        .register_osc_handler(
            7779,
            Box::new(Scoped {
                buffer: ReusableReply::new(4096).expect("precharge"),
                consumed,
            }),
        )
        .expect("register");
    readiness
}

/// Resize coalesces only its new 2048 report, never an earlier blocked query reply.
#[test]
fn resize_keeps_older_raw_reply_prefix_in_the_program_fifo() {
    let (mut session, writes, _) = fixture();
    let context = overlays::text_context(1.0);
    session.set_text_context(&context);
    session.feed(b"\x1b[?2048h", Instant::now());
    writes.lock().expect("writes").clear();
    session.terminal.replies.extend_from_slice(b"older-query");
    session.resize_grid(90, 25, [900, 500]).expect("resize");
    assert_eq!(session.terminal.replies, b"older-query");
    assert!(session.pending_resize_reply.starts_with(b"\x1b[48;25;90;"));
}
