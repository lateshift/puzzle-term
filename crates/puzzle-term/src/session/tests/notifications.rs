//! Notification reports stay on their creating owner, with shared §16 policy.
use super::*;
use crate::{
    SessionMessage,
    notification::{Effect, Feedback},
    platform::Notifier,
};
/// Host retains only owned effects, never a Session callback.
struct Probe(Arc<Mutex<Vec<Effect>>>);
impl Notifier for Probe {
    fn notify(&mut self, _: &str, _: &str) -> Result<(), String> {
        panic!("structured effect expected")
    }
    fn notification(&mut self, effect: Effect) -> Result<(), String> {
        self.0
            .lock()
            .expect("notification fixture mutex")
            .push(effect);
        Ok(())
    }
}
/// Install a queue-only host and distinct namespace before output can arrive.
fn host(namespace: u64) -> (TestHost, Writes, Arc<Mutex<Vec<Effect>>>) {
    let (mut s, w, _) = fixture();
    let effects = Arc::new(Mutex::new(Vec::new()));
    s.services.notifier = Box::new(Probe(effects.clone()));
    s.set_notification_namespace(namespace);
    (s, w, effects)
}
/// Retrieve a callback generation exactly as a native host would.
fn token(effects: &Arc<Mutex<Vec<Effect>>>) -> String {
    effects
        .lock()
        .expect("notification fixture mutex")
        .iter()
        .rev()
        .find_map(|e| match e {
            Effect::Show { token, .. } => Some(token.clone()),
            _ => None,
        })
        .expect("valid notification fixture operation")
}
#[test]
fn reports_are_ordered_isolated_and_stale_completions_are_inert() {
    let (mut a, aw, ae) = host(1);
    let (mut b, bw, be) = host(2);
    let now = Instant::now();
    let request = b"\x1b]99;i=id:a=report:c=1;Ready\x1b\\\x1b[5n";
    a.feed(request, now);
    b.feed(request, now);
    let at = token(&ae);
    let bt = token(&be);
    assert_ne!(at, bt);
    b.apply(
        SessionMessage::NotificationFeedback(at.clone(), Feedback::Activated),
        now,
    )
    .expect("valid notification fixture operation");
    assert_eq!(
        bw.lock().expect("notification fixture mutex").concat(),
        b"\x1b[0n"
    );
    a.apply(
        SessionMessage::NotificationFeedback(at.clone(), Feedback::Activated),
        now,
    )
    .expect("valid notification fixture operation");
    assert_eq!(
        aw.lock().expect("notification fixture mutex").concat(),
        b"\x1b[0n\x1b]99;i=id;\x1b\\\x1b]99;i=id:p=close;\x1b\\"
    );
    a.apply(
        SessionMessage::NotificationFeedback(at, Feedback::Closed),
        now,
    )
    .expect("valid notification fixture operation");
    assert!(
        ae.lock()
            .expect("notification fixture mutex")
            .iter()
            .any(|e| matches!(e, Effect::Focus))
    );
    b.feed(b"\x1bc", now);
    b.apply(
        SessionMessage::NotificationFeedback(bt, Feedback::Activated),
        now,
    )
    .expect("valid notification fixture operation");
    assert_eq!(
        bw.lock().expect("notification fixture mutex").concat(),
        b"\x1b[0n"
    );
}
#[test]
fn legacy_origins_share_rate_cap_and_host_focus_occasion() {
    let (mut s, _, effects) = host(1);
    let now = Instant::now();
    s.feed(
        b"\x1b]9;first\x1b\\\x1b]777;notify;second;body\x1b\\\x1b]99;;third\x1b\\",
        now,
    );
    assert_eq!(effects.lock().expect("notification fixture mutex").len(), 1);
    assert!(
        matches!(&effects.lock().expect("notification fixture mutex")[0], Effect::Show { notification, .. } if notification.title == "first")
    );
    s.apply(
        SessionMessage::Command(Command::Focus(true), Origin::Platform),
        now,
    )
    .expect("valid notification fixture operation");
    s.feed(
        b"\x1b]99;o=unfocused;hidden\x1b\\",
        now + Duration::from_secs(1),
    );
    assert_eq!(effects.lock().expect("notification fixture mutex").len(), 1);
    s.apply(SessionMessage::NotificationVisibility(true), now)
        .expect("valid notification fixture operation");
    s.feed(
        b"\x1b]99;o=invisible;hidden\x1b\\",
        now + Duration::from_secs(1),
    );
    assert_eq!(effects.lock().expect("notification fixture mutex").len(), 1);
    s.apply(
        SessionMessage::Command(Command::Focus(false), Origin::Platform),
        now,
    )
    .expect("valid notification fixture operation");
    for i in 1..=80 {
        s.feed(
            format!("\x1b]99;i=n{i}:o=unfocused;visible\x1b\\").as_bytes(),
            now + Duration::from_secs(i),
        );
    }
    assert!(s.notifications.toast_source().is_none());
    s.enable_notification_toasts();
    assert_eq!(
        s.overlay_source
            .models
            .toasts
            .as_ref()
            .expect("published toast source")
            .len(),
        64
    );
}
#[test]
fn toast_actions_complete_without_application_mouse_bytes() {
    let (mut s, w, effects) = host(3);
    let now = Instant::now();
    s.enable_notification_toasts();
    s.feed(b"\x1b]99;i=t:a=report:c=1;Toast\x1b\\", now);
    let first = s
        .overlay_source
        .models
        .toasts
        .clone()
        .expect("active toast source");
    s.pump(now);
    assert!(
        Arc::ptr_eq(
            &first,
            s.overlay_source
                .models
                .toasts
                .as_ref()
                .expect("published toast source")
        ),
        "clean source is not copied"
    );
    let serial = first[0].serial;
    s.submit(
        Command::OverlayAction(puzzle_overlay::models::Action::Notification {
            serial,
            activate: true,
        }),
        Origin::User,
    );
    s.pump(now);
    assert!(s.overlay_source.models.toasts.is_none());
    assert_eq!(
        w.lock().expect("notification fixture mutex").concat(),
        b"\x1b]99;i=t;\x1b\\\x1b]99;i=t:p=close;\x1b\\"
    );
    assert!(
        effects
            .lock()
            .expect("notification fixture mutex")
            .iter()
            .any(|e| matches!(e, Effect::Focus))
    );
}
#[test]
fn close_replacement_untracked_and_failure_have_defined_reports() {
    let (mut s, w, effects) = host(4);
    let now = Instant::now();
    s.feed(b"\x1b]99;i=x:c=1;old\x1b\\", now);
    let old = token(&effects);
    s.feed(b"\x1b]99;i=x:c=1;new\x1b\\", now + Duration::from_secs(1));
    let current = token(&effects);
    assert_ne!(old, current);
    s.notification_feedback(&old, Feedback::Activated);
    s.feed(b"\x1b]99;i=x:p=close;\x1b\\", now + Duration::from_secs(1));
    assert_eq!(
        w.lock().expect("notification fixture mutex").concat(),
        b"\x1b]99;i=x:p=close;\x1b\\"
    );
    s.feed(
        b"\x1b]99;i=y:c=1;untracked\x1b\\",
        now + Duration::from_secs(2),
    );
    s.notification_feedback(&token(&effects), Feedback::Untracked);
    assert!(
        w.lock()
            .expect("notification fixture mutex")
            .concat()
            .ends_with(b"\x1b]99;i=y:p=close;untracked\x1b\\")
    );
    let untracked = token(&effects);
    s.feed(b"\x1b]99;i=y:p=close;\x1b\\", now + Duration::from_secs(2));
    assert!(
        effects
            .lock()
            .expect("notification fixture mutex")
            .iter()
            .any(|e| matches!(e, Effect::Close(t) if *t == untracked))
    );
    let before = w.lock().expect("notification fixture mutex").concat();
    s.feed(b"\x1b]99;i=z:c=1;fail\x1b\\", now + Duration::from_secs(3));
    s.notification_feedback(&token(&effects), Feedback::Failed);
    assert_eq!(
        w.lock().expect("notification fixture mutex").concat(),
        before
    );
}

