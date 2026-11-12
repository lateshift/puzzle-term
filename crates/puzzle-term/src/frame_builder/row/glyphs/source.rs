//! Complete platform clusters can return Slug outlines, paint graphs or pixels.
//!
//! Cache hits are checked before touching font discovery or shaping again.
//! One registry key addresses either prepared vector ink or a size/DPR bitmap.
//! Every vector draw uses primary grid fitting; platform fonts cannot move cells.
//! The platform result has three storage possibilities. Outline paths enter
//! the existing Slug band/curve atlas, paint graphs enter the Slug/paint stores,
//! and device-sized samples enter the raster pages. A glyph key names one
//! result, so a warm lookup can choose the store without reopening its font.
//! Raster keys additionally contain the exact size and DPR; outline keys
//! carry D-27's ppem/mode suffix when extraction was grid-hinted.
//! Native matching records whether bold or italic had to be synthesized.
//! Bold duplicates an instance at the specified integer device offset.
//! Italic shears the instance by twelve degrees, leaving atlas curves intact.
//! A real italic face never receives the primary font's synthesis a second time.
//! Shared fitting changes only the instance scale/translation inside the
//! cluster's existing cell span. It cannot alter VT widths or cursor movement.
//! A color graph can expand one cluster into several consecutive vector draws.
//! The row's blink, selection and palette vectors must grow to the same count.
//! A bitmap instead updates the corresponding raster appearance vectors in
//! the raster emitter, so its early return must skip vector-only bookkeeping.
//! Cold bitmap preparation returns owned samples until the atlas copies them.
//! No pixel or path borrow survives the platform request or its mapped files.
//! If preparation fails, the partial row is not published as a successful frame.
//! Upload acknowledgment remains with the renderer; row rebuilding consumes
//! neither raster rectangles nor paint records before they reach the GPU.
//! Source replacement clears both prepared stores before registry IDs can
//! name a different font result. Device restoration keeps existing CPU data.
//! These different lifecycle actions share row invalidation but preserve
//! different ownership: replacement drops ink, restoration reuploads it.
//!
//! Atlas pressure can change a covered outline's owner without changing its
//! source identity. The CPU Slug oracle returns a fitted mask at the requested
//! device size. A complete color graph returns one ordered RGBA crop instead.
//! Both crops use the same raster-key dimensions as platform-prepared bitmaps.
//! A transform magnifies only the preparation copy; final geometry retains
//! the original cell placement and divides out that preparation scale once.
//! Synthetic bold/slant belongs inside an oracle crop, whereas vector draws
//! apply those operations to their instances. Applying both would double them.
//! No capacity branch may manufacture the unsupported-codepoint diagnostic.
//! That diagnostic is reached only by unresolved source requests or confirmed
//! lack of font coverage. Its component face never aliases finished source IDs.
//!
//! Source glyph identity survives eviction of coverage-request metadata.
//! Completed ink keeps its style synthesis flags with its original placement.
//! Already validated outlines transfer prepared curves and bands into the backend.
//! The unhinted validation shape is retained for later transformed or effect drawing.
//! D-27 hinted ink has a separate size-dependent key and is prepared once for drawing.
//! No side cache stores a duplicate geometry merely to answer coverage.
//! Bitmap validation checks structural coverage; decoding produces drawn samples once.
//! Raster keys include the exact device size, baseline, fitting span and DPR.
//! CPU-oracle overflow uses those same keys and the ordinary raster emitter.
//! An atlas-capacity rejection cannot turn covered ink into a permanent tofu key.
//! Provisional boxes describe pending work and are never cached as final source ink.
//! A ready source result can replace an unchanged row after its completion wake.
//! That wake invalidates row generations rather than changing the VT cell text.
//! Underlying cell advance stays fixed even when the ink backend changes.
//! Color graphs stay complete so layer ordering survives raster overflow.
//! Warm source masks skip extraction and oracle sampling on later redraws.
//! Final unsupported boxes remain separate from temporary pending diagnostics.
//! Source fitting is applied once, before the common row-relative emitter.
//! Atlas survivors never move; native raster insertion may recycle an unreferenced page.

/// Final and provisional diagnostic placement have distinct coverage semantics.
mod diagnostic;
/// Completed outline publication owns vector versus oracle-overflow selection.
mod vector;

use super::{CachedRow, FontSet, RunStyle, SlugBackend, append_color};
use puzzle_text::{
    cluster_source::ClusterGlyph,
    glyph::{GlyphPlacement, PreparedGlyph},
    raster::{RasterAtlas, RasterKey},
};

