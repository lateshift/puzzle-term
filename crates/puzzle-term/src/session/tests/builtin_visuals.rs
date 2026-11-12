//! Public-API visual sources remain distinct from PTY bytes and VT cells (§11.6).
use super::*;
use puzzle_overlay::draw::{Draw, Primitive};
use std::time::Duration;

/// Test the compiled provider list, not a second implementation of annotation layout.
fn draws(session: &TestHost, name: &str) -> Vec<Draw> {
    let builder = session.builder.as_ref().expect("builder");
    let (_, list) = builder
        .overlays
        .registry
        .lists()
        .find(|(_, list)| {
            list.draws.iter().any(|draw| match &draw.primitive {
                Primitive::Text(text) => text.text == name,
                _ => false,
            })
        })
        .expect("text provider");
    list.draws.clone()
}

/// Both scales share cursor anchors; only explicit acceptance may produce input.
#[test]
fn preedit_and_ghost_are_public_cursor_overlays_not_terminal_output() {
    for dpr in [1.0, 2.0] {
        let (mut session, writes, _) = fixture();
        let mut context = super::overlays::text_context(dpr);
        let now = Instant::now();
        session.set_text_context(&context);
        session.install_model_overlays();
        session.set_ghost_text(Some("suggested"));
        session.draw(now, &mut context).expect("ghost frame");
        let list = draws(&session, "suggested");
        assert!(
            list.iter()
                .any(|draw| matches!(draw.anchor, puzzle_overlay::anchor::Anchor::Cursor))
        );
        assert!(writes.lock().expect("writes").is_empty());
        assert_eq!(
            session.terminal.buffer().cursor().expect("cursor").column,
            0
        );
        session.submit(
            Command::Preedit(crate::messages::PreeditState {
                text: "compose".into(),
                cursor: Some(3),
            }),
            Origin::User,
        );
        session.pump(now);
        session.draw(now, &mut context).expect("preedit frame");
        let list = draws(&session, "compose");
        assert!(
            list.iter()
                .any(|draw| matches!(draw.primitive, Primitive::Rect(_)))
        );
        assert!(!list.iter().any(
            |draw| matches!(&draw.primitive, Primitive::Text(text) if text.text == "suggested")
        ));
        assert!(writes.lock().expect("writes").is_empty());
        // Clearing a candidate reveals the unchanged suggestion, without accepting it.
        session.submit(Command::Preedit(Default::default()), Origin::User);
        session.pump(now);
        session.draw(now, &mut context).expect("ghost restored");
        session.accept_ghost_text(now);
        assert_eq!(*writes.lock().expect("writes"), [b"suggested".to_vec()]);
        session.draw(now, &mut context).expect("clear suggestion");
        assert!(session.snapshot.overlays.models.visuals.ghost.is_empty());
        session.accept_ghost_text(now);
        assert_eq!(writes.lock().expect("writes").len(), 1);
    }
}

/// A progress source changes its width/color without contaminating application input.
#[test]
fn osc_progress_draws_and_clears_from_its_source() {
    let (mut session, writes, _) = fixture();
    let mut context = super::overlays::text_context(1.0);
    session.set_text_context(&context);
    session.install_model_overlays();
    let now = Instant::now();
    session.feed(b"\x1b]9;4;2;25\x07", now);
    let frame = session.draw(now, &mut context).expect("progress");
    let width = frame.target_size[0] as f32;
    assert!(
        frame
            .rects
            .iter()
            .any(|rect| rect.rect == [0.0, 0.0, width * 0.25, 2.0])
    );
    assert_eq!(
        session.snapshot.overlays.models.visuals.progress,
        Some((2, 25))
    );
    session.feed(b"\x1b]9;4;0;0\x07", now);
    let frame = session.draw(now, &mut context).expect("removed progress");
    assert!(
        !frame
            .rects
            .iter()
            .any(|rect| rect.rect[1] == 0.0 && rect.rect[3] == 2.0)
    );
    assert!(writes.lock().expect("writes").is_empty());
}

/// A reduced-motion bell has one static interval and then no further work (P2).
#[test]
fn bell_reduced_motion_is_static_and_finishes_with_zero_idle_allocations() {
    let (mut session, _, _) = fixture();
    let mut context = super::overlays::text_context(1.0);
    let now = Instant::now();
    session.set_text_context(&context);
    session.install_model_overlays();
    session.set_reduced_motion(true);
    session.set_cursor_settings(
        cursor_settings::CursorSettings {
            blink: cursor_settings::CursorBlink::Off,
            ..Default::default()
        },
        now,
    );
    session.feed(b"\x07", now);
    let frame = session.draw(now, &mut context).expect("bell");
    let borders: Vec<_> = frame
        .rects
        .iter()
        .filter(|rect| rect.stroke_width > 0.0)
        .copied()
        .collect();
    assert_eq!(borders.len(), 1);
    // An unrelated source change must neither fade nor restart a reduced-motion flash.
    session.feed(b"x", now + Duration::from_millis(100));
    let frame = session
        .draw(now + Duration::from_millis(100), &mut context)
        .expect("grid");
    assert!(
        frame
            .rects
            .iter()
            .any(|rect| borders.iter().any(|before| before.rect == rect.rect
                && before.color == rect.color
                && before.stroke_width == rect.stroke_width))
    );
    let end = now + Duration::from_millis(201);
    let frame = session.draw(end, &mut context).expect("final bell removal");
    assert!(!frame.rects.iter().any(|rect| rect.stroke_width > 0.0));
    session.draw(end, &mut context);
    let allocation =
        allocation_counter::measure(|| assert!(session.draw(end, &mut context).is_none()));
    assert_eq!(allocation.count_total, 0);
}