#[test]
fn occasions_use_focus_and_visibility_truth_table() {
    for focused in [false, true] {
        for visible in [false, true] {
            for (occasion, allowed) in [
                ("always", true),
                ("unfocused", !focused),
                ("invisible", !focused && !visible),
            ] {
                let (mut s, _, effects) = host(1);
                let now = Instant::now();
                s.apply(
                    SessionMessage::Command(Command::Focus(focused), Origin::Platform),
                    now,
                )
                .expect("valid notification fixture operation");
                s.apply(SessionMessage::NotificationVisibility(visible), now)
                    .expect("valid notification fixture operation");
                s.feed(format!("\x1b]99;o={occasion};Test\x1b\\").as_bytes(), now);
                assert_eq!(
                    !effects
                        .lock()
                        .expect("notification fixture mutex")
                        .is_empty(),
                    allowed,
                    "{occasion} focus={focused} visible={visible}"
                );
            }
        }
    }
}

#[test]
fn toast_hit_testing_uses_presented_regions_at_both_scales() {
    use crate::overlay::provider::{PointerEvent, PointerKind};
    for dpr in [1.0, 2.0] {
        let (mut s, w, _) = host(2);
        let mut context = super::overlays::text_context(dpr);
        s.set_text_context(&context);
        s.enable_notification_toasts();
        let now = Instant::now();
        s.feed(b"\x1b]99;i=click:a=report:c=1;Toast\x1b\\", now);
        s.draw(now, &mut context)
            .expect("valid notification fixture operation");
        let mut event = PointerEvent {
            kind: PointerKind::Release,
            point: [100.0, 40.0],
            button: Some(0),
            modifiers: 0,
        };
        assert!(
            !s.route_overlay_pointer(&event, now).handled,
            "unpresented regions are inert"
        );
        s.presented();
        let width = s.overlay_geometry().viewport.size[0];
        event.point = [width - 60.0, 40.0];
        assert!(s.route_overlay_pointer(&event, now).handled);
        assert_eq!(
            w.lock().expect("notification fixture mutex").concat(),
            b"\x1b]99;i=click;\x1b\\\x1b]99;i=click:p=close;\x1b\\"
        );
        s.feed(
            b"\x1b]99;i=dismiss:c=1;Toast\x1b\\",
            now + Duration::from_secs(1),
        );
        s.draw(now + Duration::from_secs(1), &mut context)
            .expect("valid notification fixture operation");
        s.presented();
        event.point = [width - 24.0, 24.0];
        assert!(s.route_overlay_pointer(&event, now).handled);
        assert!(
            w.lock()
                .expect("notification fixture mutex")
                .concat()
                .ends_with(b"\x1b]99;i=dismiss:p=close;\x1b\\")
        );
        assert!(s.overlay_source.models.toasts.is_none());
        s.draw(now + Duration::from_secs(3), &mut context);
        let allocations = allocation_counter::measure(|| {
            for _ in 0..100 {
                assert!(s.draw(now + Duration::from_secs(3), &mut context).is_none());
            }
        });
        assert_eq!(allocations.count_total, 0);
    }
}

