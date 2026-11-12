//! Untrusted font-pack parsing with bounded allocations and texel checks.
//!
//! This parser runs before a platform uploads any decoded GPU texture.
//!
//! A pack may come from a browser fetch or a native file and is treated
//! as untrusted input in both places. The header count fields are not
//! allowed to allocate memory until their byte total exactly matches the
//! supplied slice. This also rejects a valid prefix with malicious data
//! appended afterward, which otherwise could be hidden from validation.
//!
//! Directory validation checks Unicode scalars, duplicate names, finite
//! advances and bounding boxes, and band-header origins. It does not
//! replace the Slug packer's proof of C1/B1/B2; xtask makes packs from
//! that packer, and the loader verifies every address it will hand to
//! the GPU. An absent outline is represented explicitly by a sentinel
//! so a space can advance the first-frame grid without accessing a texel.
use super::{GLYPH_BYTES, MAX_BYTES, Reader};
use crate::pack::{FontPack, MAGIC, PackGlyph, PackMetrics, VERSION};
use puzzle_slug::atlas::{ShapeHandle, TEXTURE_WIDTH};
use std::collections::BTreeMap;

/// Parses a candidate pack without dereferencing unvalidated offsets.
pub(super) fn decode(bytes: &[u8]) -> Result<FontPack, String> {
    // Refuse oversized external input before even constructing counts;
    // the atlas's normal CPU+GPU memory allowance is 48 MiB.
    if bytes.len() > MAX_BYTES {
        return Err("font pack exceeds memory allowance".into());
    }
    let mut input = Reader { bytes, offset: 0 };
    // A mismatched magic or version means no later byte offset has a
    // defined interpretation under this decoder.
    if input.take(4)? != MAGIC {
        return Err("font pack magic is invalid".into());
    }
    if input.u16()? != VERSION {
        return Err("font pack version is unsupported".into());
    }
    let weight = input.u16()?;
    let source_hash = input.u64()?;
    // Weight participates in the source hash so a bold pack cannot be
    // mistaken for the regular variation of the same TTF byte stream.
    // Units per em cannot be zero: the eventual cell scale and advance
    // normalization would otherwise divide by zero.
    let upem = input.u16()?;
    if input.u16()? != 0 || upem == 0 {
        return Err("font pack header is invalid".into());
    }
    let metrics = read_metrics(&mut input, upem)?;
    // Validate the complete tail length before reading any directory
    // record, so corrupt counts cannot trigger a large partial allocation.
    let glyph_count = input.u32()? as usize;
    let curve_count = input.u32()? as usize;
    let band_count = input.u32()? as usize;
    // Each directory record has a fixed width; a count cannot change
    // its layout or smuggle variable-length data into a glyph field.
    let directory = glyph_count
        .checked_mul(GLYPH_BYTES)
        .ok_or("glyph count overflow")?;
    let curve_bytes = curve_count.checked_mul(16).ok_or("curve count overflow")?;
    // Rgba32Float is exactly sixteen bytes per texel, while Rg16Uint
    // consumes four. These are GPU upload formats, not file compression.
    let band_bytes = band_count.checked_mul(4).ok_or("band count overflow")?;
    let remaining = directory
        .checked_add(curve_bytes)
        .and_then(|n| n.checked_add(band_bytes))
        .ok_or("font pack length overflow")?;
    // Exact equality also rejects appended bytes from another file; a
    // loader never silently ignores a second unparsed payload.
    if bytes.len() - input.offset != remaining || glyph_count > 4096 {
        return Err("font pack counts disagree with byte length".into());
    }
    let glyphs = read_glyphs(&mut input, glyph_count, band_count)?;
    // The directory may refer only to band coordinates that fit the
    // declared plane; its validation precedes transfer into GPU textures.
    let curves = read_curves(input.take(curve_bytes)?)?;
    let bands = read_bands(input.take(band_bytes)?)?;
    // The returned planes contain physical row padding, so the handles
    // remain valid at their original packed band coordinates.
    Ok(FontPack {
        source_hash,
        weight,
        metrics,
        glyphs,
        curves,
        bands,
    })
}

/// Keeps non-finite font metrics out of first-frame cell arithmetic.
fn read_metrics(input: &mut Reader<'_>, upem: u16) -> Result<PackMetrics, String> {
    // A NaN baseline or cell width can escape ordinary range comparisons.
    // Check all five metrics before any row size is calculated from them.
    let metrics = PackMetrics {
        upem,
        ascent: input.f32()?,
        descent: input.f32()?,
        leading: input.f32()?,
        cap_height: input.f32()?,
        zero_advance: input.f32()?,
    };
    // Checking every field as a group also catches a malformed value in
    // an optional-looking metric such as capital height or leading.
    if [
        metrics.ascent,
        metrics.descent,
        metrics.leading,
        metrics.cap_height,
        metrics.zero_advance,
    ]
    .iter()
    .any(|value| !value.is_finite())
    {
        return Err("font pack metrics are non-finite".into());
    }
    Ok(metrics)
}

