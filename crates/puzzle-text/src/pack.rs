//! Prebuilt, unhinted Slug font packs for first-frame text (design §8.8).
//!
//! A pack stores a cmap, design advances, metrics, exact Slug handles and
//! contiguous ready-to-upload curve/band planes. It never stores hinted
//! outlines: D-27's low-DPR glyphs are prepared from the fetched font at
//! runtime per ppem, while this unhinted pack can show initial ASCII
//! before that full font download completes.
//!
//! The format has an explicit version and a stable FNV-1a source hash.
//! The hash detects an accidental pack/font mismatch; bounds checks are
//! still required for every untrusted byte because it is not a signature.
//! Parsing never trusts counts or offsets until it has checked remaining
//! input length. The loader can work without a font file at startup.
use crate::{FontData, face::FontFace, glyph::HintMode};
use puzzle_slug::{
    ShapeKey, SlugAtlas,
    atlas::{AtlasError, FillRule, ShapeHandle},
};
use std::collections::BTreeMap;

/// Binary encoding, alignment fallback and hostile-input validation.
mod codec;

/// Version of the bounded `.pzfp` on-disk structure.
pub const VERSION: u16 = 1;
/// Magic bytes distinguishing a pack from a font or arbitrary fixture.
pub const MAGIC: &[u8; 4] = b"PZFP";

/// Design-space metrics sufficient for an ASCII first frame without TTF.
#[derive(Clone, Copy, Debug)]
pub struct PackMetrics {
    /// Original face units per em, for format and source validation.
    pub upem: u16,
    /// Ascender normalized to em units.
    pub ascent: f32,
    /// Usually-negative descender normalized to em units.
    pub descent: f32,
    /// Recommended inter-row leading in em units.
    pub leading: f32,
    /// Capital height in em units if the font supplied it.
    pub cap_height: f32,
    /// Zero glyph's design advance in em units.
    pub zero_advance: f32,
}

/// One source scalar mapped to a glyph and possibly visible Slug contours.
#[derive(Clone, Copy, Debug)]
pub struct PackGlyph {
    /// Original OpenType glyph ID for takeover by the full font.
    pub glyph_id: u32,
    /// Design advance in ems; terminal layout still uses fixed cell metrics.
    pub advance_em: f32,
    /// None for spaces and other glyphs whose contours are intentionally empty.
    pub handle: Option<ShapeHandle>,
}

/// Validated CPU mirrors and directory that need no font file for upload.
#[derive(Clone)]
pub struct FontPack {
    /// Hash of the source bytes and selected weight instance.
    pub source_hash: u64,
    /// Variable-font weight used while its outlines were packed.
    pub weight: u16,
    /// Face metrics for first-frame cell geometry.
    pub metrics: PackMetrics,
    /// Codepoint directory, sorted by scalar for deterministic encoding.
    pub glyphs: BTreeMap<u32, PackGlyph>,
    /// Rgba32Float curve texels, including physical row gaps.
    pub curves: Vec<[f32; 4]>,
    /// Rg16Uint header and list texels, including physical row gaps.
    pub bands: Vec<[u16; 2]>,
}

/// Creates a pack from the approved Unicode subset and a loaded face.
impl FontPack {
    /// Builds unhinted outlines once while preserving Slug packing rules.
    pub fn build(data: FontData, weight: u16) -> Result<Self, String> {
        let source_hash = source_hash(&data, weight);
        let mut face = FontFace::new(data, weight)?;
        let raw = face.metrics()?;
        let upem = f32::from(raw.units_per_em);
        let zero = face.glyph_id('0')?.ok_or("pack face lacks digit zero")?;
        let metrics = PackMetrics {
            upem: raw.units_per_em,
            ascent: raw.ascent / upem,
            descent: raw.descent / upem,
            leading: raw.leading / upem,
            cap_height: raw.cap_height.unwrap_or(raw.ascent) / upem,
            zero_advance: face.advance_em(zero)?,
        };
        let mut atlas = SlugAtlas::default();
        let mut glyphs = BTreeMap::new();
        // The requested subsets are stable across all four style packs.
        // Missing scalars simply await the later full-font takeover.
        for code in covered_scalars() {
            let Some(ch) = char::from_u32(code) else {
                continue;
            };
            let Some(glyph_id) = face.glyph_id(ch)? else {
                continue;
            };
            let advance_em = face.advance_em(glyph_id)?;
            let path = face.outline(glyph_id, 0, HintMode::None)?;
            // Original em outlines are valid at every DPR. §8.8 keeps covered
            // glyphs on this pack path even after the full font takes over.
            let handle =
                match atlas.prepare(ShapeKey(u64::from(code) + 1), &path, FillRule::NonZero, 0) {
                    Ok(handle) => Some(handle),
                    // Spaces and zero-width format glyphs still need cmap
                    // entries so the grid can advance before font download.
                    Err(AtlasError::Shape(puzzle_slug::shape::ShapeError::Empty)) => None,
                    Err(error) => return Err(format!("pack U+{code:04X}: {error}")),
                };
            glyphs.insert(
                code,
                PackGlyph {
                    glyph_id,
                    advance_em,
                    handle,
                },
            );
        }
        Ok(Self {
            // Preserve physical row gaps from the packer: addresses in
            // glyph handles refer directly to these texture coordinates.
            source_hash,
            weight,
            metrics,
            glyphs,
            curves: atlas.curve_texels().to_vec(),
            bands: atlas.band_texels().to_vec(),
        })
    }

    /// Encodes a pack with little-endian fields and exact texel bytes.
    pub fn encode(&self) -> Vec<u8> {
        codec::encode(self)
    }

    /// Rejects malformed pack bytes with a report, never a panic.
    pub fn decode(bytes: &[u8]) -> Result<Self, String> {
        codec::decode(bytes)
    }

    /// Checks an optional full font against the pack's selected face.
    pub fn matches_source(&self, data: &FontData) -> bool {
        self.source_hash == source_hash(data, self.weight)
    }
}

/// Enumerates ASCII, Latin-1, punctuation and currency for early text.
fn covered_scalars() -> impl Iterator<Item = u32> {
    (0x20..=0x7e)
        .chain(0xa0..=0xff)
        .chain(0x2000..=0x206f)
        .chain(0x20a0..=0x20cf)
}

/// Computes a stable, noncryptographic source identity without a new crate.
fn source_hash(data: &FontData, weight: u16) -> u64 {
    let mut value = 0xcbf2_9ce4_8422_2325_u64;
    for byte in data
        .bytes()
        .iter()
        .copied()
        .chain(data.index().to_le_bytes())
        .chain(weight.to_le_bytes())
    {
        value = (value ^ u64::from(byte)).wrapping_mul(0x100_0000_01b3);
    }
    value
}

#[cfg(test)]
mod tests;

/// Evicted glyphs recover exact precomputed curves and bands, without font work.
mod ink;
