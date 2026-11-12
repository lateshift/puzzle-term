//! Link gestures use presented cells, deterministic delay and final command policy.
use super::*;
use crate::overlay::{
    draw::Primitive,
    provider::{PointerEvent, PointerKind},
};
use crate::platform::UrlOpener;

/// A recorder proves opening is a host effect, never bytes sent to the child.
struct Opens(Arc<Mutex<Vec<String>>>);
impl UrlOpener for Opens {
    fn open(&mut self, uri: &str) -> Result<(), String> {
        self.0.lock().expect("link witness").push(uri.into());
        Ok(())
    }
}
/// Samples actual compiled draw commands without inventing tooltip state.
fn has_tooltip(session: &TestHost) -> bool {
    session.builder.as_ref().expect("link witness").overlays.registry.lists().any(|(_, list)|
        list.draws.iter().any(|draw| matches!(&draw.primitive, Primitive::Text(text) if text.text == "https://a.test")))
}
/// OSC grouping, two scales, exact delay, harmless plain clicks and stale pixels.
#[test]
fn link_group_delay_and_presented_click_policy() {
    for dpr in [1.0, 2.0] {
        let (mut session, writes, _) = fixture();
        let opened = Arc::new(Mutex::new(Vec::new()));
        session.services.urls = Box::new(Opens(opened.clone()));
        let mut context = super::overlays::text_context(dpr);
        let now = Instant::now();
        session.set_text_context(&context);
        session.install_model_overlays();
        session.feed(b"\x1b]8;id=group;https://a.test\x1b\\AB\x1b]8;;\x1b\\\r\n\x1b]8;id=group;https://b.test\x1b\\CD\x1b]8;;\x1b\\", now);
        session.draw(now, &mut context).expect("link witness");
        session.presented();
        let point = session.overlay_geometry().grid.origin.map(|v| v + 1.0);
        let mut event = PointerEvent {
            kind: PointerKind::Move,
            point,
            button: Some(0),
            modifiers: 0,
        };
        session.route_overlay_pointer(&event, now);
        session.draw(now, &mut context).expect("link witness");
        assert_eq!(session.snapshot.overlays.models.link.cells.len(), 4);
        assert!(!has_tooltip(&session));
        session.draw(now + Duration::from_millis(499), &mut context);
        assert!(!has_tooltip(&session));
        session
            .draw(now + Duration::from_millis(500), &mut context)
            .expect("link witness");
        assert!(has_tooltip(&session));
        session.presented();
        event.kind = PointerKind::Press;
        assert!(
            !session
                .route_overlay_pointer(&event, now + Duration::from_millis(501))
                .handled
        );
        event.modifiers = 4; // Bare fixture follows Linux's Ctrl convention.
        assert!(
            session
                .route_overlay_pointer(&event, now + Duration::from_millis(502))
                .handled
        );
        assert_eq!(*opened.lock().expect("link witness"), ["https://a.test"]);
        event.kind = PointerKind::Release;
        session.route_overlay_pointer(&event, now + Duration::from_millis(503));
        // New output replaces the same row after presentation: no unseen URL may open.
        session.feed(
            b"\x1b[H\x1b]8;;https://evil.test\x1b\\ZZ",
            now + Duration::from_millis(504),
        );
        event.kind = PointerKind::Press;
        assert!(
            !session
                .route_overlay_pointer(&event, now + Duration::from_millis(505))
                .handled
        );
        assert_eq!(opened.lock().expect("link witness").len(), 1);
        assert!(writes.lock().expect("link witness").is_empty());
    }
}

