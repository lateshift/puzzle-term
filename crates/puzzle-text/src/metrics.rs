//! Terminal cell geometry from OpenType metrics (design §8.2).
//!
//! Font units are scaled only after a face and variation instance have
//! supplied ascent, descent and the zero glyph's advance. Cell width,
//! height and baseline are then rounded to device pixels. Those integers
//! are shared by all backends, so a hinter can change ink without moving
//! a cell, cursor or overlay anchor.
//!
//! Cap-height snapping nudges only unhinted sizes by at most three percent.
//! Hinted outlines already fit their cap and x-height through blue zones;
//! nudging them again would choose an unrelated ppem and defeat D-27.
use crate::{face::FontFace, pack::PackMetrics};

/// An additive or proportional adjustment to a measured cell dimension.
#[derive(Clone, Copy, Debug)]
pub enum MetricAdjustment {
    /// Device pixels added after the font metric is scaled.
    Px(f32),
    /// Percentage of the original device-pixel metric.
    Percent(f32),
}

/// User settings that affect grid metrics without changing font outlines.
#[derive(Clone, Copy, Debug)]
pub struct CellOptions {
    /// Logical pixels per em before display DPR.
    pub size: f32,
    /// Physical device pixels per logical pixel.
    pub dpr: f32,
    /// Multiplier applied to the font's recommended line height.
    pub line_height: f32,
    /// Additional width for each grid cell.
    pub width: MetricAdjustment,
    /// Additional height for each grid cell.
    pub height: MetricAdjustment,
    /// Vertical baseline shift in device pixels or percent of cell height.
    pub baseline: MetricAdjustment,
    /// Whether the font's capital height may be snapped when unhinted.
    pub snap_cap_height: bool,
    /// Whether the selected outline is already pixel-hinted.
    pub hinted: bool,
}

/// Integral dimensions that every renderer and grid uses identically.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CellMetrics {
    /// Width of one terminal column in device pixels.
    pub width_px: u32,
    /// Height of one terminal row in device pixels.
    pub height_px: u32,
    /// Baseline offset from the row's top edge in device pixels.
    pub baseline_px: i32,
    /// Actual device pixels per em after an eligible cap-height snap.
    pub device_size: f32,
    /// True when the size met an exact cap-height pixel boundary.
    pub cap_snapped: bool,
    /// Underline thickness, never less than one device pixel.
    pub underline_px: u32,
}

/// Turns a percentage or pixel offset into one device-pixel quantity.
impl MetricAdjustment {
    /// Applies the signed change before final integer rounding.
    fn apply(self, value: f32) -> f32 {
        match self {
            Self::Px(px) => value + px,
            Self::Percent(percent) => value * (1.0 + percent / 100.0),
        }
    }
}

/// Derives geometry from the chosen primary face, independent of backend.
impl CellMetrics {
    /// Derives the same cell grid from a prebuilt pack before its TTF arrives.
    pub fn from_pack(metrics: PackMetrics, options: CellOptions) -> Result<Self, String> {
        // The pack stores normalized source metrics and zero advance, so
        // swapping to the full face cannot shift columns or the baseline.
        // Underline thickness is irrelevant to the startup text draw;
        // one original font unit keeps its geometry valid until takeover.
        let input = MetricInput {
            advance_em: metrics.zero_advance,
            ascent_em: metrics.ascent,
            descent_em: metrics.descent,
            gap_em: metrics.leading,
            cap_em: Some(metrics.cap_height),
            underline_em: 1.0 / f32::from(metrics.upem),
        };
        Self::compute(input, options)
    }

    /// Rejects bad font settings instead of constructing unusable surfaces.
    pub fn from_face(face: &FontFace, options: CellOptions) -> Result<Self, String> {
        let zero = face.glyph_id('0')?.ok_or("primary font lacks zero glyph")?;
        let advance_em = face.advance_em(zero)?;
        Self::from_face_advance(face, options, advance_em)
    }

    /// Sizes free text even for symbol/color faces that have no ASCII zero glyph.
    /// Width is a fallback-cluster fitting allowance; shaping supplies actual advances.
    pub fn from_text(face: &FontFace, size: f32, dpr: f32) -> Result<Self, String> {
        // Symbol-only faces need not encode Latin digits. A one-em allowance
        // lets their fallback ink fit without inventing an advance for shaping.
        let advance = face
            .glyph_id('0')?
            .map_or(Ok(1.0), |id| face.advance_em(id))?;
        // Natural annotations retain exact ppem and use a single font line height.
        Self::from_face_advance(
            face,
            CellOptions {
                size,
                dpr,
                line_height: 1.0,
                width: MetricAdjustment::Px(0.0),
                height: MetricAdjustment::Px(0.0),
                baseline: MetricAdjustment::Px(0.0),
                snap_cap_height: false,
                hinted: false,
            },
            advance,
        )
    }

