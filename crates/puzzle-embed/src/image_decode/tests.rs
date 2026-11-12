//! Native decoder tests exercise real raw/zlib/PNG bytes, never a filesystem medium.
#![allow(
    clippy::unwrap_used,
    reason = "tests assert fixture construction and exact worker results"
)]
use super::*;
use std::io::Write;

/// A tiny request retains a larger allowance so decoder metadata has room too.
fn request(format: Format, size: [u32; 2], payload: Vec<u8>) -> DecodeRequest {
    DecodeRequest {
        ticket: 7,
        format,
        size,
        compressed: false,
        uncompressed_size: None,
        payload,
        limit: 4096,
    }
}

/// RGB alpha expansion, RGBA preservation and zlib validation share the same output.
#[test]
fn raw_and_zlib_decode_exact_pixels_and_reject_bombs() {
    let rgb = vec![1, 2, 3, 4, 5, 6];
    let rgba = vec![1, 2, 3, 255, 4, 5, 6, 255];
    assert_eq!(
        &*decode(request(Format::Rgb, [2, 1], rgb.clone()))
            .image
            .unwrap()
            .rgba,
        rgba
    );
    assert_eq!(
        &*decode(request(Format::Rgba, [2, 1], rgba.clone()))
            .image
            .unwrap()
            .rgba,
        rgba
    );
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(&rgb).unwrap();
    let encoded = encoder.finish().unwrap();
    let mut job = request(Format::Rgb, [2, 1], encoded.clone());
    job.compressed = true;
    assert_eq!(&*decode(job).image.unwrap().rgba, rgba);
    let mut job = request(Format::Rgb, [1, 1], encoded);
    job.compressed = true;
    assert!(decode(job).image.is_err());
    assert!(
        decode(request(Format::Rgba, [u32::MAX, u32::MAX], vec![]))
            .image
            .is_err()
    );
    assert!(
        decode(request(Format::Rgb, [2, 1], vec![1, 2, 3]))
            .image
            .is_err()
    );
}

/// PNG decoding preserves authored alpha and rejects oversized headers before pixel allocation.
#[test]
fn png_pixels_and_compression_obey_decoded_allowance() {
    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bytes, 2, 1);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder
            .write_header()
            .unwrap()
            .write_image_data(&[255, 0, 0, 128, 0, 255, 0, 0])
            .unwrap();
    }
    let decoded = decode(request(Format::Png, [0, 0], bytes.clone()))
        .image
        .unwrap();
    assert_eq!(decoded.size, [2, 1]);
    assert_eq!(&*decoded.rgba, [255, 0, 0, 128, 0, 255, 0, 0]);
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(&bytes).unwrap();
    let mut job = request(Format::Png, [0, 0], encoder.finish().unwrap());
    job.compressed = true;
    job.uncompressed_size = Some(bytes.len());
    assert_eq!(&*decode(job).image.unwrap().rgba, &*decoded.rgba);
    let mut job = request(Format::Png, [0, 0], bytes);
    job.limit = 4;
    assert!(decode(job).image.is_err());
}
