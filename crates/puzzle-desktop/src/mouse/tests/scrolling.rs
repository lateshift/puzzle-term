//! Synthetic winit wheel callbacks execute the same command as the live window.
use super::*;
use puzzle_term::{settings::Settings, viewport::ScrollInput};
use winit::{dpi::PhysicalPosition, event::MouseScrollDelta};
/// One routing row combines application modes with the physical gesture.
#[derive(Clone, Copy)]
struct Case {
    /// Zero disables reports; other entries are each supported tracking family.
    tracking: u16,
    /// Alternate screen has no local scrollback.
    alternate: bool,
    /// The application can explicitly disable mode 1007.
    alternate_scroll: bool,
    /// Cursor-key mode chooses CSI versus SS3.
    cursor: bool,
    /// Shift forces local movement regardless of selection policy.
    shift: bool,
    /// Two half-row pixel samples must become one protocol step.
    pixels: bool,
}
/// Builds retained output before installing the table's application modes.
fn setup(case: Case) -> (Session, Arc<Mutex<Vec<u8>>>, Adapter, Geometry) {
    let bytes = Arc::new(Mutex::new(Vec::new()));
    let mut session = Session::new(
        SessionConfig {
            columns: 12,
            rows: 4,
            ..Default::default()
        },
        Services::new(Box::new(Output(bytes.clone())), Box::new(Wake)),
    );
    session.apply_settings(
        &Settings {
            scrolling_multiplier: 1.0,
            scrolling_smooth: false,
            scrolling_snap_to_rows: false,
            mouse_shift_capture: puzzle_term::settings::ShiftCapture::Always,
            ..Default::default()
        },
        Instant::now(),
    );
    session.feed(
        b"one\r\ntwo\r\nthree\r\nfour\r\nfive\r\nsix\r\nseven",
        Instant::now(),
    );
    let modes = format!(
        "\x1b[?1006h\x1b[?1007{}\x1b[?1{}{}{}",
        if case.alternate_scroll { 'h' } else { 'l' },
        if case.cursor { 'h' } else { 'l' },
        if case.alternate { "\x1b[?1049h" } else { "" },
        if case.tracking == 0 {
            String::new()
        } else {
            format!("\x1b[?{}h", case.tracking)
        }
    );
    session.feed(modes.as_bytes(), Instant::now());
    let geometry = Geometry {
        cell: [8, 1],
        grid: [12, 4],
    };
    let mut adapter = Adapter::default();
    adapter.moved([33.0, 18.0], geometry, 0, MouseModes::default());
    (session, bytes, adapter, geometry)
}
/// Literal reports/keys pin routing independently of the production encoder.
fn assert_case(case: Case) {
    let (mut session, bytes, mut adapter, geometry) = setup(case);
    for _ in 0..if case.pixels { 2 } else { 1 } {
        let delta = if case.pixels {
            MouseScrollDelta::PixelDelta(PhysicalPosition::new(0.0, 0.5))
        } else {
            MouseScrollDelta::LineDelta(0.0, 1.0)
        };
        let (command, horizontal) = adapter.wheel_command(delta, geometry, u8::from(case.shift));
        assert!(horizontal.is_empty());
        let command = command.unwrap();
        assert!(
            matches!(
                &command,
                Command::Wheel {
                    delta: ScrollInput::Pixels(-0.5),
                    ..
                }
            ) == case.pixels
        );
        session.submit(command, Origin::User);
        session.pump(Instant::now());
    }
    let expected: &[u8] = if case.shift {
        b""
    } else if case.tracking != 0 {
        b"\x1b[<64;3;3M"
    } else if case.alternate && case.alternate_scroll {
        if case.cursor { b"\x1bOA" } else { b"\x1b[A" }
    } else {
        b""
    };
    assert_eq!(bytes.lock().unwrap().as_slice(), expected);
    let local = case.shift || case.tracking == 0 && !(case.alternate && case.alternate_scroll);
    assert_eq!(
        session.viewport().scroll_px(),
        if local && !case.alternate { 1.0 } else { 0.0 }
    );
}
/// Every supported tracking mode has precedence over mode 1007; Shift wins all.
#[test]
fn wheel_routing_table() {
    for tracking in [0, 9, 1000, 1002, 1003] {
        for alternate in [false, true] {
            for alternate_scroll in [false, true] {
                for cursor in [false, true] {
                    for shift in [false, true] {
                        for pixels in [false, true] {
                            assert_case(Case {
                                tracking,
                                alternate,
                                alternate_scroll,
                                cursor,
                                shift,
                                pixels,
                            });
                        }
                    }
                }
            }
        }
    }
}
/// Less-style scrolling sends one literal key per complete step in both directions.
#[test]
fn alternate_scroll_exact_keys_and_reset_policy() {
    let case = Case {
        tracking: 0,
        alternate: true,
        alternate_scroll: true,
        cursor: true,
        shift: false,
        pixels: false,
    };
    let (mut session, bytes, mut adapter, geometry) = setup(case);
    session.feed(b"\x1b[>31u", Instant::now());
    for y in [2.0, -3.0] {
        session.submit(
            adapter
                .wheel_command(MouseScrollDelta::LineDelta(0.0, y), geometry, 0)
                .0
                .unwrap(),
            Origin::User,
        );
        session.pump(Instant::now());
    }
    assert_eq!(
        bytes.lock().unwrap().as_slice(),
        b"\x1bOA\x1bOA\x1bOB\x1bOB\x1bOB"
    );
    // User-disabled alternate scroll remains disabled after either reset form.
    session.apply_settings(
        &Settings {
            mouse_alternate_scroll: false,
            ..Default::default()
        },
        Instant::now(),
    );
    for reset in [b"\x1b[?1007h\x1b[!p".as_slice(), b"\x1b[?1007h\x1bc"] {
        session.feed(reset, Instant::now());
        assert_eq!(session.terminal().modes.get(true, 1007), Some(false));
    }
}

/// Half a report cannot cross a local gesture or a protocol destination change.
#[test]
fn pixel_fraction_stays_with_destination() {
    let case = Case {
        tracking: 1000,
        alternate: false,
        alternate_scroll: true,
        cursor: false,
        shift: false,
        pixels: true,
    };
    let (mut session, bytes, mut adapter, geometry) = setup(case);
    for (modes, mods) in [
        ("", 0),
        ("", Modifiers::SHIFT),
        ("", 0),
        ("\x1b[?1000l\x1b[?1049h", 0),
    ] {
        session.feed(modes.as_bytes(), Instant::now());
        let delta = MouseScrollDelta::PixelDelta(PhysicalPosition::new(0.0, 0.5));
        session.submit(
            adapter.wheel_command(delta, geometry, mods).0.unwrap(),
            Origin::User,
        );
        session.pump(Instant::now());
        assert!(bytes.lock().unwrap().is_empty());
    }
    // The new arrow destination accumulates its own second half-row.
    let delta = MouseScrollDelta::PixelDelta(PhysicalPosition::new(0.0, 0.5));
    session.submit(
        adapter.wheel_command(delta, geometry, 0).0.unwrap(),
        Origin::User,
    );
    session.pump(Instant::now());
    assert_eq!(bytes.lock().unwrap().as_slice(), b"\x1b[A");
}
