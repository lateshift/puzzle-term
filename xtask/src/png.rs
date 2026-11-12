//! Minimal reader for the stored-deflate PNGs written by the native harness.
//!
//! D-14 deliberately avoids a PNG dependency. Golden references use only
//! RGBA8, filter zero and stored DEFLATE blocks, so this reader accepts that
//! precise subset and rejects every unsupported encoding with a report.
//! Bounds checks on each chunk and block keep a corrupt reference from
//! panicking the acceptance command (R8). Decoded rows discard their PNG
//! filter bytes before the per-channel golden tolerance is evaluated.
//!
//! A PNG begins with an eight-byte signature. It then contains length,
//! type, data and CRC fields for each chunk. IHDR gives dimensions and
//! pixel layout; IDAT contains one zlib stream; IEND ends the file. The
//! native writer uses one IDAT chunk today, but concatenating IDAT payloads
//! here also respects the PNG rule for split image data.
//!
//! The zlib stream begins with 0x78, 0x01, then stored DEFLATE blocks.
//! Each stored block starts on a byte boundary with a final flag, a 16-bit
//! length and its one's complement. The complement detects a broken block
//! header before any payload is copied. The last four bytes are Adler-32
//! over the uncompressed scanlines; unlike a PNG chunk CRC, this checksum
//! covers the decompressed content. The reader verifies it.
//!
//! Every scanline starts with PNG filter byte zero. This means pixel bytes
//! are already literal RGBA values, with no previous-row or left-pixel
//! reconstruction. The reader discards the one filter byte per row, then
//! returns the same tight byte order that GPU readback and the oracle use.
//! It rejects palette colors, interlacing and compressed DEFLATE modes:
//! accepting them without decoding would make golden comparisons invalid.
//!
//! The target dimensions are bounded at 8192 in each direction, matching
//! the harness target limit. Width times four and row count therefore stay
//! within usize on supported native hosts. Checked chunk offsets keep even
//! malformed lengths from creating an out-of-bounds slice or a panic.
use std::{fs, path::Path};

/// Tightly packed RGBA8 pixels plus dimensions for golden comparison.
pub(crate) struct Image {
    /// Device-pixel width recorded in IHDR.
    pub(crate) width: u32,
    /// Device-pixel height recorded in IHDR.
    pub(crate) height: u32,
    /// Four color bytes per pixel in top-down order.
    pub(crate) pixels: Vec<u8>,
}

/// Reads the harness writer's PNG subset, reporting malformed references.
pub(crate) fn read(path: &Path) -> Result<Image, String> {
    let bytes = fs::read(path).map_err(|error| format!("{}: {error}", path.display()))?;
    if !bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Err(format!("{}: invalid PNG signature", path.display()));
    }
    let mut offset = 8;
    // IHDR must precede IDAT in a valid PNG, but storing both separately
    // gives a clear error if either required part is absent or malformed.
    let mut size = None;
    let mut compressed = Vec::new();
    while offset < bytes.len() {
        // PNG chunks have a big-endian length followed by four type bytes,
        // their payload and a CRC. Every field is checked before slicing.
        let length = word(&bytes, offset)? as usize;
        let kind = bytes
            .get(offset + 4..offset + 8)
            .ok_or("truncated PNG type")?;
        let payload_start = offset.checked_add(8).ok_or("PNG offset overflow")?;
        let payload_end = payload_start
            .checked_add(length)
            .ok_or("PNG length overflow")?;
        let payload = bytes
            .get(payload_start..payload_end)
            .ok_or("truncated PNG chunk")?;
        let next = payload_end
            .checked_add(4)
            .ok_or("PNG CRC offset overflow")?;
        if next > bytes.len() {
            return Err("truncated PNG CRC".into());
        }
        match kind {
            b"IHDR" => size = Some(header(payload)?),
            b"IDAT" => compressed.extend_from_slice(payload),
            b"IEND" => break,
            _ => return Err("unsupported PNG chunk".into()),
        }
        offset = next;
    }
    let (width, height) = size.ok_or("PNG lacks IHDR")?;
    let rows = inflate_stored(&compressed)?;
    let pixels = rows_to_pixels(&rows, width, height)?;
    Ok(Image {
        width,
        height,
        pixels,
    })
}

