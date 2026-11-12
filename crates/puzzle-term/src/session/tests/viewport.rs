//! Viewport input policy and observable facts through public Session commands.
use super::*;
use crate::viewport::ScrollTarget;
/// Unencoded keys leave history visible; bytes and paste follow by default.
#[test]
fn input_following_and_viewport_facts() {
    let (mut session, writes, _) = fixture();
    let now = Instant::now();
    for _ in 0..30 {
        session.feed(b"history\r\n", now);
    }
    session.submit(Command::ScrollTo(ScrollTarget::Top), Origin::Api);
    session.pump(now);
    assert!(!session.viewport().following());
    // A bare Super chord cannot produce legacy input and must not yank history.
    let mut key = KeyEvent::press(Key::Char('c'));
    key.modifiers = crate::vt::input::Modifiers(crate::vt::input::Modifiers::SUPER);
    session.submit(Command::Key(key), Origin::User);
    session.pump(now);
    assert!(!session.viewport().following());
    assert!(writes.lock().expect("writes").is_empty());
    session.submit(Command::Text("x".into()), Origin::User);
    session.pump(now);
    assert!(session.viewport().following());
    session.submit(Command::ScrollTo(ScrollTarget::Top), Origin::Api);
    session.pump(now);
    session.submit(Command::Paste(String::new()), Origin::User);
    session.pump(now);
    assert!(session.viewport().following());
    let settings = crate::settings::Settings {
        scrolling_on_input: false,
        ..Default::default()
    };
    session.apply_settings(&settings, now);
    session.submit(Command::ScrollTo(ScrollTarget::Top), Origin::Api);
    session.pump(now);
    session.submit(Command::Text("y".into()), Origin::User);
    session.pump(now);
    assert!(!session.viewport().following());
}

/// A finite scroll and snap leave no extra frame requests or idle work.
#[test]
fn motion_session_stops_requesting_frames() {
    let (mut session, _, wakes) = fixture();
    let start = Instant::now();
    for _ in 0..30 {
        session.feed(b"row\r\n", start);
    }
    session.feed(b"\x1b[?25l", start);
    let pointer = crate::vt::input::MouseEvent {
        button: 64,
        kind: crate::vt::input::MouseKind::Wheel,
        column: 0,
        row: 0,
        pixel_x: 0,
        pixel_y: 0,
        modifiers: crate::vt::input::Modifiers(0),
        button_held: false,
        click_count: 0,
    };
    session.submit(
        Command::Wheel {
            delta: crate::viewport::ScrollInput::Rows(-1.0),
            pointer,
        },
        Origin::User,
    );
    session.pump(start);
    assert!(session.scroll_animation_frame(start));
    for ms in [0, 60, 120, 150, 250] {
        session.publish(start + Duration::from_millis(ms));
    }
    assert!(!session.scroll_animation_frame(start + Duration::from_secs(1)));
    let count = wakes.load(Ordering::Relaxed);
    for ms in [300, 400, 1000] {
        assert!(session.publish(start + Duration::from_millis(ms)).is_none());
    }
    assert_eq!(wakes.load(Ordering::Relaxed), count);
}
