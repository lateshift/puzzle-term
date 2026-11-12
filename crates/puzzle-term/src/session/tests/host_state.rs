//! Hosts observe policy changes once; ordinary output never copies an unchanged view.
use super::*;
use crate::{HostEvent, SessionMessage};
use puzzle_vt::input::MouseTracking;

/// Draining uses the public outbox, just like a native channel producer.
fn input_states(session: &mut TestHost) -> Vec<super::super::host_state::InputState> {
    std::iter::from_fn(|| session.take_event())
        .filter_map(|event| match event {
            HostEvent::InputState(state) => Some(state),
            _ => None,
        })
        .collect()
}

/// A resize and DEC mode change update observations without asking for a frame.
#[test]
fn changed_policy_is_published_before_drawing() {
    let (mut session, _, _) = fixture();
    session.enable_host_events().expect("host outbox");
    assert_eq!(input_states(&mut session).len(), 1);
    let now = Instant::now();
    session.feed(b"ordinary output", now);
    assert!(input_states(&mut session).is_empty());
    session.feed(b"\x1b[?1003h", now);
    let states = input_states(&mut session);
    assert_eq!(states.len(), 1);
    assert_eq!(states[0].mouse.tracking, MouseTracking::AnyMotion);
    // Native hosts supply measured cells without moving their text context to Session.
    session
        .apply(
            SessionMessage::FontMetrics(crate::session::message::FontMetrics {
                cell: puzzle_text::metrics::CellMetrics {
                    width_px: 10,
                    height_px: 20,
                    baseline_px: 15,
                    device_size: 15.0,
                    cap_snapped: false,
                    underline_px: 1,
                },
                font_size: 15.0,
                dpr: 1.0,
                atlas_generation: 0,
            }),
            now,
        )
        .expect("font metrics");
    input_states(&mut session);
    session
        .apply(
            SessionMessage::GridResize {
                columns: 100,
                rows: 40,
                surface: [1000, 800],
            },
            now,
        )
        .expect("resize");
    let states = input_states(&mut session);
    assert_eq!(states.len(), 1);
    assert_eq!(states[0].grid, [100, 40]);
    session.feed(b"\x1b[?1003h", now);
    assert!(input_states(&mut session).is_empty());
}

/// Repeated warm host drains allocate nothing and never manufacture policy events.
#[test]
fn unchanged_host_policy_has_no_allocations() {
    let (mut session, _, _) = fixture();
    session.enable_host_events().expect("host outbox");
    while session.take_event().is_some() {}
    let allocations = allocation_counter::measure(|| {
        for _ in 0..100 {
            assert!(session.take_event().is_none());
        }
    });
    assert_eq!(allocations.count_total, 0);
}

/// Animation edges follow injected time, so the host need not inspect scroll state.
#[test]
fn animation_edges_stop_after_owner_advances_time() {
    let (mut session, _, _) = fixture();
    let context = super::overlays::text_context(1.0);
    session.set_text_context(&context);
    let start = Instant::now();
    for _ in 0..30 {
        session.feed(b"history\r\n", start);
    }
    session.enable_host_events().expect("outbox");
    while session.take_event().is_some() {}
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
    session
        .apply(
            SessionMessage::Command(
                Command::Wheel {
                    delta: crate::viewport::ScrollInput::Rows(-1.0),
                    pointer,
                },
                Origin::User,
            ),
            start,
        )
        .expect("wheel");
    let animations = |session: &mut TestHost| {
        std::iter::from_fn(|| session.take_event())
            .filter_map(|event| match event {
                HostEvent::AnimationChanged(value) => Some(value),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(animations(&mut session), [true]);
    session
        .publish_frame(
            start + Duration::from_secs(1),
            &mut Default::default(),
            context.atlas_generation(),
            0,
        )
        .expect("publication");
    assert_eq!(animations(&mut session), [false]);
    assert!(animations(&mut session).is_empty());
}