/// Dispatches cold ink to its store while preserving row appearance streams.
#[allow(
    clippy::too_many_arguments,
    reason = "source dispatch crosses separate font/vector/raster owners"
)]
/// Cache lookups precede all native discovery and browser Canvas operations.
/// Full text-fitting dimensions are part of raster identity independently of size.
/// A vector hit ignores raster magnification because its outline is reusable.
/// Complete color graphs validate paint and clip generations before reuse.
/// A bitmap early return avoids extending vector-only appearance arrays.
pub(super) fn append(
    fonts: &mut FontSet,
    backend: &mut SlugBackend,
    raster: &mut RasterAtlas,
    cache: &mut CachedRow,
    p: &GlyphPlacement,
    style: RunStyle,
    transformed: bool,
    dpr: f32,
    raster_scale: f32,
) -> Result<(), String> {
    // Ordinary grid glyphs use their existing placement directly. Creating an
    // effective copy is needed only for an actual effect magnification.
    let scaled;
    let sized = if raster_scale == 1.0 {
        p
    } else {
        scaled = effective(*p, raster_scale);
        &scaled
    };
    // Raster crops depend on fitting geometry; em-space vectors depend only
    // on the selected source key and any hinting suffix already in that key.
    let key = RasterKey {
        fit: [
            sized.ink_limit_px,
            sized.cell_height_px,
            sized.baseline_px.to_bits(),
        ],
        glyph: p.key,
        size: sized.size_px.to_bits(),
        dpr: dpr.to_bits(),
    };
    // A grid mask is preferred even if a prior transformed draw also prepared its vector.
    if let Some(handle) = raster.get_for_placement(
        key,
        p.effect || transformed || !backend.grid_masks_enabled(),
    ) {
        bitmap(cache, sized, handle, style, raster_scale);
        return Ok(());
    }
    // Full paint graphs validate clip and paint generations before a warm borrow.
    if let Some(color) = backend.source_color(p.key) {
        append_color(cache, p, color, style, transformed);
    } else if let Some(handle) = backend.get_for_placement(
        p,
        [
            true,
            backend.source_style(p.key).1,
            backend.source_style(p.key).0,
        ],
    ) {
        // A real matched fallback face controls its own style synthesis policy.
        vector::outline(cache, p, handle, style, backend.source_style(p.key));
    } else {
        // Only a cold source invokes platform APIs; all returned geometry is owned.
        if cold(
            fonts,
            backend,
            raster,
            cache,
            p,
            sized,
            style,
            transformed,
            key,
            raster_scale,
        )? {
            return Ok(());
        }
    }
    // Vector appearance arrays remain parallel after a cluster expands to many layers.
    cache.glyph_palette.resize(cache.glyphs.len(), false);
    cache.glyph_blink.resize(cache.glyphs.len(), style.blink);
    cache
        .glyph_selection_ink
        .resize(cache.glyphs.len(), style.selection_ink);
    Ok(())
}

/// Publishes cold owned ink into exactly one store before emitting its instances.
#[allow(
    clippy::too_many_arguments,
    reason = "cold source dispatch borrows independent glyph stores"
)]
/// The sized copy reaches platform selection before any bitmap is decoded or drawn.
/// Outline paths remain em-space geometry and use the original unhinted request.
/// Matched native style flags are retained separately from the primary family.
/// Owned samples transfer into the bounded atlas before the platform borrow ends.
/// Direct D-27 results retain unhinted validation before the hinted drawing is published.
fn cold(
    fonts: &mut FontSet,
    backend: &mut SlugBackend,
    raster: &mut RasterAtlas,
    cache: &mut CachedRow,
    p: &GlyphPlacement,
    sized: &GlyphPlacement,
    style: RunStyle,
    transformed: bool,
    key: RasterKey,
    raster_scale: f32,
) -> Result<bool, String> {
    let (ink, synthesis) = source_style(fonts.prepare_source(sized)?);
    let ink = validation_ink(ink, backend, raster, sized, key)?;
    match ink {
        ClusterGlyph::Pending(code) => {
            // Shared parts keep provisional ink bounded and separate from final source keys.
            diagnostic::pending(fonts, backend, raster, cache, (p, code, style, key.dpr))?;
        }
        ClusterGlyph::Missing(code) => {
            // A complete native negative draws final tofu, never a covered provisional box.
            diagnostic::missing(fonts, backend, raster, cache, (p, code, style, key.dpr))?;
        }
        ClusterGlyph::Bitmap(samples) => {
            // Source pixels are already fitted. Shelf allocation only assigns UVs.
            let handle = raster.prepare(key, samples)?;
            bitmap(cache, sized, handle, style, raster_scale);
            return Ok(true);
        }
        ink @ (ClusterGlyph::Outline(_) | ClusterGlyph::PreparedOutline { .. }) => {
            // Both variants publish once; only raw paths still need curve conversion.
            return vector::cold(
                (backend, raster, cache),
                ink,
                (p, sized, style, key, raster_scale, synthesis),
            );
        }
        ClusterGlyph::StyledOutline { .. } => return Err("source style was not normalized".into()),
        ClusterGlyph::ValidatedOutline { .. } => {
            return Err("source validation was not normalized".into());
        }
        ClusterGlyph::Color(color) => {
            // Keep all graph layers together on either backend. Partial publication
            // would alter source-over ordering and could hide a later glyph.
            if let Some(samples) = backend.prepare_source_color_ink(sized, color)? {
                let handle = raster.prepare(key, samples)?;
                bitmap(cache, sized, handle, style, raster_scale);
                return Ok(true);
            }
            let prepared = backend
                .source_color(p.key)
                .ok_or("prepared source graph absent")?;
            append_color(cache, p, prepared, style, transformed);
        }
    }
    Ok(false)
}

