//! D-53 lifetime regressions: retained contexts never dereference window state.
#![allow(
    clippy::unwrap_used,
    reason = "tests require their channels and published ticks"
)]
use super::{signal::Signal, *};
use std::sync::atomic::{AtomicUsize, Ordering};

/// Restart evidence keeps the first callback even when the actor coalesces later ticks.
#[test]
fn first_callback_survives_coalescing_and_resets_on_restart() {
    let signal = Signal::new(Arc::new(|| {}));
    let start = signal.origin;
    signal.activate(1);
    for offset in [13, 21, 29] {
        let callback = start + Duration::from_millis(offset);
        signal.publish(offset, callback, Duration::from_millis(8), callback);
    }
    let mut seen = 0;
    assert_eq!(signal.take(&mut seen).unwrap().index, 29);
    assert_eq!(
        signal.first_callback(),
        Some(start + Duration::from_millis(13))
    );
    signal.activate(0);
    assert!(signal.take(&mut seen).is_none());
    signal.activate(2);
    let callback = start + Duration::from_millis(100);
    signal.publish(100, callback, Duration::from_millis(8), callback);
    assert_eq!(signal.take(&mut seen).unwrap().index, 100);
    assert_eq!(signal.first_callback(), Some(callback));
}

/// Actual CoreVideo callbacks continue across stop/restart without freeing context.
#[cfg(target_os = "macos")]
#[test]
fn native_link_close_under_flood_and_process_shutdown() {
    let (wake, wakes) = std::sync::mpsc::sync_channel(1);
    let process_wake: Wake = Arc::new(move || {
        let _ = wake.try_send(());
    });
    let display = macos::registry::install_test_source().unwrap();
    let mut links = DisplayLinks::new(move || process_wake());
    for _ in 0..3 {
        links.subscribe(display, 2).unwrap();
        for _ in 0..10 {
            wakes.recv_timeout(Duration::from_secs(3)).unwrap();
            let tick = links.take_refresh(display).unwrap();
            assert!(!tick.period.is_zero());
        }
        // Closing one window keeps its sibling's link active.
        links.subscribe(display, 1).unwrap();
        wakes.recv_timeout(Duration::from_secs(3)).unwrap();
        assert!(links.take_refresh(display).is_some());
        // Last-window close invalidates every queued tick before native stop.
        links.subscribe(display, 0).unwrap();
        assert!(links.take_refresh(display).is_none());
        while wakes.try_recv().is_ok() {}
    }
    links.subscribe(display, 1).unwrap();
    wakes.recv_timeout(Duration::from_secs(3)).unwrap();
    // Registry drop during output closes the gate without destroying native data.
    drop(links);
    let (next_wake, next_wakes) = std::sync::mpsc::sync_channel(1);
    let mut stopped = DisplayLinks::new(move || {
        let _ = next_wake.try_send(());
    });
    assert!(stopped.take_refresh(display).is_none());
    // A new App reuses the permanent link, but its render actor has a new inbox.
    // The retained context must wake that inbox, never the already-joined actor.
    stopped.subscribe(display, 1).unwrap();
    next_wakes.recv_timeout(Duration::from_secs(3)).unwrap();
    assert!(stopped.take_refresh(display).is_some());
    stopped.subscribe(display, 0).unwrap();
    // A vanished/nonexistent display cannot manufacture a source or a callback.
    assert!(stopped.subscribe(u32::MAX, 1).is_err());
    assert!(stopped.take_refresh(u32::MAX).is_none());
}

/// Closing during continuous output detaches wakes while old callbacks stay safe.
#[test]
fn close_and_shutdown_during_refresh_flood() {
    let wakes = Arc::new(AtomicUsize::new(0));
    let count = wakes.clone();
    let signal = Arc::new(Signal::new(Arc::new(move || {
        count.fetch_add(1, Ordering::Relaxed);
    })));
    signal.activate(1);
    let worker_signal = signal.clone();
    let (ready, arrived) = std::sync::mpsc::channel();
    let (release, resume) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        let time = worker_signal.origin;
        for _ in 0..1_000 {
            worker_signal.publish(2, time, Duration::from_millis(8), time);
        }
        ready.send(()).unwrap();
        resume.recv().unwrap();
        // Simulate the straggler which stop does not join. Its permanent
        // context remains valid, but the detached generation suppresses wakes.
        worker_signal.publish(2, time, Duration::from_millis(8), time);
    });
    arrived.recv_timeout(Duration::from_secs(3)).unwrap();
    signal.activate(0);
    let before = wakes.load(Ordering::Relaxed);
    release.send(()).unwrap();
    worker.join().unwrap();
    assert_eq!(wakes.load(Ordering::Relaxed), before);
    assert!(signal.take(&mut 0).is_none());
}

