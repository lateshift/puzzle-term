//! Literal v1 wire fixtures and properties over hostile WebSocket payloads.
#![allow(
    clippy::unwrap_used,
    reason = "assertions expose setup or contract failures in tests"
)]
use super::*;
use proptest::prelude::*;

/// Every type has an independent byte fixture, including LE and signed status.
#[test]
fn wire_round_trip() {
    let size = Size {
        cols: 120,
        rows: 40,
        width_px: 960,
        height_px: 640,
    };
    let clients = [
        (Client::Hello { light: true }, vec![3, 1, 1]),
        (Client::Input(b"\x04hello"), b"\0\x04hello".to_vec()),
        (Client::Resize(size), vec![1, 120, 0, 40, 0, 192, 3, 128, 2]),
        (Client::Ack(65_536), vec![2, 0, 0, 1, 0]),
    ];
    let mut encoded = Vec::new();
    for (value, bytes) in clients {
        encode_client(value, &mut encoded).unwrap();
        assert_eq!(encoded, bytes);
        assert_eq!(decode_client(&bytes).unwrap(), value);
    }
    for (value, bytes) in [
        (Server::Hello, vec![130, 1]),
        (Server::InputCredit(1_048_576), vec![132, 0, 0, 16, 0]),
        (Server::Output(b"done"), b"\x80done".to_vec()),
        (Server::Exit(-3), vec![129, 253, 255, 255, 255]),
        (
            Server::Error("wrong version"),
            b"\x83wrong version".to_vec(),
        ),
    ] {
        encode_server(value, &mut encoded).unwrap();
        assert_eq!(encoded, bytes);
        assert_eq!(decode_server(&bytes).unwrap(), value);
    }
}

/// Fixed controls reject both truncation and suffixes, not just unknown tags.
#[test]
fn malformed_controls_rejected() {
    for bytes in [
        vec![],
        vec![3, 2, 0],
        vec![3, 1, 2],
        vec![3, 1, 0, 0],
        vec![1, 0, 0, 24, 0, 0, 0, 0, 0],
        vec![1, 1, 0, 24, 0, 0, 0, 0, 0],
        vec![1, 0, 16, 0, 16, 0, 0, 0, 0],
        vec![2, 0, 0, 0],
        vec![128],
    ] {
        assert!(decode_client(&bytes).is_err(), "{bytes:?}");
    }
    for bytes in [
        vec![129, 0],
        vec![130, 2],
        vec![130, 1, 0],
        vec![131, 255],
        vec![132, 0, 0, 0],
        vec![132, 0, 0, 0, 0, 0],
        vec![0],
    ] {
        assert!(decode_server(&bytes).is_err(), "{bytes:?}");
    }
    assert!(decode_client(&vec![0; MAX_MESSAGE + 1]).is_err());
    assert!(decode_server(&vec![128; MAX_MESSAGE + 1]).is_err());
}

/// Literal FNV-1a vectors and split updates keep transfer evidence comparable.
#[test]
fn checksum_vectors_and_chunk_boundaries() {
    use super::checksum::{FNV1A_BASIS, fnv1a_update};
    assert_eq!(fnv1a_update(FNV1A_BASIS, []), FNV1A_BASIS);
    assert_eq!(fnv1a_update(FNV1A_BASIS, *b"hello"), 0xa430_d846_80aa_bd0b);
    // Network packet boundaries must not change the checksum of the same bytes.
    let first = fnv1a_update(FNV1A_BASIS, *b"he");
    assert_eq!(fnv1a_update(first, *b"llo"), 0xa430_d846_80aa_bd0b);
}

proptest! {
    // Arbitrary-byte generators rarely produce this fixed five-byte control.
    // Explicitly vary all 32 bits to cover LE grants, zero and maximum credit.
    #[test]
    fn input_credit_round_trip(grant in any::<u32>()) {
        let message = Server::InputCredit(grant);
        let mut bytes = Vec::new();
        encode_server(message, &mut bytes).unwrap();
        prop_assert_eq!(decode_server(&bytes).unwrap(), message);
    }

    // Re-encoding every accepted arbitrary packet must preserve all bytes:
    // a decoder cannot accept a partial interpretation or hidden suffix.
    #[test]
    fn arbitrary_frames_never_panic(bytes in proptest::collection::vec(any::<u8>(), 0..70_000)) {
        let mut encoded = Vec::new();
        if let Ok(message) = decode_client(&bytes) {
            encode_client(message, &mut encoded).unwrap();
            prop_assert_eq!(&encoded, &bytes);
        }
        if let Ok(message) = decode_server(&bytes) {
            encode_server(message, &mut encoded).unwrap();
            prop_assert_eq!(&encoded, &bytes);
        }
    }
}
