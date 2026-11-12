//! Natural free-text advances with the grid's faces and whole-cluster fallback.
//!
//! Terminal rows ignore Harfrust advances, whereas annotations use them. Both
//! modes still produce the same backend-neutral placements and source keys.
//! Free text always requests unhinted outlines: its final affine map can rotate,
//! shear or zoom after layout. Pixel-only fallback is sized later at that map.
//!
//! A line owns one natural pen, unlike a terminal run anchored to fixed columns.
//! Harfrust advances include spacing and substitutions; attached marks may have
//! zero advance while still contributing an outline at their base's pen position.
//! Spaces advance the pen but do not create ink instances. This matters for panel
//! measurement and for placing a button after a query without inspecting glyphs.
//!
//! Explicit loaded faces have first refusal for whole clusters. Their ascent and
//! descent define annotation fitting, while uncovered clusters retain the same
//! fallback chain as grid text. A font ID never becomes an offscreen row query.
//! Source faces retain complete grapheme requests, including emoji selectors.
//! Those platform sources do not expose a natural OpenType advance, so their
//! complete-cluster allocation remains the fallback advance at the requested size.
//!
//! Layout bounds are independent of an annotation's rotation or translation.
//! The host applies that map to copied ink after shaping and bitmap preparation.
//! Effective bitmap resolution belongs downstream, where the complete map is
//! known; it cannot feed back into this pen or move later source clusters.
//! Device pixels stay the neutral coordinate unit until the provider's measurement
//! seam divides by DPR. No global grid metrics are borrowed for a sized baseline.
use super::{Cluster, LayoutOptions, RunContext, clusters, make_placement, source};
use crate::{
    font_set::FontSet,
    glyph::GlyphPlacement,
    hinting,
    metrics::CellMetrics,
    shape::{ShapingOptions, TextShaper},
};

/// Borrowed natural layout, with placement storage retained for the next request.
pub struct Text {
    /// Drawable glyphs retain whole grapheme identities, never GPU addresses.
    pub glyphs: Vec<GlyphPlacement>,
    /// Natural advance in physical pixels, including spaces.
    pub width: f32,
    /// Primary requested-face metrics determine fallback fitting and baseline.
    pub metrics: CellMetrics,
}
/// Source clusters and output scratch are reused sequentially by the host.
#[derive(Default)]
pub struct Layouter {
    /// Whole graphemes choose faces before equal-face shaping runs form.
    clusters: Vec<Cluster>,
    /// Capacity survives rebuilding text after a provider changes its query.
    output: Option<Text>,
}
/// Free text changes advances without changing the grid's shaping contract.
impl Layouter {
    /// Shapes one bounded line, preferring a loaded face when explicitly requested.
    pub fn line(
        &mut self,
        fonts: &mut FontSet,
        shaper: &mut TextShaper,
        text: &str,
        options: LayoutOptions<'_>,
        preferred: Option<u32>,
    ) -> Result<&Text, String> {
        // One retained command cannot consume an unbounded cluster/cache allocation.
        if text.len() > 65_536 || text.contains(['\n', '\r']) {
            return Err("free text requires one line of at most 65536 bytes".into());
        }
        // The requested loaded face establishes this line's ascent and fitting limits.
        let metrics = measure(fonts, &options, preferred)?;
        // Face selection owns whole graphemes, so attached marks never split faces.
        clusters::collect_into(
            fonts,
            text,
            options.style,
            false,
            false,
            &mut self.clusters,
            preferred,
        )?;
        // Lazily create the output owner only on this layouter's first cold request.
        let output = self.output.get_or_insert_with(|| Text {
            glyphs: Vec::new(),
            width: 0.0,
            metrics,
        });
        // Reuse glyph capacity; a font or paint variant may still use identical text.
        output.metrics = metrics;
        output.glyphs.clear();
        output.width = 0.0;
        // Keep the normal placement machinery while overriding its grid hint policy.
        let context = RunContext {
            metrics,
            hint: hinting::choose(
                crate::hinting::HintingSetting::None,
                metrics.device_size,
                options.dpr,
                true,
            )?,
            options: LayoutOptions {
                transformed: true,
                ..options
            },
        };
        // Shaping consumes borrowed clusters; output capacity belongs to the host.
        place(fonts, shaper, text, &self.clusters, &context, output)?;
        Ok(output)
    }
}

