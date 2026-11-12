//! XTSHIFTESCAPE and settings share one policy in every host (M18.2).
//! Reference: xterm ctlseqs XTSHIFTESCAPE.
use super::*;
use crate::HostEvent;
use crate::settings::{Layer, SettingsStore};
use puzzle_bus::Value;
use puzzle_vt::input::{Modifiers, MouseEvent, MouseKind};

/// Settings and program requests are exercised through public admission paths.
fn configured(setting: Value, request: Option<bool>) -> TestHost {
    let (mut session, _, _) = fixture();
    let mut store = SettingsStore::default();
    assert!(
        store
            .apply_key("mouse.shiftCapture", setting, Layer::Session)
            .rejected
            .is_empty()
    );
    assert!(store.has_override("mouse.shiftCapture"));
    session.apply_settings(store.effective(), Instant::now());
    if let Some(capture) = request {
        session.feed(
            if capture { b"\x1b[>1s" } else { b"\x1b[>0s" },
            Instant::now(),
        );
    }
    session
}

/// False/true defer to an explicit program choice; always/never cannot be overridden.
#[test]
fn shift_capture_all_settings_requests_and_mouse_modes() {
    let settings = [
        Value::Bool(false),
        Value::Bool(true),
        Value::String("always".into()),
        Value::String("never".into()),
    ];
    for (index, setting) in settings.into_iter().enumerate() {
        for request in [None, Some(false), Some(true)] {
            let capture = match index {
                0 => request.unwrap_or(false),
                1 => request.unwrap_or(true),
                2 => true,
                _ => false,
            };
            for tracking in [None, Some(9), Some(1000), Some(1002), Some(1003)] {
                for encoding in [1005, 1006, 1015, 1016] {
                    for shift in [false, true] {
                        check_report(setting.clone(), request, capture, tracking, encoding, shift);
                    }
                }
            }
        }
    }
}

/// The application receives Shift only when both tracking and capture permit it.
fn check_report(
    setting: Value,
    request: Option<bool>,
    capture: bool,
    tracking: Option<u16>,
    encoding: u16,
    shift: bool,
) {
    let mut session = configured(setting, request);
    session.enable_host_events().expect("host observations");
    if let Some(mode) = tracking {
        session.feed(
            format!("\x1b[?{mode}h\x1b[?{encoding}h").as_bytes(),
            Instant::now(),
        );
    }
    assert_eq!(session.mouse_shift_selects(), !capture);
    let states: Vec<_> = std::iter::from_fn(|| session.take_event())
        .filter_map(|event| match event {
            HostEvent::InputState(state) => Some(state),
            _ => None,
        })
        .collect();
    assert_eq!(states.last().expect("policy event").shift_selects, !capture);
    let event = MouseEvent {
        button: 0,
        kind: MouseKind::Press,
        column: 1,
        row: 1,
        pixel_x: 8,
        pixel_y: 16,
        modifiers: Modifiers(if shift { Modifiers::SHIFT } else { 0 }),
        button_held: true,
        click_count: 1,
    };
    let modes = puzzle_vt::input::MouseModes::from_terminal(&session.terminal);
    let expected = puzzle_vt::input::encode_mouse(event, modes.tracking, modes.encoding, !capture);
    // The fixture's recorder verifies actual Session dispatch, not host-side filtering.
    let writes = Arc::new(Mutex::new(Vec::new()));
    session.services.transport = Box::new(RecordingTransport(writes.clone()));
    session.submit(Command::Mouse(event), Origin::User);
    session.pump(Instant::now());
    assert_eq!(writes.lock().expect("bytes").concat(), expected);
    assert_eq!(
        !expected.is_empty(),
        tracking.is_some() && (!shift || capture)
    );
}

/// InputState must change at parse time, without waiting for a frame.
#[test]
fn shift_capture_requests_publish_policy_and_reset() {
    let mut session = configured(Value::Bool(false), None);
    session.enable_host_events().expect("host events");
    while session.take_event().is_some() {}
    session.feed(b"\x1b[>1s", Instant::now());
    assert!(!session.mouse_shift_selects());
    assert!(
        std::iter::from_fn(|| session.take_event())
            .any(|e| matches!(e, HostEvent::InputState(s) if !s.shift_selects))
    );
    session.feed(b"\x1b[>2s\x1b[>0;1s\x1b[>0:1s", Instant::now());
    assert!(!session.mouse_shift_selects(), "malformed requests ignored");
    session.feed(b"\x1b[>s", Instant::now());
    assert!(session.mouse_shift_selects(), "omitted parameter is zero");
    session.feed(b"\x1b[>1s\x1bc", Instant::now());
    assert!(
        session.mouse_shift_selects(),
        "RIS restores configured default"
    );
}

/// The shared Pointer path selects locally or reports, just like direct Mouse.
#[test]
fn shift_capture_pointer_routing_uses_live_request() {
    for capture in [false, true] {
        let mut session = configured(Value::Bool(false), Some(capture));
        session.feed(b"text\x1b[?1000h\x1b[?1006h", Instant::now());
        let writes = Arc::new(Mutex::new(Vec::new()));
        session.services.transport = Box::new(RecordingTransport(writes.clone()));
        for (kind, column) in [
            (MouseKind::Press, 0),
            (MouseKind::Move, 2),
            (MouseKind::Release, 2),
        ] {
            session.submit(
                Command::Pointer(crate::pointer::PointerInput {
                    event: MouseEvent {
                        kind,
                        button: 0,
                        column,
                        row: 0,
                        pixel_x: column * 8,
                        pixel_y: 0,
                        modifiers: Modifiers(Modifiers::SHIFT),
                        button_held: kind != MouseKind::Release,
                        click_count: 1,
                    },
                    position: [f64::from(column * 8), 0.0],
                    cell: [8, 16],
                }),
                Origin::User,
            );
            session.pump(Instant::now());
        }
        assert_eq!(session.selection_range().is_none(), capture);
        if capture {
            assert_eq!(
                writes.lock().expect("report").concat(),
                b"\x1b[<4;1;1M\x1b[<4;3;1m"
            );
        } else {
            assert!(writes.lock().expect("local selection").is_empty());
            // M12 uses gaps: the pointer at column two’s left edge excludes that cell.
            assert_eq!(session.selection_text(), "te");
        }
    }
}
