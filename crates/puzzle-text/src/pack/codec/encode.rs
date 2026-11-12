//! Deterministic version-one font-pack writer.
//!
//! Header and directory values use explicit little-endian fields; raw
//! Slug rows retain their physical gaps and integer addresses.
use super::{push_f32, push_u16, push_u32, push_u64};
use crate::pack::{FontPack, MAGIC, VERSION};

/// Serializes a validated builder result in reproducible codepoint order.
pub(super) fn encode(pack: &FontPack) -> Vec<u8> {
    // This method receives a builder or validated decoder result; it
    // therefore writes exact fields rather than reinterpreting Rust structs.
    let mut out = Vec::new();
    // The fixed prefix lets a loader reject a TTF or a different pack
    // version before trusting any count in the following bytes.
    out.extend_from_slice(MAGIC);
    push_u16(&mut out, VERSION);
    push_u16(&mut out, pack.weight);
    push_u64(&mut out, pack.source_hash);
    push_u16(&mut out, pack.metrics.upem);
    // Reserved zero catches a future format extension being read by a
    // decoder that only understands version 1.
    push_u16(&mut out, 0);
    // Metrics are normalized to ems in the header so the first frame
    // can derive cells without opening an hhea or OS/2 font table.
    for metric in [
        pack.metrics.ascent,
        pack.metrics.descent,
        pack.metrics.leading,
        pack.metrics.cap_height,
        pack.metrics.zero_advance,
    ] {
        push_f32(&mut out, metric);
    }
    push_u32(&mut out, pack.glyphs.len() as u32);
    // Texture counts refer to physical packed texels, including row
    // padding created by C1/B1/B3 placement constraints.
    push_u32(&mut out, pack.curves.len() as u32);
    push_u32(&mut out, pack.bands.len() as u32);
    for (&code, glyph) in &pack.glyphs {
        // BTreeMap iteration emits increasing Unicode scalars, so two
        // machines with the same approved face produce identical bytes.
        push_u32(&mut out, code);
        push_u32(&mut out, glyph.glyph_id);
        push_f32(&mut out, glyph.advance_em);
        let (loc, band_info, bbox) = if let Some(handle) = glyph.handle {
            // Slug's shader expects low-column/high-row packing, not a
            // linear array offset; preserve the handle's bit layout.
            (
                puzzle_slug::encoding::encode_glyph_loc(handle.glyph_loc),
                handle.band_info,
                handle.bbox,
            )
        } else {
            // A space participates in cmap and advance lookup even
            // though it has no curve or band header to upload.
            (u32::MAX, 0, [0.0; 4])
        };
        push_u32(&mut out, loc);
        push_u32(&mut out, band_info);
        for value in bbox {
            // The bbox stays in em coordinates so one pack serves every
            // requested logical size until hinted outlines take over.
            push_f32(&mut out, value);
        }
    }
    // These words can be uploaded directly to Rgba32Float/Rg16Uint after
    // the decoder validates their bounds and byte order.
    for texel in &pack.curves {
        // Serializing each component avoids padding or alignment bytes
        // that `repr(Rust)` could introduce around an array or struct.
        for value in texel {
            push_f32(&mut out, *value);
        }
    }
    for texel in &pack.bands {
        // Band coordinates are exact integers; a lossy float conversion
        // would break the shader's header and curve-list addressing.
        for value in texel {
            push_u16(&mut out, *value);
        }
    }
    out
}
