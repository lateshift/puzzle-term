//! Primary-font calibration for the user's 2026-10-04 preparation definition.
//!
//! Every drawable regular glyph receives equal cold preparations. Empty glyphs
//! carry no outline and are excluded explicitly; complex glyphs remain included.
//! Curve counts use Slug's cubic-to-quadratic conversion tolerance, not the
//! font's raw path commands. The median cost retains the 20 microsecond limit.
//! Per-curve costs establish the observed spread against which all scripts
//! are compared. The central ninety percent is reported with its endpoints.
//! Full per-glyph results stay in target so outliers remain reviewable.
//! Atlas reset drops keys while retaining texture allocations between samples.
//! This measures conversion, bands and packing rather than cache hits or startup.
//! File loading and outline extraction precede the established benchmark boundary.
use super::{FillRule, ShapeKey, SlugAtlas, curve_count};
/// Measures every drawable primary glyph and returns its observed upper per-curve spread.
pub(crate) fn run() -> Result<f64, String> {
    let mut face = puzzle_term::text::face::FontFace::new(crate::font_data::bundled_font()?, 400)?;
    // Glyph IDs include alternates without cmap entries, so coverage is not a sampling filter.
    let count = face.metrics()?.glyph_count;
    // Separate distributions preserve actual time and complexity without sorting glyphs away.
    let mut times = Vec::new();
    let mut normalized = Vec::new();
    let mut counts = Vec::new();
    let mut atlas = SlugAtlas::default();
    let mut rows = String::from("glyph,curves,median_us,us_per_curve\n");
    // Traverse the complete font directory, including unencoded alternate glyphs.
    for glyph in 1..u32::from(count) {
        let path = face.outline(glyph, 0, puzzle_term::text::glyph::HintMode::None)?;
        // Spaces and other inkless glyphs cannot define cost per curve.
        if path.elements().is_empty() {
            continue;
        }
        // Count exactly the segments later consumed by the Slug band builder.
        let curves = curve_count(&path)?;
        // Short samples cover the entire glyph set without sampling only ASCII.
        // Each glyph receives the same repetitions, so no complex glyph is discarded.
        let time = sample(&mut atlas, &path)?;
        // Normalize the same timed preparation by its own complexity, never another glyph.
        times.push(time);
        normalized.push(time / curves as f64);
        counts.push(curves);
        rows.push_str(&format!(
            "{glyph},{curves},{time:.6},{:.6}\n",
            time / curves as f64
        ));
    }
    summarize(times, normalized, counts, rows)
}

/// Retains every per-glyph sample before reporting the established primary spread.
fn summarize(
    mut times: Vec<f64>,
    mut normalized: Vec<f64>,
    mut counts: Vec<usize>,
    rows: String,
) -> Result<f64, String> {
    // Summaries use medians; the CSV retains each glyph and curve count.
    times.sort_by(f64::total_cmp);
    normalized.sort_by(f64::total_cmp);
    counts.sort_unstable();
    let n = times.len();
    // Reject an unavailable fixture instead of publishing a meaningless percentile.
    if n == 0 {
        return Err("primary has no drawable glyph set".into());
    }
    // Equal weighting defines the median drawable glyph, rather than a text-frequency mix.
    let median = times[n / 2];
    let low = normalized[n / 20];
    // The upper endpoint is computed from the complete equally weighted set.
    // It is recalibrated with every fallback/overlay benchmark invocation.
    let high = normalized[n * 19 / 20];
    println!(
        "primary glyphs={n} median_curves={} median_us={median:.3} us_per_curve_median={:.6} spread_p05_p95={low:.6}..{high:.6}",
        counts[n / 2],
        normalized[n / 2]
    );
    // Keep every glyph result in ignored evidence; the spread is the central 90%.
    std::fs::write(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/m11-primary-preparation.csv"),
        rows,
    )
    .map_err(|e| e.to_string())?;
    // A nonfinite timing cannot establish compliance with the approved primary budget.
    if !median.is_finite() || median > 20.0 {
        return Err(format!("primary median {median:.3} us exceeds 20 us"));
    }
    Ok(high)
}

/// Repeats cold preparation equally for each primary glyph, retaining warm mirrors.
fn sample(
    atlas: &mut SlugAtlas,
    path: &puzzle_term::text::geometry::BezPath,
) -> Result<f64, String> {
    let mut samples = Vec::with_capacity(21);
    for _ in 0..21 {
        let start = std::time::Instant::now();
        for _ in 0..10 {
            // Remove the key so all repetitions convert and pack real geometry.
            atlas.reset().map_err(|e| e.to_string())?;
            std::hint::black_box(
                atlas
                    .prepare(ShapeKey(1), path, FillRule::NonZero, 0)
                    .map_err(|e| e.to_string())?,
            );
        }
        // Average ten operations before taking this glyph's median sample.
        samples.push(start.elapsed().as_secs_f64() * 100_000.0);
    }
    samples.sort_by(f64::total_cmp);
    Ok(samples[10])
}
