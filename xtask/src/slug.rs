//! Slug diagnostics and the native preparation budget (design §9.6).
//!
//! Synthetic outlines expose packing, while a separately parsed font supplies
//! real text for the early CPU quality probe. PNGs stay under target/.
//!
//! The synthetic mode is useful when debugging packing: it prints the
//! horizontal and vertical band lists alongside a single-shape oracle PNG.
//! Rectangle edges isolate C1/B1/B2 and analytic coverage; the circle and
//! nested contours expose rotational and fill-rule behavior. No font parse
//! can obscure the geometry evidence in this mode.
//!
//! The real-font mode delegates to the specimen module. It renders multiple
//! sizes and DPRs from one em-space atlas and supplies the human M2 review
//! image. Both modes use `puzzle-slug::reference`, not a second drawing
//! algorithm, so their output predicts the eventual GPU port.
//!
//! The benchmark times a real JetBrains Mono capital B. Font parsing and
//! outline extraction happen before timing: design §15 measures outline-to-
//! texels preparation, including kurbo conversion, band choice and packing.
//! Resetting the atlas between iterations avoids a cached-handle shortcut
//! while retaining allocated mirror memory, as in a warmed terminal.
//! Five thousand operations in five batches make timer granularity small;
//! the median gives a robust native budget estimate in release mode.
/// CPU-only small-text comparison for the open D-27 choice.
mod hinting;
/// Minimal dependency-free PNG encoding for one oracle image.
mod png;
/// Font specimen sheet through the same CPU reference sampler.
mod specimen;

use puzzle_slug::{
    ShapeKey, SlugAtlas,
    atlas::FillRule,
    bands::BandBuilder,
    reference::{RasterTransform, rasterize},
    shape::{DEFAULT_TOLERANCE, QuadShape},
    synthetic::SyntheticShape,
};
use std::{fs, hint::black_box, path::PathBuf, time::Instant};

/// Routes synthetic and real-font diagnostic modes without GPU setup.
/// The no-argument route creates the fixed M2 reviewer specimen.
pub(crate) fn run(args: &[String]) -> Result<(), String> {
    if args.first().is_some_and(|arg| arg == "--compare-hinting") {
        return hinting::run(&args[1..]);
    }
    if args.is_empty() {
        return specimen::default_sheet();
    }
    if args.first().is_some_and(|arg| arg == "--font") {
        return specimen::from_args(args);
    }
    dump(args.first().map(String::as_str))
}

/// Converts a synthetic outline, prints band indices and saves its oracle PNG.
/// This path remains independent of font tables and text layout.
pub(crate) fn dump(name: Option<&str>) -> Result<(), String> {
    let name =
        name.ok_or("usage: cargo xtask slug-dump <rectangle|circle|double-winding|typical-glyph>")?;
    let kind = SyntheticShape::from_name(name).ok_or("unknown synthetic Slug shape")?;
    let path = kind.path();
    let shape = QuadShape::from_path(&path, DEFAULT_TOLERANCE).map_err(|e| e.to_string())?;
    let lists = BandBuilder.build(&shape, 16).map_err(|e| e.to_string())?;
    // The same shape enters the real atlas to ensure printed band lists
    // correspond to the data the oracle samples from its CPU mirrors.
    let mut atlas = SlugAtlas::default();
    let handle = atlas
        .prepare(ShapeKey(1), &path, FillRule::NonZero, 0)
        .map_err(|e| e.to_string())?;
    let image = rasterize(
        &atlas,
        handle,
        96,
        96,
        RasterTransform {
            scale: [80.0, -80.0],
            translate: [8.0, 88.0],
        },
    )
    .map_err(|e| e.to_string())?;
    // A fixed transform shows the full unit-em fixture and one-pixel
    // dilation without needing a native GPU device.
    let folder = PathBuf::from("target/slug-dump");
    fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
    let output = folder.join(format!("{name}.png"));
    png::write(&output, &image)?;
    println!(
        "shape={name} curves={} hbands={} vbands={}",
        shape.curves.len(),
        lists.horizontal.len(),
        lists.vertical.len()
    );
    for (axis, bands) in [("H", &lists.horizontal), ("V", &lists.vertical)] {
        for (index, members) in bands.iter().enumerate() {
            println!("{axis}{index}: {members:?}");
        }
    }
    println!("oracle={}", output.display());
    Ok(())
}

/// Measures an uncached real JetBrains glyph through conversion and packing.
/// Letter B has curved outlines and two counters, producing representative
/// band memberships rather than a trivial straight-line rectangle.
pub(crate) fn preparation_micros() -> Result<f64, String> {
    let bytes =
        fs::read("assets/fonts/JetBrainsMono-Variable.ttf").map_err(|error| error.to_string())?;
    let font = puzzle_text::probe::ProbeFont::new(&bytes).map_err(|e| e.to_string())?;
    let (path, _) = font.glyph('B').map_err(|e| e.to_string())?;
    let mut atlas = SlugAtlas::default();
    let mut batches = Vec::with_capacity(5);
    // A median across batches reduces sensitivity to scheduler interruptions
    // while still failing a persistent regression against the 20 µs cap.
    for _ in 0..5 {
        let start = Instant::now();
        for _ in 0..1000 {
            // Reusing allocations models a warm terminal, but reset keeps
            // every iteration on the full preparation path, not key lookup.
            atlas.reset().map_err(|e| e.to_string())?;
            let handle = atlas
                .prepare(ShapeKey(1), black_box(&path), FillRule::NonZero, 0)
                .map_err(|e| e.to_string())?;
            black_box(handle);
        }
        batches.push(start.elapsed().as_secs_f64() * 1_000_000.0 / 1000.0);
    }
    // Median rejects both transient scheduler stalls and an unusually fast
    // single batch; the same rule controls the acceptance threshold.
    batches.sort_by(f64::total_cmp);
    Ok(batches[2])
}

/// Prints the measured budget value for a human and benchmark log.
/// A nonzero return makes `accept M2` fail when the native limit is missed.
pub(crate) fn bench() -> Result<(), String> {
    let micros = preparation_micros()?;
    println!("typical glyph preparation: {micros:.3} µs (budget <= 20 µs)");
    if micros <= 20.0 {
        Ok(())
    } else {
        Err(format!("{micros:.3} µs exceeds 20 µs"))
    }
}
