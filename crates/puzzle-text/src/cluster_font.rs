//! Borrowed system-font bytes are parsed only by our aligned skrifa/harfrust.
//!
//! Fontique is a discovery service in the desktop shell, never a font parser here.
//! Borrowing its mapped bytes avoids copying a 192-MB emoji collection into heap.
//! Tables are opened lazily and no borrowed table survives a source preparation.
//! The returned outline, paint graph or bitmap is owned by the existing caches.
//! Shaping keeps selectors and joiners, then removes standalone invisible output.
//! All visible scalars must have coverage; a partial cluster cannot claim a face.
//! Grid advances remain with layout, independently of any font’s proportional size.
use crate::{
    cluster_source::{ClusterGlyph, ClusterRequest},
    glyph::GlyphPlacement,
};
use harfrust::{BufferFlags, Direction, ShapeOptions, ShaperData, ShaperInstance, UnicodeBuffer};
use skrifa::{FontRef, MetadataProvider};

/// Infers an ISO 15924 script from the complete grapheme using our shaper.
pub fn script(cluster: &str) -> [u8; 4] {
    // This cold scratch owns UTF-8 only while script properties are inferred.
    let mut buffer = UnicodeBuffer::new();
    buffer.push_str(cluster);
    // Script inference respects combining marks and selectors in their original cluster.
    buffer.guess_segment_properties();
    // Return four bytes rather than letting Harfrust tags cross a dependency seam.
    let tag = buffer.script().tag();
    tag.to_be_bytes()
}

/// Validates whole-cluster coverage through our parser, never fontique’s charmap.
pub fn covers(bytes: &[u8], index: u32, cluster: &str) -> bool {
    // The collection index is checked before any lazy table offset is followed.
    let Ok(font) = FontRef::from_index(bytes, index) else {
        return false;
    };
    // A missing .notdef mapping is a rejection, even if a font can draw a box.
    let cmap = font.charmap();
    // Default-ignorables remain inside shaping but need no standalone cmap entry.
    cluster
        .chars()
        .filter(|c| !crate::font_set::is_default_ignorable(*c))
        .all(|c| cmap.map(c).is_some_and(|g| g.to_u32() != 0))
}

/// Shapes and extracts owned ink from one candidate’s borrowed collection bytes.
/// A missing/invalid drawable returns an error so the desktop tries the next font.
/// Bitmap glyphs use the host decoder and shared fitting, while COLR stays vector.
/// The host passes the primary baseline measured for this text context.
pub fn prepare(
    bytes: &[u8],
    index: u32,
    request: ClusterRequest<'_>,
    placement: &GlyphPlacement,
    decoder: &mut dyn crate::bitmap::PngDecoder,
    baseline: f32,
    variations: &[([u8; 4], f32)],
) -> Result<ClusterGlyph, String> {
    // Reparse this borrowed candidate with the engine shared by our bundled fonts.
    let font = FontRef::from_index(bytes, index).map_err(|e| e.to_string())?;
    // Native discovery returns plain axis tags/values; no foreign parser types cross.
    let settings = settings(request.style, variations);
    // One variation location must govern shaping, outline extraction and color paints.
    // Unknown axes are ignored by the font engine; discovery may describe a fixed face.
    let location = font.axes().location(
        settings
            .iter()
            .map(|(tag, value)| (skrifa::Tag::new(tag), *value)),
    );
    // This standalone probe has no resident owner. Live hosts use loaded::Face,
    // which retains these parsing caches and borrows the existing file mapping.
    let data = ShaperData::new(&font);
    let instance = ShaperInstance::from_variations(
        &font,
        settings
            .iter()
            .map(|(tag, value)| (harfrust::Tag::new(tag), *value)),
    );
    let glyphs = shaped(&font, request.cluster, &data, &instance)?;
    // A joined emoji usually becomes one glyph; only that complete image is decoded.
    if glyphs.len() == 1 {
        // Decode only a single complete substitution, never one part of a ZWJ sequence.
        let gid = glyphs[0].0;
        if let Some(image) =
            crate::bitmap::load_bytes(bytes, index, gid, placement.size_px, decoder)?
        {
            return crate::bitmap::fitted(image, placement, baseline).map(ClusterGlyph::Bitmap);
        }
        // COLR is tried after embedded PNG strikes, retaining authored paint semantics.
        if let Some(color) = crate::color::borrowed_layers(&font, &location, gid, 0xffffffff)? {
            return Ok(ClusterGlyph::Color(color));
        }
    }
    // Text glyphs and their attached marks share a single baseline-relative path.
    outlines(&font, &location, &glyphs, placement).map(ClusterGlyph::Outline)
}