/// Reads a checked big-endian 32-bit PNG field.
fn word(bytes: &[u8], offset: usize) -> Result<u32, String> {
    let field: [u8; 4] = bytes
        .get(offset..offset.saturating_add(4))
        .ok_or("truncated PNG word")?
        .try_into()
        .map_err(|_| "invalid PNG word")?;
    Ok(u32::from_be_bytes(field))
}

/// Accepts only the exact RGBA8 noninterlaced format written by D-14.
fn header(payload: &[u8]) -> Result<(u32, u32), String> {
    if payload.len() != 13 || payload[8..] != [8, 6, 0, 0, 0] {
        return Err("unsupported PNG header".into());
    }
    let width = word(payload, 0)?;
    let height = word(payload, 4)?;
    if width == 0 || height == 0 || width > 8192 || height > 8192 {
        return Err("invalid PNG dimensions".into());
    }
    Ok((width, height))
}

/// Expands the writer's byte-aligned stored DEFLATE blocks.
fn inflate_stored(bytes: &[u8]) -> Result<Vec<u8>, String> {
    if bytes.get(..2) != Some(&[0x78, 0x01]) {
        return Err("unsupported zlib header".into());
    }
    let mut offset = 2;
    let mut output = Vec::new();
    loop {
        // Stored blocks are the only DEFLATE mode emitted by our writer;
        // rejecting other flag bits prevents treating Huffman data as bytes.
        let flag = *bytes.get(offset).ok_or("truncated DEFLATE block")?;
        if flag & !1 != 0 {
            return Err("unsupported compressed DEFLATE block".into());
        }
        offset += 1;
        let len = u16::from_le_bytes([
            *bytes.get(offset).ok_or("truncated DEFLATE length")?,
            *bytes.get(offset + 1).ok_or("truncated DEFLATE length")?,
        ]);
        let nlen = u16::from_le_bytes([
            *bytes.get(offset + 2).ok_or("truncated DEFLATE length")?,
            *bytes.get(offset + 3).ok_or("truncated DEFLATE length")?,
        ]);
        if len != !nlen {
            return Err("invalid stored DEFLATE length".into());
        }
        offset += 4;
        let end = offset
            .checked_add(len as usize)
            .ok_or("DEFLATE length overflow")?;
        output.extend_from_slice(bytes.get(offset..end).ok_or("truncated DEFLATE data")?);
        offset = end;
        if flag == 1 {
            break;
        }
    }
    // The writer adds Adler-32 after the final block; checking it catches
    // accidental reference corruption without accepting other zlib modes.
    let checksum = word(bytes, offset)?;
    if checksum != adler32(&output) {
        return Err("PNG zlib checksum differs".into());
    }
    Ok(output)
}

/// Removes the per-row filter selector from tightly packed RGBA data.
fn rows_to_pixels(rows: &[u8], width: u32, height: u32) -> Result<Vec<u8>, String> {
    let stride = width as usize * 4;
    if rows.len() != (stride + 1) * height as usize {
        return Err("PNG scanline length differs from IHDR".into());
    }
    let mut pixels = Vec::with_capacity(stride * height as usize);
    // A nonzero filter would require a predictor. Treating its data as
    // literal color bytes could hide a reference corruption or change.
    for row in rows.chunks_exact(stride + 1) {
        if row[0] != 0 {
            return Err("unsupported PNG row filter".into());
        }
        pixels.extend_from_slice(&row[1..]);
    }
    Ok(pixels)
}

/// Recomputes the zlib checksum over unfiltered scanline bytes.
fn adler32(bytes: &[u8]) -> u32 {
    let (mut a, mut b) = (1_u32, 0_u32);
    for byte in bytes {
        a = (a + u32::from(*byte)) % 65_521;
        b = (b + a) % 65_521;
    }
    (b << 16) | a
}
