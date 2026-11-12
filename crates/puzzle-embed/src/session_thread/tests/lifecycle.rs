//! A held child retains its owner, and host notification never races queued facts.
use super::*;
use puzzle_term::messages::{Event, ExitInfo};

/// Waiting uses notifications only; no live Session or render-side callback is queried.
fn until(events: &Events, wakes: &Receiver<()>, mut accept: impl FnMut(HostEvent) -> bool) {
    let deadline = Instant::now() + crate::test_support::HANG_GUARD;
    loop {
        events.begin_drain();
        while let Ok(event) = events.try_recv() {
            if accept(event) {
                return;
            }
        }
        wakes
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .unwrap_or_else(|error| panic!("owner notification missing: {error}"));
    }
}

/// Child status cannot precede queued output, and a held key never reaches a dead PTY.
#[test]
fn held_exit_follows_final_output_and_waits_for_host_input() {
    let (writes, received) = mpsc::channel();
    let session = Session::new(
        SessionConfig {
            on_exit: puzzle_term::session::OnExit::Hold,
            ..SessionConfig::default()
        },
        Services::new(Box::new(Writer(writes)), Box::new(Quiet)),
    );
    let (mut producer, consumer) = crate::pty_pool::pair();
    let (exit_tx, exit_rx) = mpsc::channel();
    let (wake_tx, wake_rx) = mpsc::channel();
    let started = start(
        session,
        consumer,
        Config {
            recorder: None,
            decoder: None,
            exit: Some(exit_rx),
            notify: Some(Arc::new(move || {
                let _ = wake_tx.send(());
            })),
        },
    )
    .unwrap_or_else(|error| panic!("owner: {error}"));
    let output = b"\x1b]2;final title\x07\x1b[c";
    producer
        .read_once(&mut Cursor::new(output))
        .unwrap_or_else(|error| panic!("drain: {error}"));
    exit_tx
        .send(ExitInfo {
            code: Some(0),
            signal: None,
            runtime_ms: 500,
        })
        .unwrap_or_else(|error| panic!("exit: {error}"));
    drop(producer);
    assert!(transaction(&received).starts_with(b"\x1b[?"));
    let mut saw_title = false;
    until(&started.events, &wake_rx, |event| match event {
        HostEvent::Fact(Event::TitleChanged(title)) => {
            saw_title = title == "final title";
            false
        }
        HostEvent::Fact(Event::SessionExited(_)) => true,
        _ => false,
    });
    assert!(saw_title);
    assert_eq!(started.handle.parsed_total(), output.len() as u64);
    assert!(!started.worker.is_finished());
    held_key_closes(started, wake_rx, received);
}

/// This is a second host turn after EOF, proving the owner still accepts input.
fn held_key_closes(started: Started, wakes: Receiver<()>, writes: Receiver<Vec<u8>>) {
    started
        .handle
        .send(SessionMessage::Command(
            Command::Key(puzzle_term::vt::input::KeyEvent::press(
                puzzle_term::vt::input::Key::Char('x'),
            )),
            Origin::User,
        ))
        .unwrap_or_else(|error| panic!("key: {error}"));
    until(&started.events, &wakes, |event| {
        matches!(event, HostEvent::Fact(Event::SessionCloseRequested))
    });
    assert!(writes.try_recv().is_err());
    // Releasing the final sending capability ends the held owner without a query.
    drop(started.handle);
    assert!(
        started
            .worker
            .join()
            .unwrap_or_else(|error| panic!("{error:?}"))
            .is_ok()
    );
}

/// Last-sender destruction wakes an idle owner even when the drain remains open.
#[test]
fn releasing_host_capabilities_does_not_wait_for_a_pty_read() {
    let (writes, _) = mpsc::channel();
    let session = Session::new(
        SessionConfig::default(),
        Services::new(Box::new(Writer(writes)), Box::new(Quiet)),
    );
    let (producer, consumer) = crate::pty_pool::pair();
    let started =
        start(session, consumer, Config::default()).unwrap_or_else(|error| panic!("{error}"));
    let (joined, completed) = mpsc::channel();
    drop(started.handle);
    let joiner = thread::spawn(move || {
        let _ = joined.send(started.worker.join());
    });
    assert!(
        completed
            .recv_timeout(crate::test_support::HANG_GUARD)
            .unwrap_or_else(|error| panic!("idle shutdown: {error}"))
            .unwrap_or_else(|error| panic!("{error:?}"))
            .is_ok()
    );
    // The producer is deliberately retained until after the worker has completed.
    drop(producer);
    joiner.join().unwrap_or_else(|error| panic!("{error:?}"));
}

/// An arrival during a host drain retains a wake even if that drain consumes its value.
#[test]
fn host_results_coalesce_and_rearm_before_drain() {
    let (sender, receiver) = mpsc::channel();
    let (wake_tx, wake_rx) = mpsc::channel();
    let (events, signal) = Events::pair(
        receiver,
        Some(Arc::new(move || {
            let _ = wake_tx.send(());
        })),
    );
    for _ in 0..10 {
        sender
            .send(HostEvent::Fact(Event::Bell))
            .unwrap_or_else(|error| panic!("{error}"));
        signal.delivered();
    }
    assert_eq!(wake_rx.try_iter().count(), 1);
    events.begin_drain();
    assert!(matches!(
        events.try_recv(),
        Ok(HostEvent::Fact(Event::Bell))
    ));
    // This publication happens after acknowledgment but before the drain ends.
    sender
        .send(HostEvent::AnimationChanged(true))
        .unwrap_or_else(|error| panic!("{error}"));
    signal.delivered();
    assert_eq!(wake_rx.try_iter().count(), 1);
    assert_eq!(std::iter::from_fn(|| events.try_recv().ok()).count(), 10);
    events.begin_drain();
    assert!(events.try_recv().is_err());
    assert!(wake_rx.try_recv().is_err());
}
