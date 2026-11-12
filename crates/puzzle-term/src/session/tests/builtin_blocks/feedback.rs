//! Copy feedback follows execution and expires through injected frame time only.
use super::*;
use crate::{SessionMessage, overlay::geometry::Rect};

/// Clicks only the region captured from a real hover presentation.
fn click(host: &mut TestHost, now: Instant) {
    host.presented();
    let geometry = host.overlay_geometry();
    let point = button(host);
    let position = std::array::from_fn(|axis| {
        f64::from((point[axis] - geometry.origin()[axis]) * geometry.dpr)
    });
    let event = crate::vt::input::MouseEvent {
        kind: crate::vt::input::MouseKind::Press,
        button: 0,
        column: 0,
        row: 0,
        pixel_x: 0,
        pixel_y: 0,
        modifiers: crate::vt::input::Modifiers(0),
        button_held: true,
        click_count: 1,
    };
    host.apply(
        SessionMessage::Command(
            Command::Pointer(crate::pointer::PointerInput {
                event,
                position,
                cell: [1, 1],
            }),
            Origin::User,
        ),
        now,
    )
    .expect("click");
}

/// Inspect normal retained primitives, not a second copy of the feedback state.
fn label(host: &TestHost, expected: &str) -> Option<u32> {
    host.builder
        .as_ref()
        .expect("builder")
        .overlays
        .registry
        .lists()
        .flat_map(|(_, list)| &list.draws)
        .find_map(|draw| match &draw.primitive {
            Primitive::Text(text) if text.text == expected => Some(text.color),
            _ => None,
        })
}

/// BelowText contains the selection-like flash; the passive observer is not ink.
fn tint(host: &TestHost) -> Option<u32> {
    host.builder
        .as_ref()
        .expect("builder")
        .overlays
        .registry
        .lists()
        .flat_map(|(_, list)| &list.draws)
        .find_map(|draw| match draw.primitive {
            Primitive::Rect(rect) if draw.slot == crate::frame::ZSlot::BelowText => {
                Some(rect.color)
            }
            _ => None,
        })
}

#[test]
fn copy_receipt_fades_once_reverts_and_respects_reduced_motion() {
    for dpr in [1.0, 2.0] {
        for reduced in [false, true] {
            let (mut host, mut context, now, _) = super::occlusion::hovered(dpr);
            host.apply_settings(&crate::settings::Settings::default(), now);
            host.set_reduced_motion(reduced);
            click(&mut host, now);
            host.draw(now, &mut context).expect("receipt");
            assert_eq!(
                label(&host, "Copied ✓"),
                Some(host.snapshot.overlays.theme.color("zoneOk", 0))
            );
            let initial = tint(&host).expect("copied output tint").to_le_bytes()[3];
            host.draw(now + Duration::from_millis(100), &mut context);
            let halfway = tint(&host).expect("flash").to_le_bytes()[3];
            assert_eq!(
                halfway,
                if reduced {
                    initial
                } else {
                    (f32::from(initial) / 2.0).round() as u8
                }
            );
            host.draw(now + Duration::from_millis(200), &mut context)
                .expect("flash expiry");
            assert_eq!(tint(&host), None);
            assert!(label(&host, "Copied ✓").is_some());
            host.draw(now + Duration::from_millis(1500), &mut context)
                .expect("label expiry");
            assert_eq!(label(&host, "Copied ✓"), None);
            assert!(label(&host, "Copy output").is_some());
        }
    }
}

#[test]
fn copy_and_write_vetoes_report_blocked_without_tint_or_clipboard_effect() {
    for write in [false, true] {
        let (mut host, mut context, now, _) = super::occlusion::hovered(2.0);
        host.apply_settings(&crate::settings::Settings::default(), now);
        let copied = Arc::new(Mutex::new(Vec::new()));
        host.services.clipboard = Box::new(ClipboardProbe(copied.clone()));
        struct Deny;
        impl Interceptor<Command, Event> for Deny {
            fn intercept(&mut self, _: &mut Command, _: &mut HookCtx<'_, Command, Event>) -> Flow {
                Flow::Stop("denied by host policy")
            }
        }
        // Use variant-derived kind bits: both request and actual write must be interceptable.
        use puzzle_bus::message::Message;
        let command = if write {
            Command::ClipboardWrite(String::new().into())
        } else {
            Command::Copy(CopyTarget::CommandOutput)
        };
        host.bus_mut()
            .intercept(CommandKinds::at(command.kind_index()), 0, "copy-veto", Deny)
            .expect("policy");
        click(&mut host, now);
        host.draw(now, &mut context).expect("blocked result");
        assert_eq!(
            label(&host, "Copy blocked"),
            Some(host.snapshot.overlays.theme.color("semanticWarning", 0))
        );
        assert_eq!(tint(&host), None);
        assert!(copied.lock().expect("clipboard").is_empty());
    }
}

