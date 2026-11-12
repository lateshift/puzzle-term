//! Incremental glyph-coverage acceptance in M11 task order (R14, R15).
//!
//! Each contract executes an exact test filter, which must run a real test.
//! Later task criteria are added with their implementations. A green result
//! covers only the implemented tasks; Chrome review is always a human STOP.
//!
//! Exact test filters use the shared M7 helper, which rejects zero-test runs.
//! Renaming a witness therefore fails rather than silently removing coverage.
//! Platform-neutral contracts run natively, then the full workspace gate checks
//! native and wasm builds, target dependency boundaries and source conventions.
//! Native Metal readback is a separate step because the CPU tests have no GPU.
//! Golden crops are candidates until the user's reference review is recorded.
/// Native strike/filter/codec and installed-font bitmap contracts.
mod bitmap;
/// Nonblocking discovery, bounded tofu and actual native Unicode submissions.
mod discovery;
/// Final native frame/bitmap measurements and the desktop review STOP.
mod measurements;
/// Raster memory, shipped dependencies and real Chrome witnesses.
mod raster;
/// Installed-family coverage, bounded discovery and native preparation budgets.
mod system;
use super::{m7::test, report};

/// Runs all implemented glyph contracts before citing confirmed human reviews.
/// The presentation table varies style as well as selectors, while the
/// source-invariance table compares the next ASCII cell after every glyph.
/// A separate complete parity table retains the measured repeated-selector
/// and unpaired-regional-indicator cases from D-31's terminal conformance.
pub(super) fn run(browser: super::BrowserGate) -> bool {
    let mut passed = true;
    // The full gate compiles neutral crates and the web shell for wasm too.
    report(
        "native and wasm check",
        crate::commands::check(),
        &mut passed,
    );
    // The cache witness includes a newly covered Nerd Font icon alongside
    // unaffected primary, CJK and emoji-first decisions to catch broad clears.
    for (label, name) in [
        (
            "M11.1 presentation and whole-cluster source ordering",
            "font_set::presentation_tests::presentation_source_table",
        ),
        (
            "M11.1 D-31 cell spans and source-invariant positions",
            "font_set::presentation_tests::source_cell_positions_table",
        ),
        (
            "M11.1 complete D-31 width table",
            "font_set::presentation_tests::d31_width_parity_table",
        ),
        (
            "M11.1 selective fallback-choice invalidation",
            "font_set::presentation_tests::new_face_invalidates_only_affected_choices",
        ),
        (
            "M11.1 inset, height, width and centering at DPR 1/2",
            "fitting::tests::device_fitting_table",
        ),
    ] {
        // A source may change ink, but each label pins an independent contract.
        report(label, test("puzzle-text", name), &mut passed);
    }
    color_contracts(&mut passed);
    // U2 schedules real PTY/socket witnesses beside the Chrome closure gates.
    super::bridge::closure(browser, &mut passed);
    raster::run(&mut passed, browser);
    bitmap::run(&mut passed);
    system::run(&mut passed);
    measurements::run(&mut passed);
    discovery::run(&mut passed);
    // Cite the supplied reviewer verdict; mechanical checks cannot certify comments.
    println!(
        "REVIEW M11 comment quality: presentation, raster eviction, COLR/paint.wgsl, browser fallback and fontique; Claude confirmed 2026-10-04; README.md / README.md#m11"
    );
    passed
}

