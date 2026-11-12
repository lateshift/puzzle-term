//! D-47 measures conversion, packing and coverage separately for CPU and GPU owners.
//!
//! Paths are extracted, reduced and banded before timing, outside the §15 outline gate.
//! Every repetition prepares new glyph keys; cached lookup cannot satisfy this probe.
//! GPU samples include batched Slug rendering and atlas copies, with no readback.
//! A batch has 128 cold glyphs, amortizing queue work as a changed Unicode frame does.
//! The first sample retains lazy shader compilation rather than hiding startup work.
//! CPU samples evaluate the same Slug oracle and include mask byte production.
//! Both paths report cost per glyph, original quadratic and device sample.
//! Queue CPU time and completed GPU wall time are distinct recorded observations.
//! The completion wait observes submission without mapping or copying pixels to CPU.
//! A first-use sample includes lazy shader compilation. Later samples retain resources.
//! CPU sampling includes the isolated oracle pack and coverage bytes; GPU sampling
//! includes scratch pack, staging draw and final sampled-atlas copies. These stages
//! scale with pixel area and are separate from outline-to-bands preparation.
//! All sample timings remain in target; a median does not erase cold outliers.
//! No new ceiling is introduced and no font or third-party fixture is bundled.
//!
//! This developer probe excludes source discovery and outline conversion from
//! the raster clock. Those remain covered by the separate primary preparation
//! calibration. Coverage sampling depends on the crop's physical pixel area,
//! so each raw row records that area alongside its original quadratic count.
//! GPU enqueue time includes packing, draw submission and atlas copy encoding.
//! Completed wall time additionally waits for that batch's device work; it is
//! not reported as an isolated shader timestamp or an exclusive GPU duration.
//! CPU completion includes oracle evaluation and owned coverage byte creation.
//! Both paths use the same fitted placement, font size, source outline and DPR.
//! Each sample uses fresh glyph identities but retains its producer's resources.
//! This distinguishes cold glyph cost from shader compilation and warm cache hits.
//! The benchmark's completion fence belongs to the measurement itself; production
//! frames never wait for these masks on the CPU. Their uploads and draws use queue
//! ordering. The CSV reports both fence-inclusive wall time and enqueue CPU time
//! so neither can be mistaken for an exclusive GPU shader duration.
use puzzle_render::Renderer;
use puzzle_term::frame_builder::context::TextContext;
/// Reusable cold-batch owners measure enqueue/completed cost outside geometry conversion.
mod batch;
use batch::samples;
use puzzle_term::text::{
    cluster_source::{ClusterGlyph, ClusterRequest},
    font_set::FontStyle,
    glyph::oracle::OracleRasterizer,
    preparation::Outline,
    presentation::Presentation,
    raster::{RasterAtlas, RasterKey},
};

/// Runs both raster producers with identical installed-font outlines and device geometry.
pub(crate) fn run() -> Result<(), String> {
    let primary = crate::system_fonts::bench::calibrate_primary()?;
    // Reuse one native device across every glyph, with no window/compositor in this probe.
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let renderer =
        crate::block_on::block_on(Renderer::new(&instance, None)).map_err(|e| e.to_string())?;
    let mut rows = String::from(
        "glyph,dpr,path,curves,sample,device_pixels,queue_us_per_glyph,completed_us_per_glyph,us_per_curve,us_per_pixel\n",
    );
    for dpr in [1.0, 2.0] {
        // Font fitting is device-specific even though outline geometry is size independent.
        let context = TextContext::new(crate::font_data::bundled_font_set()?, 14.0, dpr)?;
        let mut source = crate::system_fonts::NativeFonts::new(
            vec!["Heiti SC".into()],
            false,
            true,
            context.metrics(),
        );
        for cluster in ["漢", "字", "鬱", "語", "한", "글", "뿔", "𝄞"] {
            // Native source discovery finishes before any raster sample clock starts.
            let p = source.probe_placement();
            let path = resolve(&mut source, cluster, &p)?;
            let curves = Outline::new(path.clone())?
                .ok_or("empty outline")?
                .shape
                .curves
                .len();
            record(
                &renderer,
                &p,
                &path,
                (cluster, dpr, curves, primary),
                &mut rows,
            )?;
        }
    }
    std::fs::write("target/m16-4/raster-cost.csv", rows).map_err(|e| e.to_string())?;
    renderer.device().destroy(); // Release the measurement device before another GPU cohort.
    Ok(())
}

/// Records every chronological raster sample and reports the median without discarding startup.
fn record(
    renderer: &Renderer,
    p: &puzzle_term::text::glyph::GlyphPlacement,
    path: &puzzle_term::text::geometry::BezPath,
    (cluster, dpr, curves, primary): (&str, f32, usize, f64),
    rows: &mut String,
) -> Result<(), String> {
    for gpu in [false, true] {
        // Alternating implementations here retains each path's own cold first sample.
        let (times, pixels, queued) = samples(renderer, p, path, gpu)?;
        let mut sorted = times.clone();
        sorted.sort_by(f64::total_cmp); // Sorting a copy preserves chronological raw rows.
        let label = if gpu { "gpu" } else { "cpu" };
        for (i, time) in times.iter().enumerate() {
            // Every sample is retained; medians never remove the startup outlier.
            rows.push_str(&format!(
                "{cluster},{dpr},{label},{curves},{i},{pixels},{:.6},{time:.6},{:.6},{:.6}\n",
                queued[i],
                time / curves as f64,
                time / f64::from(pixels) // Stage-normalized cost.
            ));
        }
        println!(
            "grid_raster glyph={cluster} dpr={dpr} path={label} curves={curves} median_us={:.3} us_per_curve={:.6} us_per_pixel={:.6} pixels={pixels} primary_p95={primary:.6} first_us={:.3}",
            sorted[10],
            sorted[10] / curves as f64,
            sorted[10] / f64::from(pixels),
            times[0]
        );
    }
    Ok(())
}

/// Finishes native source extraction outside the timed raster stage.
fn resolve(
    source: &mut crate::system_fonts::NativeFonts,
    cluster: &str,
    p: &puzzle_term::text::glyph::GlyphPlacement,
) -> Result<puzzle_term::text::geometry::BezPath, String> {
    let ink = source
        .resolve(
            ClusterRequest {
                cluster,
                style: FontStyle::Regular,
                presentation: Presentation::Text,
            },
            p,
        )
        .ok_or("benchmark outline unavailable")?;
    // Only outline-backed glyphs can compare the oracle and analytic GPU paths.
    let path = match ink {
        ClusterGlyph::Outline(path) | ClusterGlyph::StyledOutline { path, .. } => path,
        _ => return Err("benchmark glyph has no outline".into()),
    };
    Ok(path)
}