/// Rewrites and direct API requests cannot bypass immutable scheme restrictions.
#[test]
fn link_open_rechecks_rewritten_uri_and_web_file_policy() {
    use puzzle_bus::hooks::{CommandKinds, Flow, HookCtx, Interceptor};
    struct Rewrite;
    impl Interceptor<Command, Event> for Rewrite {
        fn intercept(
            &mut self,
            command: &mut Command,
            _: &mut HookCtx<'_, Command, Event>,
        ) -> Flow {
            *command = Command::OpenLink("javascript:alert(1)".into());
            Flow::Continue
        }
    }
    let (mut session, _, _) = fixture();
    let opened = Arc::new(Mutex::new(Vec::new()));
    session.services.urls = Box::new(Opens(opened.clone()));
    let now = Instant::now();
    for uri in [
        "javascript:alert(1)",
        "data:text/plain,x",
        "https://a.test\n",
    ] {
        session.submit(Command::OpenLink(uri.into()), Origin::Api);
        session.pump(now);
    }
    assert!(opened.lock().expect("link witness").is_empty());
    session.submit(Command::OpenLink("file:///tmp/a".into()), Origin::Api);
    session.pump(now);
    assert_eq!(opened.lock().expect("link witness").len(), 1);
    let mut table = crate::keybind::Keybindings::default();
    table.apply(
        &[],
        crate::keybind::Source::Defaults,
        crate::keybind::Platform::Mac,
        true,
    );
    session.set_keybindings(table);
    session.submit(Command::OpenLink("file:///tmp/b".into()), Origin::Api);
    session.pump(now);
    assert_eq!(opened.lock().expect("link witness").len(), 1);
    session
        .bus_mut()
        .intercept(CommandKinds::at(22), 0, "rewrite-link", Rewrite)
        .expect("link witness");
    session.submit(Command::OpenLink("https://a.test".into()), Origin::Api);
    session.pump(now);
    assert_eq!(opened.lock().expect("link witness").len(), 1);
}

/// An obscured source must not remain clickable through its passive hover observer.
#[test]
fn occlusion_removes_active_link_target_without_moving_its_underline() {
    let (mut host, _, _) = fixture();
    let opened = Arc::new(Mutex::new(Vec::new()));
    host.services.urls = Box::new(Opens(opened.clone()));
    let mut context = super::overlays::text_context(2.0);
    host.set_text_context(&context);
    host.install_model_overlays();
    let now = Instant::now();
    host.feed(b"\x1b]8;;https://a.test\x1b\\AB\x1b]8;;\x1b\\", now);
    host.draw(now, &mut context).expect("link");
    host.presented();
    let grid = host.overlay_geometry().grid;
    let event = PointerEvent {
        kind: PointerKind::Move,
        point: grid.origin.map(|value| value + 1.0),
        button: Some(0),
        modifiers: 4,
    };
    host.route_overlay_pointer(&event, now);
    host.draw(now, &mut context).expect("hover");
    host.presented();
    host.apply(crate::SessionMessage::OccludedRects(vec![grid]), now)
        .expect("coverage");
    host.draw(now, &mut context).expect("covered link");
    host.presented();
    assert_eq!(host.snapshot.overlays.models.link.cells.len(), 2);
    assert!(
        !host
            .route_overlay_pointer(
                &PointerEvent {
                    kind: PointerKind::Press,
                    ..event
                },
                now
            )
            .handled
    );
    assert!(opened.lock().expect("open witness").is_empty());
    assert!(
        host.builder
            .as_ref()
            .expect("builder")
            .overlays
            .registry
            .lists()
            .flat_map(|(_, list)| &list.draws)
            .any(|draw| draw.slot == crate::frame::ZSlot::AboveText
                && matches!(draw.primitive, Primitive::Rect(_))),
        "source underline ink is retained"
    );
}

/// Native UI elsewhere must not make a modifier-click wait for a hover redraw.
#[test]
fn host_occlusion_elsewhere_preserves_immediate_modifier_click() {
    let (mut host, _, _) = fixture();
    let opened = Arc::new(Mutex::new(Vec::new()));
    host.services.urls = Box::new(Opens(opened.clone()));
    let mut context = super::overlays::text_context(1.0);
    host.set_text_context(&context);
    host.install_model_overlays();
    let now = Instant::now();
    host.feed(b"\x1b]8;;https://a.test\x1b\\AB\x1b]8;;\x1b\\", now);
    host.apply(
        crate::SessionMessage::OccludedRects(vec![crate::overlay::geometry::Rect {
            origin: [1000.0, 1000.0],
            size: [20.0, 20.0],
        }]),
        now,
    )
    .expect("host UI elsewhere");
    host.draw(now, &mut context).expect("initial link frame");
    host.presented();
    let point = host.overlay_geometry().grid.origin.map(|value| value + 1.0);
    let result = host.route_overlay_pointer(
        &PointerEvent {
            kind: PointerKind::Press,
            point,
            button: Some(0),
            modifiers: 4,
        },
        now,
    );
    assert!(
        result.handled,
        "a real presented link needs no hover redraw"
    );
    assert_eq!(*opened.lock().expect("open witness"), ["https://a.test"]);
}