/// Active toasts must remain free while idle, including queue/source capture.
#[test]
fn retained_toast_idle_zero_work_and_allocations() {
    let (mut s, _, _) = host(5);
    let mut context = super::overlays::text_context(2.0);
    s.set_text_context(&context);
    s.enable_notification_toasts();
    let now = Instant::now();
    s.feed(b"\x1b[?25l\x1b]99;;Idle toast\x1b\\", now);
    s.draw(now, &mut context)
        .expect("valid notification fixture operation");
    s.presented();
    s.draw(now + Duration::from_secs(3), &mut context);
    let a = allocation_counter::measure(|| {
        for _ in 0..100 {
            assert!(s.draw(now + Duration::from_secs(3), &mut context).is_none());
        }
    });
    assert_eq!(a.count_total, 0);
    assert_eq!(
        s.overlay_source
            .models
            .toasts
            .as_ref()
            .expect("published toast source")
            .len(),
        1
    );
}

/// Presentation cost is optimized; debug never asserts scheduler wall-time budgets.
#[test]
#[cfg_attr(debug_assertions, ignore = "requires the release profile")]
fn toast_overlay_build_budget() {
    for dpr in [1.0, 2.0] {
        toast_budget(dpr);
    }
}

/// Keep all raw dirty-build samples; no percentiles or outliers replace the maximum gate.
fn toast_budget(dpr: f32) {
    let (mut s, _, _) = host(6);
    let mut context = super::overlays::text_context(dpr);
    s.set_text_context(&context);
    s.enable_notification_toasts();
    s.bus_mut()
        .observe(
            puzzle_bus::hooks::EventKinds::at(16),
            0,
            "toast-budget",
            NotificationStats,
        )
        .expect("valid notification fixture operation");
    let now = Instant::now();
    for i in 0..3 {
        s.feed(format!("\x1b]99;i=n{i}:u={i}:d=0;Build finished\x1b\\\x1b]99;i=n{i}:p=body;Click activates; x dismisses\x1b\\").as_bytes(), now + Duration::from_secs(i));
    }
    s.draw(now + Duration::from_secs(3), &mut context)
        .expect("valid notification fixture operation");
    let mut samples = Vec::new();
    for i in 0..240 {
        s.overlay_changed(puzzle_overlay::provider::EventKinds::ALL);
        s.needs_frame = true;
        s.draw(
            now + Duration::from_secs(4) + Duration::from_millis(i * 16),
            &mut context,
        )
        .expect("valid notification fixture operation");
        samples.push(
            s.builder
                .as_mut()
                .expect("valid notification fixture operation")
                .take_frame_statistics()
                .expect("valid notification fixture operation")
                .cpu_ms[4],
        );
    }
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/m18/notification-budget");
    std::fs::create_dir_all(&path).expect("valid notification fixture operation");
    let raw = samples
        .iter()
        .enumerate()
        .map(|(i, ms)| format!("{i},{ms:.9}\n"))
        .collect::<String>();
    std::fs::write(
        path.join(format!("dpr{dpr}.csv")),
        format!("sample,overlay_ms\n{raw}"),
    )
    .expect("valid notification fixture operation");
    samples.sort_by(f64::total_cmp);
    let summary = format!(
        "dpr={dpr} samples=240 p50_ms={:.9} p95_ms={:.9} max_ms={:.9}",
        samples[120], samples[228], samples[239]
    );
    std::fs::write(path.join(format!("dpr{dpr}.txt")), &summary)
        .expect("valid notification fixture operation");
    assert!(samples[239] <= 0.5, "{summary}");
}

/// Enables public phase clocks for the budget witness without changing source work.
struct NotificationStats;
impl puzzle_bus::hooks::Observer<Command, Event> for NotificationStats {
    fn observe(&mut self, _: &Event, _: &mut puzzle_bus::hooks::HookCtx<'_, Command, Event>) {}
}
