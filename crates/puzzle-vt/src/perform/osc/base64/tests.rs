//! Malformed clipboard payloads must fail without a panic (design §16).

use super::decode;
use proptest::prelude::*;

/// Known valid and malformed encodings pin the strict padding rule.
#[test]
fn padding_and_utf8() {
    assert_eq!(decode(b"aGVsbG8="), Some("hello".into()));
    assert_eq!(decode(b"aGVsbG8"), None);
    assert_eq!(decode(b"AA=A"), None);
    assert_eq!(decode(b"////"), None);
}

proptest! {
    /// Arbitrary PTY bytes must not panic in the clipboard decoder.
    #[test]
    fn arbitrary_base64_bytes_are_safe(bytes in proptest::collection::vec(any::<u8>(), 0..4096)) {
        let _ = decode(&bytes);
    }
}
