//! Synthetic physical callbacks cross the live adapter, bus and VT encoder.
//! Literal byte expectations follow https://invisible-island.net/xterm/ctlseqs/.
#![allow(
    clippy::unwrap_used,
    reason = "assertions name missing reports and poisoned test outputs"
)]
use super::*;
use puzzle_term::{
    Origin, Session, SessionConfig,
    messages::Command,
    platform::{PtySize, Services, Transport, Waker},
};
use std::{
    sync::{Arc, Mutex},
    time::Instant,
};

/// Captured transport bytes remain inspectable after Session takes ownership.
struct Output(Arc<Mutex<Vec<u8>>>);
/// Captures accepted reports without a real PTY.
impl Transport for Output {
    /// Appends complete reports in dispatch order for byte assertions.
    fn write(&mut self, bytes: &[u8]) -> Result<(), String> {
        self.0.lock().unwrap().extend(bytes);
        Ok(())
    }
    /// Pointer tests do not change the child grid.
    fn resize(&mut self, _: PtySize) -> Result<(), String> {
        Ok(())
    }
    /// No OS handle exists in this test transport.
    fn close(&mut self) -> Result<(), String> {
        Ok(())
    }
}
/// Input tests never need an OS redraw or a timer.
struct Wake;
/// No frame is needed to execute a pointer command.
impl Waker for Wake {
    /// Discards render requests because byte dispatch must stand alone.
    fn request_frame(&mut self) {}
    /// No host clock is needed for immediate pointer encoding.
    fn wake_at(&mut self, _: Instant) {}
}
/// A device-pixel grid at 2× keeps pixel reports distinct from cell reports.
fn geometry() -> Geometry {
    Geometry {
        cell: [16, 32],
        grid: [80, 24],
    }
}

/// Actual bus execution must use modes written by the application.
fn transmitted(event: MouseEvent, modes: &str, shift_selects: bool) -> Vec<u8> {
    let bytes = Arc::new(Mutex::new(Vec::new()));
    let services = Services::new(Box::new(Output(bytes.clone())), Box::new(Wake));
    let mut session = Session::new(SessionConfig::default(), services);
    let settings = puzzle_term::settings::Settings {
        mouse_shift_capture: if shift_selects {
            puzzle_term::settings::ShiftCapture::Never
        } else {
            puzzle_term::settings::ShiftCapture::Always
        },
        ..Default::default()
    };
    session.apply_settings(&settings, Instant::now());
    session.feed(modes.as_bytes(), Instant::now());
    session.submit(Command::Mouse(event), Origin::User);
    session.pump(Instant::now());
    bytes.lock().unwrap().clone()
}

/// Each encoding has an independent literal wire expectation, including mods.
#[test]
fn desktop_mouse_encoding_table() {
    let mut adapter = Adapter::default();
    adapter.moved([49.0, 81.0], geometry(), 0, MouseModes::default());
    let press = adapter.button(0, true, geometry(), 4, 0);
    assert_eq!(
        (press.column, press.row, press.pixel_x, press.pixel_y),
        (2, 2, 33, 65)
    );
    for (mode, expected) in [
        ("", "\x1b[M0##"),
        ("\x1b[?1005h", "\x1b[M0##"),
        ("\x1b[?1006h", "\x1b[<16;3;3M"),
        ("\x1b[?1015h", "\x1b[48;3;3M"),
        ("\x1b[?1016h", "\x1b[<16;34;66M"),
    ] {
        assert_eq!(
            transmitted(press, &format!("\x1b[?1000h{mode}"), true),
            expected.as_bytes()
        );
    }
    let release = adapter.button(0, false, geometry(), 0, 10);
    assert_eq!(
        transmitted(release, "\x1b[?1000;1006h", true),
        b"\x1b[<0;3;3m"
    );
    assert!(transmitted(release, "\x1b[?9;1006h", true).is_empty());
    assert!(transmitted(press, "", true).is_empty());
    let shifted = MouseEvent {
        modifiers: Modifiers(1),
        ..press
    };
    assert!(transmitted(shifted, "\x1b[?1000;1006h", true).is_empty());
    assert_eq!(
        transmitted(shifted, "\x1b[?1000;1006h", false),
        b"\x1b[<4;3;3M"
    );
    let wheel = adapter.wheel([0.0, -1.0], geometry(), 0)[0];
    assert_eq!(
        transmitted(wheel, "\x1b[?1000;1006h", true),
        b"\x1b[<65;3;3M"
    );
    for (button, expected) in [
        (1, "\x1b[<1;3;3M"),
        (2, "\x1b[<2;3;3M"),
        (128, "\x1b[<128;3;3M"),
    ] {
        let event = adapter.button(button, true, geometry(), 0, 20);
        assert_eq!(
            transmitted(event, "\x1b[?1000;1006h", true),
            expected.as_bytes()
        );
    }
}

