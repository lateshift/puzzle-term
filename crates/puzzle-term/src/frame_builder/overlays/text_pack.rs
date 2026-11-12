//! Natural startup annotations use imported outlines and design advances (§8.8).
//!
//! No font parser or platform fallback exists before the full font arrives.
//! The validated pack supplies its cmap, baseline and size-independent outlines.
//! Takeover invalidates these descriptors through the context generation, after
//! which annotations use ordinary Harfrust shaping and complete-cluster fallback.
//! Unsupported scalars retain an advance but await the same takeover as grid ink.
//! The provider never receives a font-pack or atlas address through measurement.
//!
//! Pack metrics normalize the primary face to em units, just like full-font
//! metrics. Annotation size scales these values without hinting or cap snapping.
//! The terminal's row dimensions never constrain an annotation's natural pen.
//! Pack whitespace has an advance even when its directory has no drawable shape.
//! A retained descriptor records context and atlas generations independently:
//! full-font takeover changes the former, atlas page eviction changes the latter.
//! RGBA opacity is applied after solid ink emission, keeping the same contract
//! as color-layer annotation output after the complete fonts become available.
use super::{
    text::Entry,
    text_metrics::{alpha, bounds},
};
use crate::frame_builder::{CachedRow, context::TextContext};
use puzzle_overlay::{draw::TextSpec, text::TextMetrics};
use puzzle_text::{
    glyph::{GlyphKey, GlyphPlacement, HintMode},
    metrics::{CellMetrics, CellOptions, MetricAdjustment},
};

/// Builds sized pack ink without fetching or extracting an outline at first frame.
/// `scale` records the effective bitmap variant for the shared descriptor key;
/// imported pack outlines themselves stay independent of transform resolution.
/// Returns natural measurements together with reusable neutral vector instances.
/// An absent pack or evicted outline returns an error, without publishing a row.
/// The caller validates UTF-8 length, single-line input and positive device size.
pub(super) fn build(
    context: &mut TextContext,
    spec: &TextSpec,
    scale: f32,
    generation: u64,
) -> Result<Entry, String> {
    let pack = context
        .pack
        .as_ref()
        .ok_or("free-text font source unavailable")?;
    let dpr = context.dpr();
    // These metrics describe the requested annotation, not the live grid font size.
    let metrics = CellMetrics::from_pack(
        pack.metrics,
        CellOptions {
            size: spec.size,
            dpr,
            line_height: 1.0,
            // Grid-specific metric adjustments must not stretch natural annotations.
            width: MetricAdjustment::Px(0.0),
            height: MetricAdjustment::Px(0.0),
            baseline: MetricAdjustment::Px(0.0),
            // A later rotation must see the original size-independent outline.
            snap_cap_height: false,
            hinted: false,
        },
    )?;
    let mut row = CachedRow::default();
    // Retain fractional advances between glyphs; only final origins round to pixels.
    let mut width = 0.0;
    for scalar in spec.text.chars() {
        // Unavailable scalars await fonts without shifting later supported text back.
        let Some(glyph) = pack.glyphs.get(&(scalar as u32)) else {
            width += pack.metrics.zero_advance * metrics.device_size;
            continue;
        };
        let advance = glyph.advance_em * metrics.device_size;
        // Whitespace still reaches the common pen increment below this branch.
        if glyph.handle.is_some() {
            // Space entries advance but never address an absent contour directory.
            let placement = placement(spec, metrics, glyph.glyph_id, width, advance);
            let handle = context
                .backend
                .get(placement.key)
                .ok_or("free-text pack outline evicted")?;
            // The ordinary emitter retains baseline and span fitting for both paths.
            crate::glyph_ink::append_ink(&mut row.glyphs, &placement, handle, false, false, false);
        }
        width += advance;
    }
    alpha(&mut context.backend, &mut row, spec.color)?;
    // Provider measurements are logical pixels; backend instances remain physical.
    let measurement = TextMetrics {
        width: width / dpr,
        height: metrics.height_px as f32 / dpr,
        baseline: metrics.baseline_px as f32 / dpr,
        ink: bounds(&row, dpr),
    };
    Ok(Entry {
        // Retaining the request enables exact cache matching after provider rebuilds.
        spec: spec.clone(),
        scale,
        generation,
        slug_generation: context.backend.atlas().generation(),
        raster_generation: context.raster.generation(),
        row,
        metrics: measurement,
    })
}

/// Selects imported regular unhinted ink; full styles arrive with the OpenType face.
/// The pen and advance are physical pixels derived from pack design metrics.
/// No terminal column exists for this free-floating request, so its column is zero.
/// The glyph key matches the imported regular pack directory already in the atlas.
/// A standalone span supplies fitting bounds without changing natural advancement.
fn placement(
    spec: &TextSpec,
    metrics: CellMetrics,
    glyph_id: u32,
    pen: f32,
    advance: f32,
) -> GlyphPlacement {
    GlyphPlacement {
        key: GlyphKey {
            face: 0,
            glyph_id,
            instance: 0,
            // Pack outlines have no per-ppem hinting incarnation.
            hint_ppem: 0,
            hint_mode: HintMode::None,
        },
        // Natural pen advancement is independent of terminal cell positions.
        origin_px: [pen.round() as i32, metrics.baseline_px],
        baseline_px: metrics.baseline_px as f32,
        // These bounds allow a fallback-sized contour independently of grid cells.
        size_px: metrics.device_size,
        advance_px: advance.ceil() as u32,
        ink_limit_px: advance.ceil() as u32,
        cell_height_px: metrics.height_px,
        cell_column: 0,
        cell_span: 1,
        color: spec.color | 0xff000000,
        effect: true,
        color_glyph: false,
    }
}
