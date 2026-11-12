//! Deferred I/O stays FIFO; native DOM text still passes ordinary bus policy.
use super::*;
use crate::{
    keybind::{Keybindings, Platform, Source},
    platform::{TransportEffect, WakeRequests},
};
use std::collections::VecDeque;

/// Browser services retain plain data, allowing Session to remain Send.
#[derive(Default)]
struct DeferredTransport(VecDeque<TransportEffect>);
/// Tests exercise the same typed drain used before invoking JS callbacks.
impl Transport for DeferredTransport {
    /// Encoded input owns its bytes until the host takes them after pump.
    fn write(&mut self, bytes: &[u8]) -> Result<(), String> {
        self.0.push_back(TransportEffect::Write(bytes.to_vec()));
        Ok(())
    }
    /// A resize keeps its position relative to preceding writes and replies.
    fn resize(&mut self, size: PtySize) -> Result<(), String> {
        self.0.push_back(TransportEffect::Resize(size));
        Ok(())
    }
    /// Closing is queued instead of calling foreign code inside the core.
    fn close(&mut self) -> Result<(), String> {
        self.0.push_back(TransportEffect::Close);
        Ok(())
    }
    /// The host moves effects without a shared or downcast service reference.
    fn take_effect(&mut self) -> Option<TransportEffect> {
        self.0.pop_front()
    }
}

/// One wake record combines damage and the earliest requested deadline.
#[derive(Default)]
struct DeferredWaker(WakeRequests);
/// Requests never allocate a browser closure inside Session.
impl Waker for DeferredWaker {
    /// Damage requests coalesce even when several commands execute together.
    fn request_frame(&mut self) {
        self.0.frame = true;
    }
    /// The earliest timer wins so a later blink cannot hide an earlier timeout.
    fn wake_at(&mut self, instant: Instant) {
        self.0.deadline = Some(self.0.deadline.map_or(instant, |old| old.min(instant)));
    }
    /// Taking the record leaves no idle work for the next platform turn.
    fn take_requests(&mut self) -> WakeRequests {
        std::mem::take(&mut self.0)
    }
}

/// Keys and parser replies reach the host without asking for a frame.
#[test]
fn deferred_effects_preserve_input_reply_fifo() {
    let mut session = TestHost::new(
        SessionConfig::default(),
        Services::new(
            Box::<DeferredTransport>::default(),
            Box::<DeferredWaker>::default(),
        ),
    );
    let now = Instant::now();
    session.submit(Command::Text("a".into()), Origin::User);
    session.pump(now);
    session.feed(b"\x1b[5n", now);
    session.submit(Command::Text("b".into()), Origin::Api);
    session.pump(now);
    for expected in [b"a".as_slice(), b"\x1b[0n", b"b"] {
        let Some(TransportEffect::Write(bytes)) = session.take_transport_effect() else {
            panic!("missing ordered write");
        };
        assert_eq!(bytes, expected);
    }
    assert!(session.take_transport_effect().is_none());
    session.services.waker.request_frame();
    session
        .services
        .waker
        .wake_at(now + Duration::from_millis(10));
    session
        .services
        .waker
        .wake_at(now + Duration::from_millis(20));
    let requests = session.take_wake_requests();
    assert!(requests.frame);
    assert_eq!(requests.deadline, Some(now + Duration::from_millis(10)));
    let empty = session.take_wake_requests();
    assert!(!empty.frame && empty.deadline.is_none());
}

/// A shortcut owns its press; an unmatched DOM press waits for native input.
#[test]
fn native_text_bindings_and_repeated_leader_fifo() {
    let (mut session, writes, _) = fixture();
    let mut table = Keybindings::default();
    table.apply(
        &["a>b=ignore".into(), "x=sendText:X".into()],
        Source::User,
        Platform::Mac,
        true,
    );
    session.set_keybindings(table);
    let now = Instant::now();
    let key = |ch| KeyEvent::press(Key::Char(ch));
    assert!(!session.pump_native_key(key('x'), Origin::User, now));
    assert!(!session.pump_native_key(key('a'), Origin::User, now));
    // A mismatching continuation flushes the first a, then the second a
    // starts a fresh leader. Native input must not duplicate either press.
    assert!(!session.pump_native_key(key('a'), Origin::User, now));
    assert_eq!(writes.lock().expect("writes").concat(), b"Xa");
    // An unrelated press flushes the second held a but belongs to the DOM;
    // only its later committed text may encode z.
    assert!(session.pump_native_key(key('z'), Origin::User, now));
    assert_eq!(writes.lock().expect("writes").concat(), b"Xaa");
    session.submit(Command::Text("z".into()), Origin::User);
    session.pump(now);
    assert_eq!(writes.lock().expect("writes").concat(), b"Xaaz");
}

/// A native-text candidate cannot bypass a higher-priority veto or exit hold.
#[test]
fn native_text_respects_veto_and_held_exit() {
    /// This host policy runs before the borrowed binding interceptor.
    struct Veto;
    /// A denied key must never return ownership to the textarea.
    impl Interceptor<Command, Event> for Veto {
        fn intercept(&mut self, _: &mut Command, _: &mut HookCtx<'_, Command, Event>) -> Flow {
            Flow::Stop("denied")
        }
    }
    let (mut session, writes, _) = fixture();
    let id = session
        .bus_mut()
        .intercept(CommandKinds::at(0), 200, "veto", Veto)
        .expect("policy");
    let key = KeyEvent::press(Key::Char('z'));
    let now = Instant::now();
    assert!(!session.pump_native_key(key.clone(), Origin::User, now));
    assert!(writes.lock().expect("writes").is_empty());
    session.bus_mut().remove(id);
    session.on_exit = OnExit::Hold;
    session.submit(
        Command::ChildExited(ExitInfo {
            code: Some(0),
            signal: None,
            runtime_ms: 500,
        }),
        Origin::Platform,
    );
    session.pump(now);
    assert!(!session.pump_native_key(key, Origin::User, now));
    assert!(session.close_requested());
    assert!(writes.lock().expect("writes").is_empty());
}
