//! Host control payloads reach only their registered owner, never through PTY bytes.
use super::*;
use crate::SessionMessage;
use crate::osc::{Endpoint, EndpointReceipt, OscContext, OscHandler, OscRequest};
use puzzle_bus::{
    Value,
    wire::{Extension, WireExt},
};

/// Test-only private control uses a redacted wire value that cannot recreate it.
struct Control {
    /// Endpoint is owned by a handler; a sibling receipt must never resolve.
    receipt: Option<EndpointReceipt>,
    /// Owner fills the resolved current gap for the test's direct observation.
    observed: Option<Endpoint>,
    /// Current handler work-turn count, not changed by control delivery.
    turns: usize,
}
impl WireExt for Control {
    const TYPE_NAME: &'static str = "ext:control-fixture/private";
    fn to_value(&self) -> Value {
        Value::Null
    }
    fn from_value(_: &Value) -> Result<Self, puzzle_bus::value::ValidationReport> {
        Err(puzzle_bus::wire::reject(
            "private host control has no wire authority",
        ))
    }
}

/// The mutable counter and all source operations stay with this handler.
#[derive(Default)]
struct Controlled {
    /// Callback count makes accidental work-credit replenishment observable.
    turns: usize,
}
impl OscHandler for Controlled {
    fn handle(&mut self, _: &OscRequest, replies: &mut Vec<u8>, _: &mut Bus<Command, Event>) {
        replies.extend_from_slice(b"handled");
    }
    fn begin_turn(&mut self) {
        self.turns += 1;
    }
    fn control(
        &mut self,
        control: &mut Extension,
        source: &mut OscContext<'_>,
        _: &mut Bus<Command, Event>,
    ) -> Result<(), String> {
        let control = control
            .downcast_mut::<Control>()
            .ok_or("wrong control type")?;
        if control.receipt.is_none() {
            control.receipt = Some(source.capture_endpoint(8).map_err(|_| "capture refused")?);
        }
        control.observed = control
            .receipt
            .as_ref()
            .and_then(|r| source.query_endpoint(r));
        control.turns = self.turns;
        Ok(())
    }
}

/// There is deliberately no copyable secret field or reconstructible wire payload.
fn control() -> Extension {
    Extension::new(Control {
        receipt: None,
        observed: None,
        turns: usize::MAX,
    })
    .expect("control")
}

#[test]
fn host_control_is_owner_scoped_and_does_not_parse_or_refill_credits() {
    let (mut session, writes, _) = fixture();
    for number in [7778, 7779] {
        session
            .register_osc_handler(number, Box::<Controlled>::default())
            .expect("register");
    }
    session.feed(b"abc", Instant::now());
    let mut value = control();
    session
        .control_osc_handler(7779, &mut value)
        .expect("capture");
    let own = value.downcast_ref::<Control>().expect("typed");
    assert_eq!(own.observed.expect("current gap").column, 3);
    assert_eq!(own.turns, 1);
    // The same opaque receipt is not authority in another registration.
    session
        .control_osc_handler(7778, &mut value)
        .expect("foreign receipt query");
    let foreign = value.downcast_ref::<Control>().expect("typed");
    assert!(foreign.observed.is_none());
    assert_eq!(foreign.turns, 1);
    assert!(writes.lock().expect("writes").is_empty());
    assert_eq!(session.terminal().cursor.column, 3);
}

#[test]
fn control_message_preserves_typed_payload_and_refuses_unregistered_or_ended_sessions() {
    let (mut session, _, _) = fixture();
    let mut value = control();
    assert!(session.control_osc_handler(7779, &mut value).is_err());
    assert!(
        value
            .downcast_ref::<Control>()
            .expect("typed")
            .receipt
            .is_none()
    );
    session
        .register_osc_handler(7779, Box::<Controlled>::default())
        .expect("register");
    session
        .apply(
            SessionMessage::OscControl {
                number: 7779,
                control: value,
            },
            Instant::now(),
        )
        .expect("message");
    assert_eq!(session.osc.endpoint_count(), 1);
    session.submit(
        Command::ChildExited(ExitInfo {
            code: Some(0),
            signal: None,
            runtime_ms: 1000,
        }),
        Origin::Platform,
    );
    session.pump(Instant::now());
    assert_eq!(session.osc.endpoint_count(), 0);
    assert!(session.control_osc_handler(7779, &mut control()).is_err());
}

