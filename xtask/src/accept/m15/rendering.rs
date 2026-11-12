//! Sealed placement metadata and actual raster readbacks prove M15.3 independently.
//!
//! Native GPU features come from puzzle-embed just as they do in workspace tests.
//! Missing exact test names fail instead of accepting Cargo's empty filter success.
//!
//! Protocol success alone cannot prove source ink. Core tests retain immutable
//! generations across deletion and derive geometry at two DPRs; native readback
//! tests then validate alpha and LRU against actual raster pixels. The six scene
//! cases additionally exercise PNG decode, clipping and reflow end to end.
//! Each line executes its named witness, and a failure does not suppress later
//! criteria. Human approval of the new references remains a separate REVIEW.
use crate::accept::{m7, report};
/// Image-free runs do not create GPU images; visible sources exercise real sampling.
pub(super) fn run(passed: &mut bool) {
    // Test names are module-qualified to reject accidental duplicate-name matches.
    for (name, test) in [
        (
            "M15.3 immutable placements and three image z layers",
            "published_images_seal_sources_and_order_all_three_layers",
        ),
        (
            "M15.3 warm placement copies allocate nothing and idle has no frame",
            "image_publications_are_idle_and_warm_captures_allocate_nothing",
        ),
        (
            "M15.3 current DPR/font cell anchors preserve natural pixels",
            "image_anchors_follow_current_font_metrics_and_dpr",
        ),
    ] {
        // The common exact-test runner rejects a removed or renamed witness.
        report(
            name,
            m7::test("puzzle-term", &format!("session::tests::images::{test}")),
            passed,
        );
    }
    for test in [
        "hardware_borders_are_charged_before_protocol_success",
        "admission_never_displaces_other_visible_images",
    ] {
        // Admission and upload share the exact device partition, not separate estimates.
        // Raw dimensions fail before decoding; PNG dimensions fail before source commit.
        // Neither error is allowed to remove an already visible source generation.
        report(
            &format!("M15.3 {test}"),
            m7::test("puzzle-term", &format!("images::tests::admission::{test}")),
            passed,
        );
    }
    // The readback witness includes both cold upload and warmed source reuse.
    // Hardware-sized sources cross a real tile seam, including bilinear border sampling.
    report(
        "M15.3 lazy image pixels, alpha, bounded LRU and recovery",
        gpu(),
        passed,
    );
    // Reference comparison keeps the earlier tolerance and never blesses on acceptance.
    report(
        "M15.3 six raster golden cases at DPR 1 and 2",
        crate::golden::images::run(false, false),
        passed,
    );
    // Mechanical success leaves the exact twelve new crops pending human approval.
    println!(
        "REVIEW M15.3 golden references: assets/scenarios/images/, pending user review; docs/history/M15.md"
    );
}
/// The existing native owner enables the shared renderer's Metal/WGSL features.
fn gpu() -> Result<(), String> {
    // This lifecycle witness also samples a hardware tile seam after scaled cropping.
    let name = "images::tests::images_use_raster_pixels_and_independent_bounded_residency";
    // Include a native owner to select its approved wgpu backends without a manifest change.
    // Execute the fixture through Cargo so test-profile GPU assertions stay enabled.
    let output = crate::commands::capture(
        "cargo",
        &[
            "test",
            "-p",
            "puzzle-render",
            "-p",
            "puzzle-embed",
            "--lib",
            name,
            "--",
            "--exact",
        ],
    )?;
    // Cargo exits successfully for empty filters, which would not prove these pixels.
    if !output.contains(&format!("test {name} ... ok")) {
        return Err("GPU image witness did not run".into());
    }
    // The output transcript supplies proof that the GPU fixture actually executed.
    Ok(())
}
