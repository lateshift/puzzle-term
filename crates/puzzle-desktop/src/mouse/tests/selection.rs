//! Direct-to-core selection policy and the live view-local handoff regression (§6.9).
use super::*;
use puzzle_term::selection::{CopyTarget, SelectionCommand};
use puzzle_term::vt::view::Format;

/// Small physical grid makes independent expected copied strings easy to read.
fn grid() -> Geometry {
    Geometry {
        cell: [8, 16],
        grid: [12, 4],
    }
}
/// Output and pointer input share the public Session bus without a native window.
fn fixture() -> (Session, Adapter, Arc<Mutex<Vec<u8>>>) {
    let bytes = Arc::new(Mutex::new(Vec::new()));
    let services = Services::new(Box::new(Output(bytes.clone())), Box::new(Wake));
    let mut session = Session::new(
        SessionConfig {
            columns: 12,
            rows: 4,
            ..Default::default()
        },
        services,
    );
    session.feed(b"foo bar baz!\r\nsecond line\r\nthird", Instant::now());
    (session, Adapter::default(), bytes)
}
/// Native motion is sampled once before the shared model receives the hit.
fn move_to(session: &mut Session, adapter: &mut Adapter, col: f64, row: f64, mods: u8) {
    let modes = MouseModes::from_terminal(session.terminal());
    adapter.moved(
        [16.0 + col * 8.0 + 1.0, 16.0 + row * 16.0 + 1.0],
        grid(),
        mods,
        modes,
    );
    if let Some(input) = adapter.flush_pointer(grid(), modes, session.mouse_shift_selects()) {
        session.submit(Command::Pointer(input), Origin::User);
        session.pump(Instant::now());
    }
}
/// Press/release stay immediate, retaining the adapter's actual click chain.
fn button(session: &mut Session, adapter: &mut Adapter, down: bool, mods: u8, ms: u64) {
    let event = adapter.button(0, down, grid(), mods, ms);
    session.submit(
        Command::Pointer(adapter.pointer_input(event, grid())),
        Origin::User,
    );
    session.pump(Instant::now());
}

/// Drag, repeated clicks, block and Shift extension have independently authored text.
#[test]
fn desktop_pointer_selection_table() {
    // M12 selects a character past its midpoint; the endpoint remains in the
    // same reported cell. Expanded and rectangular cases retain their old hits.
    for (count, mods, endpoint, expected) in [
        (1, 0, (6.5, 0.0), "foo bar"),
        (2, 0, (1.0, 0.0), "foo"),
        (3, 0, (1.0, 0.0), "foo bar baz!"),
        (1, 2, (2.0, 1.0), "foo\nsec\n"),
    ] {
        let (mut session, mut adapter, bytes) = fixture();
        move_to(&mut session, &mut adapter, 0.0, 0.0, mods);
        for click in 0..count {
            button(&mut session, &mut adapter, true, mods, click * 20);
            if click + 1 != count {
                button(&mut session, &mut adapter, false, mods, click * 20 + 1);
            }
        }
        if count == 1 {
            move_to(&mut session, &mut adapter, endpoint.0, endpoint.1, mods);
        }
        button(&mut session, &mut adapter, false, mods, 100);
        assert_eq!(
            session.selection_text(),
            expected,
            "count={count}, mods={mods}"
        );
        assert!(bytes.lock().unwrap().is_empty());
    }
    let (mut session, mut adapter, _) = fixture();
    move_to(&mut session, &mut adapter, 1.0, 0.0, 0);
    button(&mut session, &mut adapter, true, 0, 0);
    move_to(&mut session, &mut adapter, 3.0, 0.0, 0);
    button(&mut session, &mut adapter, false, 0, 10);
    move_to(&mut session, &mut adapter, 6.5, 0.0, 1);
    button(&mut session, &mut adapter, true, 1, 1000);
    button(&mut session, &mut adapter, false, 1, 1010);
    assert_eq!(session.selection_text(), "oo bar");
}