#[test]
fn clear_retires_old_control_receipts_and_wire_cannot_recreate_private_authority() {
    let (mut session, _, _) = fixture();
    session
        .register_osc_handler(7779, Box::<Controlled>::default())
        .expect("register");
    let mut value = control();
    session
        .control_osc_handler(7779, &mut value)
        .expect("capture");
    assert!(
        value
            .downcast_ref::<Control>()
            .expect("typed")
            .observed
            .is_some()
    );
    session.feed(b"\x1b[2J", Instant::now());
    session
        .control_osc_handler(7779, &mut value)
        .expect("query old");
    assert!(
        value
            .downcast_ref::<Control>()
            .expect("typed")
            .observed
            .is_none()
    );
    assert!(Control::from_value(&Value::Null).is_err());
    let mut registry = puzzle_bus::wire::ExtensionRegistry::default();
    registry.register::<Control>().expect("registered name");
    assert!(registry.decode(Control::TYPE_NAME, &Value::Null).is_err());
}

#[test]
fn ordinary_no_registration_work_is_unchanged_by_the_control_seam() {
    let (mut session, _, _) = fixture();
    session.feed(b"warm", Instant::now());
    let before = session.terminal().source_watch_counts();
    let measured = allocation_counter::measure(|| {
        // Overwrite the warmed row: ordinary history growth has its own
        // allocations and is unrelated to whether a control registry exists.
        for _ in 0..100 {
            session.feed(b"\rplain", Instant::now());
        }
    });
    assert_eq!(measured.count_total, 0);
    assert_eq!(session.terminal().source_watch_counts(), before);
    assert_eq!(session.terminal().private_marker_storage_bytes(), 0);
}

/// A private host revocation can finish a retained boundary even if no writer wakes.
struct Waiting {
    /// A trusted control retires the old externally blocked operation.
    ready: bool,
}
impl OscHandler for Waiting {
    fn handle(&mut self, _: &OscRequest, _: &mut Vec<u8>, _: &mut Bus<Command, Event>) {
        panic!("context callback required");
    }
    fn try_handle_with_context(
        &mut self,
        _: &OscRequest,
        _: &mut OscContext<'_>,
        _: &mut Vec<u8>,
        _: &mut Bus<Command, Event>,
    ) -> crate::osc::OscProgress {
        if self.ready {
            crate::osc::OscProgress::Complete
        } else {
            crate::osc::OscProgress::AwaitReply
        }
    }
    fn control(
        &mut self,
        value: &mut Extension,
        _: &mut OscContext<'_>,
        _: &mut Bus<Command, Event>,
    ) -> Result<(), String> {
        value.downcast_ref::<Control>().ok_or("wrong type")?;
        self.ready = true;
        Ok(())
    }
}

#[test]
fn successful_control_releases_an_external_wait_without_parsing_in_the_callback() {
    let (mut session, _, _) = fixture();
    session
        .register_osc_handler(7779, Box::new(Waiting { ready: false }))
        .expect("register");
    session.feed(b"\x1b]7779;pending\x1b\\X", Instant::now());
    assert!(session.waiting_reply_credit());
    assert_eq!(session.terminal().cursor.column, 0);
    assert!(session.control_osc_handler(7778, &mut control()).is_err());
    assert!(
        session.waiting_reply_credit(),
        "rejected control preserves wait"
    );
    session
        .apply(
            SessionMessage::OscControl {
                number: 7779,
                control: control(),
            },
            Instant::now(),
        )
        .expect("control");
    assert!(!session.waiting_reply_credit());
    assert_eq!(
        session.terminal().cursor.column,
        0,
        "control cannot parse application data"
    );
    session.feed(&[], Instant::now());
    assert_eq!(session.terminal().cursor.column, 1);
    assert_eq!(
        session
            .terminal()
            .screen()
            .ring
            .visible(0)
            .expect("visible row")
            .cells[0]
            .ch,
        u32::from('X')
    );
}

/// Local optional work stays runnable while the program declines reply credit.
mod work;
