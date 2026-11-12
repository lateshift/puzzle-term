//! M9.4 behavior through public Session input, M8 data and submitted frame contracts.
use super::*;
/// Session query editing, reopening and configured clipboard behavior.
mod query;
use crate::{
    overlay::provider::{PointerEvent, PointerKind},
    search::SearchCommand,
    viewport::{ScrollTarget, scrollbar::MarkerKind},
    vt::input::Modifiers,
};

/// Runs one accepted key/text command at the caller's injected time.
fn input(session: &mut TestHost, command: Command, now: Instant) {
    session.submit(command, Origin::User);
    session.pump(now);
}

/// An unmodified physical key is the native adapter's accepted encoder event.
fn key(session: &mut TestHost, key: Key, now: Instant) {
    input(session, Command::Key(KeyEvent::press(key)), now);
}

/// Test samples use real M8 geometry at both scales, with cursor blink disabled.
fn scroll_fixture(dpr: f32) -> (TestHost, TextContext, Instant) {
    let (mut session, _, _) = fixture();
    let context = super::overlays::text_context(dpr);
    let now = Instant::now();
    session.set_text_context(&context);
    session.install_model_overlays();
    session.set_cursor_settings(
        cursor_settings::CursorSettings {
            blink: cursor_settings::CursorBlink::Off,
            ..Default::default()
        },
        now,
    );
    for _ in 0..100 {
        session.feed(b"row\r\n", now);
    }
    session.scroll_to(ScrollTarget::Top);
    session.pump(now);
    (session, context, now)
}

/// One provider rectangle must equal the exact physical M8 bounds and given alpha.
fn thumb(session: &mut TestHost, context: &mut TextContext, now: Instant) -> Option<u8> {
    let [x, _, w, _] = session.viewport().scrollbar.track();
    let [y, h] = session.viewport().scrollbar.thumb();
    let frame = session.draw(now, context)?;
    let expected = [x as f32, y as f32, w as f32, h as f32];
    frame
        .rects
        .iter()
        .find(|rect| rect.rect == expected)
        .map(|rect| rect.color.to_le_bytes()[3])
}

#[test]
fn scrollbar_geometry_and_finite_auto_hide_at_two_dprs() {
    for dpr in [1.0, 2.0] {
        let (mut session, mut context, now) = scroll_fixture(dpr);
        session.draw(now, &mut context).expect("initial geometry");
        assert_eq!(
            thumb(&mut session, &mut context, now + Duration::from_millis(50)),
            Some(51)
        );
        assert_eq!(
            thumb(&mut session, &mut context, now + Duration::from_millis(100)),
            Some(102)
        );
        assert!(
            session
                .draw(now + Duration::from_millis(799), &mut context)
                .is_none()
        );
        assert_eq!(
            thumb(&mut session, &mut context, now + Duration::from_millis(800)),
            Some(102)
        );
        assert_eq!(
            thumb(&mut session, &mut context, now + Duration::from_millis(875)),
            Some(51)
        );
        assert_eq!(
            thumb(&mut session, &mut context, now + Duration::from_millis(950)),
            None
        );
        let allocations = allocation_counter::measure(|| {
            assert!(
                session
                    .draw(now + Duration::from_secs(2), &mut context)
                    .is_none()
            );
        });
        assert_eq!(allocations.count_total, 0);
        // Reduced motion removes ink at the same 800-ms boundary, with no fade frames.
        let (mut session, mut context, now) = scroll_fixture(dpr);
        session.set_reduced_motion(true);
        session.draw(now, &mut context).expect("opaque initial");
        assert_eq!(
            thumb(&mut session, &mut context, now + Duration::from_millis(800)),
            None
        );
        assert!(
            session
                .draw(now + Duration::from_millis(900), &mut context)
                .is_none()
        );
    }
}

/// Shared routing consumes a pressed/released region before any host selection path.
fn pointer(session: &mut TestHost, kind: PointerKind, point: [f32; 2], now: Instant) {
    let result = session.route_overlay_pointer(
        &PointerEvent {
            kind,
            point,
            button: Some(0),
            modifiers: 0,
        },
        now,
    );
    assert!(result.handled, "overlay owns {kind:?}");
}

