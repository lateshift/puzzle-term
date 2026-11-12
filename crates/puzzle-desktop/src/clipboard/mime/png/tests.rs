//! Binary MIME conversion must preserve every RGBA byte and reject malformed PNGs.
#![allow(
    clippy::expect_used,
    reason = "Tests report invalid protocol fixtures directly"
)]
use super::*;
#[test]
fn clipboard_mime_png_roundtrip_preserves_rgba() {
    let pixels = vec![1, 2, 3, 255, 255, 0, 42, 80, 90, 80, 70, 0, 30, 40, 50, 255];
    let image = arboard::ImageData {
        width: 2,
        height: 2,
        bytes: Cow::Borrowed(&pixels),
    };
    let png = encode(image).expect("PNG encode");
    let restored = decode(&png).expect("PNG decode");
    assert_eq!((restored.width, restored.height), (2, 2));
    assert_eq!(restored.bytes.as_ref(), pixels);
    assert!(decode(b"not a PNG").is_err());
    assert!(
        encode(arboard::ImageData {
            width: MAX_BYTES,
            height: 2,
            bytes: Cow::Borrowed(&[])
        })
        .is_err()
    );
}
