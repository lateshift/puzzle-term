//! §16 single-use paste grants are capabilities, never a change to read defaults.
use super::clipboard_mime::deferred;
use super::*;
use crate::clipboard::{Answer, MimeData, Paste, Status};
use puzzle_vt::clipboard::Secret;
/// A fixed test credential makes the protocol exchange deterministic, unlike host tokens.
const READ: &[u8] =
    b"\x1b]5522;type=read:name=UGFzdGUgZXZlbnQ=:pw=Z2VzdHVyZQ==;dGV4dC9wbGFpbg==\x1b\\";
/// A clipboard generation identifies its contents, including a change then restoration.
fn gesture(revision: u64) -> Paste {
    Paste {
        secret: Secret("gesture".into()),
        revision,
        primary: false,
        mimes: vec!["text/plain".into()],
        text: Some("ordinary paste".into()),
    }
}
#[test]
fn clipboard_grant_one_read_and_normal_policy_stays_denied() {
    let (mut session, writes, requests) = deferred();
    let now = Instant::now();
    session.feed(b"\x1b[?2004h\x1b[?5522h\x1b[?5522$p", now);
    session.clipboard_paste(gesture(7), now);
    let listing = writes.lock().expect("writer").concat();
    assert!(listing.starts_with(b"\x1b[?5522;1$y"));
    assert!(
        listing
            .windows(16)
            .any(|bytes| bytes == b":pw=Z2VzdHVyZQ==")
    );
    assert!(!listing.windows(14).any(|bytes| bytes == b"ordinary paste"));
    writes.lock().expect("writer").clear();
    session.feed(READ, now);
    let request = requests.lock().expect("requests")[0].clone();
    assert_eq!(request.expected_revision, Some(7));
    session.clipboard_answer(Answer {
        token: request.token,
        revision: Some(7),
        status: Status::Done,
        items: vec![MimeData {
            mime: "text/plain".into(),
            data: (&b"secret"[..]).into(),
        }],
    });
    assert!(
        writes
            .lock()
            .expect("writer")
            .concat()
            .windows(8)
            .any(|bytes| bytes == b"c2VjcmV0")
    );
    writes.lock().expect("writer").clear();
    session.feed(READ, now);
    assert_eq!(requests.lock().expect("requests").len(), 1);
    assert!(
        writes
            .lock()
            .expect("writer")
            .concat()
            .windows(12)
            .any(|bytes| bytes == b"status=EPERM")
    );
    // OSC 52 cannot redeem a MIME grant and its default read behavior is unchanged.
    writes.lock().expect("writer").clear();
    session.clipboard_paste(gesture(8), now);
    writes.lock().expect("writer").clear();
    session.feed(b"\x1b]52;c;?\x1b\\", now);
    assert!(writes.lock().expect("writer").is_empty());
    assert!(!session.clipboard.allow_read);
}
#[test]
fn clipboard_grant_each_input_and_mode_expiry_path() {
    for reset in [
        b"\x1b[?5522l\x1b[?5522h".as_slice(),
        b"\x1bc\x1b[?5522h",
        b"\x1b[!p\x1b[?5522h",
    ] {
        let (mut session, writes, requests) = deferred();
        let now = Instant::now();
        session.feed(b"\x1b[?5522h", now);
        session.clipboard_paste(gesture(1), now);
        session.feed(reset, now);
        writes.lock().expect("writer").clear();
        session.feed(READ, now);
        assert!(requests.lock().expect("requests").is_empty());
        assert!(
            writes
                .lock()
                .expect("writer")
                .concat()
                .windows(12)
                .any(|bytes| bytes == b"status=EPERM")
        );
    }
    for message in [
        crate::SessionMessage::UserActivity,
        crate::SessionMessage::Command(Command::Text("key".into()), Origin::User),
    ] {
        let (mut session, _, requests) = deferred();
        let now = Instant::now();
        session.feed(b"\x1b[?5522h", now);
        session.clipboard_paste(gesture(1), now);
        session.apply(message, now).expect("input");
        session.feed(READ, now);
        assert!(requests.lock().expect("requests").is_empty());
    }
}
#[test]
fn clipboard_grant_next_gesture_cross_session_and_location_refused() {
    let (mut session, _, requests) = deferred();
    let (mut other, _, other_requests) = deferred();
    let now = Instant::now();
    session.feed(b"\x1b[?5522h", now);
    session.clipboard_paste(gesture(1), now);
    other.feed(READ, now);
    assert!(other_requests.lock().expect("other").is_empty());
    session.feed(b"\x1b]5522;type=read:loc=primary:name=UGFzdGUgZXZlbnQ=:pw=Z2VzdHVyZQ==;dGV4dC9wbGFpbg==\x1b\\", now);
    assert!(requests.lock().expect("requests").is_empty());
    // Even unchanged contents create a fresh capability at the next paste gesture.
    let mut next = gesture(1);
    next.secret = Secret("different".into());
    session.clipboard_paste(next, now);
    session.feed(READ, now);
    assert!(requests.lock().expect("requests").is_empty());
}
#[test]
fn clipboard_grant_changed_revision_refuses_all_contents_and_redacts_diagnostics() {
    let (mut session, writes, requests) = deferred();
    let now = Instant::now();
    session.feed(b"\x1b[?5522h", now);
    session.clipboard_paste(gesture(1), now);
    writes.lock().expect("writer").clear();
    session.feed(READ, now);
    let request = requests.lock().expect("requests")[0].clone();
    assert!(!format!("{request:?}").contains("gesture"));
    session.clipboard_answer(Answer {
        token: request.token,
        revision: Some(2),
        status: Status::Done,
        items: vec![MimeData {
            mime: "text/plain".into(),
            data: (&b"private"[..]).into(),
        }],
    });
    assert_eq!(
        writes.lock().expect("writer").concat(),
        b"\x1b]5522;type=read:status=EPERM\x1b\\"
    );
}

#[test]
fn clipboard_grant_has_no_timer_and_mode_off_preserves_sanitized_paste() {
    let (mut session, writes, requests) = deferred();
    let now = Instant::now();
    session.feed(b"\x1b[?5522h", now);
    session.clipboard_paste(gesture(1), now);
    session.pump(now + Duration::from_secs(3600));
    session.feed(READ, now + Duration::from_secs(3600));
    assert_eq!(requests.lock().expect("requests").len(), 1);
    session.feed(b"\x1bc\x1b[?2004h", now);
    writes.lock().expect("writer").clear();
    let mut paste = gesture(2);
    paste.text = Some("one\ntwo\x00".into());
    session.clipboard_paste(paste, now);
    assert_eq!(
        writes.lock().expect("writer").concat(),
        b"\x1b[200~one\rtwo\x1b[201~"
    );
}
