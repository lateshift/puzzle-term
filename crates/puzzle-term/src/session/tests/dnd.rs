//! Owner messages preserve drop isolation, reset cleanup and reply ordering.
use super::*;
use crate::{
    HostEvent, SessionMessage,
    clipboard::MimeData,
    dnd::{Effect, Input, Kind},
};
/// Drain owned callback values exactly as a host tick does.
fn registration(s: &mut TestHost) -> u64 {
    let events = std::iter::from_fn(|| s.take_event()).collect::<Vec<_>>();
    events
        .into_iter()
        .find_map(|e| match e {
            HostEvent::Drop(Effect::Registration {
                generation,
                mimes: Some(_),
            }) => Some(generation),
            _ => None,
        })
        .expect("registration callback")
}
#[test]
fn native_drop_reply_order_stale_generation_and_program_exit() {
    let (mut s, w, _) = fixture();
    let now = Instant::now();
    s.enable_host_events().expect("host observation");
    s.feed(b"\x1b]72;t=a;\x1b\\", now);
    let generation = registration(&mut s);
    s.apply(
        SessionMessage::Drop(Input {
            generation,
            kind: Kind::Drop,
            position: [8.0, 8.0],
            operations: 1,
            items: vec![MimeData {
                mime: "text/plain".into(),
                data: (&b"drop"[..]).into(),
            }],
        }),
        now,
    )
    .expect("drop input");
    s.feed(b"\x1b]72;t=r:x=1\x1b\\\x1b[5n", now);
    let bytes = w.lock().expect("writer").concat();
    assert!(
        bytes.ends_with(b"\x1b]72;t=r:x=1:m=0;ZHJvcA==\x1b\\\x1b]72;t=r:x=1:m=0;\x1b\\\x1b[0n")
    );
    s.apply(
        SessionMessage::Command(
            Command::ChildExited(ExitInfo {
                code: Some(0),
                signal: None,
                runtime_ms: 1000,
            }),
            Origin::Platform,
        ),
        now,
    )
    .expect("exit");
    assert!(
        std::iter::from_fn(|| s.take_event())
            .collect::<Vec<_>>()
            .into_iter()
            .any(|e| matches!(e, HostEvent::Drop(Effect::Registration { mimes: None, .. })))
    );
    let prior = w.lock().expect("writer").concat();
    s.apply(
        SessionMessage::Drop(Input {
            generation,
            kind: Kind::Drop,
            position: [8.0, 8.0],
            operations: 1,
            items: vec![MimeData {
                mime: "text/plain".into(),
                data: (&b"stale"[..]).into(),
            }],
        }),
        now,
    )
    .expect("stale ignored");
    assert_eq!(w.lock().expect("writer").concat(), prior);
}
#[test]
fn unregistered_and_reset_drop_have_no_effect() {
    let (mut s, w, _) = fixture();
    let now = Instant::now();
    s.enable_host_events().expect("host observation");
    s.apply(
        SessionMessage::Drop(Input {
            generation: 1,
            kind: Kind::Drop,
            position: [0.0; 2],
            operations: 1,
            items: vec![],
        }),
        now,
    )
    .expect("unregistered");
    assert!(w.lock().expect("writer").is_empty());
    s.feed(b"\x1b]72;t=a;\x1b\\", now);
    registration(&mut s);
    s.feed(b"\x1bc", now);
    assert!(
        std::iter::from_fn(|| s.take_event())
            .collect::<Vec<_>>()
            .into_iter()
            .any(|e| matches!(e, HostEvent::Drop(Effect::Registration { mimes: None, .. })))
    );
}

#[test]
fn drop_points_use_presented_padding_scale_and_fractional_row_geometry() {
    use puzzle_overlay::{
        geometry::{Line, TextGeometry},
        regions::PresentedRegions,
    };
    for dpr in [1.0, 2.0] {
        for inset in [0.0, 8.0] {
            let (mut s, w, _) = fixture();
            let now = Instant::now();
            s.enable_host_events().expect("observe");
            s.feed(b"\x1b]72;t=a;\x1b\\", now);
            let generation = registration(&mut s);
            let geometry = TextGeometry {
                cell: [10.0, 20.0],
                inset,
                dpr,
                columns: 80,
                lines: vec![Line {
                    id: crate::vt::LineId(0),
                    version: 0,
                    y: inset - 5.0,
                }],
                ..Default::default()
            };
            let mut regions = PresentedRegions::default();
            regions.capture(&geometry, std::iter::empty());
            s.apply(
                SessionMessage::FrameResult(crate::FrameResult::Presented(regions)),
                now,
            )
            .expect("presented");
            s.apply(
                SessionMessage::Drop(Input {
                    generation,
                    kind: Kind::Move,
                    position: [f64::from(inset) + 15.0, f64::from(inset) + 16.0],
                    operations: 1,
                    items: vec![MimeData {
                        mime: "text/plain".into(),
                        data: (&b""[..]).into(),
                    }],
                }),
                now,
            )
            .expect("move");
            let expected = format!(
                "\x1b]72;t=m:x=1:y=1:X={}:Y={}:o=1:m=0;text/plain\x1b\\",
                (15.0 * dpr) as u32,
                (16.0 * dpr) as u32
            );
            assert_eq!(w.lock().expect("writer").concat(), expected.as_bytes());
        }
    }
}

#[test]
fn native_drop_registration_passes_through_host_policy() {
    use puzzle_bus::hooks::{CommandKinds, Flow, HookCtx, Interceptor};
    struct Deny;
    impl Interceptor<Command, Event> for Deny {
        fn intercept(&mut self, _: &mut Command, _: &mut HookCtx<'_, Command, Event>) -> Flow {
            Flow::Stop("native drops disabled")
        }
    }
    let (mut s, _, _) = fixture();
    s.enable_host_events().expect("observe");
    s.bus_mut()
        .intercept(CommandKinds::at(34), 200, "drop-policy", Deny)
        .expect("policy");
    s.feed(b"\x1b]72;t=a;\x1b\\", Instant::now());
    assert!(!std::iter::from_fn(|| s.take_event()).any(|e| matches!(e, HostEvent::Drop(_))));
}
