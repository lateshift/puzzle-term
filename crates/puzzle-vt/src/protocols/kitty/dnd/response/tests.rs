//! Lazy OSC 72 cursors retain the eager wire bytes across all chunk boundaries.
use super::*;
use crate::reply_stream::ReplySource;

/// Empty, exact-chunk and partial-chunk values keep padding, m and EOF unchanged.
#[test]
fn lazy_data_and_lists_match_eager_packets() {
    for bell in [false, true] {
        for len in [0, 1, MAX_RAW_CHUNK_BYTES, MAX_RAW_CHUNK_BYTES + 1, 65_536] {
            let bytes: std::sync::Arc<[u8]> = vec![0xa5; len].into();
            let mut expected = Vec::new();
            data(&mut expected, 2, 7, &bytes, bell);
            let mut cursor = stream::Data::new(2, 7, bytes, bell);
            let mut actual = Vec::new();
            while cursor.append_next(&mut actual) {}
            assert_eq!(actual, expected);
            assert!(!cursor.append_next(&mut actual));
            assert_eq!(actual, expected);
        }
        for len in [0, 1, MAX_PAYLOAD_BYTES, MAX_PAYLOAD_BYTES + 1, 65_536] {
            let payload = vec![b'a'; len];
            let mut expected = Vec::new();
            encode(&mut expected, "t=m", 7, &payload, bell);
            let mut cursor = stream::List::new("t=m", 7, &payload, bell);
            let mut actual = Vec::new();
            while cursor.append_next(&mut actual) {}
            assert_eq!(actual, expected);
            assert!(!cursor.append_next(&mut actual));
            assert_eq!(actual, expected);
        }
    }
}