/// Checks independent color semantics, GPU fidelity and memory incarnations.
///
/// COLRv1 graph traversal exercises brush types and frozen clip transforms.
/// Intentional upstream recursion cycles must return reports without publishing
/// partial layers. Palette tests change the supplied text color and require
/// exactly the foreground-sentinel fills to change, preserving CPAL colors.
/// A synthetic COLRv0 table reuses the approved outlines and palette, checking
/// that the older format enters the same consecutive-layer drawing surface.
///
/// The live backend contract checks more than raw font traversal: solids must
/// carry colors in instances and changing tint must reuse the prepared records
/// without allocating or uploading another row. Geometry/extend tests isolate
/// color-line edge cases such as negative offsets and duplicate hard stops.
///
/// Paint-store tests use an intentionally small allowance to force growth and
/// eviction, then compare restoration bytes with the retained CPU mirror.
/// The GPU probe separately recreates a real device; a fake reset could miss
/// stale bindings or a missing row upload even if pure memory tests passed.
/// Its golden comparison keeps the pre-existing global tolerance unchanged.
///
/// A fixture benchmark measures the outline-to-bands-to-texels preparation
/// boundary used by the original glyph budget. Font discovery and traversal
/// remain cold work outside that boundary, just as for the M2 benchmark.
/// Each report is independent, so one failure does not suppress later evidence.
fn color_contracts(passed: &mut bool) {
    // These criteria pin independent color semantics and memory incarnations.
    report(
        "M11.2 COLRv1 brushes, clips and malformed cycles",
        test("puzzle-text", "color::tests::colr_brush_and_clip_contract"),
        passed,
    );
    report(
        "M11.2 foreground sentinel preserves palette colors",
        test(
            "puzzle-text",
            "color::tests::foreground_only_palette_contract",
        ),
        passed,
    );
    report(
        "M11.2 COLRv0 ordered palette and foreground layers",
        test(
            "puzzle-text",
            "color::tests::colrv0_order_and_foreground_contract",
        ),
        passed,
    );
    report(
        "M11.2 live palette solids and allocation-free tint hits",
        test(
            "puzzle-text",
            "glyph::slug::color::tests::palette_solids_and_warm_tint_contract",
        ),
        passed,
    );
    paint_contracts(passed);
}

/// Separates texture lifetime and numerical paint contracts from font traversal.
fn paint_contracts(passed: &mut bool) {
    // Live insertion defers relocation until the damage snapshot can copy every row.
    report(
        "M11.2 live eviction precedes instance capture",
        test(
            "puzzle-slug",
            "paint::tests::live_eviction_precedes_instance_capture",
        ),
        passed,
    );
    report(
        "M11.2 CPU gradient geometry and extension",
        test(
            "puzzle-slug",
            "paint::tests::color_line_and_geometry_contract",
        ),
        passed,
    );
    report(
        "M11.2 paint growth, eviction and CPU restore",
        test(
            "puzzle-slug",
            "paint::tests::paint_growth_eviction_restore_contract",
        ),
        passed,
    );
    report(
        "M11.2 GPU gradients, layers, texture growth and actual device restore",
        crate::golden::color::run(false, false),
        passed,
    );
    report(
        "M11.2 COLR preparation median ≤20 µs",
        colr_budget().map(|_| ()),
        passed,
    );
}

/// Reads divan's actual COLR median rather than turning a recorded number into PASS.
///
/// The filter runs only the approved two-layer fixture preparation benchmark.
/// Divan reports fastest, slowest, median and mean in separate visual columns;
/// acceptance uses the median, matching the specification's microsecond budget.
/// Units are normalized explicitly because fast hosts may print nanoseconds.
/// An unknown unit or missing result is a failure, never an assumed pass.
/// Nonfinite values fail too: a malformed measurement cannot establish a bound.
/// The raw command output remains available through the existing capture layer.
/// Benchmark timings are remeasured each acceptance run and recorded in PROGRESS.
/// This does not certify appearance or browser behavior; those remain R15 review.
fn colr_budget() -> Result<f64, String> {
    let output = crate::commands::capture(
        "cargo",
        &[
            "bench",
            "-p",
            "puzzle-text",
            "--bench",
            "prepare",
            "--",
            "colr_two_layer_fixture",
        ],
    )?;
    let row = output
        .lines()
        .find(|line| line.contains("colr_two_layer_fixture"))
        .ok_or("COLR benchmark result missing")?;
    // Divan's columns are fastest, slowest, median, mean, samples, iterations.
    let median = row
        .split('│')
        .nth(2)
        .ok_or("COLR median column missing")?
        .trim();
    let value = median
        .split_whitespace()
        .next()
        .ok_or("COLR median value missing")?
        .parse::<f64>()
        .map_err(|e| e.to_string())?;
    let micros = if median.contains(" ns") {
        value / 1000.0
    } else if median.contains(" µs") {
        value
    } else if median.contains(" ms") {
        value * 1000.0
    } else {
        return Err(format!("unknown divan unit: {median}"));
    };
    if !micros.is_finite() || micros > 20.0 {
        return Err(format!("COLR preparation {micros:.3} µs exceeds 20 µs"));
    }
    Ok(micros)
}
