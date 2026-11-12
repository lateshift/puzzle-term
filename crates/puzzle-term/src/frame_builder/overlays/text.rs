//! Cached free-text ink and provider measurements through the shared text context.
//!
//! Cache keys exclude translation/rotation, which are applied to copied instances.
//! Bitmap resolution includes the largest device magnification of that map.
//! Font/source or atlas changes invalidate entries through the context generation.
//! Annotations own UTF-8 request strings only while their bounded entries live.
//! Font bytes, shaping plans and glyph atlases belong to the host text context.
//! Two terminals can therefore share prepared glyphs without sharing annotation rows.
//! The descriptor allowance is independent of GPU atlas byte budgets.
//! Replacing the oldest descriptor releases its text and row vectors on a cold miss.
//! Font takeover drops obsolete entries before a stale face ID can be selected.
//! The standalone natural layout uses the same complete-cluster fallback chain.
//! Its widths include whitespace even when the ink union contains no instance.
//! Latin combining marks retain their attached offsets under the final map.
//! An explicit loaded face permits symbol/color annotations without changing defaults.
//! Provider measurements expose logical width, height, baseline and ink union.
//! Their values are independent of the grid's cell height or cursor placement.
//! Color channels remain straight RGBA at the instance boundary.
//! Raster shader metadata uses premultiplied tint after its sized source crop.
//! Prepared writes are collected with grid uploads after all commands compile.
//! A hidden or failed presentation preserves those writes until renderer acknowledgment.
use super::text_metrics::{alpha, bounds, options};
use super::{ink, shapes};
use crate::frame_builder::{
    CachedRow,
    context::TextContext,
    row::{RunStyle, glyphs::append_glyphs},
};
use puzzle_frame::{Batch, PipelineKind, RectU32, RenderFrame, ZSlot};
use puzzle_overlay::{
    draw::TextSpec,
    text::{TextLayouter, TextMetrics},
};
use std::cell::Cell;

/// One sized layout variant, with ordered vector layers and bitmap crops.
pub(super) struct Entry {
    /// Text/face/style/size/color identity; transform is deliberately excluded.
    pub(super) spec: TextSpec,
    /// Exact resolution multiplier used by pixel-only sources.
    pub(super) scale: f32,
    /// Host incarnation changes after font takeover, source replacement or eviction.
    pub(super) generation: u64,
    /// Slug page eviction independently invalidates cleared band and clip addresses.
    pub(super) slug_generation: u64,
    /// In-build raster recycling can precede a retained annotation lookup.
    pub(super) raster_generation: u64,
    /// Prepared ink remains neutral until each retained draw's affine map is applied.
    pub(super) row: CachedRow,
    /// Measurements remain in logical units for extension panel sizing.
    pub(super) metrics: TextMetrics,
}
/// Bounded descriptors reuse glyph caches and own no font or GPU resources.
#[derive(Default)]
pub(in crate::frame_builder) struct Cache {
    /// Up to 256 sized requests, cold replacements keep the vector's capacity.
    entries: Vec<Entry>,
}
/// Frame-only measurement borrows a context without a lock or RefCell (D-29).
pub(super) struct ProviderLayouter<'a> {
    /// Ownership temporarily leaves the cell during synchronous measurement.
    context: Cell<Option<&'a mut TextContext>>,
}
/// A callback cannot retain this frame-scoped font borrow.
impl<'a> ProviderLayouter<'a> {
    /// Creates a view-only measurement seam over the render-owned text context.
    /// Views use sealed snapshot rows; Session-owned models never borrow fonts.
    /// No callback can keep a font borrow beyond this builder invocation.
    /// The Cell moves the mutable owner during synchronous measurement.
    /// Recursive entry returns an error rather than causing borrow aliasing.
    pub(super) fn new(context: &'a mut TextContext) -> Self {
        Self {
            context: Cell::new(Some(context)),
        }
    }
}
/// The provider and compiler share the exact natural-layout cache.
impl TextLayouter for ProviderLayouter<'_> {
    /// Name resolution borrows only render-owned UI metadata, never grid fallback.
    /// Return an explicit face/scalar pair; the provider writes that face into TextSpec.
    /// Pack-only startup has no private UI face yet and returns no icon.
    /// Moving the temporary owner out of Cell follows the measurement lease below.
    fn icon(&self, name: &str) -> Option<(u32, char)> {
        let context = self.context.take()?;
        let result = context.fonts.as_ref().and_then(|fonts| fonts.ui_icon(name));
        self.context.set(Some(context));
        result
    }

    /// Prepares and measures ink, returning a report if fonts are still loading.
    /// Measurement and compilation select the same cache descriptor.
    /// The result describes natural text before any anchor or arbitrary affine.
    /// Each request restores the mutable context even after a preparation error.
    /// No retained-buffer lookup is needed to size an annotation.
    fn measure(&self, spec: &TextSpec) -> Result<TextMetrics, String> {
        let context = self.context.take().ok_or("recursive text measurement")?;
        // Restore ownership on both success and recoverable failure.
        let result = prepare(context, spec).map(|index| context.free_text.entries[index].metrics);
        self.context.set(Some(context));
        result
    }
}

