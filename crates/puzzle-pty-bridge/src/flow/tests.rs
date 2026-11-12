//! Credit cannot be forged, and both ACK and close release a waiting reader.
#![allow(
    clippy::unwrap_used,
    reason = "credit assertions fail directly on broken test setup"
)]
use super::*;
use std::{
    sync::{Arc, mpsc},
    thread,
    time::Duration,
};

/// A single reader may overshoot once; another read must wait for parsed bytes.
#[test]
fn credit_bounds_and_resume() {
    let flow = Arc::new(Flow::default());
    assert!(flow.ack(1).is_err());
    for _ in 0..=OUTPUT_WINDOW / PAYLOAD_CHUNK {
        assert!(flow.reserve().unwrap());
        flow.read_complete(PAYLOAD_CHUNK).unwrap();
        flow.sent(PAYLOAD_CHUNK).unwrap();
    }
    assert_eq!(
        flow.state.lock().unwrap().pending,
        OUTPUT_WINDOW + PAYLOAD_CHUNK
    );
    assert!(
        flow.ack((OUTPUT_WINDOW + PAYLOAD_CHUNK + 1) as u32)
            .is_err()
    );
    let (sent, received) = mpsc::channel();
    let reader = flow.clone();
    let worker = thread::spawn(move || sent.send(reader.reserve().unwrap()).unwrap());
    assert!(received.recv_timeout(Duration::from_millis(30)).is_err());
    flow.ack(PAYLOAD_CHUNK as u32).unwrap();
    assert!(received.recv_timeout(Duration::from_secs(1)).unwrap());
    worker.join().unwrap();
    // Returning a zero-byte read undoes its entire reservation, not its credit.
    flow.read_complete(0).unwrap();
    assert_eq!(flow.state.lock().unwrap().pending, OUTPUT_WINDOW);
}

/// A disconnect wakes the credit waiter without permitting another PTY read.
#[test]
fn disconnect_wakes_credit_waiter() {
    let flow = Arc::new(Flow::default());
    for _ in 0..=OUTPUT_WINDOW / PAYLOAD_CHUNK {
        flow.reserve().unwrap();
        flow.read_complete(PAYLOAD_CHUNK).unwrap();
    }
    let reader = flow.clone();
    let (sent, received) = mpsc::channel();
    let worker = thread::spawn(move || sent.send(reader.reserve().unwrap()).unwrap());
    flow.close();
    assert!(!received.recv_timeout(Duration::from_secs(1)).unwrap());
    worker.join().unwrap();
}
