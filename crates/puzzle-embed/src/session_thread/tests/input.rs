//! Raw host callbacks preserve one composition/modifier stream on the Session owner.
#![allow(
    clippy::unwrap_used,
    reason = "fixture setup and channel failures must identify their exact operation"
)]
use super::*;
use crate::{LEFT_OPTION, RawIme, RawInput, RawKey, RawLogical};
use puzzle_term::{session::OptionAsAlt, vt::input::Modifiers};

/// Layout text differs from the base key for German Option+7.
fn option_seven() -> RawInput {
    RawInput::Key(RawKey {
        logical: RawLogical::Character("|".into()),
        base: RawLogical::Character("7".into()),
        physical: "Digit7".into(),
        text: Some("|".into()),
        location: "Standard".into(),
        pressed: true,
        repeat: false,
    })
}

/// Diagnostic clocks do not change the FIFO behavior of a raw host callback.
fn input(handle: &Handle, value: RawInput) {
    handle
        .input(
            value,
            #[cfg(feature = "lock-metrics")]
            Instant::now(),
        )
        .unwrap();
}

#[test]
fn raw_input_policy_composition_and_focus_are_owner_ordered() {
    let (writes, received) = mpsc::channel();
    let mut session = Session::new(
        SessionConfig::default(),
        Services::new(Box::new(Writer(writes)), Box::new(Quiet)),
    );
    // Enable focus reporting before transfer, without querying the running owner.
    session.feed(b"\x1b[?1004h", Instant::now());
    let (producer, consumer) = crate::pty_pool::pair();
    let started = start(session, consumer, Config::default()).unwrap();
    let clone = started.handle.clone();
    input(&started.handle, RawInput::Focus(false));
    assert_eq!(transaction(&received), b"\x1b[O");
    input(
        &started.handle,
        RawInput::Modifiers(Modifiers::ALT | LEFT_OPTION),
    );
    input(&clone, option_seven());
    assert_eq!(transaction(&received), b"|");
    // A settings update retains the active modifiers but changes the very next key.
    started.handle.input_policy(OptionAsAlt::Left).unwrap();
    input(&clone, option_seven());
    assert_eq!(transaction(&received), b"\x1b7");
    input(&clone, RawInput::Modifiers(0));
    input(&clone, RawInput::Ime(RawIme::Preedit("に".into())));
    input(
        &started.handle,
        RawInput::Key(RawKey::character("n", Some("n"))),
    );
    input(&clone, RawInput::Ime(RawIme::Commit("日本".into())));
    // Composition suppresses the intermediate key even when another clone submits it.
    assert_eq!(transaction(&received), "日本".as_bytes());
    input(&clone, RawInput::Focus(true));
    assert_eq!(transaction(&received), b"\x1b[I");
    drop(producer);
    assert!(started.worker.join().unwrap().is_ok());
    assert!(
        received.try_recv().is_err(),
        "raw stream emitted duplicate bytes"
    );
}