/// Looks up a complete layout key before shaping or invoking any platform rasterizer.
/// A hit matches text, face, style, color, natural size and effective raster size.
/// Translation changes only copied instances and never creates a layout variant.
/// Rotation can still change effective bitmap resolution under an anisotropic map.
/// Context and Slug generations must both match before stored addresses are read.
/// Returns an entry index; malformed maps or unloaded faces produce local errors.
fn prepare(context: &mut TextContext, spec: &TextSpec) -> Result<usize, String> {
    // Apply the same bounded single-line contract before either font or pack layout.
    if spec.text.len() > 65_536 || spec.text.contains(['\n', '\r']) || spec.style > 3 {
        return Err("free text requires one bounded line and style 0..3".into());
    }
    let scale = ink::raster_scale(spec.transform, spec.size * context.dpr())?;
    // Source replacement, font takeover and paint relocation share this host epoch.
    let generation = context.atlas_generation();
    if let Some(index) = context.free_text.entries.iter().position(|e| {
        e.generation == generation
            && e.slug_generation == context.backend.atlas().generation()
            && e.raster_generation == context.raster.generation()
            && e.scale == scale
            && e.spec.text == spec.text
            && e.spec.size == spec.size
            && e.spec.face == spec.face
            && e.spec.style == spec.style
            && e.spec.color == spec.color
    }) {
        // A warm annotation must pin its pages before a later annotation prepares ink.
        for draw in &context.free_text.entries[index].row.rasters {
            crate::frame_builder::raster::mark_draw(&mut context.raster, draw);
        }
        return Ok(index);
    }
    // Font/source changes release obsolete annotations before cold insertion.
    context
        .free_text
        .entries
        .retain(|e| e.generation == generation);
    // No partially prepared annotation descriptor is published after an error.
    let entry = build(context, spec, scale, generation)?;
    if context.free_text.entries.len() >= 256 {
        // Replacement bounds retained strings and instance vectors independently of atlases.
        context.free_text.entries.remove(0);
    }
    let index = context.free_text.entries.len();
    context.free_text.entries.push(entry);
    Ok(index)
}

