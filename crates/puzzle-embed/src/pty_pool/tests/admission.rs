//! Nonblocking output admission must keep every pool allocation under wake traffic.
#![allow(clippy::unwrap_used, reason = "test fixtures expose failures directly")]
use super::*;

/// Control wakes can fill the inbox while an empty payload is available. Failed
/// admission must retain that payload and leave all its bytes with the caller.
#[test]
fn wake_tokens_cannot_lose_admission_storage_or_bytes() {
    let (mut producer, mut consumer) = pair();
    let wake = consumer.wake_handle();
    for _ in 0..POOL_COUNT {
        wake.notify();
    }
    assert_eq!(producer.admit(b"first").unwrap(), 0);
    assert_eq!(consumer.pending_bytes(), 0);
    assert!(consumer.next_slice().is_none());
    for _ in 0..POOL_COUNT {
        assert_eq!(producer.admit(&[7; BUFFER_SIZE]).unwrap(), BUFFER_SIZE);
    }
    assert_eq!(producer.admit(b"blocked").unwrap(), 0);
    assert_eq!(consumer.pending_bytes(), POOL_COUNT * BUFFER_SIZE);
    let mut count = 0;
    while let Some(bytes) = consumer.next_slice() {
        assert!(bytes.iter().all(|byte| *byte == 7));
        let len = bytes.len();
        count += len;
        consumer.consume(len).unwrap();
    }
    assert_eq!(count, POOL_COUNT * BUFFER_SIZE);
    assert_eq!(producer.admit(b"retry").unwrap(), 5);
    drop(producer);
    assert_eq!(consumer.next_slice().unwrap(), b"retry");
    consumer.consume(5).unwrap();
    assert!(consumer.next_slice().is_none());
    assert!(consumer.closed());
}

/// Caller-side suffix retries transfer 100 MB with exact order and fixed memory.
#[test]
fn host_hundred_mb_exact_and_bounded() {
    let (mut producer, mut consumer) = pair();
    let mut source = vec![0; 3 * BUFFER_SIZE + 17];
    let (mut sent, mut received) = (0, 0);
    let mut pauses = 0;
    while received < TEST_BYTES {
        let length = source.len().min(TEST_BYTES - sent);
        for (offset, byte) in source[..length].iter_mut().enumerate() {
            *byte = pattern(sent + offset);
        }
        let count = producer.admit(&source[..length]).unwrap();
        sent += count;
        if count < length {
            pauses += 1;
        }
        // Consuming less than admission ensures the fixture exhausts pool credit.
        if let Some(bytes) = consumer.next_slice() {
            for (offset, byte) in bytes.iter().enumerate() {
                assert_eq!(*byte, pattern(received + offset));
            }
            let count = bytes.len();
            consumer.consume(count).unwrap();
            received += count;
        }
        assert!(consumer.pending_bytes() <= POOL_COUNT * BUFFER_SIZE);
    }
    assert_eq!(sent, TEST_BYTES);
    assert!(pauses > 0);
    assert!(consumer.peak_bytes() <= POOL_COUNT * BUFFER_SIZE);
}

/// Buffer ownership cycles allocate nothing once channel storage has warmed.
#[test]
fn warm_admission_allocates_nothing() {
    let (mut producer, mut consumer) = pair();
    let bytes = [0; BUFFER_SIZE];
    let mut turn = || {
        assert_eq!(producer.admit(&bytes).unwrap(), BUFFER_SIZE);
        for _ in 0..(BUFFER_SIZE / SLICE_SIZE) {
            assert_eq!(consumer.next_slice().unwrap().len(), SLICE_SIZE);
            consumer.consume(SLICE_SIZE).unwrap();
        }
    };
    for _ in 0..100 {
        turn();
    }
    let measured = allocation_counter::measure(|| {
        for _ in 0..100 {
            turn();
        }
    });
    assert_eq!(measured.count_total, 0);
}