/// P12 coalesces and deduplicates in the encoding's coordinate unit.
#[test]
fn desktop_motion_tracking_and_coalescing() {
    let mut adapter = Adapter::default();
    let normal = MouseModes {
        tracking: MouseTracking::Normal,
        encoding: MouseEncoding::Sgr,
    };
    assert!(!adapter.moved([20.0, 20.0], geometry(), 0, normal));
    assert!(adapter.flush(normal).is_none());
    let any = MouseModes {
        tracking: MouseTracking::AnyMotion,
        ..normal
    };
    adapter.moved([20.0, 20.0], geometry(), 0, any);
    adapter.moved([49.0, 81.0], geometry(), 0, any);
    let motion = adapter.flush(any).unwrap();
    assert_eq!(
        transmitted(motion, "\x1b[?1003;1006h", true),
        b"\x1b[<35;3;3M"
    );
    assert!(adapter.flush(any).is_none());
    adapter.moved([50.0, 82.0], geometry(), 0, any);
    assert!(adapter.flush(any).is_none());
    let pixels = MouseModes {
        encoding: MouseEncoding::SgrPixels,
        ..any
    };
    adapter.moved([50.0, 82.0], geometry(), 0, pixels);
    assert_eq!(
        transmitted(adapter.flush(pixels).unwrap(), "\x1b[?1003;1016h", true),
        b"\x1b[<35;35;67M"
    );
    let drag = MouseModes {
        tracking: MouseTracking::ButtonMotion,
        ..normal
    };
    assert!(!adapter.moved([55.0, 81.0], geometry(), 0, drag));
    adapter.button(0, true, geometry(), 0, 0);
    assert!(adapter.moved([70.0, 90.0], geometry(), 0, drag));
    assert_eq!(
        transmitted(adapter.flush(drag).unwrap(), "\x1b[?1002;1006h", true),
        b"\x1b[<32;4;3M"
    );
    adapter.moved([90.0, 90.0], geometry(), 0, any);
    assert!(adapter.flush(normal).is_none());
}

/// Click chains survive releases; wheel fractions preserve direction/order.
#[test]
fn desktop_click_counts_and_wheel_fraction() {
    let mut adapter = Adapter::default();
    for (time, expected) in [(0, 1), (100, 2), (200, 3), (300, 1), (1000, 1)] {
        let press = adapter.button(0, true, geometry(), 0, time);
        assert_eq!(press.click_count, expected);
        adapter.button(0, false, geometry(), 0, time + 1);
    }
    assert!(adapter.wheel([0.4, -0.4], geometry(), 0).is_empty());
    let reports = adapter.wheel([0.6, -0.6], geometry(), 0);
    assert_eq!(
        reports.iter().map(|r| r.button).collect::<Vec<_>>(),
        [67, 65]
    );
}

/// Native identities are tested on the exact conversion used by winit callbacks.
#[test]
fn native_button_identity_table() {
    use winit::event::MouseButton;
    for (button, code) in [
        (MouseButton::Left, 0),
        (MouseButton::Middle, 1),
        (MouseButton::Right, 2),
        (MouseButton::Back, 128),
        (MouseButton::Forward, 129),
        (MouseButton::Other(5), 133),
    ] {
        assert_eq!(button_code(button), code);
    }
}

/// The held exit policy never writes into a dead PTY or closes on a click.
#[test]
fn pointer_after_exit_is_inert() {
    let bytes = Arc::new(Mutex::new(Vec::new()));
    let services = Services::new(Box::new(Output(bytes.clone())), Box::new(Wake));
    let mut session = Session::new(SessionConfig::default(), services);
    session.feed(b"\x1b[?1000;1006h", Instant::now());
    session.submit(
        Command::ChildExited(puzzle_term::messages::ExitInfo {
            code: Some(1),
            signal: None,
            runtime_ms: 1,
        }),
        Origin::Platform,
    );
    session.pump(Instant::now());
    let click = Adapter::default().button(0, true, geometry(), 0, 0);
    session.submit(Command::Mouse(click), Origin::User);
    session.pump(Instant::now());
    assert!(bytes.lock().unwrap().is_empty());
    assert!(!session.close_requested());
}

/// Shared wheel routing is exercised through synthetic native input units.
mod scrolling;

/// Synthetic gestures use the same signed-coordinate adapter as the window.
mod selection;
