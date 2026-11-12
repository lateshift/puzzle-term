//! Shared bridge decoder rejects malformed frames atomically, without a socket.
#![no_main]
use libfuzzer_sys::fuzz_target;
use puzzle_term::bridge;
fuzz_target!(|data: &[u8]| {
    // This is the exact codec also compiled by puzzle-pty-bridge. Error return
    // is the connection's clean-close condition, before PTY allocation or I/O.
    // Successful decoding must retain every byte: accepting only a prefix
    // would let a malformed control smuggle a second request past validation.
    let mut encoded = Vec::new();
    if let Ok(message) = bridge::decode_client(data) {
        assert!(bridge::encode_client(message, &mut encoded).is_ok());
        assert_eq!(encoded, data);
    }
    // Server direction has distinct tags; reflecting one direction into the
    // other is checked by the same production decoder bounds.
    encoded.clear();
    if let Ok(message) = bridge::decode_server(data) {
        assert!(bridge::encode_server(message, &mut encoded).is_ok());
        assert_eq!(encoded, data);
    }
});
