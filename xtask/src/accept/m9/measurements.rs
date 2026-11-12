//! M9.7 measures real typical painters, idle ownership and shipped size.
//!
//! Timing runs optimized and retains every sample. Debug checks retain the
//! functional zero-allocation contract without asserting scheduler speed.
//! Human image comparison remains separate from these mechanical receipts.
//!
//! The typical scene combines Session-owned search and scrollbar sources with
//! parsed OSC command zones and declarative annotations. Invalidating all its
//! providers prevents cached draw-list reuse from understating build cost.
//! The phase clock excludes row shaping: chunk preparation has its own M9.2
//! primary-font calibration, and dirty row assembly has the §15 frame budgets.
//! Both density scales use the same sources and immutable publication path.
//!
//! The shipped-size row uses Trunk, including wasm-bindgen and wasm-opt, rather
//! than Cargo's intermediate wasm. Fonts remain separate distribution assets.
//! Compression counts complete output streams, including their headers. It
//! neither switches optimization profiles nor edits the dependency feature set.
//! The 1 MB target is informational; the existing 1.5 MB limit remains binding.
//!
//! Golden scenes compare previously recorded candidates; harness-only review
//! scenes still execute their authored ink assertions without inventing a
//! reference. Neither route grants visual approval. The review STOP lists the
//! actual image paths and the missing real-terminal comparison separately.
use super::super::{m7, report};
use std::fs;

/// Runs the release-only phase gate and exposes its freshly written summaries.
fn overlay_cost() -> Result<(), String> {
    let name = "session::tests::overlay_budget::typical_overlay_build_budget";
    // The public FrameRendered clock measures overlay compilation separately
    // from row layout. Every typical provider is invalidated before each sample.
    let output = crate::commands::capture(
        "cargo",
        &[
            "test",
            "--release",
            "-p",
            "puzzle-term",
            name,
            "--",
            "--exact",
        ],
    )?;
    // Cargo returns success for an empty filter, so require this named execution.
    if !output.contains(&format!("test {name} ... ok")) {
        return Err("optimized typical overlay witness did not execute".into());
    }
    // Summary files are written by this exact execution, never a stored baseline.
    for dpr in [1, 2] {
        let summary = fs::read_to_string(format!("target/m9-overlay-budget/dpr{dpr}.txt"))
            .map_err(|e| e.to_string())?;
        println!("M9_METRIC overlay {}", summary.trim());
    }
    Ok(())
}

/// Measures the default distribution; no benchmark feature enters this build.
fn shipped() -> Result<(), String> {
    crate::commands::web(Some("build"))?;
    // Trunk names artifacts by content hash. Require one wasm rather than
    // accidentally reporting a stale or development artifact from another build.
    let mut files = Vec::new();
    for entry in fs::read_dir("dist/web").map_err(|e| e.to_string())? {
        let path = entry.map_err(|e| e.to_string())?.path();
        if path.extension().is_some_and(|ext| ext == "wasm") {
            files.push(path);
        }
    }
    let [path] = files.as_slice() else {
        return Err("expected exactly one shipped wasm artifact".into());
    };
    let raw = fs::metadata(path).map_err(|e| e.to_string())?.len();
    let path = path.to_str().ok_or("non-UTF-8 shipped wasm path")?;
    // Decimal MB matches §15.2. Target overshoot is reported, never optimized
    // away here; only the unchanged hard limit requires a user decision.
    let gzip = crate::commands::compressed("gzip", &["-c", path])?;
    let brotli = crate::commands::compressed("brotli", &["-c", path])?;
    let summary = format!("raw={raw} gzip={gzip} brotli={brotli} target=1000000 hard=1500000\n");
    fs::write("target/m9-shipped-size.txt", &summary).map_err(|e| e.to_string())?;
    println!("M9_METRIC shipped {}", summary.trim());
    if brotli > 1_500_000 {
        return Err(format!("shipped wasm {brotli} B exceeds 1.5 MB hard limit"));
    }
    Ok(())
}

/// Retains every script's result even when the aggregate criterion succeeds.
fn harness() -> Result<(), String> {
    let result = crate::commands::capture("cargo", &["xtask", "harness", "--all", "--check"]);
    // The command runner includes stdout/stderr in errors. Preserve that full
    // failure too, so a later rerun cannot erase which script originally failed.
    let text = match &result {
        Ok(output) => output,
        Err(error) => error,
    };
    fs::write("target/m9-harness-check.log", text).map_err(|e| e.to_string())?;
    result.map(|_| ())
}

/// Every criterion accumulates independently, including all harness regressions.
pub(super) fn run(passed: &mut bool) {
    report(
        "M9.7 typical installed overlays idle: zero work and allocations",
        m7::test(
            "puzzle-term",
            "session::tests::overlay_budget::typical_overlays_idle_zero_work_and_allocations",
        ),
        passed,
    );
    report(
        "M9.7 typical overlay build <=0.5 ms DPR 1/2",
        overlay_cost(),
        passed,
    );
    report(
        "M9.7 shipped wasm size (target report; 1.5 MB hard)",
        shipped(),
        passed,
    );
    // Golden comparisons preserve established tolerances and never bless images.
    report(
        "M9.7 all GPU golden references",
        crate::golden::run_all(false, false),
        passed,
    );
    report(
        "M9.7 complete harness scenarios --all --check",
        harness(),
        passed,
    );
    println!(
        "REVIEW M9.7 text quality: target/harness/text-quality; real reference comparison pending; docs/history/M9.md"
    );
}
