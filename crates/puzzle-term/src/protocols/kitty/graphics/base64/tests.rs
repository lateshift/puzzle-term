//! Strict quartet and aggregate-budget regression cases.
#![allow(
    clippy::unwrap_used,
    reason = "tests assert exact Base64 validation outcomes"
)]
use super::*;

/// Invalid suffixes, illegal padding and budget failures must commit no prefix.
#[test]
fn invalid_chunk_does_not_mutate_transfer() {
    for source in [
        b"YWJj!!!!".as_slice(),
        b"Zh==",
        b"Zm9=",
        b"Zg=A",
        b"Zg==AAAA",
    ] {
        let mut out = vec![9];
        assert_eq!(
            append(source, false, 10, &mut out),
            Err(ImageError::Invalid)
        );
        assert_eq!(out, [9]);
    }
    let mut out = vec![9];
    assert_eq!(
        append(b"YWJj", false, 3, &mut out),
        Err(ImageError::NoSpace)
    );
    assert_eq!(out, [9]);
    append(b"YWJj", true, 10, &mut out).unwrap();
    append(b"Zg==", false, 10, &mut out).unwrap();
    assert_eq!(out, [9, b'a', b'b', b'c', b'f']);
}
