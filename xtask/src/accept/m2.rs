//! M2 acceptance inventory for the CPU Slug pipeline (design §17, R14).
//!
//! Every property is run by name so later failures cannot conceal which
//! packing or oracle invariant broke. The specimen and its review steps are
//! reported separately from mechanical assertions (R15).
//!
//! M2 spans three seams: kurbo paths become Slug quadratics and bands;
//! those lists and curves become bounded two-texture blocks; then the CPU
//! oracle reads those exact blocks with the reference shader's math. Tests
//! at each seam identify whether a regression is geometric, packed-data
//! related or in coverage. The workspace gate still runs first to catch
//! warnings, wasm boundary violations and comment-rule failures.
//!
//! The rectangle truth tests distinguish one-edge pixels, which Slug can
//! integrate exactly, from corners, where its two directional rays are an
//! approximation. The centre and nonmonotonic fixtures protect the final
//! reference-derived criterion against future accidental "improvements".
//! Quarter-turn symmetry and fill-rule tests cover properties that a
//! rectangle alone cannot expose.
//!
//! Real font bytes and license files are deliverables because a synthetic
//! PNG would not reveal text quality at 10–13 device pixels. The specimen
//! command rejects an all-background image before writing its PNG. The
//! glyph benchmark runs in release mode because design §15's native budget
//! measures optimized preparation, not debug instrumentation overhead.
//!
//! Human review remains separate: sharpness, stem weight and artifacts
//! require eyes, while comment quality is an editorial R1 check. Their
//! evidence lines print even when a mechanical criterion fails (R15).
use super::report;
use std::{fs, path::Path};

/// Runs a named geometry or oracle property through the real crate tests.
/// Cargo output is captured so one criterion stays on one result line.
fn slug_test(name: &str) -> Result<(), String> {
    crate::commands::quiet("cargo", &["test", "-p", "puzzle-slug", name])
}

/// Combines the fixed inside/outside fixture with random single-edge pixels.
fn analytic_edges() -> Result<(), String> {
    slug_test("fractional_rectangle_classifies_pixel_coverage")?;
    slug_test("single_edge_pixels_match_fractional_area")
}

/// Requires all approved font and license assets before using the specimen.
/// Absence is distinct from a blank but successfully encoded specimen.
fn fonts_present() -> Result<(), String> {
    for path in [
        // Regular and italic variable faces are needed for the later M4
        // style paths even though M2's specimen uses the regular face.
        "assets/fonts/JetBrainsMono-Variable.ttf",
        "assets/fonts/JetBrainsMono-Italic-Variable.ttf",
        "assets/fonts/JetBrainsMono-OFL.txt",
        "assets/fonts/SymbolsNerdFontMono-Regular.ttf",
        "assets/fonts/NerdFontsSymbolsOnly-LICENSE",
        "assets/fonts/NerdFontsSymbolsOnly-README.md",
        "assets/fonts/README.md",
        "crates/puzzle-slug/reference/SlugPixelShader.hlsl",
        "crates/puzzle-slug/reference/SlugVertexShader.hlsl",
        "crates/puzzle-slug/reference/LICENSE",
    ] {
        // The reference source and its terms ship together for R11 credit.
        if !Path::new(path).is_file() {
            return Err(format!("missing approved asset: {path}"));
        }
    }
    Ok(())
}

/// Renders a fresh real-font sheet; the command checks ink before writing.
/// Metadata is read only after that command succeeds, preventing stale files
/// left by an earlier passing run from satisfying the current attempt.
fn specimen() -> Result<(), String> {
    crate::commands::quiet("cargo", &["xtask", "slug-dump"])?;
    let metadata = fs::metadata("target/slug/specimen.png").map_err(|error| error.to_string())?;
    if metadata.len() < 100 {
        return Err("specimen PNG is too short".into());
    }
    Ok(())
}

/// Measures real-glyph preparation in release mode against the 20 µs budget.
/// The child exits nonzero itself when its measured median exceeds the cap.
fn real_glyph_budget() -> Result<(), String> {
    crate::commands::quiet(
        "cargo",
        &["run", "--release", "-p", "xtask", "--", "slug-bench"],
    )
}

/// Keeps independent property failures visible after the shared gate.
/// Each result maps to one concrete claim from the M2 acceptance row.
fn report_properties(passed: &mut bool) {
    for (criterion, test) in [
        // Conversion and band assignment are tested before packed texture
        // invariants because later oracle failures often originate there.
        ("kurbo cubic conversion and path closure", "shape::tests"),
        (
            "band overlap, exclusion, epsilon and descending order",
            "bands::tests",
        ),
        (
            "C1, B1, B2 packing, sharing and page eviction",
            "atlas::tests",
        ),
        (
            "corner approximation bound",
            "fractional_corners_stay_within_reference_bound",
        ),
        (
            "circle and square quarter-turn symmetry",
            "circle_and_square_have_ninety_degree_symmetry",
        ),
        (
            "nonzero and even-odd fill rules",
            "overlapping_contours_obey_both_fill_rules",
        ),
    ] {
        // Running focused tests continues after failures. A whole workspace
        // test failure still leaves precise evidence in these lines.
        report(criterion, slug_test(test), passed);
    }
    report(
        "single-edge and inside/outside analytic coverage",
        analytic_edges(),
        passed,
    );
    // These counterexamples were discovered while reconciling R11 with
    // analytic area; retaining them prevents a future criterion regression.
    report(
        "exact-centre corner regression",
        slug_test("pixel_centered_corner_has_maximum_reference_error"),
        passed,
    );
    report(
        "nonmonotonic corner regression",
        slug_test("nonmonotonic_corner_still_respects_area_error_bound"),
        passed,
    );
}

/// Reports every M2 criterion even when an earlier check fails.
/// The return value controls the xtask process exit, after review lines.
pub(super) fn run() -> bool {
    let mut passed = true;
    report(
        "native and wasm check",
        crate::commands::check(),
        &mut passed,
    );
    report_properties(&mut passed);
    report(
        "reference and font licenses are bundled",
        fonts_present(),
        &mut passed,
    );
    report(
        "real JetBrains specimen PNG contains oracle ink",
        specimen(),
        &mut passed,
    );
    report(
        "real glyph preparation <= 20 us",
        real_glyph_budget(),
        &mut passed,
    );
    println!("REVIEW text quality probe: target/slug/specimen.png; README.md review steps");
    println!(
        "REVIEW comment quality: crates/puzzle-slug/src/; crates/puzzle-text/src/probe.rs; xtask/src/slug/; README.md"
    );
    passed
}
