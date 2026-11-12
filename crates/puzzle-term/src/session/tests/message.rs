//! D-50 host messages preserve policy, query ownership and ordered result delivery.
use super::*;
use crate::{
    HostEvent, SessionMessage,
    keybind::{Keybindings, Platform, Source},
};

/// Publication's pending-suffix drain counts bytes at the same boundary as sealed rows.
#[test]
fn consumed_byte_offset_includes_publication_suffix() {
    let (mut session, _, _) = fixture();
    let context = super::overlays::text_context(1.0);
    session.set_text_context(&context);
    let now = Instant::now();
    let bytes = vec![b'x'; 70_000];
    let feed = session.feed(&bytes, now);
    assert_eq!(feed.parsed_bytes, 65_536);
    assert_eq!(session.parsed_bytes_total(), 65_536);
    let mut slot = crate::frame_snapshot::FrameSnapshot::default();
    session
        .publish_frame(now, &mut slot, context.atlas_generation(), 0)
        .expect("publication");
    assert_eq!(session.parsed_bytes_total(), bytes.len() as u64);
    assert_eq!(session.pending_bytes(), 0);
}

/// Native insertion must follow the same query ownership as encoded key messages.
#[test]
fn native_key_message_reports_text_ownership_without_duplicate_bytes() {
    let (mut session, writes, _) = fixture();
    let now = Instant::now();
    let key = || KeyEvent::press(Key::Char('x'));
    session
        .apply(SessionMessage::NativeKey(key(), Origin::User), now)
        .expect("key");
    assert!(
        std::iter::from_fn(|| session.take_event())
            .any(|event| matches!(event, HostEvent::NativeTextAllowed(true)))
    );
    // Native beforeinput supplies the commit; keydown itself encodes nothing.
    assert!(writes.lock().expect("writes").is_empty());
    session
        .apply(
            SessionMessage::Command(
                Command::Search(crate::search::SearchCommand::Request),
                Origin::User,
            ),
            now,
        )
        .expect("query");
    session
        .apply(SessionMessage::NativeKey(key(), Origin::User), now)
        .expect("query key");
    assert!(
        std::iter::from_fn(|| session.take_event())
            .any(|event| matches!(event, HostEvent::NativeTextAllowed(true)))
    );
    // Query focus consumes the eventual native commit, not its keydown candidate.
    assert_eq!(session.search_bar_query(), Some(""));
    session
        .apply(
            SessionMessage::Command(Command::Text("x".into()), Origin::User),
            now,
        )
        .expect("native commit");
    assert_eq!(session.search_bar_query(), Some("x"));
    assert!(writes.lock().expect("writes").is_empty());
}

/// Message-driven input samples Session's query mode before encoding the same key.
#[test]
fn message_query_focus_and_policy_remain_on_one_owner() {
    let (mut session, writes, _) = fixture();
    let now = Instant::now();
    let apply = |session: &mut TestHost, command| {
        session
            .apply(SessionMessage::Command(command, Origin::User), now)
            .expect("message");
    };
    apply(
        &mut session,
        Command::Search(crate::search::SearchCommand::Request),
    );
    apply(&mut session, Command::Text("漢".into()));
    assert_eq!(session.search_bar_query(), Some("漢"));
    assert!(writes.lock().expect("writes").is_empty());
    apply(&mut session, Command::Key(KeyEvent::press(Key::Escape)));
    apply(&mut session, Command::Text("x".into()));
    assert_eq!(writes.lock().expect("writes").concat(), b"x");
}

/// An unchanged settings reload must preserve a leader already waiting for its next key.
#[test]
fn message_binding_reload_preserves_pending_leader() {
    let (mut session, writes, _) = fixture();
    let now = Instant::now();
    let make = || {
        let mut bindings = Keybindings::default();
        assert!(
            bindings
                .apply(
                    &["a>b=sendText:X".into()],
                    Source::User,
                    Platform::Mac,
                    true
                )
                .is_empty()
        );
        bindings
    };
    session
        .apply(SessionMessage::Keybindings(make()), now)
        .expect("bindings");
    session
        .apply(
            SessionMessage::Command(Command::Key(KeyEvent::press(Key::Char('a'))), Origin::User),
            now,
        )
        .expect("leader");
    session
        .apply(SessionMessage::Keybindings(make()), now)
        .expect("same bindings");
    session
        .apply(
            SessionMessage::Command(Command::Key(KeyEvent::press(Key::Char('b'))), Origin::User),
            now,
        )
        .expect("continuation");
    assert_eq!(writes.lock().expect("writes").concat(), b"X");
}

