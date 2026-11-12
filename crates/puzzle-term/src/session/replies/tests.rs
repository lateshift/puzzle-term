//! Credit proofs use a transport that retains every write until explicitly consumed.
#![allow(
    clippy::unwrap_used,
    reason = "test failures identify broken reply admission invariants"
)]
use super::*;
use crate::platform::{PtySize, reply::PENDING_BYTE_ALLOWANCE};

/// The fake peer deliberately never consumes input unless the test completes a ticket.
#[derive(Default)]
struct Stalled {
    /// Exact FIFO receipt and bytes, as held by a blocked native writer.
    replies: VecDeque<ReplyWrite>,
    /// User input delivered after the complete logical reply stream.
    input: Vec<Vec<u8>>,
}
impl Transport for Stalled {
    fn write(&mut self, bytes: &[u8]) -> Result<(), String> {
        self.input.push(bytes.to_vec());
        Ok(())
    }
    fn deferred_replies(&self) -> bool {
        true
    }
    fn write_reply(&mut self, reply: ReplyWrite) -> Result<ReplyDelivery, String> {
        self.replies.push_back(reply);
        Ok(ReplyDelivery::Pending)
    }
    fn resize(&mut self, _: PtySize) -> Result<(), String> {
        Ok(())
    }
    fn close(&mut self) -> Result<(), String> {
        Ok(())
    }
}

#[test]
fn nonreading_peer_defers_reply_flood_without_discarding_or_consuming_twice() {
    let mut lane = Lane::default();
    let mut peer = Stalled::default();
    let mut expected = Vec::new();
    let packet = b"\x1b[0n".repeat(4096);
    while lane.can_parse() {
        let mut bytes = packet.clone();
        expected.extend_from_slice(&bytes);
        assert!(lane.replies(&mut peer, &mut bytes).unwrap());
        assert!(bytes.is_empty());
        assert!(lane.pending_bytes() <= PENDING_BYTE_ALLOWANCE);
    }
    let outstanding = lane.pending_bytes();
    assert!(outstanding > PENDING_BYTE_ALLOWANCE - CALLBACK_HEADROOM - packet.len());
    lane.input(&mut peer, b"key").unwrap();
    assert_eq!(peer.input, [b"key".to_vec()]);
    let mut actual = Vec::new();
    while let Some(reply) = peer.replies.pop_front() {
        actual.extend_from_slice(&reply.bytes);
        lane.complete(&mut peer, reply.ticket.id()).unwrap();
        lane.pump(&mut peer).unwrap();
        assert!(lane.complete(&mut peer, reply.ticket.id()).is_err());
    }
    assert_eq!(actual, expected);
    assert!(lane.can_parse());
    assert_eq!(lane.pending_bytes(), 0);
}

#[test]
fn large_clipboard_result_stays_bounded_and_keys_follow_done() {
    use puzzle_vt::clipboard::{MimeData, Operation, Request, Response, Status};
    let request = Request {
        id: "read".into(),
        primary: false,
        password: None,
        operation: Operation::Read(vec!["image/png".into()]),
    };
    let items = vec![MimeData {
        mime: "image/png".into(),
        data: vec![0x55; PENDING_BYTE_ALLOWANCE * 2].into(),
    }];
    let mut expected = Vec::new();
    puzzle_vt::clipboard::respond(&request, Status::Done, &items, &mut expected);
    let mut lane = Lane::default();
    let mut peer = Stalled::default();
    lane.stream(
        &mut peer,
        Box::new(Response::new(request, Status::Done, items)),
    )
    .unwrap();
    lane.input(&mut peer, b"key").unwrap();
    while !lane.waiting_credit() && lane.pending() {
        lane.pump(&mut peer).unwrap();
    }
    assert!(peer.input.is_empty(), "key overtook the lazy DATA stream");
    let mut actual = Vec::new();
    while lane.pending() || !peer.replies.is_empty() {
        assert!(lane.pending_bytes() <= PENDING_BYTE_ALLOWANCE);
        if let Some(reply) = peer.replies.pop_front() {
            actual.extend_from_slice(&reply.bytes);
            lane.complete(&mut peer, reply.ticket.id()).unwrap();
            lane.pump(&mut peer).unwrap();
        } else {
            lane.pump(&mut peer).unwrap();
        }
    }
    assert_eq!(actual, expected);
    assert_eq!(peer.input, [b"key".to_vec()]);
    assert_eq!(lane.pending_bytes(), 0);
}

#[test]
fn precharged_error_reply_needs_no_new_payload_when_state_is_full() {
    let mut lane = Lane::default();
    let mut peer = Stalled::default();
    let mut buffer = ReusableReply::new(4096).unwrap();
    // The caller state cap includes the prepaid packet allocation.
    // Fill all other available state; replying must not ask that table for space.
    let full_state = vec![0u8; 1024 * 1024 - buffer.allocated_bytes()];
    assert_eq!(
        full_state.capacity() + buffer.allocated_bytes(),
        1024 * 1024
    );
    let warm = lane.precharged(&mut buffer, b"warm").unwrap();
    lane.pump(&mut peer).unwrap();
    drop(peer.replies.pop_front().unwrap());
    lane.complete(&mut peer, warm).unwrap();
    let allocation = allocation_counter::measure(|| {
        lane.precharged(&mut buffer, b"limit").unwrap();
        lane.pump(&mut peer).unwrap();
    });
    assert_eq!(
        allocation.count_total, 0,
        "full state required another reply allocation"
    );
    let reply = peer.replies.pop_front().unwrap();
    let id = reply.ticket.id();
    assert_eq!(&*reply.bytes, b"limit");
    assert_eq!(lane.pending_bytes(), buffer.allocated_bytes());
    let callback = reply.bytes.lease().unwrap();
    assert_eq!(
        u64::from_le_bytes(callback.framed()[..8].try_into().unwrap()),
        id
    );
    assert_eq!(callback.framed().len(), 8 + b"limit".len());
    drop(reply);
    lane.complete(&mut peer, id).unwrap();
    assert!(
        !buffer.available(),
        "a callback still borrows this allocation"
    );
    assert!(lane.precharged(&mut buffer, b"next").is_none());
    drop(callback);
    assert!(buffer.available());
    assert!(lane.precharged(&mut buffer, b"next").is_some());
    std::hint::black_box(full_state);
}