/// Separates source matching style from the ink kind before backend selection.
/// Native matching owns these flags; ordinary paths require no synthesis.
fn source_style(ink: ClusterGlyph) -> (ClusterGlyph, (bool, bool)) {
    match ink {
        ClusterGlyph::PreparedOutline { ink, bold, italic } => (
            ClusterGlyph::PreparedOutline { ink, bold, italic },
            (bold, italic),
        ),
        ClusterGlyph::StyledOutline { path, bold, italic } => {
            (ClusterGlyph::Outline(path), (bold, italic))
        }
        ClusterGlyph::ValidatedOutline {
            unhinted,
            hinted,
            bold,
            italic,
        } => (
            ClusterGlyph::ValidatedOutline {
                unhinted,
                hinted,
                bold,
                italic,
            },
            (bold, italic),
        ),
        ink => (ink, (false, false)),
    }
}

/// Saves D-27's reusable validation ink through the same vector/oracle stores as drawing.
fn validation_ink(
    ink: ClusterGlyph,
    backend: &mut SlugBackend,
    raster: &mut RasterAtlas,
    p: &GlyphPlacement,
    mut key: RasterKey,
) -> Result<ClusterGlyph, String> {
    let ClusterGlyph::ValidatedOutline {
        unhinted,
        hinted,
        bold,
        italic,
    } = ink
    else {
        return Ok(ink);
    };
    if backend.grid_cache_enabled() && !p.effect {
        // Structural validation already consumed this input; keep only the drawn hinted form.
        // A later effect asks the native source for its unhinted vector through the same key.
        return Ok(ClusterGlyph::PreparedOutline {
            ink: hinted,
            bold,
            italic,
        });
    }
    let mut validation = *p;
    validation.key.hint_ppem = 0;
    validation.key.hint_mode = puzzle_text::glyph::HintMode::None;
    key.glyph = validation.key;
    // A full vector store does not discard validation work: its exact-device
    // oracle crop becomes normal raster ink, available to a later unhinted draw.
    if let PreparedGlyph::Rasterized(samples) =
        backend.prepare_source_outline(&validation, unhinted, bold, italic)?
    {
        raster.prepare(key, samples)?;
    }
    Ok(ClusterGlyph::PreparedOutline {
        ink: hinted,
        bold,
        italic,
    })
}

/// Sizes bitmap preparation at the largest magnification of the effect's linear map.
/// All dimensions belong to this preparation copy, never to the neutral cell.
/// Size, baseline and fitting span grow together so rasterizers do not shrink zoom.
/// The resulting key separates these samples from ordinary-sized cached bitmaps.
fn effective(mut p: GlyphPlacement, scale: f32) -> GlyphPlacement {
    // Unit scale is an exact identity, including integer coordinates beyond
    // f32's exact range; it must not round-trip those fields through floats.
    if scale == 1.0 {
        return p;
    }
    // No field of the original neutral placement is modified. Only preparation sees this copy.
    p.size_px *= scale;
    p.baseline_px *= scale;
    p.origin_px = p.origin_px.map(|v| (v as f32 * scale).round() as i32);
    p.advance_px = (p.advance_px as f32 * scale).ceil() as u32;
    p.ink_limit_px = (p.ink_limit_px as f32 * scale).ceil() as u32;
    p.cell_height_px = (p.cell_height_px as f32 * scale).ceil() as u32;
    p
}

/// Returns sized samples to neutral device coordinates before applying the final affine map.
/// UVs keep the full effective-size crop while the destination returns to neutral units.
/// Later affine compilation applies the effect once, without double magnification.
/// Blink and selection arrays grow through the same ordinary raster emitter.
fn bitmap(
    cache: &mut CachedRow,
    p: &GlyphPlacement,
    handle: puzzle_text::raster::RasterHandle,
    style: RunStyle,
    scale: f32,
) {
    super::super::super::raster::append(cache, p, handle, style.blink, style.selection_ink);
    // The rasterizer drew at the effective size. Dividing its quad avoids applying zoom twice.
    if scale != 1.0
        && let Some(draw) = cache.rasters.last_mut()
    {
        for value in &mut draw.rect {
            *value /= scale;
        }
    }
}

/// Identity sizing and real magnification preserve the neutral source contract.
#[cfg(test)]
mod tests;
