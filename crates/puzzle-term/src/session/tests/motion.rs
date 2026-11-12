//! D-50 physical samples are interpreted by the Session owner, after queued output.
use super::*;
use crate::{
    SessionMessage,
    pointer::PointerInput,
    vt::input::{Modifiers, MouseEvent, MouseKind},
};

/// Coordinates deliberately describe the host's older grid; signed pixels are authoritative.
fn sample(x: f64, modifiers: u8) -> PointerInput {
    PointerInput {
        position: [x, 21.0],
        cell: [8, 16],
        event: MouseEvent {
            kind: MouseKind::Move,
            button: 3,
            column: 1,
            row: 1,
            pixel_x: x.max(0.0) as u32,
            pixel_y: 21,
            modifiers: Modifiers(modifiers),
            button_held: false,
            click_count: 1,
        },
    }
}

/// Apply the actual message rather than bypassing owner-side mode and overlay routing.
fn motion(session: &mut TestHost, input: PointerInput, now: Instant) {
    session
        .apply(SessionMessage::PointerMotion(input), now)
        .expect("motion");
    // Main-thread observations are consumed between callbacks in the live host.
    while session.take_event().is_some() {}
}

/// The next sample uses changed geometry/modes without waiting for an InputState event.
#[test]
fn queued_motion_uses_current_owner_geometry_and_encoding() {
    let (mut session, writes, _) = fixture();
    let context = super::overlays::text_context(1.0);
    session.set_text_context(&context);
    let now = Instant::now();
    let width = context.metrics().width_px;
    let x = f64::from(width * 4 + 1);
    session.feed(b"\x1b[?1003h\x1b[?1006h", now);
    motion(&mut session, sample(x, 0), now);
    motion(&mut session, sample(x + 1.0, 0), now);
    let row = 21 / context.metrics().height_px + 1;
    assert_eq!(
        writes.lock().expect("writes").concat(),
        format!("\x1b[<35;5;{row}M").as_bytes()
    );
    // 1016 switches the identity to pixels even at the same physical position.
    session.feed(b"\x1b[?1016h", now);
    motion(&mut session, sample(x + 1.0, 0), now);
    motion(&mut session, sample(x + 2.0, 0), now);
    let recorded = writes.lock().expect("writes");
    assert_eq!(recorded.len(), 3);
    assert_eq!(
        recorded[1],
        format!("\x1b[<35;{};22M", x as u32 + 2).as_bytes()
    );
    assert_eq!(
        recorded[2],
        format!("\x1b[<35;{};22M", x as u32 + 3).as_bytes()
    );
}

/// Shift locally selects; a new button transition must not inherit a previous drag's identity.
#[test]
fn shift_override_and_new_drag_preserve_motion_reports() {
    let (mut session, writes, _) = fixture();
    let now = Instant::now();
    session.feed(b"\x1b[?1003h\x1b[?1006h", now);
    motion(&mut session, sample(9.0, Modifiers::SHIFT), now);
    assert!(writes.lock().expect("writes").is_empty());
    motion(&mut session, sample(9.0, 0), now);
    motion(&mut session, sample(9.0, 0), now);
    assert_eq!(writes.lock().expect("writes").len(), 1);
    let mut press = sample(9.0, 0);
    press.event.kind = MouseKind::Press;
    press.event.button = 0;
    press.event.button_held = true;
    session
        .apply(
            SessionMessage::Command(Command::Pointer(press), Origin::User),
            now,
        )
        .expect("press");
    motion(&mut session, sample(9.0, 0), now);
    assert_eq!(writes.lock().expect("writes").len(), 3);
}

/// Repeated idle hover needs neither cluster storage nor another host event allocation.
#[test]
fn warm_suppressed_motion_allocates_nothing() {
    let (mut session, _, _) = fixture();
    let now = Instant::now();
    motion(&mut session, sample(9.0, 0), now);
    let counted = allocation_counter::measure(|| {
        for _ in 0..32 {
            motion(&mut session, sample(9.0, 0), now);
        }
    });
    assert_eq!(counted.count_total, 0);
}
