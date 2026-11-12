//! Dense overlay preparation at the established path-to-packed-texels boundary.
//!
//! Stroke expansion precedes the timer, just as font extraction precedes the
//! primary calibration. Every sample converts, clips, builds bands and packs
//! all chunks; resetting the atlas forbids a cached-handle timing result.
//! The original quadratic count is the denominator. Artificial cut edges and
//! duplicated AA guards therefore cannot make costly subdivision look cheaper.
//! All raw samples remain in target; the limit comes from this run's complete
//! primary-font p95 rather than a frozen or shape-specific microsecond budget.
//! The gate includes every generated cut edge in elapsed time but not in its denominator.
use puzzle_term::{
    slug,
    text::geometry::{Affine, BezPath},
};

/// Reports fresh comparable costs and rejects a shape above primary per-curve p95.
/// The primary calibration visits every drawable regular glyph with equal samples.
/// Its returned p95 bounds this run's cost per original quadratic segment.
/// The plot is expanded before timing, matching native font outline extraction.
/// Reset removes all shape identities while retaining allocated texture mirrors.
/// Every measured draw packs all final pieces, including duplicated AA guards.
/// The sample CSV retains scheduler outliers and errors are never turned into PASS.
pub(crate) fn run() -> Result<(), String> {
    let primary = crate::system_fonts::bench::calibrate_primary()?;
    let path = plot();
    // Count after the identical tolerance conversion used by overlay preparation.
    let curves = slug::QuadShape::from_path(&path, 1.0 / 2048.0)
        .map_err(|e| e.to_string())?
        .curves
        .len();
    // Cold preparations include both conversion and complete texel packing.
    let (mut samples, mut chunk_times, count, packed) = sample(&path)?;
    // Preserve original sample order before any percentile sorting.
    let rows = samples
        .iter()
        .enumerate()
        .map(|(i, t)| format!("{i},{t:.6}\n"))
        .collect::<String>();
    std::fs::write(
        "target/m9-shape-preparation.csv",
        format!("sample,us\n{rows}"),
    )
    .map_err(|e| e.to_string())?;
    // Both summaries use the same complete sample set; packing remains included.
    chunk_times.sort_by(f64::total_cmp);
    println!("chunk-only median_us={:.3}", chunk_times[15]);
    samples.sort_by(f64::total_cmp);
    // Median avoids a single scheduler stall defining the preparation gate.
    let median = samples[15];
    // Original curves forbid duplicated clipping edges from diluting normalized cost.
    let per_curve = median / curves as f64;
    println!(
        "overlay_prepare chunks={count} original_curves={curves} packed_curves={packed} median_us={median:.3} us_per_curve={per_curve:.6} primary_p95={primary:.6}"
    );
    if !per_curve.is_finite() || per_curve > primary {
        return Err(format!(
            "overlay cost {per_curve:.6} us/curve exceeds primary p95 {primary:.6}"
        ));
    }
    Ok(())
}

/// Builds the same locally generated five-thousand-point round-stroked plot.
/// Uniform points form a continuous sine centerline with multiple extrema.
/// Round joins and caps are expanded together before spatial subdivision.
/// Clipping therefore cannot invent caps at internal boundaries or lose joins.
/// The generated path owns its geometry and requires no third-party fixture asset.
/// Its coordinates match the public-provider plot used for native pixel review.
fn plot() -> BezPath {
    let mut source = BezPath::new();
    source.move_to((0.0, 50.0));
    for index in 1..5000 {
        // Uniform samples retain high density throughout the visible shape.
        let x = index as f64 / 9.0;
        source.line_to((x, 50.0 + (x * 0.11).sin() * 35.0));
    }
    // Round expansion occurs outside the preparation boundary, like outline loading.
    puzzle_term::text::geometry::stroke(
        source.iter(),
        &puzzle_term::text::geometry::Stroke::new(3.0)
            .with_caps(puzzle_term::text::geometry::Cap::Round)
            .with_join(puzzle_term::text::geometry::Join::Round),
        &puzzle_term::text::geometry::StrokeOpts::default(),
        0.0005,
    )
}

/// Repeats complete cold shape preparation while retaining atlas mirror capacity.
fn sample(path: &BezPath) -> Result<(Vec<f64>, Vec<f64>, usize, usize), String> {
    let mut atlas = slug::SlugAtlas::default();
    let mut samples = Vec::with_capacity(31);
    // Keep subdivision cost visible separately without subtracting it from the gate.
    let mut chunk_times = Vec::new();
    let mut packed = 0;
    let mut count = 0;
    for _ in 0..31 {
        let start = std::time::Instant::now();
        atlas.reset().map_err(|e| e.to_string())?;
        let chunks = slug::chunks::prepare(path, Affine::IDENTITY)?;
        chunk_times.push(start.elapsed().as_secs_f64() * 1_000_000.0);
        count = chunks.len();
        packed = chunks.iter().map(|c| c.shape.curves.len()).sum::<usize>();
        for (index, chunk) in chunks.into_iter().enumerate() {
            // Packing is included exactly once; no second membership pass is timed.
            std::hint::black_box(
                atlas
                    .prepare_quadratics(
                        slug::ShapeKey(index as u64),
                        chunk.shape,
                        chunk.bands,
                        slug::atlas::FillRule::NonZero,
                        0,
                    )
                    .map_err(|e| e.to_string())?,
            );
        }
        samples.push(start.elapsed().as_secs_f64() * 1_000_000.0);
    }
    Ok((samples, chunk_times, count, packed))
}
