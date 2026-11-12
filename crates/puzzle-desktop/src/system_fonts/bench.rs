//! Installed fallback outline preparation uses the existing §15 budget boundary.
//!
//! Discovery, shaping and outline extraction happen before the timer. The timer
//! includes reset, path conversion, band building and texel packing, matching the
//! original divan glyph benchmark. CPU texture allocations are reused across
//! repetitions; cached handle lookup cannot masquerade as preparation work.
//! Every sample is retained; the primary median retains the twenty-microsecond bound.
//! Font files have already been unmapped while these owned paths are measured.
//! These are actual native CJK/symbol outlines, not the bundled Latin benchmark.
//! The CJK/Hangul characters exercise different contour counts, and the rare
//! symbol exercises a different installed family. No discovery time is
//! subtracted from a measured number: the timing boundary excludes it outright.
//! Each sample averages one hundred genuine preparations to reduce clock
//! granularity error; the median of 101 samples limits scheduler noise.
//! Reset increments atlas generation and removes handles, so each iteration
//! rebuilds bands rather than finding a glyph prepared on the previous turn.
//! The warm mirrors retain their backing allocations, as in normal rendering.
//! This matters because repeatedly creating an entire empty atlas would time
//! megabytes of zero filling instead of typical glyph preparation.
//! The user defined primary median and per-curve scaling on 2026-10-04.
//! Complex glyphs are checked against measured primary cost per curve.
//! Pixel quality remains a separate user review: timing cannot certify it.
use super::NativeFonts;
use puzzle_term::slug::{ShapeKey, SlugAtlas, atlas::FillRule};
use puzzle_term::text::{
    cluster_source::{ClusterGlyph, ClusterRequest},
    font_set::FontStyle,
    presentation::Presentation,
};
/// Measures typical installed CJK and symbol conversion with all samples retained.
pub(crate) fn run() -> Result<(), String> {
    let context = puzzle_term::frame_builder::context::TextContext::new(
        crate::font_data::bundled_font_set()?,
        14.0,
        1.0,
    )?;
    let primary = primary::run()?;
    // Explicit Heiti SC fulfills the requested CJK specimen even if generic
    // family ordering changes with system discovery metadata.
    let mut source = NativeFonts::new(vec!["Heiti SC".into()], false, true, context.metrics());
    let mut failure = None;
    for cluster in ["漢", "字", "鬱", "語", "한", "글", "뿔", "𝄞"] {
        let r = ClusterRequest {
            cluster,
            style: FontStyle::Regular,
            presentation: Presentation::Text,
        };
        let p = source.probe_placement();
        let ink = source
            .resolve(r, &p)
            .ok_or("installed benchmark font unavailable")?;
        let path = match ink {
            ClusterGlyph::Outline(path) | ClusterGlyph::StyledOutline { path, .. } => path,
            _ => return Err("installed text benchmark did not produce an outline".into()),
        };
        // Count after cubic conversion: raw path commands are not Slug curve segments.
        let curves = curve_count(&path)?;
        let median = measure(&path)?;
        let per_curve = median / curves as f64;
        println!(
            "fallback_prepare cluster={cluster} family={:?} curves={curves} median_us={median:.3} us_per_curve={per_curve:.6} samples=101 iterations=100 peak_mapped_bytes={} retained_font_bytes=0",
            source.last_family, source.peak_mapped_bytes
        );
        // The upper observed primary spread is a measured scaling reference, not
        // a script-specific microsecond ceiling. Faster per-curve work also holds.
        if !per_curve.is_finite() || per_curve > primary {
            failure = Some(format!(
                "{cluster} cost {per_curve:.6} us/curve exceeds primary measured spread {primary:.6}"
            ));
        }
    }
    if let Some(error) = failure {
        return Err(error);
    }
    Ok(())
}
/// Times uncached conversion/packing using a warmed reusable atlas mirror.
fn measure(path: &puzzle_term::text::geometry::BezPath) -> Result<f64, String> {
    let mut atlas = SlugAtlas::default();
    let mut samples = Vec::with_capacity(101);
    for _ in 0..101 {
        let start = std::time::Instant::now();
        for _ in 0..100 {
            // Reset removes the identity while retaining allocated texture planes.
            atlas.reset().map_err(|e| e.to_string())?;
            std::hint::black_box(
                atlas
                    .prepare(ShapeKey(1), path, FillRule::NonZero, 0)
                    .map_err(|e| e.to_string())?,
            );
        }
        samples.push(start.elapsed().as_secs_f64() * 10_000.0);
    }
    samples.sort_by(f64::total_cmp);
    Ok(samples[50])
}

/// Counts quadratic segments at the precise geometry-to-Slug boundary.
fn curve_count(path: &puzzle_term::text::geometry::BezPath) -> Result<usize, String> {
    let shape =
        puzzle_term::slug::QuadShape::from_path(path, puzzle_term::slug::shape::DEFAULT_TOLERANCE)
            .map_err(|e| e.to_string())?;
    Ok(shape.curves.len())
}

/// Full-primary calibration establishes a script-independent scaling reference.
mod primary;
/// Uses the same complete primary set for the M9 shape preparation comparison.
pub(crate) use primary::run as calibrate_primary;

/// Cold bitmap preparation is measured separately because pixels have no curve count.
pub(crate) mod bitmap;
