//! Native App decoder handoff preserves queued VT, priority keys and joined EOF.
#![allow(
    clippy::unwrap_used,
    reason = "tests assert fixture construction and exact worker results"
)]
use super::*;

/// Multiple Sessions share one bounded worker without dropping images or later replies.
#[test]
fn shared_image_worker_preserves_replies_and_joins_at_eof() {
    let decoder = crate::image_decode::worker::Worker::start().unwrap();
    let mut owners = Vec::new();
    for _ in 0..8 {
        let (write, received) = mpsc::channel();
        let mut session = Session::new(
            SessionConfig::default(),
            Services::new(Box::new(Writer(write)), Box::new(Quiet)),
        );
        session.enable_images(4096);
        let (mut producer, consumer) = crate::pty_pool::pair();
        let started = start(
            session,
            consumer,
            Config {
                decoder: decoder.sender(),
                ..Config::default()
            },
        )
        .unwrap();
        producer
            .read_once(&mut Cursor::new(
                b"\x1b_Ga=q,i=1,s=1,v=1;/wAA/w==\x1b\\\x1b[c",
            ))
            .unwrap();
        drop(producer);
        owners.push((started, received));
    }
    for (started, received) in owners {
        let first = transaction(&received);
        let second = transaction(&received);
        let all = [first, second].concat();
        assert_eq!(all, b"\x1b_Gi=1;OK\x1b\\\x1b[?62;22c");
        assert!(started.worker.join().unwrap().is_ok());
    }
    // All Session senders are gone; the worker recv ends and its thread joins.
    drop(decoder);
}

/// Yazi's unknown-brand detector requires the direct probe before its DA1 sentinel.
#[test]
fn yazi_exact_probe_precedes_da1_and_rejects_shared_memory() {
    let decoder = crate::image_decode::worker::Worker::start().unwrap();
    let (write, received) = mpsc::channel();
    let mut session = Session::new(
        SessionConfig::default(),
        Services::new(Box::new(Writer(write)), Box::new(Quiet)),
    );
    session.enable_images(4096);
    let (mut producer, consumer) = crate::pty_pool::pair();
    let started = start(
        session,
        consumer,
        Config {
            decoder: decoder.sender(),
            ..Config::default()
        },
    )
    .unwrap();
    // Exact yazi 26.9.1 direct and shared-memory probes, followed by DA1.
    producer.read_once(&mut Cursor::new(b"\x1b_Gi=278941603,s=1,v=1,a=q,t=d,f=24;AAAA\x1b\\\x1b_Gi=916472805,s=1,v=1,a=q,t=s,f=24,S=3;L3lhaS1wcm9iZQ==\x1b\\\x1b[c")).unwrap();
    drop(producer);
    let mut all = Vec::new();
    while !all.ends_with(b"\x1b[?62;22c") {
        all.extend(transaction(&received));
    }
    assert_eq!(all, b"\x1b_Gi=278941603;OK\x1b\\\x1b_Gi=916472805;ENOTSUP:unsupported graphics feature\x1b\\\x1b[?62;22c");
    assert!(started.worker.join().unwrap().is_ok());
    drop(decoder);
}
