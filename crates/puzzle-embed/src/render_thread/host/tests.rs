//! First ink and caret facts follow actual presentation, with no idle allocations.
#![allow(
    clippy::unwrap_used,
    reason = "test mutex poisoning is a failed presentation assertion"
)]
use super::*;
use std::sync::Mutex;

#[test]
fn presentation_events_are_once_and_change_only() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let sink = events.clone();
    let mut host = HostView::new(
        SurfaceInfo {
            extent: Extent {
                width: 800,
                height: 480,
            },
            scale: 2.0,
            display: 1,
            millihertz: 60000,
        },
        Visibility::default(),
        Arc::new(move |event| sink.lock().unwrap().push(event)),
    );
    assert!(events.lock().unwrap().is_empty());
    host.presented(Some([4.0, 20.0, 9.0, 18.0]));
    let first = std::mem::take(&mut *events.lock().unwrap());
    assert!(matches!(
        first.as_slice(),
        [HostAction::FirstFrame, HostAction::CursorRect(Some(_))]
    ));
    // Warm blink presentations neither allocate nor copy an unchanged geometry fact.
    let count = allocation_counter::measure(|| host.presented(Some([4.0, 20.0, 9.0, 18.0])));
    assert_eq!(count.count_total, 0);
    assert!(events.lock().unwrap().is_empty());
    host.presented(None);
    assert!(matches!(
        events.lock().unwrap().as_slice(),
        [HostAction::CursorRect(None)]
    ));
    events.lock().unwrap().clear();
    // Equal logical coordinates after a scale change still invalidate native IME geometry.
    host.geometry_changed(SurfaceInfo {
        scale: 1.0,
        ..host.info
    });
    host.presented(None);
    assert!(matches!(
        events.lock().unwrap().as_slice(),
        [HostAction::CursorRect(None)]
    ));
}

/// D-54 allows stale-size ink, while an old-display FIFO is explicitly cancelled.
#[test]
fn acquisition_gate_preserves_size_only_frames_and_returns_to_target_display() {
    use crate::render_thread::surface::{Pending, SurfaceUpdate};
    let mailbox = Arc::new(Mutex::new(Pending::default()));
    let info = SurfaceInfo {
        extent: Extent {
            width: 800,
            height: 600,
        },
        scale: 2.0,
        display: 3,
        millihertz: 60_000,
    };
    let mut host = HostView::new(info, Visibility::default(), Arc::new(|_| {}));
    host.attach_updates(Arc::downgrade(&mailbox));
    for width in [400, 1600] {
        let changed = SurfaceInfo {
            extent: Extent {
                width,
                ..info.extent
            },
            ..info
        };
        mailbox
            .lock()
            .unwrap()
            .merge(SurfaceUpdate::Geometry(changed));
        assert!(
            host.acquisition_current(),
            "size-only frame remains clipped and usable"
        );
    }
    let target = SurfaceInfo { display: 1, ..info };
    mailbox
        .lock()
        .unwrap()
        .merge(SurfaceUpdate::Geometry(target));
    assert!(!host.acquisition_current());
    host.geometry_changed(target);
    assert!(
        host.acquisition_current(),
        "target-display frame can acquire"
    );
}
