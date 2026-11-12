//! Stored-DEFLATE PNG encoding for oracle coverage images (SPEC D-14).
//!
//! Oracle images are small, so a dependency-free uncompressed PNG avoids
//! adding a decoder or compressor to the workspace. CRC-32 protects chunks;
//! Adler-32 protects the zlib scanline stream.
//!
//! PNG wraps filtered scanlines in a zlib stream. Filter type zero leaves
//! each coverage byte untouched, which makes the generated evidence easier
//! to inspect with a byte-level tool. Stored DEFLATE blocks cap their
//! payload at 65535 bytes; splitting rows into such blocks avoids adding an
//! unapproved compression crate (D-14). The rasterized specimen may be
//! larger than a single block, but still fits a bounded 4096×4096 image.
//!
//! Every PNG chunk has a CRC over its four-byte name and payload. The zlib
//! stream ends in Adler-32 over the uncompressed scanlines. Readers reject
//! a bad checksum instead of showing a misleading review image.
use puzzle_slug::reference::OracleImage;
use std::{fs, path::Path};

/// Writes top-down RGBA8 oracle pixels to a valid PNG file.
/// The image's row stride is fixed at four bytes per pixel by the oracle.
pub(super) fn write(path: &Path, image: &OracleImage) -> Result<(), String> {
    let row_bytes = image.width as usize * 4;
    let mut raw = Vec::with_capacity((row_bytes + 1) * image.height as usize);
    for row in image.pixels.chunks_exact(row_bytes) {
        // Filter type zero leaves reference coverage bytes unchanged.
        // Its marker byte precedes each scanline inside the zlib payload.
        raw.push(0);
        raw.extend_from_slice(row);
    }
    let mut zlib = vec![0x78, 0x01];
    for (index, block) in raw.chunks(65_535).enumerate() {
        // Stored blocks cannot exceed 65535 bytes by the DEFLATE format.
        // The one-byte final-block flag tells zlib when to stop decoding;
        // LEN and one's-complement NLEN protect each stored block's length.
        zlib.push(u8::from((index + 1) * 65_535 >= raw.len()));
        let length = block.len() as u16;
        zlib.extend_from_slice(&length.to_le_bytes());
        zlib.extend_from_slice(&(!length).to_le_bytes());
        zlib.extend_from_slice(block);
    }
    zlib.extend_from_slice(&adler32(&raw).to_be_bytes());
    let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut header = Vec::with_capacity(13);
    header.extend_from_slice(&image.width.to_be_bytes());
    header.extend_from_slice(&image.height.to_be_bytes());
    header.extend_from_slice(&[8, 6, 0, 0, 0]);
    chunk(&mut png, b"IHDR", &header);
    chunk(&mut png, b"IDAT", &zlib);
    chunk(&mut png, b"IEND", &[]);
    fs::write(path, png).map_err(|error| error.to_string())
}

/// Appends one length-prefixed PNG chunk with its reflected CRC-32.
/// The checksum covers the name too, so a damaged chunk type is detectable.
fn chunk(output: &mut Vec<u8>, name: &[u8; 4], payload: &[u8]) {
    output.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    output.extend_from_slice(name);
    output.extend_from_slice(payload);
    let mut check = Vec::with_capacity(name.len() + payload.len());
    check.extend_from_slice(name);
    check.extend_from_slice(payload);
    output.extend_from_slice(&crc32(&check).to_be_bytes());
}

/// Uses PNG's standard reflected IEEE polynomial, bit by bit.
/// This compact implementation runs only when a diagnostic image is saved.
fn crc32(bytes: &[u8]) -> u32 {
    let mut value = !0_u32;
    for byte in bytes {
        value ^= u32::from(*byte);
        for _ in 0..8 {
            value = (value >> 1) ^ if value & 1 == 1 { 0xedb8_8320 } else { 0 };
        }
    }
    !value
}

/// Keeps zlib's two modulo-65521 sums in their prescribed order.
/// The sums cover the original filtered bytes, not the stored block headers.
fn adler32(bytes: &[u8]) -> u32 {
    let (mut a, mut b) = (1_u32, 0_u32);
    for byte in bytes {
        a = (a + u32::from(*byte)) % 65_521;
        b = (b + a) % 65_521;
    }
    (b << 16) | a
}