/// Validates each glyph location and finite bbox before making it visible.
fn read_glyphs(
    input: &mut Reader<'_>,
    count: usize,
    band_count: usize,
) -> Result<BTreeMap<u32, PackGlyph>, String> {
    let mut glyphs = BTreeMap::new();
    // Directory order is irrelevant for lookup after loading, but
    // duplicate scalars are rejected to keep source meaning unambiguous.
    for _ in 0..count {
        let (code, glyph) = read_glyph(input, band_count)?;
        if glyphs.insert(code, glyph).is_some() {
            // Duplicate scalars would make the apparent font coverage
            // depend on directory order, which the format forbids.
            return Err("pack cmap repeats a codepoint".into());
        }
    }
    Ok(glyphs)
}

/// Reads one cmap entry and checks the handle before directory insertion.
fn read_glyph(input: &mut Reader<'_>, band_count: usize) -> Result<(u32, PackGlyph), String> {
    // Unicode surrogate values and out-of-range integers are not
    // characters and cannot enter a public cmap lookup.
    let code = input.u32()?;
    if char::from_u32(code).is_none() {
        return Err("pack cmap has invalid scalar".into());
    }
    let glyph_id = input.u32()?;
    // The ID is retained for later matching to the fetched full font;
    // it is not used as an unchecked array index during pack upload.
    // Advances inform later font takeover; grid cells remain authoritative.
    let advance_em = input.f32()?;
    let loc = input.u32()?;
    let band_info = input.u32()?;
    let bbox = [input.f32()?, input.f32()?, input.f32()?, input.f32()?];
    // Non-finite control bounds could create an unbounded raster quad
    // before wgpu validation catches the malformed instance.
    if !advance_em.is_finite() || bbox.iter().any(|v| !v.is_finite()) {
        return Err("pack glyph metrics are non-finite".into());
    }
    let handle = if loc == u32::MAX {
        // The all-ones marker is reserved for a contourless glyph.
        // Space still has a design advance and a cmap entry.
        None
    } else {
        // B1 headers never wrap, so their first physical texel must exist.
        let [x, y] = puzzle_slug::encoding::decode_glyph_loc(loc);
        let index = usize::from(y) * TEXTURE_WIDTH + usize::from(x);
        // The shader reads the header at this exact 4096-wide location;
        // a plain row-bound check would miss an x value past used texels.
        if index >= band_count || bbox[0] >= bbox[2] || bbox[1] >= bbox[3] {
            return Err("pack glyph location or bbox is invalid".into());
        }
        Some(ShapeHandle {
            generation: 1,
            glyph_loc: [x, y],
            band_info,
            bbox,
        })
    };
    Ok((
        code,
        PackGlyph {
            glyph_id,
            advance_em,
            handle,
        },
    ))
}

/// Views aligned f32 texels through bytemuck, copying unaligned input.
fn read_curves(bytes: &[u8]) -> Result<Vec<[f32; 4]>, String> {
    // A successful bytemuck view avoids per-component conversion when
    // the caller's buffer happens to meet f32 alignment.
    if cfg!(target_endian = "little")
        && let Ok(words) = bytemuck::try_cast_slice::<u8, [f32; 4]>(bytes)
    {
        if words.iter().flatten().any(|value| !value.is_finite()) {
            return Err("pack curve is non-finite".into());
        }
        return Ok(words.to_vec());
    }
    // Browser fetch buffers and subslices need not be f32 aligned. This
    // copy makes identical texels without a hand-written unsafe cast.
    let mut curves = Vec::with_capacity(bytes.len() / 16);
    for chunk in bytes.chunks_exact(16) {
        // The validated tail length guarantees complete sixteen-byte
        // chunks, including any deliberately unused row-padding texels.
        let mut texel = [0.0; 4];
        for (slot, word) in texel.iter_mut().zip(chunk.chunks_exact(4)) {
            // Endian conversion is explicit in the fallback and works
            // even if the host CPU differs from the format's byte order.
            let raw: [u8; 4] = word.try_into().map_err(|_| "curve word is short")?;
            *slot = f32::from_le_bytes(raw);
            if !slot.is_finite() {
                return Err("pack curve is non-finite".into());
            }
        }
        curves.push(texel);
    }
    Ok(curves)
}

/// Views aligned band texels through bytemuck, copying unaligned input.
fn read_bands(bytes: &[u8]) -> Result<Vec<[u16; 2]>, String> {
    // Integer texels use two u16 fields; their values must remain exact
    // because they are indexes, not interpolated image data.
    if cfg!(target_endian = "little")
        && let Ok(words) = bytemuck::try_cast_slice::<u8, [u16; 2]>(bytes)
    {
        return Ok(words.to_vec());
    }
    let mut bands = Vec::with_capacity(bytes.len() / 4);
    // Every chunk has four bytes because the header-length check proved
    // the band plane's byte count is a multiple of one texel.
    for chunk in bytes.chunks_exact(4) {
        // Little-endian pairs preserve packed header offsets and curve
        // locations even when the input slice begins at an odd address.
        let x: [u8; 2] = chunk[..2].try_into().map_err(|_| "band word is short")?;
        let y: [u8; 2] = chunk[2..].try_into().map_err(|_| "band word is short")?;
        bands.push([u16::from_le_bytes(x), u16::from_le_bytes(y)]);
    }
    Ok(bands)
}
