//! COLR preparation includes every layer and clip curve, plus paint packing.
//!
//! The reviewer’s §15 rule applies to this font as well as CJK and Hangul.
//! The existing divan fixture measures six actual paths in two paint layers.
//! Counting after cubic conversion matches Slug's geometry boundary exactly.
//! Primary calibration runs immediately before this criterion in M11.6.
//! Its full-glyph CSV retains the equally weighted normalized observations.
//! The p95 index matches that calibration; no script-specific ceiling is added.
//! Remeasuring COLR here prevents an old timing file from standing in for work.
//! The legacy twenty-microsecond fixture check remains enforced as well.
//! The approved test font is opened only by this development acceptance tool.
//! Neither its bytes nor these diagnostics enter the distributed wasm/app.

/// Compares all prepared COLR geometry against the fresh primary calibration.
pub(super) fn run() -> Result<(), String> {
    // Regular weight matches the existing divan fixture's variation location.
    let bytes = std::fs::read("assets/fonts/test/test_glyphs-glyf_colr_1.ttf")
        .map_err(|e| e.to_string())?;
    let face = puzzle_text::face::FontFace::new(puzzle_text::FontData::new(bytes, 0), 400)?;
    let layers = face
        .color_layers(156, 0xffffffff)?
        .ok_or("COLR preparation fixture has no layers")?
        .layers;
    let mut curves = 0;
    // Repeated clips count on each layer because the benchmark prepares each copy.
    for layer in &layers {
        // Outer clips are separately prepared shapes, so their curves count too.
        for path in std::iter::once(&layer.path).chain(&layer.clips) {
            let shape =
                puzzle_slug::QuadShape::from_path(path, puzzle_slug::shape::DEFAULT_TOLERANCE)
                    .map_err(|e| e.to_string())?;
            curves += shape.curves.len();
        }
    }
    // Empty geometry cannot define a cost per quadratic segment.
    if curves == 0 {
        return Err("COLR fixture has no quadratic curves".into());
    }
    let median = super::super::colr_budget()?;
    // Whole-glyph time includes six small shape preparations rather than one lookup.
    let per_curve = median / curves as f64;
    let primary = primary_p95()?;
    println!(
        "COLR preparation curves={curves} median_us={median:.3} us_per_curve={per_curve:.6} primary_p95={primary:.6}"
    );
    // Paint packing is included in the timed numerator, never subtracted away.
    if !per_curve.is_finite() || per_curve > primary {
        return Err(format!(
            "COLR {per_curve:.6} us/curve exceeds primary p95 {primary:.6}"
        ));
    }
    Ok(())
}

/// Reads the full equally weighted glyph set produced by this acceptance run.
fn primary_p95() -> Result<f64, String> {
    // CSV costs use the calibration's six-decimal microsecond precision.
    let csv =
        std::fs::read_to_string("target/m11-primary-preparation.csv").map_err(|e| e.to_string())?;
    // Require the exact calibration columns before interpreting numeric fields.
    if !csv.starts_with("glyph,curves,median_us,us_per_curve\n") {
        return Err("primary preparation CSV has unexpected columns".into());
    }
    let mut costs = Vec::new();
    for row in csv.lines().skip(1) {
        let cost = row
            .split(',')
            .nth(3)
            .ok_or("primary cost missing")?
            .parse::<f64>()
            .map_err(|e| e.to_string())?;
        // Nonfinite or empty observations must fail rather than skew the percentile.
        if !cost.is_finite() || cost <= 0.0 {
            return Err("primary per-curve observation invalid".into());
        }
        costs.push(cost);
    }
    if costs.is_empty() {
        return Err("primary glyph set is empty".into());
    }
    // Sorting preserves every glyph's equal weight, including complex outliers.
    costs.sort_by(f64::total_cmp);
    Ok(costs[costs.len() * 19 / 20])
}