/// Every tracking mode reports normally and honors exactly the enabled Shift policy.
#[test]
fn desktop_tracking_selection_table() {
    for tracking in [9, 1000, 1002, 1003] {
        for shift in [false, true] {
            for shift_selects in [false, true] {
                let (mut session, mut adapter, bytes) = fixture();
                session.apply_settings(
                    &puzzle_term::settings::Settings {
                        mouse_shift_capture: if shift_selects {
                            puzzle_term::settings::ShiftCapture::Never
                        } else {
                            puzzle_term::settings::ShiftCapture::Always
                        },
                        ..Default::default()
                    },
                    Instant::now(),
                );
                session.feed(format!("\x1b[?{tracking};1006h").as_bytes(), Instant::now());
                let mods = u8::from(shift);
                move_to(&mut session, &mut adapter, 0.0, 0.0, mods);
                button(&mut session, &mut adapter, true, mods, 0);
                // Cross the text midpoint without changing the application cell.
                move_to(&mut session, &mut adapter, 2.5, 0.0, mods);
                button(&mut session, &mut adapter, false, mods, 10);
                if shift && shift_selects {
                    assert_eq!(session.selection_text(), "foo");
                    assert!(bytes.lock().unwrap().is_empty());
                } else {
                    assert!(session.selection_range().is_none());
                    assert!(bytes.lock().unwrap().windows(2).any(|b| b == b"\x1b["));
                }
            }
        }
    }
}

/// A single click clears selection; triple click follows all soft-wrapped segments.
#[test]
fn desktop_click_clear_and_wrapped_line() {
    let (mut session, mut adapter, _) = fixture();
    session.feed(b"\x1bcabcdefghijklmnop", Instant::now());
    move_to(&mut session, &mut adapter, 2.0, 1.0, 0);
    for click in 0..3 {
        button(&mut session, &mut adapter, true, 0, click * 20);
        button(&mut session, &mut adapter, false, 0, click * 20 + 1);
    }
    assert_eq!(session.selection_text(), "abcdefghijklmnop");
    button(&mut session, &mut adapter, true, 0, 1000);
    button(&mut session, &mut adapter, false, 0, 1010);
    assert!(session.selection_range().is_none());
    // The public copy request stays separate from gesture handling and has no PTY bytes.
    session.submit(Command::Select(SelectionCommand::All), Origin::Api);
    session.submit(
        Command::Copy(CopyTarget::Selection(Format::PlainText)),
        Origin::User,
    );
    session.pump(Instant::now());
}

/// Releasing Shift during tracking ends local ownership even in press-only report modes.
#[test]
fn tracking_shift_release_ends_local_drag() {
    for mode in [9, 1000] {
        let (mut session, mut adapter, bytes) = fixture();
        session.feed(format!("\x1b[?{mode};1006h").as_bytes(), Instant::now());
        move_to(&mut session, &mut adapter, 0.0, 0.0, 1);
        button(&mut session, &mut adapter, true, 1, 0);
        move_to(&mut session, &mut adapter, 2.5, 0.0, 1);
        assert_eq!(session.selection_text(), "foo");
        // These modes ignore application motion, but the local drag must still end.
        move_to(&mut session, &mut adapter, 4.0, 0.0, 0);
        // Restoring Shift without a fresh press must not resume the canceled drag.
        move_to(&mut session, &mut adapter, 5.0, 0.0, 1);
        assert_eq!(session.selection_text(), "foo");
        assert!(bytes.lock().unwrap().is_empty());
    }
}

/// The embedding seam must receive raw view points, not the legacy core inset.
#[test]
fn host_handoff_keeps_exact_subcell_positions_with_padding_and_gutter() {
    for scale in [1.0, 2.0] {
        for padding in [0.0, 4.0, 8.0] {
            for gutter in [0.0, 16.0] {
                let origin = padding + gutter;
                for column in [1.0, 2.0] {
                    for fraction in [0.25, 0.5, 0.75] {
                        let logical = [origin + (column + fraction) * 8.0, padding + 8.0];
                        let mut adapter = Adapter::default();
                        adapter.moved(logical.map(|n| n * scale), grid(), 0, MouseModes::default());
                        let event = adapter.button(0, true, grid(), 0, 0);
                        let host = adapter.surface_pointer(event, scale);
                        assert_eq!(
                            host.position, logical,
                            "scale={scale}, padding={padding}, gutter={gutter}"
                        );
                        // Motion and release retain the same raw origin as the press.
                        adapter.moved(logical.map(|n| n * scale), grid(), 0, MouseModes::default());
                        let motion = adapter.take_motion().unwrap();
                        assert_eq!(adapter.surface_pointer(motion, scale).position, logical);
                        let release = adapter.button(0, false, grid(), 0, 1);
                        assert_eq!(adapter.surface_pointer(release, scale).position, logical);
                    }
                }
            }
        }
    }
}
