//! OSC 5522 uses host completion messages, never a synchronous clipboard wait.
use super::*;
use crate::SessionMessage;
use crate::{
    clipboard::{Answer, MimeData, Status, Transaction},
    platform::Clipboard,
};
/// A deferred host records requests but deliberately cannot finish them inline.
struct Deferred(Arc<Mutex<Vec<Transaction>>>);
impl Clipboard for Deferred {
    fn write_text(&mut self, _: &str) -> Result<(), String> {
        panic!("MIME must stay typed")
    }
    fn request_text(&mut self) -> Result<Option<String>, String> {
        panic!("no synchronous read")
    }
    fn mime(&mut self, transaction: Transaction) -> Result<(), Status> {
        self.0.lock().expect("effects").push(transaction);
        Ok(())
    }
}
/// One complete write is small enough that parsing speed is irrelevant to this test.
const WRITE: &[u8] = b"\x1b]5522;type=write:id=w\x1b\\\x1b]5522;type=wdata:mime=dGV4dC9wbGFpbg==;aGk=\x1b\\\x1b]5522;type=wdata\x1b\\";
/// Installs a deferred native capability while retaining the real Session and writer.
pub(super) fn deferred() -> (TestHost, Writes, Arc<Mutex<Vec<Transaction>>>) {
    let (mut session, writes, _) = fixture();
    let requests = Arc::new(Mutex::new(Vec::new()));
    session.services.clipboard = Box::new(Deferred(requests.clone()));
    (session, writes, requests)
}
#[test]
fn clipboard_mime_completion_orders_with_replies_while_output_continues() {
    let (mut session, writes, requests) = deferred();
    let now = Instant::now();
    session.feed(WRITE, now);
    let token = requests.lock().expect("requests")[0].token;
    assert!(writes.lock().expect("writes").is_empty());
    // The host has not answered, yet parsing and unrelated query/input replies proceed.
    session.feed(b"flowing\x1b[5n", now);
    session.submit(Command::Text("key".into()), Origin::User);
    session.pump(now);
    assert_eq!(
        session
            .terminal
            .screen()
            .row(0)
            .expect("row")
            .cells
            .get(0)
            .expect("cell")
            .ch,
        b'f' as u32
    );
    assert_eq!(writes.lock().expect("writes").concat(), b"\x1b[0nkey");
    session
        .apply(
            SessionMessage::ClipboardAnswer(Answer {
                token,
                revision: None,
                status: Status::Done,
                items: Vec::new(),
            }),
            now,
        )
        .expect("completion");
    assert_eq!(
        writes.lock().expect("writes").concat(),
        b"\x1b[0nkey\x1b]5522;type=write:status=DONE:id=w\x1b\\"
    );
}
#[test]
fn clipboard_mime_never_completing_host_is_bounded_and_stale_answers_ignored() {
    let (mut session, writes, requests) = deferred();
    let now = Instant::now();
    session.feed(WRITE, now);
    let token = requests.lock().expect("requests")[0].token;
    // Repeated complete requests cannot grow the native effect queue.
    for _ in 0..40 {
        session.feed(WRITE, now);
    }
    assert_eq!(requests.lock().expect("requests").len(), 1);
    assert_eq!(writes.lock().expect("writes").len(), 40);
    assert!(
        writes
            .lock()
            .expect("writes")
            .iter()
            .all(|w| w.windows(12).any(|p| p == b"status=EBUSY"))
    );
    session.pump(now + Duration::from_secs(11));
    let count = writes.lock().expect("writes").len();
    session
        .apply(
            SessionMessage::ClipboardAnswer(Answer {
                token,
                revision: None,
                status: Status::Done,
                items: Vec::new(),
            }),
            now + Duration::from_secs(12),
        )
        .expect("completion");
    assert_eq!(writes.lock().expect("writes").len(), count);
    session.feed(WRITE, now + Duration::from_secs(12));
    assert_eq!(requests.lock().expect("requests").len(), 2);
}
#[test]
fn clipboard_mime_reads_require_permission_and_filter_host_results() {
    let (mut session, writes, requests) = deferred();
    let now = Instant::now();
    let read = b"\x1b]5522;type=read:id=r;dGV4dC9wbGFpbg==\x1b\\";
    session.feed(read, now);
    assert!(requests.lock().expect("requests").is_empty());
    assert_eq!(
        writes.lock().expect("writes").concat(),
        b"\x1b]5522;type=read:status=EPERM:id=r\x1b\\"
    );
    session.clipboard.allow_read = true;
    writes.lock().expect("writes").clear();
    session.feed(read, now);
    let token = requests.lock().expect("requests")[0].token;
    session.clipboard_answer(Answer {
        token,
        revision: None,
        status: Status::Done,
        items: vec![
            MimeData {
                mime: "text/plain".into(),
                data: (&b"hi"[..]).into(),
            },
            MimeData {
                mime: "secret/extra".into(),
                data: (&b"private"[..]).into(),
            },
        ],
    });
    let reply = writes.lock().expect("writes").concat();
    assert!(reply.windows(4).any(|p| p == b"aGk="));
    assert!(!reply.windows(12).any(|p| p == b"cHJpdmF0ZQ=="));
    assert!(reply.ends_with(b"status=DONE:id=r\x1b\\"));
}
#[test]
fn clipboard_mime_ris_invalidates_old_completion_and_veto_replies_denied() {
    struct Veto;
    impl Interceptor<Command, Event> for Veto {
        fn intercept(&mut self, _: &mut Command, _: &mut HookCtx<'_, Command, Event>) -> Flow {
            Flow::Stop("test denial")
        }
    }
    let (mut session, writes, requests) = deferred();
    let now = Instant::now();
    session.feed(WRITE, now);
    let old = requests.lock().expect("requests")[0].token;
    session.feed(b"\x1bc", now);
    session.clipboard_answer(Answer {
        token: old,
        revision: None,
        status: Status::Done,
        items: Vec::new(),
    });
    assert!(writes.lock().expect("writes").is_empty());
    session
        .bus_mut()
        .intercept(CommandKinds::ALL, 200, "deny", Veto)
        .expect("policy");
    session.feed(WRITE, now);
    assert_eq!(requests.lock().expect("requests").len(), 1);
    assert_eq!(
        writes.lock().expect("writes").concat(),
        b"\x1b]5522;type=write:status=EPERM:id=w\x1b\\"
    );
}

#[test]
fn clipboard_mime_public_wire_write_reaches_policy_and_host() {
    use puzzle_bus::wire::{FromWire, ToWire};
    let (mut session, _, requests) = deferred();
    let command = Command::ClipboardWrite(crate::messages::ClipboardContent::Mime {
        token: 0,
        items: vec![MimeData {
            mime: "image/png".into(),
            data: (&b"binary\0data"[..]).into(),
        }],
    });
    let value = command.to_value().expect("MIME wire");
    let registry = puzzle_bus::wire::ExtensionRegistry::default();
    let decoded =
        Command::from_value("platform/clipboardWrite", &value, &registry).expect("decode MIME");
    assert_eq!(decoded.to_value().expect("round trip"), value);
    session.submit(decoded, Origin::Api);
    session.pump(Instant::now());
    let requests = requests.lock().expect("requests");
    assert_eq!(requests.len(), 1);
    let crate::clipboard::Operation::Write(items) = &requests[0].request.operation else {
        panic!("write");
    };
    assert_eq!(items[0].data.as_ref(), b"binary\0data");
}
