//! Wire control tests pin the supported subset and checked integer admission.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests assert protocol outcomes and fixture ownership"
)]
use super::*;

/// Bad pairs must fail before storage changes; valid signed z must retain its meaning.
#[test]
fn control_validation_and_scope_errors() {
    let (control, payload) = Control::parse(b"a=T,f=24,s=2,v=1,i=7,z=-2147483648;YWJj").unwrap();
    assert_eq!(control.z().unwrap(), i32::MIN);
    assert_eq!(control.format().unwrap(), Format::Rgb);
    assert_eq!(payload, b"YWJj");
    for wire in [
        b"i=1,I=2".as_slice(),
        b"i=1,i=2",
        b"s=-1",
        b"i=4294967296",
        b"q=3",
        b"U=2",
        b"m=2",
        b"i=+1",
        b"z=2147483648",
    ] {
        assert!(
            matches!(Control::parse(wire), Err(ImageError::Invalid)),
            "{wire:?}"
        );
    }
    for wire in [
        b"t=f".as_slice(),
        b"t=t",
        b"t=s",
        b"a=f",
        b"a=a",
        b"a=c",
        b"P=1",
        b"Q=1",
        b"H=0",
        b"V=0",
        b"o=x",
    ] {
        assert!(
            matches!(Control::parse(wire), Err(ImageError::Unsupported)),
            "{wire:?}"
        );
    }
}

proptest::proptest! {
    /// Arbitrary APC bodies never panic or escape the protocol chunk bound.
    #[test]
    fn arbitrary_controls_are_bounded(bytes in proptest::collection::vec(proptest::prelude::any::<u8>(), 0..6000)) {
        if let Ok((_, payload)) = Control::parse(&bytes) {
            proptest::prop_assert!(payload.len() <= crate::vt::apc::CHUNK_BYTES);
        }
    }
}
