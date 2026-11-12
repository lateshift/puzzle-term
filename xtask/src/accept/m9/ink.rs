//! M9.2 independently verifies chunk bounds, neutral placement and bitmap quality.
//!
//! Every filter must execute its named public witness, including integration tests.
//! Chunked winding and AA are compared to an unsplit source at fractional seams.
//! Complete original curve counts normalize cold path-to-packed-texel timings.
//! A fresh complete primary-font calibration supplies the approved p95 limit.
//! Full Chrome suites run at both DPRs; source-size and filtered-edge witnesses
//! must each occur twice. No browser quality verdict follows from their status.
//! System-font images use a separate labeled review command, outside goldens.
//! Model-region DOM witnesses additionally cover tracked application mouse modes.
use super::super::{m7::test, report};

/// Checks every implemented ink obligation while retaining independent failures.
/// The bounds witness varies rotation, shear and device magnification.
/// A separate winding witness covers holes and fractional ownership edges.
/// The dense plot inspects actual band lists rather than diagnostic curve counts.
/// Session effects compare neutral geometry before applying any ink transform.
/// Removing effects restores the ordinary row cache byte for byte.
/// The hidden variant retains BufferView text and warms reusable allocations.
/// Free annotations explicitly select the approved COLRv1 face amid PUA collisions.
/// Paint variants preserve dynamic foreground stops for selection and cursor tint.
/// Palette stops receive opacity in the row, without changing their geometry.
/// All bundled reference scenes are rerendered before the pixel comparator runs.
/// Native system-font images are checked separately and never become pixel goldens.
pub(super) fn run(passed: &mut bool, gate: super::super::BrowserGate) {
    for name in [
        "chunk_limits_hold_under_zoom_rotation_and_shear",
        "guarded_chunks_preserve_holes_curves_and_fractional_translation",
        "dense_plot_keeps_every_actual_band_bounded",
    ] {
        // Cargo's empty-filter success cannot establish a removed chunk contract.
        report(&format!("M9.2 {name}"), chunk(name), passed);
    }
    for (package, name) in [
        (
            "puzzle-term",
            "session::tests::overlays::effects::effects_use_unhinted_ink_and_restore_neutral_grid",
        ),
        (
            "puzzle-term",
            "session::tests::overlays::effects::hidden_effect_preserves_text_and_warm_variants_allocate_nothing",
        ),
        (
            "puzzle-text",
            "layout::free::tests::explicit_colr_face_shapes_and_prepares_real_color_layers",
        ),
        (
            "puzzle-slug",
            "paint::tests::effect_alpha_preserves_foreground_sentinels_and_palette_stops",
        ),
    ] {
        // Provider effects must not move cells or lose dynamic selection/cursor tint.
        report(&format!("M9.2 {name}"), test(package, name), passed);
    }
    report(
        "M9.2 preparation per original curve ≤ fresh primary p95",
        budget(),
        passed,
    );
    report(
        "M9.2 bundled shape/text/effect DPR references",
        crate::golden::overlays::run(false, false),
        passed,
    );
    report(
        "M9.2 separate machine-specific CJK/sbix ink and effective strikes",
        crate::commands::quiet(
            "cargo",
            &["run", "-p", "puzzle-desktop", "--", "overlay-review"],
        ),
        passed,
    );
    browser(passed, gate);
    println!(
        "REVIEW M9.2 references and machine-specific Heiti/Apple Color Emoji: pending; assets/scenarios/overlay-*; target/m9-review; README.md"
    );
}

/// Executes an exact integration name and verifies the runner's positive witness.
/// Integration tests use their own Cargo target, rather than the library filter.
/// An exact test name protects the acceptance obligation from accidental removal.
/// The output line must include success for this name, not merely an exit code.
/// Failures return captured diagnostics through the shared command runner.
/// No property case or numerical seam tolerance is rewritten by this helper.
fn chunk(name: &str) -> Result<(), String> {
    let output = crate::commands::capture(
        "cargo",
        &[
            "test",
            "-p",
            "puzzle-slug",
            "--test",
            "chunks",
            name,
            "--",
            "--exact",
        ],
    )?;
    // Test removal or renaming yields no valid success line, even with exit zero.
    if !output.contains(&format!("test {name} ... ok")) {
        return Err(format!("chunk witness {name} did not execute"));
    }
    Ok(())
}

/// Measures all cold subdivisions and packing rather than accepting a stored number.
/// The probe calibrates every drawable primary glyph before timing the dense shape.
/// Stroke expansion matches outline extraction and happens outside preparation.
/// Timed conversion, clipping, band creation and packing all remain included.
/// The original quadratic count prevents duplicated guards diluting the cost.
/// The probe retains all samples in CSV and rejects a nonfinite measurement.
/// Its exit status enforces the fresh primary p95, without a fixed shape ceiling.
/// Captured output gives the review a reproducible measured comparison.
fn budget() -> Result<(), String> {
    let output = crate::commands::capture(
        "cargo",
        &[
            "run",
            "--release",
            "-p",
            "puzzle-desktop",
            "--",
            "overlay-bench",
        ],
    )?;
    // The probe rejects its measured median above the fresh complete primary p95.
    std::fs::write("target/m9-shape-accept.log", &output).map_err(|e| e.to_string())?;
    let row = output
        .lines()
        .find(|line| line.starts_with("overlay_prepare "))
        .ok_or("shape measurement absent")?;
    println!("M9.2 {row}");
    Ok(())
}

/// Both actual source-size and transparent-border sampling run in the full suite.
/// Chrome uses the existing served fonts and local bridge at both device scales.
/// The zoom witness reads atlas UV dimensions, not destination quad dimensions.
/// The rotated-border witness uses the production shader and poisoned transparent RGB.
/// An independent coverage oracle catches straight-alpha interpolation fringes.
/// Every required name must succeed twice, once in each complete DPR suite.
/// Full-suite failure propagates even when an individual effect test passed.
fn browser(passed: &mut bool, gate: super::super::BrowserGate) {
    let output = gate.suite();
    gate.report_suite(
        "M9.2 complete Chrome suite DPR 1/2",
        &output,
        |_| Ok(()),
        passed,
    );
    for name in [
        "fallback::effects::zoomed_fallback_redraws_at_effective_device_size",
        "shaders::effects::rotated_bitmap_filters_premultiplied_border",
        "overlays::overlay_regions_consume_dom_clicks_and_set_hover_cursor",
        "overlays::models::scrollbar_and_pill_dom_clicks_consume_tracking_and_selection",
    ] {
        // A failed suite fails the criterion even if this individual name passed.
        let result = |text: &str| {
            let witness = format!("test runtime::tests::{name} ... ok");
            if text.matches(&witness).count() == 2 {
                Ok(())
            } else {
                Err(format!("two-DPR witness missing: {name}"))
            }
        };
        let task = if name.starts_with("overlays::models::") {
            "M9.4"
        } else if name.starts_with("overlays::") {
            "M9.3"
        } else {
            "M9.2"
        };
        gate.report_suite(&format!("{task} {name}"), &output, result, passed);
    }
}