#[test]
fn scrollbar_drag_page_hover_and_pill_use_m8_operations() {
    for dpr in [1.0, 2.0] {
        let (mut session, mut context, now) = scroll_fixture(dpr);
        session.set_reduced_motion(true);
        session.draw(now, &mut context).expect("regions");
        session.presented();
        let [x, y, w, h] = session.viewport().scrollbar.track();
        let [top, extent] = session.viewport().scrollbar.thumb();
        let point = |x: f64, y: f64| [x as f32 / dpr, y as f32 / dpr];
        pointer(
            &mut session,
            PointerKind::Press,
            point(x + w / 2.0, top + extent / 2.0),
            now,
        );
        // Capture lets this move reach the thumb even beyond its old hit rectangle.
        pointer(
            &mut session,
            PointerKind::Move,
            point(x + w / 2.0, y + h - extent / 2.0),
            now,
        );
        pointer(
            &mut session,
            PointerKind::Release,
            point(x + w / 2.0, y + h - extent / 2.0),
            now,
        );
        assert!(session.viewport().following());
        session.draw(now, &mut context).expect("bottom geometry");
        session.presented();
        page_and_pill(&mut session, &mut context, dpr, now);
    }
}

/// Track paging and the new-output button reuse public M8 navigation at each scale.
fn page_and_pill(session: &mut TestHost, context: &mut TextContext, dpr: f32, now: Instant) {
    let [x, y, w, _] = session.viewport().scrollbar.track();
    let point = |x: f64, y: f64| [x as f32 / dpr, y as f32 / dpr];
    let before = session.viewport().scroll_px();
    pointer(
        session,
        PointerKind::Press,
        point(x + w / 2.0, y + 1.0),
        now,
    );
    pointer(
        session,
        PointerKind::Release,
        point(x + w / 2.0, y + 1.0),
        now,
    );
    assert_eq!(
        session.viewport().scroll_px() - before,
        (session.terminal().size().1 - 1) as f64 * f64::from(context.metrics().height_px)
    );
    session.feed(b"new\r\n", now);
    assert!(session.viewport().new_output());
    let frame = session.draw(now, context).expect("pill");
    let size = frame.target_size.map(|v| v as f32 / dpr);
    let pill = [size[0] / 2.0, size[1] - 24.0];
    session.presented();
    pointer(session, PointerKind::Press, pill, now);
    pointer(session, PointerKind::Release, pill, now);
    assert!(session.viewport().following());
    assert!(!session.viewport().new_output());
    assert!(session.selection_range().is_none());
}

#[test]
fn every_marker_source_and_search_panel_are_drawn() {
    for dpr in [1.0, 2.0] {
        let (mut session, _, _) = fixture();
        let mut context = super::overlays::text_context(dpr);
        session.set_text_context(&context);
        crate::test_support::overlay_scene::models::install(&mut session).expect("model fixture");
        let now = Instant::now();
        session.draw(now, &mut context).expect("model frame");
        let markers = session.viewport().scrollbar.markers();
        for kind in [
            MarkerKind::Prompt(Some(0)),
            MarkerKind::Prompt(Some(1)),
            MarkerKind::Match,
            MarkerKind::Selection,
            MarkerKind::Bell,
        ] {
            assert!(markers.iter().any(|m| m.kind == kind), "missing {kind:?}");
        }
        assert_eq!(session.search_state().count, 3);
        let model = &session.snapshot.overlays.models;
        assert_eq!(model.markers.len(), markers.len());
        assert_eq!(model.search.query, "hit");
        assert_eq!(model.search.count, 3);
        assert!(model.search.open && model.new_output);
        for (sample, source) in model.markers.iter().zip(markers) {
            assert_eq!((sample.start, sample.end), (source.start, source.end));
        }
    }
}

#[test]
fn scrollbar_release_applies_final_position_without_a_motion_frame() {
    for dpr in [1.0, 2.0] {
        let (mut session, mut context, now) = scroll_fixture(dpr);
        session.draw(now, &mut context).expect("regions");
        session.presented();
        let [x, y, w, h] = session.viewport().scrollbar.track();
        let [top, extent] = session.viewport().scrollbar.thumb();
        let point = |y: f64| [(x + w / 2.0) as f32 / dpr, y as f32 / dpr];
        pointer(
            &mut session,
            PointerKind::Press,
            point(top + extent / 2.0),
            now,
        );
        // No Move callback: the host can release before its coalesced rAF runs.
        pointer(
            &mut session,
            PointerKind::Release,
            point(y + h + extent),
            now,
        );
        assert!(session.viewport().following());
    }
}