/// Shapes and prepares a cold annotation through the same glyph emitter as grid text.
/// The requested face defines a sized baseline, independently of terminal metrics.
/// Harfrust natural advances are retained rather than snapped onto grid columns.
/// The common glyph emitter preserves COLR layer ordering and complete fallback.
/// Cold bitmap requests use the transform resolution before returning neutral quads.
/// Errors leave no published cache entry; prepared atlas uploads stay pending.
fn build(
    context: &mut TextContext,
    spec: &TextSpec,
    scale: f32,
    generation: u64,
) -> Result<Entry, String> {
    let dpr = context.dpr();
    if context.fonts.is_none() {
        return super::text_pack::build(context, spec, scale, generation);
    }
    let fonts = context
        .fonts
        .as_mut()
        .ok_or("free-text fonts are still loading")?;
    let options = options(spec, dpr)?;
    let style = options.style;
    let layout =
        context
            .free_layout
            .line(fonts, &mut context.shaper, &spec.text, options, spec.face)?;
    let mut row = CachedRow::default();
    let rgba = spec.color.to_le_bytes();
    // Synthetic style is separate from the arbitrary affine text transform.
    append_glyphs(
        fonts,
        &mut context.backend,
        &mut context.raster,
        &mut row,
        &layout.glyphs,
        RunStyle {
            face: style,
            ink: [rgba[0], rgba[1], rgba[2]],
            selection_ink: spec.color,
            hidden: false,
            blink: false,
        },
        fonts.needs_synthetic_italic(style),
        dpr,
        scale,
    )?;
    alpha(&mut context.backend, &mut row, spec.color)?;
    let metrics = TextMetrics {
        width: layout.width / dpr,
        height: layout.metrics.height_px as f32 / dpr,
        baseline: layout.metrics.baseline_px as f32 / dpr,
        ink: bounds(&row, dpr),
    };
    Ok(Entry {
        spec: spec.clone(),
        scale,
        generation,
        slug_generation: context.backend.atlas().generation(),
        raster_generation: context.raster.generation(),
        row,
        metrics,
    })
}

/// Emits copied neutral ink in explicit provider order with its final affine map.
#[allow(
    clippy::too_many_arguments,
    reason = "text crosses independent logical and painter state"
)]
/// `origin` is logical anchor position; `clip` already names physical scissors.
/// Vector maps include existing glyph fitting and preserve root-space paint data.
/// Pixel-only quads grow into their cleared source border for rotated filtering.
/// The caller retains order within the chosen slot and later collects all uploads.
/// Returns an error before an invalid map can reach GPU instance coordinates.
pub(super) fn append(
    context: &mut TextContext,
    frame: &mut RenderFrame,
    spec: &TextSpec,
    origin: [f32; 2],
    slot: ZSlot,
    clip: Option<RectU32>,
    dpr: f32,
) -> Result<(), String> {
    let index = prepare(context, spec)?;
    // Anchor and authored logical translation are conjugated into device pixels once.
    let map = ink::device_map(spec.transform, origin, dpr);
    let entry = &mut context.free_text.entries[index];
    // A retained annotation draws on every frame even when its provider is idle.
    context.backend.use_row_batch(
        // The batch owns stable shape keys, so a warm stamp visits no glyph instances.
        &mut entry.row.shape_batch,
        entry
            .row
            .glyphs
            .iter()
            .map(|draw| (draw.glyph_loc, draw.paint)),
        false,
    );
    let first = frame.slugs.len() as u32;
    // Applying the affine map to copies leaves cached addresses and layout reusable.
    for source in &entry.row.glyphs {
        let mut draw = *source;
        ink::vector(&mut draw, map);
        frame.slugs.push(draw);
    }
    shapes::batch(frame, slot, clip, first..frame.slugs.len() as u32);
    let first = frame.rasters.len() as u32;
    // Pixel resolution was chosen during preparation; this pass changes destination geometry.
    for source in &entry.row.rasters {
        let mut draw = *source;
        ink::bitmap(
            &mut context.backend,
            &mut draw,
            map,
            entry.scale,
            spec.color,
            false,
        )?;
        frame.rasters.push(draw);
    }
    if first != frame.rasters.len() as u32 {
        frame.batches.push(Batch {
            slot,
            pipeline: PipelineKind::Raster,
            instances: first..frame.rasters.len() as u32,
            scissor: clip,
        });
    }
    Ok(())
}
