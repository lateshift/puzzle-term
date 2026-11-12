//! Decoder failures and the real system font are native platform contracts.
#![allow(
    clippy::expect_used,
    reason = "test-owned PNG encoder output is valid input"
)]
use super::*;

#[test]
/// CRC/truncation is rejected; RGB, gray and alpha samples become straight RGBA.
fn png_decoding_and_failures() {
    assert!(Decoder.decode(b"not a png").is_err());
    for (kind, bytes, expected) in [
        (
            ::png::ColorType::Rgb,
            vec![10, 20, 30],
            vec![10, 20, 30, 255],
        ),
        (
            ::png::ColorType::Grayscale,
            vec![127],
            vec![127, 127, 127, 255],
        ),
        (
            ::png::ColorType::GrayscaleAlpha,
            vec![127, 64],
            vec![127, 127, 127, 64],
        ),
        (
            ::png::ColorType::Rgba,
            vec![255, 0, 0, 128],
            vec![255, 0, 0, 128],
        ),
    ] {
        let mut encoded = Vec::new();
        let mut encoder = ::png::Encoder::new(&mut encoded, 1, 1);
        encoder.set_color(kind);
        encoder.set_depth(::png::BitDepth::Eight);
        encoder
            .write_header()
            .expect("PNG header")
            .write_image_data(&bytes)
            .expect("PNG pixels");
        assert_eq!(
            Decoder.decode(&encoded).expect("decoded PNG").pixels,
            expected
        );
        encoded.truncate(encoded.len() / 2);
        assert!(Decoder.decode(&encoded).is_err());
    }
}
