//! Generic protocols preserve parser order, bounds and reset/lifecycle semantics.
use super::*;
use crate::osc::{OscHandler, OscRequest};

/// The fixture makes callback order visible without a protocol-specific core variant.
struct Echo(Arc<Mutex<Vec<String>>>);
impl OscHandler for Echo {
    fn handle(&mut self, request: &OscRequest, replies: &mut Vec<u8>, _: &mut Bus<Command, Event>) {
        self.0.lock().expect("trace").push(format!(
            "{}:{}:{}",
            String::from_utf8_lossy(&request.payload),
            request.bell_terminated,
            request.truncated
        ));
        replies.extend_from_slice(b"extension");
    }
    fn on_event(&mut self, event: &Event, _: &mut Bus<Command, Event>) {
        self.0.lock().expect("trace").push(event.type_name().into());
    }
    fn reset(&mut self, _: &mut Bus<Command, Event>) {
        self.0.lock().expect("trace").push("reset".into());
    }
}

#[test]
fn registered_osc_replies_follow_source_order_across_chunks() {
    for terminator in ["\x07", "\x1b\\"] {
        let bytes = format!("\x1b[5n\x1b]7501;a;b{terminator}\x1b[5n");
        for split in 0..=bytes.len() {
            let (mut session, writes, _) = fixture();
            let trace = Arc::new(Mutex::new(Vec::new()));
            session
                .register_osc_handler(7501, Box::new(Echo(trace.clone())))
                .expect("registration");
            session.feed(&bytes.as_bytes()[..split], Instant::now());
            session.feed(&bytes.as_bytes()[split..], Instant::now());
            assert_eq!(
                writes.lock().expect("writes").concat(),
                b"\x1b[0nextension\x1b[0n"
            );
            assert_eq!(
                trace.lock().expect("trace").as_slice(),
                [format!("a;b:{}:false", terminator == "\x07")]
            );
        }
    }
}

#[test]
fn generic_lifecycle_and_reset_are_source_ordered() {
    let (mut session, _, _) = fixture();
    let trace = Arc::new(Mutex::new(Vec::new()));
    session
        .register_osc_handler(7501, Box::new(Echo(trace.clone())))
        .expect("registration");
    session.feed(
        b"\x1b]7501;one\x07\x1b]133;A\x07\x1b]7501;two\x07\x1bc\x1b]7501;three\x07\x1b[!p",
        Instant::now(),
    );
    assert_eq!(
        *trace.lock().expect("trace"),
        [
            "one:true:false",
            "shell/promptStarted",
            "two:true:false",
            "reset",
            "three:true:false"
        ]
    );
    session.submit(
        Command::ChildExited(ExitInfo {
            code: Some(0),
            signal: None,
            runtime_ms: 1000,
        }),
        Origin::Platform,
    );
    session.pump(Instant::now());
    assert!(
        trace
            .lock()
            .expect("trace")
            .last()
            .expect("exit")
            .contains("session/exited")
    );
}

#[test]
fn unknown_osc_is_inert_and_builtins_cannot_be_shadowed() {
    let (mut session, writes, _) = fixture();
    session.feed(b"\x1b]7501;?\x07", Instant::now());
    assert!(writes.lock().expect("writes").is_empty());
    let trace = Arc::new(Mutex::new(Vec::new()));
    assert!(
        session
            .register_osc_handler(2, Box::new(Echo(trace.clone())))
            .is_err()
    );
    session
        .register_osc_handler(7501, Box::new(Echo(trace.clone())))
        .expect("registration");
    assert!(
        session
            .register_osc_handler(7501, Box::new(Echo(trace.clone())))
            .is_err()
    );
    let mut bytes = b"\x1b]7501;".to_vec();
    bytes.resize(70_000, b'x');
    bytes.push(7);
    session.feed(&bytes, Instant::now());
    session.feed(&[], Instant::now());
    assert!(trace.lock().expect("trace")[0].ends_with(":true:true"));
}

/// A source namespace rebases before its first callback, even on a late registration.
#[test]
fn registered_source_context_activates_clock_before_first_callback() {
    let (mut session, _, _) = fixture();
    session.feed(b"abcdefgh\rX", Instant::now());
    let before = session.terminal.screen().row(0).expect("row").version;
    assert_eq!(session.terminal.write_clock(), 0);
    let trace = Arc::new(Mutex::new(Vec::new()));
    session
        .register_osc_handler(7501, Box::new(Echo(trace.clone())))
        .expect("registration");
    assert!(session.terminal.write_clock() > before);
    assert_eq!(
        session.terminal.screen().row(0).expect("row").version,
        before
    );
    let captured = session.terminal.write_clock();
    session.feed(b"\rY\x1b]7501;now\x07", Instant::now());
    assert!(session.terminal.screen().row(0).expect("row").version > captured);
    assert_eq!(trace.lock().expect("trace").as_slice(), ["now:true:false"]);
}
