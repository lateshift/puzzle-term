//! Valid foreign buffers and malformed ranges exercise the audited helpers under Miri.
#![allow(
    unsafe_code,
    clippy::unwrap_used,
    reason = "D-43: tests supply live stack buffers"
)]
use super::*;
#[test]
fn marshalling_rejects_null_overflow_and_invalid_utf8() {
    // SAFETY: empty null buffers are explicitly permitted without dereference.
    let empty = unsafe {
        bytes(Bytes {
            data: std::ptr::null(),
            len: 0,
        })
    };
    assert!(empty.unwrap().is_empty());
    // SAFETY: null nonempty input is rejected before any memory read.
    let null = unsafe {
        bytes(Bytes {
            data: std::ptr::null(),
            len: 1,
        })
    };
    assert!(null.is_err());
    // SAFETY: oversized ranges fail before accessing the one-byte allocation.
    let huge = unsafe {
        bytes(Bytes {
            data: b"a".as_ptr(),
            len: usize::MAX,
        })
    };
    assert!(huge.is_err());
    // SAFETY: initialized stack bytes remain readable through UTF-8 validation.
    assert!(unsafe { text(borrowed(&[0xff])) }.is_err());
    // SAFETY: the literal allocation is live and the returned text is an owned copy.
    let text = unsafe { text(borrowed("世界\0".as_bytes())) }.unwrap();
    assert_eq!(text, "世界\0");
    let mut output = 0_u64;
    // SAFETY: output is one live aligned writable u64, exclusively borrowed here.
    unsafe {
        write(&mut output, 42).unwrap();
    }
    assert_eq!(output, 42);
    // SAFETY: null destinations are rejected before writing any byte.
    assert!(unsafe { write(std::ptr::null_mut::<u64>(), 1) }.is_err());
}
