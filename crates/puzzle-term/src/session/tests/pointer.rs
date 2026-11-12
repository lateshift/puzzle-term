//! Injected outside-drag time, source invalidation and steady selection allocation.
use super::*;
use crate::{
    pointer::{PointerInput, edge_speed},
    selection::SelectionCommand,
    vt::input::{Modifiers, MouseEvent, MouseKind},
};

/// Typed samples retain raw distance while protocol rows remain within the grid.
pub(super) fn sample(kind: MouseKind, y: f64) -> PointerInput {
    PointerInput {
        event: MouseEvent {
            kind,
            button: 0,
            column: 1,
            row: 0,
            pixel_x: 9,
            pixel_y: 0,
            modifiers: Modifiers(0),
            button_held: kind != MouseKind::Release,
            click_count: 1,
        },
        position: [9.0, y],
        cell: [8, 16],
    }
}
/// Pumps actual pointer input at a deterministic timestamp.
fn pointer(session: &mut TestHost, kind: MouseKind, y: f64, now: Instant) {
    session.submit(Command::Pointer(sample(kind, y)), Origin::User);
    session.pump(now);
}
/// Create history and a row-height policy before beginning a drag.
fn history() -> TestHost {
    let (mut session, _, _) = fixture();
    session.terminal.resize(12, 4);
    session.viewport.set_row_height(16);
    for _ in 0..50 {
        session.feed(b"abcdefghij\r\n", Instant::now());
    }
    session
}
/// Outside distance increases speed symmetrically and finite limits prevent runaway.
#[test]
fn edge_speed_and_stationary_autoscroll_timeline() {
    assert_eq!(edge_speed(0.0, 64.0, 16.0), 0.0);
    assert_eq!(edge_speed(64.0, 64.0, 16.0), 0.0);
    assert_eq!(edge_speed(-16.0, 64.0, 16.0), -15.0);
    assert_eq!(edge_speed(96.0, 64.0, 16.0), 27.0);
    assert_eq!(edge_speed(-1e9, 64.0, 16.0), -120.0);
    assert_eq!(edge_speed(f64::NAN, 64.0, 16.0), 0.0);
    let now = Instant::now();
    let mut near = history();
    let mut far = history();
    for (session, y) in [(&mut near, -16.0), (&mut far, -32.0)] {
        pointer(session, MouseKind::Press, 20.0, now);
        pointer(session, MouseKind::Move, y, now);
        assert!(session.scroll_animation_frame(now));
        session.advance_scroll(now + Duration::from_millis(40));
        assert!(session.selection_range().is_some());
    }
    assert!((near.viewport.scroll_px() - 9.6).abs() < 1e-6);
    assert!((far.viewport.scroll_px() - 17.28).abs() < 1e-6);
    pointer(
        &mut near,
        MouseKind::Release,
        -16.0,
        now + Duration::from_millis(40),
    );
    assert!(!near.pointer.animating(&near.terminal, &near.viewport));
    near.advance_scroll(now + Duration::from_millis(200));
    near.advance_scroll(now + Duration::from_millis(300));
    assert_eq!(near.viewport.scroll_px() % 16.0, 0.0);
    near.viewport.presented();
    assert!(!near.scroll_animation_frame(now + Duration::from_millis(310)));
}
/// Rewriting selected text or switching its screen must stop edge dragging immediately.
#[test]
fn edge_drag_cancellation_and_boundaries() {
    let now = Instant::now();
    for output in [b"\rZ".as_slice(), b"\x1b[?1049h\x1b[?1049l", b"\x1bc"] {
        let mut session = history();
        // Include a real character on the current row: a gap at column zero
        // after M12 snapping excludes that row from a backward selection.
        session.feed(b"last", now);
        pointer(&mut session, MouseKind::Press, 50.0, now);
        pointer(&mut session, MouseKind::Move, -16.0, now);
        assert!(
            session
                .pointer
                .animating(&session.terminal, &session.viewport)
        );
        session.feed(output, now);
        assert!(
            !session
                .pointer
                .animating(&session.terminal, &session.viewport)
        );
    }
    let mut session = history();
    session.submit(
        Command::ScrollTo(crate::viewport::ScrollTarget::Top),
        Origin::User,
    );
    session.pump(now);
    pointer(&mut session, MouseKind::Press, 20.0, now);
    pointer(&mut session, MouseKind::Move, -100.0, now);
    assert!(
        !session
            .pointer
            .animating(&session.terminal, &session.viewport)
    );
    session.submit(
        Command::ScrollTo(crate::viewport::ScrollTarget::Bottom),
        Origin::User,
    );
    session.pump(now);
    pointer(&mut session, MouseKind::Move, 100.0, now);
    assert!(
        !session
            .pointer
            .animating(&session.terminal, &session.viewport)
    );
}
/// Repeated warmed drag intervals must reuse row witnesses and never copy selected text.
#[test]
fn warm_selection_drag_allocates_nothing() {
    let mut session = history();
    let ids = session.buffer().line_ids().expect("history");
    let first = *ids.start();
    let last = *ids.end();
    let request = SelectionCommand::Range {
        kind: crate::vt::selection::SelectionKind::Simple,
        start: crate::vt::selection::SelectPoint {
            line: first,
            column: 1,
        },
        end: crate::vt::selection::SelectPoint {
            line: last,
            column: 5,
        },
    };
    session
        .session
        .selection
        .apply(&mut session.session.terminal, request);
    // Warm the first replacement too: the registry's release free-list grows once.
    session
        .session
        .selection
        .apply(&mut session.session.terminal, request);
    let counted = allocation_counter::measure(|| {
        for _ in 0..10 {
            session
                .session
                .selection
                .apply(&mut session.session.terminal, request);
        }
    });
    assert_eq!(counted.count_total, 0);
}
/// Copy-on-select uses the final release hit and enters the ordinary clipboard lane.
#[test]
fn pointer_copy_on_select_release() {
    let mut session = history();
    session.selection.copy_on_select = true;
    let now = Instant::now();
    pointer(&mut session, MouseKind::Press, 20.0, now);
    pointer(&mut session, MouseKind::Move, 40.0, now);
    pointer(&mut session, MouseKind::Release, 50.0, now);
    // The default MemoryClipboard's plain paste returns exactly the selected text.
    assert_eq!(
        session.clipboard_text().expect("clipboard"),
        Some(session.selection_text())
    );
}

/// Tracking can claim an active local edge drag; fractional text still settles afterward.
#[test]
fn tracking_claim_cancels_edge_drag_and_snaps() {
    let now = Instant::now();
    let mut session = history();
    pointer(&mut session, MouseKind::Press, 20.0, now);
    pointer(&mut session, MouseKind::Move, -16.0, now);
    session.advance_scroll(now + Duration::from_millis(40));
    assert!((session.viewport.scroll_px() - 9.6).abs() < 1e-6);
    session.feed(b"\x1b[?1000h", now + Duration::from_millis(50));
    assert!(
        !session
            .pointer
            .animating(&session.terminal, &session.viewport)
    );
    session.advance_scroll(now + Duration::from_millis(200));
    session.advance_scroll(now + Duration::from_millis(300));
    assert_eq!(session.viewport.scroll_px() % 16.0, 0.0);
    session.viewport.presented();
    assert!(!session.scroll_animation_frame(now + Duration::from_millis(310)));
}
