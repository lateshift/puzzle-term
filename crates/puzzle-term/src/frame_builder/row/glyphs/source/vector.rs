//! Completed source outlines select vector storage or exact-device oracle overflow.
//!
//! Both routes keep the original cell origin and source identity. Synthetic
//! style is baked into oracle masks, while vector instances apply it once.
//! A boolean return distinguishes raster bookkeeping from vector appearance
//! streams, so the caller never extends the wrong row arrays.
//! The effective placement may be enlarged for an overlay transform. Raster
//! quads divide out that enlargement before final affine compilation, while
//! vectors reuse em-space geometry and the original placement.
//! Atlas pressure never becomes a missing-font result: the backend supplies
//! CPU-oracle coverage when the current frame cannot reserve another page.
//! Upload ownership remains with the frame executor after row publication.
//! GridOutline transfers geometry directly into the selected coverage owner.
//! Its returned shelf handle uses the same placement, blink and selected tint streams.
//! Effects never choose this result; their em-space vector keys remain reusable.
//! CPU-only hosts and native GPU hosts differ only in how coverage is produced.
use super::*;

/// Prepares one outline and publishes it without changing neutral placement.
/// `stores` lends atlas and row ownership; `draw` keeps geometry and style together.
/// Prepared input already owns its curves/bands; raw legacy paths convert here.
/// Raster overflow transfers its samples to the ordinary bounded atlas.
pub(super) fn cold(
    stores: (&mut SlugBackend, &mut RasterAtlas, &mut CachedRow),
    ink: ClusterGlyph,
    draw: (
        &GlyphPlacement,
        &GlyphPlacement,
        RunStyle,
        RasterKey,
        f32,
        (bool, bool),
    ),
) -> Result<bool, String> {
    // Independent store borrows end together when this complete publication returns.
    let (backend, raster, cache) = stores;
    let (p, sized, style, key, scale, synthesis) = draw;
    // Prepare using the effective device size; emit using the neutral grid geometry.
    let prepared = prepare_vector(backend, sized, ink, synthesis)?;
    publish(
        prepared, raster, cache, p, sized, style, key, scale, synthesis,
    )
}

/// Normalizes owned or legacy raw outline input before the shared publication branch.
fn prepare_vector(
    backend: &mut SlugBackend,
    p: &GlyphPlacement,
    ink: ClusterGlyph,
    synthesis: (bool, bool),
) -> Result<puzzle_text::glyph::PreparedGlyph, String> {
    match ink {
        // Legacy direct sources still provide a path instead of converted curves/bands.
        ClusterGlyph::Outline(path) => {
            backend.prepare_styled_path(p, path, synthesis.0, synthesis.1)
        }
        // Host-owned preparation is the drawn shape; no validation side cache is needed.
        ClusterGlyph::PreparedOutline { ink, .. } => {
            backend.prepare_source_outline(p, ink, synthesis.0, synthesis.1)
        }
        // Normalization must have removed styled/validated wrappers before this branch.
        _ => Err("source outline variant changed during dispatch".into()),
    }
}

/// Owned prepared ink chooses a store without changing neutral cell placement.
#[allow(
    clippy::too_many_arguments,
    reason = "publishing crosses independent ink and row owners"
)]
fn publish(
    prepared: PreparedGlyph,
    raster: &mut RasterAtlas,
    cache: &mut CachedRow,
    p: &GlyphPlacement,
    sized: &GlyphPlacement,
    style: RunStyle,
    key: RasterKey,
    scale: f32,
    synthesis: (bool, bool),
) -> Result<bool, String> {
    // Returning true tells the caller that raster bookkeeping is complete;
    // false lets it extend the vector appearance streams after outline emission.
    match prepared {
        PreparedGlyph::Vector(handle) => {
            // This address is stable for the frame; page use marking protects its ink.
            outline(cache, p, handle, style, synthesis);
            Ok(false)
        }
        PreparedGlyph::GridOutline {
            ink,
            placement,
            style: ink_style,
        } => {
            let handle = raster.prepare_grid(key, &placement, *ink, ink_style)?;
            bitmap(cache, sized, handle, style, scale);
            Ok(true)
        }
        PreparedGlyph::Rasterized(samples) => {
            // Device fitting and synthetic style are already baked into this mask.
            // Its neutral quad must not shear or duplicate the same coverage again.
            let handle = raster.prepare_outline_mask(key, samples)?;
            bitmap(cache, sized, handle, style, scale);
            Ok(true)
        }
        // Empty contextual components retain cells but never publish a quad.
        PreparedGlyph::Empty | PreparedGlyph::Pending => Ok(false),
        PreparedGlyph::Bitmap(_, _) => Err("prepared bitmap lacks raster page ownership".into()),
    }
}

/// Fallback outlines use the shared uniform fit without changing the grid origin.
pub(super) fn outline(
    cache: &mut CachedRow,
    p: &GlyphPlacement,
    handle: puzzle_slug::ShapeHandle,
    style: RunStyle,
    synthesis: (bool, bool),
) {
    // Native matching supplies real weight/slant; synthetic bold remains needed where absent.
    crate::glyph_ink::append_ink(&mut cache.glyphs, p, handle, synthesis.1, synthesis.0, true);
    // Synthetic bold may add a second instance with the same selection appearance.
    cache
        .glyph_selection_ink
        .resize(cache.glyphs.len(), style.selection_ink);
}