/// Complete platform clusters advance by their allocated span; faces use shaped advances.
fn place(
    fonts: &mut FontSet,
    shaper: &mut TextShaper,
    text: &str,
    clusters: &[Cluster],
    context: &RunContext<'_>,
    output: &mut Text,
) -> Result<(), String> {
    // Runs advance through source clusters even when the font emits no contour.
    let mut start = 0;
    while start < clusters.len() {
        // Reserved sources return one complete cluster and cannot join a face run.
        if let Some(mut p) = source::special(fonts, &clusters[start], text, context)? {
            // The platform draws a whole cluster, using this line's own sized baseline.
            p.origin_px[0] = output.width.round() as i32;
            // A platform source has no OpenType advance; retain its whole-cluster span.
            output.width += p.advance_px as f32;
            output.glyphs.push(p);
            start += 1;
            continue;
        }
        // Find the next equal-face boundary before borrowing the shaper result.
        let mut end = start + 1;
        // Never shape across a fallback change or split a combining grapheme.
        while end < clusters.len() && clusters[end].face == clusters[start].face {
            end += 1;
        }
        // All glyphs in this run share one natural pen, including whitespace.
        run(fonts, shaper, text, &clusters[start..end], context, output)?;
        start = end;
    }
    Ok(())
}

/// Maps Harfrust's natural pen into neutral glyph origins, preserving attached marks.
fn run(
    fonts: &FontSet,
    shaper: &mut TextShaper,
    text: &str,
    clusters: &[Cluster],
    context: &RunContext<'_>,
    output: &mut Text,
) -> Result<(), String> {
    // Empty runs are rejected before indexing their first and last source bytes.
    let first = clusters.first().ok_or("empty free-text run")?;
    let last = clusters.last().ok_or("empty free-text run")?;
    // Glyph IDs belong to this loaded face rather than the primary fallback slot.
    let face = fonts
        .face(first.face)
        .ok_or("free-text shaping face missing")?;
    // The existing run cache retains features and ligature decisions for this face.
    let glyphs = shaper.shape(
        first.face,
        face,
        &text[first.byte..last.end],
        ShapingOptions {
            ligatures: context.options.ligatures,
            features: context.options.features,
        },
    )?;
    for glyph in glyphs {
        let owner = owner(clusters, first.byte, glyph.cluster)?;
        // Use shaped advances; only the cmap fast path needs a cold metrics lookup.
        let advance = match glyph.advance_em {
            Some(value) => value,
            // The grid's ASCII fast path never pays for proportional advances.
            None => face.advance_em(glyph.glyph_id)?,
        } * context.metrics.device_size;
        // Reject unreliable device coordinates before converting a pen to integers.
        if !advance.is_finite() || (output.width + advance).abs() > 1_048_576.0 {
            return Err("free-text advance exceeds reliable pixel coordinates".into());
        }
        // Spaces move the natural pen even though they contribute no drawable outline.
        if !crate::font_set::is_blank_cluster(&text[owner.byte..owner.end]) {
            // The common placement constructor keeps source keys and mark y offsets.
            let mut p = make_placement(first.face, *glyph, owner, context)?;
            p.origin_px[0] = (output.width + glyph.x_offset_em * p.size_px).round() as i32;
            // A zero-advance mark stays attached while the next base advances normally.
            p.advance_px = advance.max(0.0).ceil() as u32;
            // Fallback fitting must allow the requested face's proportional span.
            p.ink_limit_px = p.ink_limit_px.max(p.advance_px);
            output.glyphs.push(p);
        }
        // Every shaped glyph advances the pen once, whether or not it draws ink.
        output.width += advance;
    }
    Ok(())
}

/// Converts a run-relative UTF-8 cluster into its validated whole-grapheme owner.
fn owner(clusters: &[Cluster], first: usize, relative: usize) -> Result<&Cluster, String> {
    // Harfrust byte clusters are relative to its borrowed run slice.
    let byte = first
        .checked_add(relative)
        .ok_or("free-text cluster overflow")?;
    // Binary lookup handles ligatures and attached marks without per-glyph scans.
    let index = clusters.partition_point(|cluster| cluster.byte <= byte);
    index
        .checked_sub(1)
        .and_then(|i| clusters.get(i))
        .ok_or_else(|| "free-text shaper returned an invalid cluster".into())
}

/// Resolves requested-face metrics without applying grid cap-height snapping.
fn measure(
    fonts: &FontSet,
    options: &LayoutOptions<'_>,
    preferred: Option<u32>,
) -> Result<CellMetrics, String> {
    // Glyph IDs belong to this loaded face rather than the primary fallback slot.
    let face = fonts
        .face(preferred.unwrap_or(fonts.primary_face(options.style)))
        .ok_or("free-text face is not loaded")?;
    // Fallback ink is fitted to these metrics, rather than the terminal row height.
    // A color/icon face may have no ASCII zero; natural text must still be measurable.
    let metrics = CellMetrics::from_text(face, options.size, options.dpr)?;
    Ok(metrics)
}

/// Annotation contracts differ from grid advancement and need independent font checks.
#[cfg(test)]
mod tests;