/// The host gets title, bell and exit without inspecting live Session state.
#[test]
fn host_result_facts_preserve_fifo_and_close_policy() {
    let (mut session, _, _) = fixture();
    session.enable_host_events().expect("host results");
    let now = Instant::now();
    session.feed(b"\x1b]2;first\x07\x07\x1b]2;second\x07", now);
    let mut titles = Vec::new();
    while let Some(event) = session.take_event() {
        match event {
            HostEvent::Fact(Event::TitleChanged(title)) => titles.push(title),
            HostEvent::Fact(Event::Bell) => titles.push("bell".into()),
            _ => {}
        }
    }
    assert_eq!(titles, ["first", "bell", "second"]);
    let info = ExitInfo {
        code: Some(0),
        signal: None,
        runtime_ms: 500,
    };
    session
        .apply(
            SessionMessage::Command(Command::ChildExited(info.clone()), Origin::Platform),
            now,
        )
        .expect("exit");
    assert!(
        matches!(session.take_event(), Some(HostEvent::Fact(Event::SessionExited(value))) if value == info)
    );
    assert!(matches!(
        session.take_event(),
        Some(HostEvent::Fact(Event::SessionCloseRequested))
    ));
    assert!(session.take_event().is_none());
}

/// Renderer measurements are plain values; Session needs no font owner to resize VT.
#[test]
fn font_measurement_message_supplies_pty_geometry_without_text_resources() {
    let (mut session, _, _) = fixture();
    let now = Instant::now();
    let metrics = puzzle_text::metrics::CellMetrics {
        width_px: 9,
        height_px: 20,
        baseline_px: 15,
        device_size: 15.0,
        cap_snapped: false,
        underline_px: 1,
    };
    session
        .apply(
            SessionMessage::FontMetrics(crate::FontMetrics {
                cell: metrics,
                font_size: 15.0,
                dpr: 1.0,
                atlas_generation: 1,
            }),
            now,
        )
        .expect("metrics");
    session
        .apply(
            SessionMessage::GridResize {
                columns: 31,
                rows: 7,
                surface: [279, 140],
            },
            now,
        )
        .expect("grid");
    assert_eq!(session.cell_metrics(), Some(metrics));
    assert_eq!(session.terminal.cell_pixels, (9, 20));
    assert_eq!(session.terminal.size(), (31, 7));
}

/// Corrupt host metrics must not poison pointer conversion or replace working geometry.
#[test]
fn invalid_measurement_message_leaves_current_geometry_intact() {
    let (mut session, _, _) = fixture();
    let context = super::overlays::text_context(2.0);
    session.set_text_context(&context);
    let previous = session.cell_metrics();
    let result = session.apply(
        SessionMessage::FontMetrics(crate::FontMetrics {
            cell: context.metrics(),
            font_size: context.font_size(),
            dpr: f32::NAN,
            atlas_generation: context.atlas_generation(),
        }),
        Instant::now(),
    );
    assert!(result.is_err());
    assert_eq!(session.cell_metrics(), previous);
}

/// A successful presentation installs its hit table through a value message.
#[test]
fn presentation_message_routes_search_buttons_without_a_builder_loan() {
    use crate::overlay::provider::{PointerEvent, PointerKind};
    let (mut session, _, _) = fixture();
    let mut context = super::overlays::text_context(1.0);
    session.set_text_context(&context);
    let now = Instant::now();
    session
        .apply(SessionMessage::InstallModelOverlays, now)
        .expect("models");
    session.feed(b"hit hit", now);
    for command in [
        Command::Search(crate::search::SearchCommand::Request),
        Command::Text("hit".into()),
    ] {
        session
            .apply(SessionMessage::Command(command, Origin::User), now)
            .expect("query");
    }
    let width = session.draw(now, &mut context).expect("frame").target_size[0] as f32;
    let mut regions = crate::overlay::regions::PresentedRegions::default();
    session
        .builder
        .as_ref()
        .expect("builder")
        .capture_presented_regions(&mut regions);
    session
        .apply(
            SessionMessage::FrameResult(crate::FrameResult::Presented(regions)),
            now,
        )
        .expect("presented");
    let panel_width = width.min(420.0);
    let panel_x = (width - 432.0).max(0.0);
    // Close is a real region emitted by SearchView, not a direct action shortcut.
    let point = [panel_x + panel_width - 35.0, 54.0];
    for kind in [PointerKind::Press, PointerKind::Release] {
        session
            .apply(
                SessionMessage::OverlayPointer(PointerEvent {
                    kind,
                    point,
                    button: Some(0),
                    modifiers: 0,
                }),
                now,
            )
            .expect("pointer");
    }
    assert_eq!(session.search_bar_query(), None);
    assert_eq!(session.search_state().count, 0);
    assert!(
        std::iter::from_fn(|| session.take_event())
            .any(|event| matches!(event, HostEvent::RegionsReturned(_)))
    );
}
