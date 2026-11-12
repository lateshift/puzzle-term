//! Embedding teardown must join real IO, even with retained input and snapshot handles.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test failures name teardown and native-child preconditions"
)]
use super::*;
use crate::{pty, pty_pool, session_thread, test_support, window::live::start_reader};
use puzzle_term::{
    HostEvent, Session, SessionConfig,
    messages::Event,
    platform::{PtySize, PublicationWaker, Services},
};
use std::{sync::mpsc, time::Instant};

/// Joins real worker teardown under a deadlock guard, never a debug speed budget.
fn drop_joined(pipeline: Pipeline) {
    let (send, receive) = mpsc::channel();
    let joiner = std::thread::spawn(move || {
        drop(pipeline);
        send.send(()).unwrap();
    });
    receive
        .recv_timeout(test_support::HANG_GUARD)
        .expect("pipeline teardown deadlocked");
    joiner.join().unwrap();
}

/// Uses the product reader and writer with a live, held-screen Session owner.
fn live(
    command: &str,
) -> (
    Pipeline,
    session_thread::Handle,
    session_thread::Events,
    session_thread::Frames,
    mpsc::Receiver<()>,
) {
    let mut child = spawn(command);
    let killer = child.child.clone_killer();
    let writer = child.transport.start_writer().unwrap();
    let (producer, consumer) = pty_pool::pair();
    let (send_exit, exit) = mpsc::channel();
    let drain = start_reader(
        child.reader,
        producer,
        child.child,
        child.started,
        send_exit,
    );
    let mut pipeline = Pipeline::new(killer, writer, drain);
    let session = Session::new(
        SessionConfig {
            on_exit: puzzle_term::session::OnExit::Hold,
            ..SessionConfig::default()
        },
        Services::new(
            Box::new(child.transport),
            Box::<PublicationWaker>::default(),
        ),
    );
    let (send_wake, wake) = mpsc::channel();
    let started = session_thread::start(
        session,
        consumer,
        session_thread::Config {
            exit: Some(exit),
            notify: Some(std::sync::Arc::new(move || {
                let _ = send_wake.send(());
            })),
            recorder: None,
            decoder: None,
        },
    )
    .unwrap();
    pipeline.attach(started.handle.clone(), started.worker);
    (
        pipeline,
        started.handle,
        started.events,
        started.frames,
        wake,
    )
}

/// An output flood may saturate the fixed pool but cannot strand App teardown.
#[test]
fn close_during_flood_joins_session_drain_and_writer() {
    let (pipeline, handle, _events, _frames, _wake) = live("stty raw -echo; exec yes output");
    let end = Instant::now() + test_support::HANG_GUARD;
    // Only a hang guard bounds waiting for real parse progress. No speed is asserted.
    while handle.parsed_total() < 65_536 {
        assert!(Instant::now() < end, "child never produced the flood");
        std::thread::yield_now();
    }
    drop_joined(pipeline);
    // Retained host handles must not keep a cancelled owner alive.
    assert!(
        handle
            .send(puzzle_term::SessionMessage::Command(
                puzzle_term::messages::Command::Focus(true),
                puzzle_term::Origin::User
            ))
            .is_err()
    );
}

/// A held final screen still owns its Session after every output byte is drained.
#[test]
fn held_final_screen_joins_all_workers_with_host_handles_alive() {
    let (pipeline, handle, events, _frames, wake) =
        live("stty raw -echo; printf 'final screen\\r\\n'");
    loop {
        events.begin_drain();
        if std::iter::from_fn(|| events.try_recv().ok())
            .any(|event| matches!(event, HostEvent::Fact(Event::SessionExited(_))))
        {
            break;
        }
        wake.recv_timeout(test_support::HANG_GUARD)
            .expect("no child exit event");
    }
    assert_eq!(handle.parsed_total(), 14);
    assert!(!pipeline.owner.as_ref().unwrap().is_finished());
    drop_joined(pipeline);
}

/// Without a Session, cancellation must also release an idle writer queue and OS read.
#[test]
fn failure_before_session_attachment_joins_io() {
    let mut child = spawn("stty raw -echo; exec sleep 60");
    let killer = child.child.clone_killer();
    let writer = child.transport.start_writer().unwrap();
    let (producer, consumer) = pty_pool::pair();
    let (exit, _exits) = mpsc::channel();
    let drain = start_reader(child.reader, producer, child.child, child.started, exit);
    let pipeline = Pipeline::new(killer, writer, drain);
    // Session construction failure releases its pool first; retain the transport
    // deliberately to prove cancellation does not depend on the last writer sender.
    drop(consumer);
    drop_joined(pipeline);
    drop(child.transport);
}

/// The same native geometry applies to all lifecycle witnesses, without a GPU.
fn spawn(script: &str) -> pty::SpawnedPty {
    let mut command = portable_pty::CommandBuilder::new("/bin/sh");
    command.args(["-c", script]);
    pty::spawn(
        command,
        PtySize {
            columns: 80,
            rows: 24,
            pixel_width: 800,
            pixel_height: 480,
        },
    )
    .unwrap()
}

/// Shutdown targets the tty group even if the shell already exited and a background
/// process alone holds the slave. These are real PTYs, not simulated EOF channels.
#[test]
#[cfg(unix)]
fn background_slave_holders_cannot_strand_teardown() {
    for command in [
        "stty raw -echo; sleep 1000 & printf ready; exit",
        "stty raw -echo; nohup sleep 1000 >/dev/tty 2>&1 & printf ready; exit",
        "stty raw -echo; trap '' HUP; exec yes output",
        "stty raw -echo; trap '' HUP TERM; exec yes output",
    ] {
        let (pipeline, handle, _events, _frames, _wake) = live(command);
        let pid = pipeline.drain.as_ref().unwrap().process_id();
        let end = Instant::now() + test_support::HANG_GUARD;
        while handle.parsed_total() < 5 {
            assert!(Instant::now() < end, "child never started: {command}");
            std::thread::yield_now();
        }
        drop_joined(pipeline);
        assert_group_gone(pid);
    }
}

/// ps proves both the direct child and its inherited-tty descendants were reaped
/// or collected by the OS, rather than hidden behind a successful channel receipt.
#[cfg(unix)]
pub(crate) fn assert_group_gone(pid: u32) {
    let end = Instant::now() + test_support::HANG_GUARD;
    loop {
        let output = std::process::Command::new("/bin/ps")
            .args(["-axo", "pid=,pgid=,stat="])
            .output()
            .unwrap();
        assert!(output.status.success(), "ps failed: {:?}", output);
        let text = String::from_utf8(output.stdout).unwrap();
        let survivors: Vec<_> = text
            .lines()
            .filter(|line| {
                let mut words = line.split_whitespace();
                words.next().and_then(|s| s.parse::<u32>().ok()) == Some(pid)
                    || words.next().and_then(|s| s.parse::<u32>().ok()) == Some(pid)
            })
            .collect();
        if survivors.is_empty() {
            return;
        }
        assert!(
            Instant::now() < end,
            "surviving tty group {pid}: {survivors:?}"
        );
        // Orphan reparent/reap is kernel scheduling, not a product speed assertion.
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}
