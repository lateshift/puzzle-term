//! A stalled echoing child cannot strand ACKs behind a ten-megabyte paste.
#![allow(
    clippy::unwrap_used,
    reason = "ledger assertions fail on any unexpected validation result"
)]
use super::*;

/// Input never exceeds a grant, and keys/replies retain their application order.
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn paste_credit_and_reply_fifo() {
    let mut input = InputCredit::default();
    let paste: Vec<_> = (0..10_000_000).map(|n| (n % 251) as u8).collect();
    input.push(paste.clone());
    input.push(b"key\x1b[0n".to_vec());
    assert!(input.take_chunk().is_none());
    input.grant(INPUT_WINDOW).unwrap();
    let mut received = Vec::new();
    while let Some(chunk) = input.take_chunk() {
        assert!(chunk.len() <= PAYLOAD_CHUNK);
        received.extend_from_slice(&chunk);
        // ACK/resize processing is independent of this ledger. Only actual
        // PTY writes produce grants, so this models an echoing consumer.
        input.grant(chunk.len() as u32).unwrap();
    }
    assert_eq!(&received[..paste.len()], paste.as_slice());
    assert_eq!(&received[paste.len()..], b"key\x1b[0n");
}

/// A corrupt server cannot grow an unbounded credit balance or wrap it.
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn invalid_grants_are_transactional() {
    let mut input = InputCredit::default();
    input.grant(INPUT_WINDOW).unwrap();
    assert!(input.grant(1).is_err());
    assert!(input.grant(u32::MAX).is_err());
    assert!(input.grant(0).is_err());
    input.push(vec![1; INPUT_WINDOW as usize + 1]);
    let count: usize = std::iter::from_fn(|| input.take_chunk())
        .map(|v| v.len())
        .sum();
    assert_eq!(count, INPUT_WINDOW as usize);
}

/// Sending a reply is not consumption; even its last sent chunk retains the lease.
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn reply_completion_requires_actual_writer_credit_and_releases_lease_first() {
    use std::sync::Arc;
    let storage: Arc<[u8]> = vec![0x55; PAYLOAD_CHUNK + 7].into();
    let mut input = InputCredit::default();
    input.push(b"key".to_vec());
    input.push_lease(Box::new(storage.clone()), 91);
    input.grant(INPUT_WINDOW).unwrap();
    assert!(input.take_completed().is_none());
    let mut sent = 0;
    while let Some(chunk) = input.take_chunk() {
        sent += chunk.len();
    }
    assert_eq!(sent, 3 + storage.len());
    assert_eq!(
        Arc::strong_count(&storage),
        2,
        "send released prepaid storage too early"
    );
    assert!(input.take_completed().is_none());
    input.grant((sent - 1) as u32).unwrap();
    assert!(input.take_completed().is_none());
    assert_eq!(Arc::strong_count(&storage), 2);
    input.grant(1).unwrap();
    assert_eq!(input.take_completed(), Some(91));
    assert_eq!(
        Arc::strong_count(&storage),
        1,
        "completion arrived before lease release"
    );
    assert!(input.take_completed().is_none());
    assert!(
        input.grant(1).is_err(),
        "a duplicate grant invented writer consumption"
    );
}
