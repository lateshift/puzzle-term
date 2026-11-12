//! Startup cmap placement before Harfrust's full font arrives (§8.8).
//!
//! The VT supplies the exact column of each scalar or cluster. The pack
//! selects only a visible base glyph, so unsupported combining text waits
//! for full-font shaping without moving later cells. Origins and ink use
//! the same neutral placement and Slug adapter as the full-font path.
//!
//! Pack placement deliberately does not substitute ligatures: the compact
//! startup directory contains glyph addresses, not OpenType shaping tables.
//! Full-font takeover invalidates these rows once those tables are available.
//! Until then, a missing cluster leaves a blank cell rather than a guessed advance.
//! Pack glyphs retain their unhinted placements for later provider ink effects.
//! These requests refer to already imported shapes and require no font parser.
//! A normal row rebuild replaces them together with the original ink arrays.
//! Style lookup borrows the precomputed weight and slant rather than synthesizing
//! a second bold copy or shear on top of an already styled pack outline.
use super::*;
use crate::glyph_ink::append_ink;
use puzzle_text::glyph::{GlyphKey, GlyphPlacement, HintMode};

/// Places a pack glyph at the cell's authoritative device-pixel origin.
pub(super) fn visit(
    shape: &mut RowShape<'_>,
    column: usize,
    style: RunStyle,
    text: CellText<'_>,
) -> Result<(), String> {
    if style.hidden {
        return Ok(());
    }
    // The terminal has already segmented clusters and assigned columns.
    // Selecting the first scalar here is only a startup cmap lookup;
    // it never recomputes widths or allocates a second grapheme buffer.
    let ch = match text {
        CellText::Scalar(ch) => ch,
        CellText::Cluster(text) => match text.chars().next() {
            Some(ch) => ch,
            None => return Ok(()),
        },
        CellText::Empty => return Ok(()),
    };
    shape.pack.ok_or("startup font pack missing")?;
    // Unsupported scalars await the asynchronously loaded fallback faces.
    // Their cells remain occupied in VT; following ASCII cannot slide left.
    let face = style.face as u32;
    let (face, glyph) = match shape.backend.packed_glyph(face, ch) {
        Some(glyph) => (face, Some(glyph)),
        None => (0, shape.backend.packed_glyph(0, ch)),
    };
    let Some(glyph) = glyph else {
        return Ok(());
    };
    // An advance-only entry (space) is intentionally contourless.
    // Passing it to Slug would address no band header and corrupt ink.
    if glyph.handle.is_none() {
        return Ok(());
    }
    // The style face selects its exact precomputed weight/slant; instance
    // matches the face slot for that default variation. Size stays a transform, while
    // hint_ppem zero identifies reusable em-space outlines (§8.8).
    // Row-relative y keeps scrolling independent of glyph preparation.
    let placement = GlyphPlacement {
        // Preserve the VT column independently from the pack glyph's ink bounds.
        cell_column: column as u32,
        key: GlyphKey {
            face,
            glyph_id: glyph.glyph_id,
            instance: face,
            hint_ppem: 0,
            hint_mode: HintMode::None,
        },
        origin_px: [
            column_x(column, shape.cell_width)?,
            shape.metrics.baseline_px,
        ],
        baseline_px: shape.metrics.baseline_px as f32,
        size_px: shape.metrics.device_size,
        advance_px: shape.cell_width,
        ink_limit_px: shape.cell_width,
        cell_height_px: shape.metrics.height_px,
        cell_span: 1,
        color: colors::rgba(style.ink),
        effect: false,
        color_glyph: false,
    };
    // The directory points into the backend's imported CPU mirror. No
    // outline extraction or alternative glyph drawing path is involved.
    let handle = shape
        .backend
        .get(placement.key)
        .ok_or("startup pack was evicted")?;
    // Placement and row metadata use the same helper for every startup glyph.
    append_pack_ink(shape, column, style, &placement, handle);
    Ok(())
}

/// Attaches cached colors, blink state and cursor ink to the emitted pack instance.
fn append_pack_ink(
    shape: &mut RowShape<'_>,
    column: usize,
    style: RunStyle,
    placement: &GlyphPlacement,
    handle: puzzle_slug::atlas::ShapeHandle,
) {
    // Cursor ink and blinking are attached after the backend emits its
    // instance, exactly as on the full-font path. A block cursor can then
    // redraw this same geometry without a second lookup or outline.
    // Pack glyphs already carry unhinted keys and remain valid effect sources.
    shape
        .cache
        .sources
        .push(crate::frame_builder::overlays::effects::Source {
            placement: *placement,
            style,
            italic: false,
            empty: false,
        });
    let first = shape.cache.glyphs.len();
    append_ink(
        &mut shape.cache.glyphs,
        placement,
        handle,
        false,
        false,
        false,
    );
    super::glyphs::geometry::retain(shape.cache, placement, first, shape.cache.rasters.len());
    shape
        .cache
        .glyph_palette
        .resize(shape.cache.glyphs.len(), false);
    shape
        .cache
        .glyph_blink
        .resize(shape.cache.glyphs.len(), style.blink);
    // Selected colors use the same source policy as ordinary glyphs, but
    // apply contrast against selected paper after the optional foreground token.
    // Retain one color per emitted instance; synthetic paths can emit duplicates.
    // Assembly uses this stream without mutating the imported pack placement.
    shape
        .cache
        .glyph_selection_ink
        .resize(shape.cache.glyphs.len(), style.selection_ink);
    retain_cursor_ink(shape.cache, first, shape.cursor_column == Some(column));
}
