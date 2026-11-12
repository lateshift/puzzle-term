//! Cached grid-ink variants leave cells, shaping ownership and query geometry neutral.
//!
//! Callbacks run against the ordinary frame first. A changed effect prepares its
//! affected row from retained placements, then swaps that variant into assembly
//! temporarily. The ordinary cache survives removal and never accumulates maps.
//! Sources are original placement requests, never transformed instance snapshots.
//! A COLR source may expand to several layers, which are rebuilt as one request.
//! A wide cluster or ligature is selected whole when any of its cells is affected.
//! Providers see ordinary geometry before their own ink transforms are compiled.
//! This prevents a rotated glyph from moving an anchor or changing hit-test cells.
//! Variants retain normal cell backgrounds and underline geometry from their row.
//! Only glyph and bitmap streams change; selection still partitions cell rectangles.
//! Effects compose in retained provider order rather than in cache insertion order.
//! The equality key contains complete descriptors, avoiding visual hash collisions.
//! Row version guards protect against rewritten cells reusing stale ink variants.
//! Style epochs capture resolved terminal colors and synthetic face changes.
//! Context generations invalidate font or platform source identities after takeover.
//! The independent Slug generation catches page-address invalidation.
//! The cache has one slot per visible row and releases entries after a shrink.
//! It never snapshots or reads a retained off-screen row through a provider.
//! Warm invalidations reuse variant arrays and do not re-extract font outlines.
//! An effect touching a lower OSC 66 fragment selects the root ink stream.
//! Its source map composes after sizing; continuations never create duplicate ink.
//! Active indices identify only rows that need replacement for this assembly.
//! Swapping transfers ownership without cloning row vectors each frame.
//! The second swap restores every ordinary row for later effect removal.
//! Pending texture writes survive instance clearing because upload ownership is separate.
//! The ordinary cursor code runs while effect variants own the affected row slots.
//! The caret still covers its original cells even when selected glyph ink moves.
//! Tracked ranges are sampled before callbacks; raw ranges remain literal cell queries.
//! Hidden anchors contribute no effect and Gone anchors cannot revive stale ink.
//! Malformed transforms are rejected before sized bitmap preparation can allocate.
use super::super::{CachedRow, FrameBuilder, context::TextContext};
use crate::frame_snapshot::FrameSnapshot;
use puzzle_frame::RenderFrame;
use puzzle_overlay::{
    anchor::{Anchor, Resolved},
    draw::{Effect, Primitive},
    geometry::Range,
};
use puzzle_text::glyph::GlyphPlacement;
/// Per-placement appearance is retained before a backend expands layers or bold.
#[derive(Clone, Copy)]
pub(in crate::frame_builder) struct Source {
    /// Original grid origin, cluster span and keyed outline request.
    pub placement: GlyphPlacement,
    /// Resolved grid appearance includes selection contrast and blink.
    pub style: super::super::row::RunStyle,
    /// Synthetic italic is independent of provider transforms.
    pub italic: bool,
    /// The backend confirmed a contourless shaping component; no ink is expected.
    pub empty: bool,
}
/// Frozen cell ranges are sampled under the same Session borrow as other anchors.
#[derive(Clone, Copy, PartialEq)]
pub(super) struct Request {
    /// Inclusive interval; source glyphs touching either endpoint are affected whole.
    pub range: Range,
    /// Ink-only affine and appearance overrides.
    pub effect: Effect,
}
/// One reusable variant per visible row; no whole-buffer cache exists.
#[derive(Default)]
struct Entry {
    /// Line/version/style/context/Slug incarnation guards every retained address.
    key: Option<[u64; 6]>,
    /// Complete equality avoids hash collisions changing visual effects.
    requests: Vec<Request>,
    /// Ordinary paper and decorations accompany re-prepared glyph ink.
    row: CachedRow,
}
/// Scratch capacity survives repeated animation and scrollbar-only frames.
#[derive(Default)]
pub(super) struct Cache {
    /// Variants are bounded by visible row count, never history length.
    rows: Vec<Entry>,
    /// Provider order determines composition of overlapping effects.
    requests: Vec<Request>,
    /// Swapped row indices restore ordinary ownership after assembly.
    active: Vec<usize>,
}
/// Re-preparation and color/affine application are separate from frame ownership.
mod compile;
/// Reassembles grid ink only when at least one retained effect touches visible text.
pub(super) fn apply(
    builder: &mut FrameBuilder,
    context: &mut TextContext,
    snapshot: &FrameSnapshot,
    frame: &mut RenderFrame,
    width: u32,
    height: u32,
) -> Result<(), String> {
    collect(builder);
    // Provider descriptors were frozen with this snapshot; no live terminal reads follow.
    let cache = &mut builder.overlays.effects;
    if cache.requests.is_empty() {
        return Ok(());
    }
    // Shrinking the viewport releases excess variants; growing is a cold allocation.
    cache.rows.resize_with(builder.visible_rows, Entry::default);
    cache.rows.truncate(builder.visible_rows);
    cache.active.clear();
    for index in 0..builder.visible_rows {
        let source = &builder.rows[index];
        let Some(line) = source.id else {
            continue;
        };
        if !cache.requests.iter().any(|r| {
            compile::touches(r.range, line, 0, snapshot.columns())
                || source.block_transforms.iter().any(|block| {
                    block.lines.iter().flatten().any(|line| {
                        compile::touches(r.range, *line, block.cells[0], block.cells[1])
                    })
                })
        }) {
            continue;
        }
        let key = [
            // Line identity and version distinguish scroll reuse from content rewriting.
            line.0,
            source.version,
            source.block_signature,
            source.style_epoch,
            context.atlas_generation(),
            context.backend.atlas().generation(),
        ];
        let entry = &mut cache.rows[index];
        let changed = entry.key != Some(key) || entry.requests != cache.requests;
        if changed {
            // A cold change replaces ink, while warm copies retain every array capacity.
            compile::rebuild(
                context,
                source,
                &mut entry.row,
                &cache.requests,
                builder.dpr,
                builder.metrics.width_px,
            )?;
            entry.requests.clone_from(&cache.requests);
            entry.key = Some(key);
        }
        // Retained effect variants are drawn instead of ordinary ink at these cells.
        context.backend.use_row_batch(
            &mut entry.row.shape_batch,
            entry
                .row
                .glyphs
                .iter()
                .map(|draw| (draw.glyph_loc, draw.paint)),
            changed,
        );
        cache.active.push(index);
        // Only affected rows join the swap set; untouched rows retain their ordinary ink.
    }
    assemble(builder, context, snapshot, frame, width, height);
    Ok(())
}
/// Substitutes only active variants, preserving the ordinary rows after drawing.
fn assemble(
    builder: &mut FrameBuilder,
    context: &mut TextContext,
    snapshot: &FrameSnapshot,
    frame: &mut RenderFrame,
    width: u32,
    height: u32,
) {
    if builder.overlays.effects.active.is_empty() {
        return;
    }
    swap(builder);
    // Cursor redraw reads the temporarily installed effect ink but keeps its grid cells.
    // Pending uploads belong to the renderer; only transient instances are discarded.
    frame.clear_instances();
    builder.assemble(snapshot, frame, width, height);
    super::super::cursor::append(builder, context, snapshot, frame, width, height);
    swap(builder);
    // Restore ordinary ownership before the next provider query or effect removal.
}
/// Moves variant ownership into assembly and restores the same caches afterward.
fn swap(builder: &mut FrameBuilder) {
    for index in &builder.overlays.effects.active {
        std::mem::swap(
            &mut builder.rows[*index],
            &mut builder.overlays.effects.rows[*index].row,
        );
    }
}
/// Copies only effect descriptors; no provider or buffer callback runs during assembly.
fn collect(builder: &mut FrameBuilder) {
    let requests = &mut builder.overlays.effects.requests;
    requests.clear();
    for (_, list) in builder.overlays.registry.lists() {
        for draw in &list.draws {
            let Primitive::GlyphEffect(mut range, effect) = draw.primitive else {
                continue;
            };
            if matches!(draw.resolved, Resolved::Hidden | Resolved::Gone) {
                continue;
            }
            if matches!(draw.anchor, Anchor::Range(_))
                && let Resolved::Cells(sampled) = draw.resolved
            {
                range = sampled;
            }
            // Invalid extension transforms fail locally before any atlas request.
            if !effect.opacity.is_finite()
                || super::ink::raster_scale(effect.transform, 1.0).is_err()
            {
                tracing::warn!(
                    event = "overlay.effect_rejected",
                    "nonfinite or singular grid effect skipped"
                );
                continue;
            }
            requests.push(Request { range, effect });
        }
    }
}