/// Harfrust returns glyph IDs and attachment offsets in the same em space as Slug.
fn shaped(
    font: &FontRef<'_>,
    text: &str,
    data: &ShaperData,
    instance: &ShaperInstance,
) -> Result<Vec<(u32, f32, f32)>, String> {
    // Building a temporary shaper borrows pre-parsed tables and variation metadata;
    // it does not repeat the expensive first-use face setup on a display frame.
    let shaper = data.shaper(font).instance(Some(instance)).build();
    // Attachment offsets are design units and must be normalized to primary em space.
    let upem = shaper.units_per_em() as f32;
    if upem <= 0.0 {
        return Err("system face has zero units per em".into());
    }
    // This cold scratch owns UTF-8 only while script properties are inferred.
    let mut buffer = UnicodeBuffer::new();
    buffer.push_str(text);
    // Joiners/selectors select substitutions before their own invisible glyphs are removed.
    buffer.set_flags(BufferFlags::REMOVE_DEFAULT_IGNORABLES);
    buffer.guess_segment_properties();
    // Terminal cell order is LTR; BiDi remains deferred by design §3.2.
    buffer.set_direction(Direction::LeftToRight);
    let output = shaper.shape(buffer, ShapeOptions::new());
    // Preserve attachment offsets but omit advances, which the grid already owns.
    let glyphs: Vec<_> = output
        .glyph_infos()
        .iter()
        .zip(output.glyph_positions())
        .map(|(g, p)| {
            (
                g.glyph_id,
                p.x_offset as f32 / upem,
                p.y_offset as f32 / upem,
            )
        })
        .collect();
    // .notdef is not drawable coverage, even when every input scalar had a cmap entry.
    if glyphs.is_empty() || glyphs.iter().any(|g| g.0 == 0) {
        return Err("system shaping has missing glyphs".into());
    }
    // Owned numeric output is independent of the transient font/table views.
    Ok(glyphs)
}

/// Owned outline extraction and D-27 normalization share the neutral engine.
mod outline;
use outline::outlines;

/// Converts last-resort cell-top geometry into baseline-relative primary ems.
/// This is used if an installed file disappears between coverage and drawing.
pub fn tofu(p: &GlyphPlacement, baseline: f32, code: u32) -> Result<ClusterGlyph, String> {
    if !p.size_px.is_finite() || p.size_px <= 0.0 {
        return Err("invalid source size".into());
    }
    let cell = crate::builtin::BuiltinCell::new(p.ink_limit_px, p.cell_height_px, 1)?;
    // Builtin y starts at the row top; font paths start at the primary baseline.
    let map = kurbo::Affine::translate((0.0, f64::from(baseline / p.size_px)))
        * kurbo::Affine::scale(f64::from(p.cell_height_px as f32 / p.size_px));
    Ok(ClusterGlyph::Outline(map * cell.tofu(code).path))
}

/// Applies native matching axes over the requested default weight, including slant.
fn settings(style: crate::font_set::FontStyle, native: &[([u8; 4], f32)]) -> Vec<([u8; 4], f32)> {
    // Native values can include ital/slnt/wdth; copying only these scalars preserves R9.
    // This vector is cold scalar metadata and is released before prepared glyph caching.
    let mut settings = native.to_vec();
    if !settings.iter().any(|(tag, _)| tag == b"wght") {
        // A weight axis absent from discovery still obeys terminal bold selection.
        let bold = matches!(
            style,
            crate::font_set::FontStyle::Bold | crate::font_set::FontStyle::BoldItalic
        );
        settings.push((*b"wght", if bold { 700.0 } else { 400.0 }));
    }
    settings
}

/// Enumerates a cmap once for a platform's background coverage index.
/// Glyph zero and invalid scalars never establish drawable coverage.
pub fn coverage_scalars(bytes: &[u8], index: u32) -> Result<Vec<u32>, String> {
    // The aligned parser enumerates its Unicode cmap directly, rather than testing every scalar.
    use skrifa::MetadataProvider;
    let font = skrifa::FontRef::from_index(bytes, index).map_err(|e| e.to_string())?;
    // Return scalar identities only; no borrowed mapping or foreign glyph handle escapes.
    Ok(font
        .charmap()
        .mappings()
        .filter_map(|(scalar, glyph)| {
            (glyph.to_u32() != 0 && char::from_u32(scalar).is_some()).then_some(scalar)
        })
        .collect())
}

/// Returns whether this face can carry any supported authored-color glyphs.
pub fn has_color_tables(bytes: &[u8], index: u32) -> bool {
    let Ok(font) = FontRef::from_index(bytes, index) else {
        return false;
    };
    // Table presence is only an eligibility hint. Whole-cluster shaping and decoding
    // still validate the actual glyph; monochrome-only faces cannot satisfy emoji policy.
    [b"COLR", b"sbix", b"CBDT"]
        .iter()
        .any(|tag| font.table_data(skrifa::Tag::new(tag)).is_some())
}

/// Resident parsing metadata borrows persistent mapped bytes on each draw.
pub mod loaded;
