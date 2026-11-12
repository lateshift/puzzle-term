//! Actual layer assignment, coalescing and geometry retirement are independent of UI ticks.
#![allow(
    clippy::unwrap_used,
    reason = "test-owned effects must be present in exact order"
)]
use super::*;
fn fixture() -> (Submissions, mpsc::Receiver<Posted>) {
    let (posts, received) = mpsc::channel();
    (
        Submissions(Mutex::new(Delivery {
            submissions: VecDeque::with_capacity(3),
            endpoint: Arc::new(OnceLock::new()),
            session: None,
            posts,
            wake: Arc::new(|| {}),
            id: SurfaceId(1),
            first: true,
            cursor: None,
            carets: caret::Facts::default(),
            perf: None,
        })),
        received,
    )
}
fn geometry() -> Geometry {
    Geometry {
        size: [800, 480],
        scale: 2.0,
    }
}
fn frame(serial: u64, extent: Geometry) -> Submission {
    Submission {
        serial,
        parsed: 0,
        recorder: None,
        geometry: extent,
        cursor: Some([serial as f32, 8.0, 9.0, 18.0]),
        regions: PresentedRegions::default(),
        statistics: None,
    }
}
fn committed(serial: u64) -> Receipt {
    Receipt {
        serial: Some(serial),
        main_busy: None,
        requested: geometry(),
        committed: Some(geometry()),
        waited: std::time::Duration::ZERO,
        timed_out: false,
    }
}
#[test]
fn ready_submission_does_not_emit_first_frame_until_actual_commit_and_coalesces() {
    let (delivery, received) = fixture();
    delivery.submit(Some(frame(0, geometry())));
    delivery.submit(Some(frame(1, geometry())));
    assert!(received.try_recv().is_err());
    delivery.presented(committed(1));
    assert!(matches!(
        received.try_recv().unwrap(),
        Posted::Event(_, AppEvent::Render(HostAction::FirstFrame))
    ));
    match received.try_recv().unwrap() {
        Posted::Event(_, AppEvent::Render(HostAction::CursorRect(Some(rect)))) => {
            assert_eq!(rect[0], 1.0)
        }
        _ => panic!("latest actually presented caret expected"),
    }
    assert!(received.try_recv().is_err());
    assert!(delivery.0.lock().unwrap().submissions.is_empty());
    delivery.submit(Some(frame(2, geometry())));
    delivery.presented(committed(2));
    assert!(matches!(
        received.try_recv().unwrap(),
        Posted::Event(_, AppEvent::Render(HostAction::CursorRect(_)))
    ));
    assert!(received.try_recv().is_err());
}
#[test]
fn geometry_changes_retire_stale_metadata_even_when_event_tracking_prevents_tick() {
    let (delivery, received) = fixture();
    let mut key = geometry();
    for serial in 0..1_000 {
        // Native events may change bounds without letting App::tick run. CA's
        // own callback retires old metadata, so the history cannot grow per drag.
        key.size[0] = 500 + serial as u32;
        delivery.geometry(key);
        delivery.submit(Some(frame(serial, key)));
        assert_eq!(delivery.0.lock().unwrap().submissions.len(), 1);
    }
    assert!(received.try_recv().is_err());
    // A timed-out callback keeps the previous frame and emits no fake presentation.
    let mut timeout = committed(999);
    timeout.serial = None;
    timeout.timed_out = true;
    delivery.presented(timeout);
    assert!(received.try_recv().is_err());
    assert_eq!(delivery.0.lock().unwrap().submissions.len(), 1);
    delivery.submit(None);
    assert!(delivery.0.lock().unwrap().submissions.is_empty());
}

#[test]
fn host_wake_runs_after_ledger_unlock_and_only_for_new_effects() {
    let (delivery, received) = fixture();
    let delivery = Arc::new(delivery);
    let weak = Arc::downgrade(&delivery);
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let count = Arc::clone(&calls);
    delivery.0.lock().unwrap().wake = Arc::new(move || {
        // Even a host inspecting pending work must never reenter a held ledger.
        let delivery = weak.upgrade().unwrap();
        assert!(delivery.0.try_lock().is_ok());
        count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    });
    delivery.submit(Some(frame(0, geometry())));
    delivery.presented(committed(0));
    assert_eq!(calls.load(std::sync::atomic::Ordering::Relaxed), 1);
    assert_eq!(received.try_iter().count(), 2);
    delivery.presented(committed(0));
    assert_eq!(calls.load(std::sync::atomic::Ordering::Relaxed), 1);
}

#[test]
fn coalesced_native_images_release_metadata_without_an_app_tick() {
    let (delivery, received) = fixture();
    for serial in 0..1_000 {
        if serial > 1 {
            // The completion mailbox superseded this image before CA took it.
            delivery.retire(serial - 2);
        }
        delivery.submit(Some(frame(serial, geometry())));
        assert!(delivery.0.lock().unwrap().submissions.len() <= 2);
        assert!(received.try_recv().is_err());
    }
    delivery.presented(committed(999));
    assert_eq!(received.try_iter().count(), 2);
    assert!(delivery.0.lock().unwrap().submissions.is_empty());
}

/// CA timeout/coalescing does not invent facts; an undrained inbox is observable.
#[test]
fn caret_counts_distinguish_assignment_from_host_delivery() {
    let (delivery, received) = fixture();
    delivery.submit(Some(frame(0, geometry())));
    delivery.submit(Some(frame(1, geometry())));
    let mut timeout = committed(1);
    timeout.serial = None;
    timeout.timed_out = true;
    delivery.presented(timeout);
    assert_eq!(delivery.0.lock().unwrap().carets.emitted, 0);
    delivery.presented(committed(1));
    {
        let facts = delivery.0.lock().unwrap();
        assert_eq!((facts.carets.emitted, facts.carets.delivered), (1, 0));
    }
    // Match App::tick: only actual cursor delivery advances the second total.
    for event in received.try_iter() {
        if let Posted::Event(id, AppEvent::Render(HostAction::CursorRect(rect))) = event {
            delivery.0.lock().unwrap().carets.delivered(id, rect);
        }
    }
    // A later image with the same caret is correctly silent, never a lost fact.
    let mut same = frame(2, geometry());
    same.cursor = frame(1, geometry()).cursor;
    delivery.submit(Some(same));
    delivery.presented(committed(2));
    let facts = delivery.0.lock().unwrap();
    assert_eq!((facts.carets.emitted, facts.carets.delivered), (1, 1));
    assert!(received.try_recv().is_err());
}
