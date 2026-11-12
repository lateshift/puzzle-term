//! M11.3 contracts distinguish memory lifetimes from submitted browser pixels.
//!
//! A deliberately tiny allowance forces a page to be reused before its address
//! can escape into the next frame. Generation changes protect every row cache.
//! The LRU witness keeps an older page visibly drawn so insertion age cannot
//! masquerade as recent use. Restoration compares full CPU pages and gutters.
//! Invalid dimensions and unsupported formats must fail without reserving bytes.
//!
//! Chrome runs the complete existing suite at both device scales. A successful
//! isolated glyph test cannot conceal regressions in transports or selection.
//! Each new criterion also requires its exact successful name at both scales.
//! The fallback witness uses public write, real served fonts and actual submitted
//! canvas pixels. It requires authored emoji color and independently tinted CJK.
//! A hidden prepared frame retains uploads until its next real presentation.
//! Recreated texture storage is filled from CPU mirrors without glyph preparation.
//! The array witness grows both formats, then redraws page zero with no uploads.
//!
//! A separate production Trunk build reports the actual shipped engine size.
//! That build excludes wasm test code, fonts used only by tests and dev probes.
//! Size is observational here: target overruns are reported under §15, not tuned.
//! The resolved wasm tree independently excludes desktop PNG and font discovery.
//! Neither a cached dependency note nor a recorded size is used as a passing test.
//! Human appearance, prompt and real-app verdicts remain at the M11.4 STOP.
use super::{report, test};

/// Executes every raster criterion even if an earlier independent one fails.
pub(super) fn run(passed: &mut bool, gate: super::super::BrowserGate) {
    for (label, name) in [
        (
            "small-budget eviction, generation, CPU restore and warm allocations",
            "raster::tests::eviction_generation_and_cpu_restore_contract",
        ),
        (
            "least-recently-drawn page eviction",
            "raster::tests::least_recently_drawn_page_is_evicted",
        ),
        (
            "invalid bitmap dimensions and formats return reports",
            "raster::tests::invalid_bitmap_inputs_return_reports",
        ),
    ] {
        // Exact Cargo filters must execute a witness, never a zero-test result.
        report(&format!("M11.3 {label}"), test("puzzle-text", name), passed);
    }
    browser(passed, gate);
    report(
        "M11.3 wasm excludes desktop png and fontique",
        dependencies(),
        passed,
    );
    report(
        "M11.3 shipped wasm size recorded (report only)",
        shipped(),
        passed,
    );
    println!(
        "REVIEW M11.4 Chrome: prompt, emoji sequences, CJK/symbols, tmux/nvim and 1×/2×; README.md user confirmed 2026-10-04; overall verdict, without individual DPR claims"
    );
}

/// Requires each browser witness to execute successfully at both device scales.
///
/// The complete suite protects earlier terminal contracts as well as raster ink.
/// Exact names avoid counting unrelated tests or trusting a total test count.
/// Failed output remains available to every criterion, including its diagnostics.
/// No Safari or Firefox process is launched: D-37 limits this review to Chrome.
/// Canvas restoration checks texture uploads; automatic device recovery is M14.
/// Submitted pixels are read before browser composition discards the drawing buffer.
/// Reference appearance is never inferred from these mechanical pixel guards.
fn browser(passed: &mut bool, gate: super::super::BrowserGate) {
    // D-46 keeps the same labels when a desktop task defers this suite to closure.
    let output = gate.suite();
    gate.report_suite(
        "M11.3 complete Chrome suite at DPR 1 and 2",
        &output,
        |_| Ok(()),
        passed,
    );
    for (label, name) in [
        (
            "fallback cells, color/mask tint, hidden uploads and CPU restore",
            "fallback::fallback_cells_color_masks_and_hidden_uploads",
        ),
        (
            "GPU array growth preserves existing raster pages",
            "shaders::pipelines_validate_and_raster_glyph_draws_ink",
        ),
        (
            "Unicode flood submissions ≥30 fps, child key ≤50 ms and progressive fallback",
            "unicode_flood::unicode_flood_presents_and_key_reaches_child",
        ),
    ] {
        // Full-suite failure fails these labels even if a single name printed ok.
        let result = |text: &str| {
            let witness = format!("test runtime::tests::{name} ... ok");
            if text.matches(&witness).count() == 2 {
                Ok(())
            } else {
                Err(format!("two-DPR witness missing: {name}"))
            }
        };
        gate.report_suite(&format!("M11.3 {label}"), &output, result, passed);
    }
}

/// Checks the resolved target tree, including transitive desktop dependencies.
fn dependencies() -> Result<(), String> {
    let tree = crate::commands::capture(
        "cargo",
        &[
            "tree",
            "-p",
            "puzzle-web",
            "--target",
            "wasm32-unknown-unknown",
            "--format",
            "{p}",
            "--prefix",
            "none",
        ],
    )?;
    // Package-name boundaries avoid accidental matches in unrelated paths.
    if tree
        .lines()
        .any(|line| line.starts_with("png ") || line.starts_with("fontique "))
    {
        return Err("desktop raster/discovery dependency entered wasm".into());
    }
    Ok(())
}

/// Measures the full production build and preserves its exact compressor counts.
fn shipped() -> Result<(), String> {
    let output = crate::commands::capture("cargo", &["xtask", "web", "build"])?;
    let row = output
        .lines()
        .find(|line| line.starts_with("wasm bytes:"))
        .ok_or("shipped wasm size report missing")?;
    // Keep build evidence outside commits while exposing only decisive numbers.
    std::fs::write("target/m11-raster-shipped-build.log", &output).map_err(|e| e.to_string())?;
    println!("M11.3 shipped {row}");
    Ok(())
}
