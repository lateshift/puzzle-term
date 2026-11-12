//! Dependency-free RGBA PNG writer with stored DEFLATE blocks (D-14).
//!
//! PNG's zlib stream is intentionally uncompressed: screenshots are small and
//! the format can be inspected without introducing another crate.
//! The writer emits one filter byte before each row, then wraps the scanlines
//! in stored DEFLATE blocks. Stored blocks cap their payload at 65,535 bytes,
//! which is why a screenshot can span several blocks without compression.
//! PNG chunks use CRC-32; the enclosing zlib stream uses Adler-32 instead.
use puzzle_render::RgbaImage;
use std::{fs, path::Path};
/// Writes a valid PNG containing the supplied tightly packed RGBA pixels.
/// The image is already top-down and RGBA8, so no color conversion occurs.
/// This preserves the exact bytes checked by the scenario's pixel assertions.
pub fn write(path: &Path, image: &RgbaImage) -> Result<(), String> {
    let mut raw = Vec::with_capacity((image.width as usize * 4 + 1) * image.height as usize);
    for row in image.pixels.chunks_exact(image.width as usize * 4) {
        // Every scanline starts with its filter selector before pixel bytes.
        raw.push(0); // Filter type 0 leaves bytes unchanged.
        raw.extend_from_slice(row);
    }
    let mut zlib = vec![0x78, 0x01]; // Fastest legal zlib header.
    for (index, block) in raw.chunks(65_535).enumerate() {
        // The final-block flag tells the DEFLATE decoder when to stop reading.
        let last = (index + 1) * 65_535 >= raw.len();
        zlib.push(u8::from(last)); // Stored DEFLATE block, final bit when appropriate.
        let len = block.len() as u16;
        zlib.extend_from_slice(&len.to_le_bytes());
        zlib.extend_from_slice(&(!len).to_le_bytes());
        zlib.extend_from_slice(block);
    }
    zlib.extend_from_slice(&adler32(&raw).to_be_bytes());
    let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&image.width.to_be_bytes());
    ihdr.extend_from_slice(&image.height.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]); // 8-bit RGBA, no interlace.
    chunk(&mut png, b"IHDR", &ihdr);
    chunk(&mut png, b"IDAT", &zlib);
    chunk(&mut png, b"IEND", &[]);
    fs::write(path, png).map_err(|error| error.to_string())
}
/// Appends one length-prefixed, CRC-protected PNG chunk.
/// The checksum covers the four-byte type and data, excluding the length.
fn chunk(png: &mut Vec<u8>, name: &[u8; 4], data: &[u8]) {
    png.extend_from_slice(&(data.len() as u32).to_be_bytes());
    png.extend_from_slice(name);
    png.extend_from_slice(data);
    let mut crc_input = Vec::with_capacity(4 + data.len());
    crc_input.extend_from_slice(name);
    crc_input.extend_from_slice(data);
    png.extend_from_slice(&crc32(&crc_input).to_be_bytes());
}
/// Calculates PNG's reflected IEEE CRC-32 over a chunk type and payload.
/// The reflected polynomial processes each input bit least significant first.
fn crc32(bytes: &[u8]) -> u32 {
    let mut value = !0_u32;
    for byte in bytes {
        value ^= u32::from(*byte);
        for _ in 0..8 {
            value = (value >> 1) ^ (if value & 1 == 1 { 0xedb8_8320 } else { 0 });
        }
    }
    !value
}
/// Calculates the Adler-32 checksum required at the end of a zlib stream.
/// Modulo 65,521 keeps both running sums inside the prescribed 16-bit fields.
fn adler32(bytes: &[u8]) -> u32 {
    let (mut a, mut b) = (1_u32, 0_u32);
    for byte in bytes {
        a = (a + u32::from(*byte)) % 65_521;
        b = (b + a) % 65_521;
    }
    (b << 16) | a
}