/// A move or display disappearance invalidates its old ticks before another starts.
#[test]
fn moving_and_disappearing_displays_reject_obsolete_refreshes() {
    let source = Signal::new(Arc::new(|| {}));
    let destination = Signal::new(Arc::new(|| {}));
    let time = source.origin;
    source.activate(1);
    source.publish(2, time, Duration::from_millis(16), time);
    source.activate(0);
    destination.activate(1);
    assert!(source.take(&mut 0).is_none());
    destination.publish(
        2,
        destination.origin,
        Duration::from_millis(8),
        destination.origin,
    );
    assert_eq!(
        destination.take(&mut 0).unwrap().period,
        Duration::from_millis(8)
    );
    destination.activate(0);
    destination.publish(
        2,
        destination.origin,
        Duration::from_millis(8),
        destination.origin,
    );
    assert!(destination.take(&mut 0).is_none());
    // Returning to the retained source must not replay its preceding incarnation.
    source.activate(2);
    assert!(source.take(&mut 0).is_none());
    source.publish(2, time, Duration::from_millis(16), time);
    assert_eq!(source.take(&mut 0).unwrap().index, 2);
}

/// Steady callback publication and receipt allocate nothing (P1).
#[test]
fn refreshed_mailbox_is_allocation_free_and_coalesces() {
    // Match the actor's bounded wake channel rather than an empty callback.
    let (wake, wakes) = std::sync::mpsc::sync_channel(1);
    let signal = Signal::new(Arc::new(move || {
        let _ = wake.try_send(());
    }));
    signal.activate(1);
    let now = signal.origin;
    signal.publish(2, now, Duration::from_millis(8), now);
    let mut seen = 0;
    let allocations = allocation_counter::measure(|| {
        for _ in 0..1_000 {
            signal.publish(2, now, Duration::from_millis(8), now);
            signal.publish(2, now, Duration::from_millis(8), now);
            assert!(signal.take(&mut seen).is_some());
            assert!(signal.take(&mut seen).is_none());
            assert!(wakes.try_recv().is_ok());
        }
    });
    assert_eq!(allocations.count_total, 0);
    assert_eq!(seen / 2, 2_001);
}

/// Stop/restart overlap may coalesce ticks but must never mix two timestamp tuples.
#[test]
fn overlapping_native_producers_publish_only_complete_clock_facts() {
    let signal = Arc::new(Signal::new(Arc::new(|| {})));
    signal.activate(1);
    let mut workers = Vec::new();
    for index in [1, 2] {
        let signal = signal.clone();
        workers.push(std::thread::spawn(move || {
            for _ in 0..10_000 {
                signal.publish(
                    index,
                    signal.origin + Duration::from_millis(index),
                    Duration::from_millis(index * 8),
                    signal.origin,
                );
            }
        }));
    }
    let mut seen = 0;
    while workers.iter().any(|worker| !worker.is_finished()) {
        if let Some(tick) = signal.take(&mut seen) {
            assert!(matches!(tick.index, 1 | 2));
            assert_eq!(
                tick.output,
                signal.origin + Duration::from_millis(tick.index)
            );
            assert_eq!(tick.period, Duration::from_millis(tick.index * 8));
            assert_eq!(tick.callback, signal.origin);
        }
        std::thread::yield_now();
    }
    for worker in workers {
        worker.join().unwrap();
    }
    // Even if the scheduler ran both producers first, their last sealed tuple
    // remains observable; this test measures coherence, not scheduling fairness.
    if let Some(tick) = signal.take(&mut seen) {
        assert!(matches!(tick.index, 1 | 2));
        assert_eq!(
            tick.output,
            signal.origin + Duration::from_millis(tick.index)
        );
        assert_eq!(tick.period, Duration::from_millis(tick.index * 8));
        assert_eq!(tick.callback, signal.origin);
    }
    assert!(seen > 0);
}

/// Other targets keep the timer path without foreign code or dependencies.
#[cfg(not(target_os = "macos"))]
#[test]
fn other_platforms_report_unsupported() {
    let mut links = DisplayLinks::new(|| {});
    assert!(matches!(links.subscribe(1, 1), Err(LinkError::Unsupported)));
    assert!(links.take_refresh(1).is_none());
}

/// One callback must wake every App, and independent cursors read the same tick.
#[test]
fn shared_signal_fans_out_without_stealing_ticks_or_losing_survivors() {
    let a = Arc::new(AtomicUsize::new(0));
    let b = Arc::new(AtomicUsize::new(0));
    let first = Arc::clone(&a);
    let second = Arc::clone(&b);
    let aw: Wake = Arc::new(move || {
        first.fetch_add(1, Ordering::Relaxed);
    });
    let bw: Wake = Arc::new(move || {
        second.fetch_add(1, Ordering::Relaxed);
    });
    let signal = Signal::empty();
    signal.set_wakes(Arc::from([aw, Arc::clone(&bw)]));
    signal.activate(1);
    let now = signal.origin;
    let mut seen_a = signal.cursor();
    let mut seen_b = signal.cursor();
    signal.publish(1, now, Duration::from_millis(8), now);
    assert_eq!(signal.take(&mut seen_a).unwrap().index, 1);
    assert_eq!(signal.take(&mut seen_b).unwrap().index, 1);
    assert_eq!(
        (a.load(Ordering::Relaxed), b.load(Ordering::Relaxed)),
        (1, 1)
    );
    // Detach one App without stopping or consuming the surviving reader's clock.
    signal.set_wakes(Arc::from([bw]));
    signal.publish(2, now, Duration::from_millis(8), now);
    assert_eq!(signal.take(&mut seen_b).unwrap().index, 2);
    assert_eq!(
        (a.load(Ordering::Relaxed), b.load(Ordering::Relaxed)),
        (1, 2)
    );
}