#[test]
fn occluded_relocated_button_still_copies_the_presented_output() {
    let (mut host, mut context, now, original) = super::occlusion::hovered(2.0);
    let copied = Arc::new(Mutex::new(Vec::new()));
    host.services.clipboard = Box::new(ClipboardProbe(copied.clone()));
    host.apply(
        SessionMessage::OccludedRects(vec![Rect {
            origin: [
                original.origin[0] + original.size[0] / 2.0,
                original.origin[1],
            ],
            size: original.size,
        }]),
        now,
    )
    .expect("find box");
    host.draw(now, &mut context).expect("relocated hover");
    click(&mut host, now);
    assert_eq!(
        copied.lock().expect("clipboard").as_slice(),
        ["one\ntwo\nthree\nfour\nfive\nsix\n"]
    );
}

#[test]
fn clipboard_service_error_is_blocked_and_feedback_survives_pointer_leave() {
    let (mut host, mut context, now, _) = super::occlusion::hovered(1.0);
    struct Broken;
    impl Clipboard for Broken {
        fn write_text(&mut self, _: &str) -> Result<(), String> {
            Err("clipboard unavailable".into())
        }
        fn request_text(&mut self) -> Result<Option<String>, String> {
            Ok(None)
        }
    }
    host.services.clipboard = Box::new(Broken);
    click(&mut host, now);
    host.route_overlay_pointer(
        &PointerEvent {
            kind: PointerKind::Leave,
            point: [0.0; 2],
            button: None,
            modifiers: 0,
        },
        now,
    );
    host.draw(now, &mut context).expect("error after leave");
    assert!(label(&host, "Copy blocked").is_some());
    assert_eq!(tint(&host), None);
    host.draw(now + Duration::from_millis(1500), &mut context)
        .expect("remove feedback");
    assert_eq!(label(&host, "Copy blocked"), None);
    assert_eq!(label(&host, "Copy output"), None);
}

#[test]
fn warmed_copy_flash_rebuilds_without_allocation() {
    let (mut host, mut context, now, _) = super::occlusion::hovered(2.0);
    click(&mut host, now);
    // Publication seals the §6.13 journal even with no cell edits. Warm its
    // complete bounded history too, so its one-time deque growth is not mistaken
    // for a copy-flash allocation (the existing bound is the warm-up length).
    let warm = crate::vt::view::CHANGE_LOG_FRAMES as u64;
    for millis in 0..warm {
        host.draw(now + Duration::from_millis(millis), &mut context)
            .expect("warm flash");
    }
    let mut counts = [(0, 0); 20];
    for (index, sample) in counts.iter_mut().enumerate() {
        let allocations = allocation_counter::measure(|| {
            host.draw(
                now + Duration::from_millis(warm + index as u64),
                &mut context,
            )
            .expect("rebuild flash");
        });
        *sample = (allocations.count_total, allocations.bytes_total);
    }
    assert_eq!(
        counts,
        [(0, 0); 20],
        "finite copy animation must reuse strings/state"
    );
}

#[test]
fn one_row_command_keeps_a_clickable_control_inside_its_output() {
    let (mut host, _, _) = fixture();
    let mut context = super::super::overlays::text_context(2.0);
    host.set_text_context(&context);
    host.install_model_overlays();
    let now = Instant::now();
    host.feed(b"\x1b]133;C\x07one\x1b]133;D;0\x07", now);
    host.draw(now, &mut context).expect("one row");
    host.presented();
    let origin = host.overlay_geometry().grid.origin;
    host.route_overlay_pointer(
        &PointerEvent {
            kind: PointerKind::Move,
            point: origin.map(|v| v + 1.0),
            button: None,
            modifiers: 0,
        },
        now,
    );
    host.draw(now, &mut context).expect("hover");
    let copied = Arc::new(Mutex::new(Vec::new()));
    host.services.clipboard = Box::new(ClipboardProbe(copied.clone()));
    click(&mut host, now);
    assert_eq!(copied.lock().expect("clipboard").as_slice(), ["one"]);
}
