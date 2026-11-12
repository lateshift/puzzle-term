//! Installed emoji preparation has pixels rather than quadratic curves.
//!
//! The user's primary-median/per-curve rule cannot normalize bitmap cost by
//! a nonexistent curve count. M11.7 records these costs without a new ceiling.
//! Native enumeration is warmed once; successful faces stay mapped while each trial shapes the complete cluster, selects/decodes its strike and area-fits
//! premultiplied pixels. Atlas lookup and upload are deliberately outside here.
//! This is an inclusive source preparation cost, not a PNG-only microbenchmark.
//! Every sample survives in CSV, including OS scheduling interruptions.
//! Two DPRs select different strikes and target sizes. Both the real crab and
//! the presentation-selected victory hand exercise installed bitmap output.
//! No system font or generated bitmap is vendored for these measurements.
//! Loaded-face owners are bounded independently of the prepared samples.
//! Each timed result is inspected and retained in the ordinary evidence boundary.
use super::super::NativeFonts;
use puzzle_term::text::{
    cluster_source::{ClusterGlyph, ClusterRequest},
    font_set::FontStyle,
    presentation::Presentation,
};
/// Times uncached whole-cluster bitmap preparation after discovery is initialized.
pub(crate) fn run() -> Result<(), String> {
    let mut csv = String::from("cluster,dpr,sample,prepare_us\n");
    for dpr in [1.0, 2.0] {
        let context = puzzle_term::frame_builder::context::TextContext::new(
            crate::font_data::bundled_font_set()?,
            14.0,
            dpr,
        )?;
        // Separate scale-bound primary geometry determines each bitmap fit and strike size.
        let mut source = NativeFonts::new(vec![], true, true, context.metrics());
        for text in ["🦀", "✌️"] {
            cluster(&mut source, dpr, text, &mut csv)?;
        }
    }
    // Diagnostic results are ignored assets and cannot enter the distributed build.
    std::fs::write(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/m11-bitmap-preparation.csv"),
        csv,
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// Preserves uncached source work and all samples for one cluster at one display scale.
fn cluster(
    source: &mut NativeFonts,
    dpr: f32,
    cluster: &str,
    csv: &mut String,
) -> Result<(), String> {
    let r = ClusterRequest {
        cluster,
        style: FontStyle::Regular,
        presentation: Presentation::Emoji,
    };
    // Indexing is distinct from preparation. The first decoded glyph remains
    // a retained timing sample rather than discarded coverage-validation work.
    source.ensure_index();
    // The placement supplies the same primary baseline and inset as an actual row.
    let p = source.probe_placement();
    let mut samples = Vec::with_capacity(31);
    let mut dimensions = [0, 0];
    for sample in 0..31 {
        // This inclusive boundary starts before mapping, shaping and PNG decode.
        let start = std::time::Instant::now();
        let ink = source.resolve(r, &p).ok_or("bitmap source vanished")?;
        // Result inspection and evidence formatting stay outside the measured interval.
        let us = start.elapsed().as_secs_f64() * 1_000_000.0;
        // A fallback outline cannot masquerade as bitmap preparation evidence.
        let ClusterGlyph::Bitmap(image) = ink else {
            return Err("emoji was not a bitmap".into());
        };
        // The fitted target dimensions show which device-sized result was measured.
        dimensions = [image.width, image.height];
        std::hint::black_box(&image.pixels);
        // Keep the original sample order in evidence, independent of median sorting.
        samples.push(us);
        csv.push_str(&format!("{cluster},{dpr},{sample},{us:.6}\n"));
    }
    // Sort only for the displayed median; all unsorted samples remain in CSV.
    samples.sort_by(f64::total_cmp);
    println!(
        "fallback_bitmap cluster={cluster} dpr={dpr} median_us={:.3} width={} height={} samples=31 scope=map_shape_decode_fit",
        samples[15], dimensions[0], dimensions[1]
    );
    Ok(())
}
