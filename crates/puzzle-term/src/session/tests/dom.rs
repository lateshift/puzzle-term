//! DOM commands retain provisional text and transport ordering (§12.2).
use super::*;
use crate::messages::PreeditState;

/// Only committed composition reaches stdin; preedit is queryable UI state.
#[test]
fn preedit_is_not_application_input() {
    let (mut session, writes, _) = fixture();
    let now = Instant::now();
    let state = PreeditState {
        text: "日本".into(),
        cursor: Some(3),
    };
    session.submit(Command::Preedit(state.clone()), Origin::User);
    session.pump(now);
    assert_eq!(session.preedit(), &state);
    assert!(writes.lock().expect("writes").is_empty());
    session.submit(Command::Preedit(PreeditState::default()), Origin::User);
    session.submit(Command::Text("日本".into()), Origin::User);
    session.pump(now);
    assert_eq!(*writes.lock().expect("writes"), ["日本".as_bytes()]);
    assert!(session.preedit().text.is_empty());
}

/// Focus reports are policy-visible and stay FIFO with keys without a frame.
#[test]
fn focus_reports_share_input_fifo() {
    let (mut session, writes, _) = fixture();
    let now = Instant::now();
    session.feed(b"\x1b[?1004h", now);
    session.submit(Command::Focus(true), Origin::Platform);
    session.submit(Command::Text("x".into()), Origin::User);
    session.submit(Command::Focus(false), Origin::Platform);
    session.pump(now);
    assert_eq!(
        *writes.lock().expect("writes"),
        [b"\x1b[I".to_vec(), b"x".to_vec(), b"\x1b[O".to_vec()]
    );
}