    /// Normalizes font measurements independently of grid or natural advance policy.
    fn from_face_advance(
        face: &FontFace,
        options: CellOptions,
        advance_em: f32,
    ) -> Result<Self, String> {
        let metrics = face.metrics()?;
        let upem = f32::from(metrics.units_per_em);
        // Skrifa exposes unscaled values in font units. Normalizing first
        // keeps the formulas identical for fonts with different UPEM values.
        let cap_em = metrics.cap_height.map(|cap| cap / upem);
        let underline_em = metrics
            .underline
            .map_or(1.0 / upem, |mark| mark.thickness / upem);
        let input = MetricInput {
            advance_em,
            ascent_em: metrics.ascent / upem,
            descent_em: metrics.descent / upem,
            gap_em: metrics.leading / upem,
            cap_em,
            underline_em,
        };
        Self::compute(input, options)
    }

    /// Computes the §8.2 formulas after metrics are normalized to ems.
    fn compute(input: MetricInput, options: CellOptions) -> Result<Self, String> {
        if !options.size.is_finite()
            || options.size <= 0.0
            || !options.dpr.is_finite()
            || options.dpr <= 0.0
            || !options.line_height.is_finite()
            || options.line_height <= 0.0
        {
            return Err("font size, DPR and line height must be positive".into());
        }
        let requested = options.size * options.dpr;
        let (device_size, cap_snapped) = snap_size(requested, input.cap_em, options);
        // OpenType descent is negative. Subtracting it gives the positive
        // content span that must fit around the baseline.
        let content_h = (input.ascent_em - input.descent_em) * device_size;
        // Leading enlarges rows; it does not change the glyph's ascent.
        let natural_h = (content_h + input.gap_em * device_size) * options.line_height;
        let width = options.width.apply(input.advance_em * device_size);
        let height = options.height.apply(natural_h.ceil()).ceil();
        let baseline = options
            .baseline
            .apply((input.ascent_em * device_size + (height - content_h) * 0.5).round());
        // External percentages can produce non-finite or enormous extents.
        // Reject them before a float-to-integer cast could saturate.
        if !width.is_finite()
            || !height.is_finite()
            || !baseline.is_finite()
            || width <= 0.0
            || height <= 0.0
            || width > u32::MAX as f32
            || height > u32::MAX as f32
        {
            return Err("font metrics produce an invalid cell".into());
        }
        Ok(Self {
            width_px: width.round().max(1.0) as u32,
            height_px: height as u32,
            baseline_px: baseline.round() as i32,
            device_size,
            cap_snapped,
            underline_px: (input.underline_em * device_size).round().max(1.0) as u32,
        })
    }
}

/// Unitless font measurements needed by the cell formula.
#[derive(Clone, Copy)]
struct MetricInput {
    /// Advance of the zero glyph in ems.
    advance_em: f32,
    /// Height above the baseline in ems.
    ascent_em: f32,
    /// Usually negative distance below the baseline in ems.
    descent_em: f32,
    /// Extra font-recommended leading in ems.
    gap_em: f32,
    /// Capital height in ems, if the font supplies it.
    cap_em: Option<f32>,
    /// Underline stroke thickness in ems.
    underline_em: f32,
}

/// Keeps unsnappable sizes unchanged instead of nudging without alignment.
fn snap_size(requested: f32, cap_em: Option<f32>, options: CellOptions) -> (f32, bool) {
    if options.hinted || !options.snap_cap_height {
        return (requested, false);
    }
    let Some(cap_em) = cap_em.filter(|height| height.is_finite() && *height > 0.0) else {
        return (requested, false);
    };
    let snapped = (requested * cap_em).round() / cap_em;
    // A partial nudge adds no pixel alignment. Keeping an unsnappable size
    // unchanged preserves the visual decision made in the D-27 probe.
    if (snapped / requested - 1.0).abs() <= 0.03 {
        (snapped, true)
    } else {
        (requested, false)
    }
}

#[cfg(test)]
mod tests;
