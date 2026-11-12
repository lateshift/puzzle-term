//! M5.8 native parser budgets and the release browser-size measurement.
//!
//! Each rate comes from a fresh five-sample median of an 8 MiB end-to-end
//! `Terminal::advance` feed. The native examples include parser callbacks,
//! grid edits and scrolling, while their fixture generation and file I/O
//! remain outside timed samples. Thresholds are the unchanged §15.2 floors.
use super::report;

/// Checks one named native rate without rounding a failure into PASS.
fn floor(rate: Result<f64, String>, minimum: f64) -> Result<(), String> {
    // Compare the unrounded parsed median; the displayed three decimals
    // must never turn a rate just below the floor into apparent success.
    let value = rate?;
    if value >= minimum {
        Ok(())
    } else {
        Err(format!("{value:.3} MB/s below {minimum:.0} MB/s"))
    }
}

/// Builds the browser and compares its brotli size with the 1 MB target.
fn wasm_size() -> Result<(), String> {
    // Use the actual Trunk release artifact rather than a debug wasm or
    // dependency-tree estimate. The browser target is measured brotli.
    let output = crate::commands::capture("cargo", &["xtask", "web", "build"])?;
    let compressed = output
        .split("brotli=")
        .nth(1)
        .and_then(|part| part.split_whitespace().next())
        .ok_or("web build omitted brotli size")?
        .parse::<usize>()
        .map_err(|error| format!("invalid brotli size: {error}"))?;
    if compressed > 1_000_000 {
        // The 1 MB target is already approved in §15.2. Raising it here
        // would be a budget change and require an R5 user decision.
        return Err(format!("{compressed} B brotli exceeds the 1 MB target"));
    }
    Ok(())
}

/// Reports generated inputs, both new native floors and the wasm size.
pub(super) fn report_close(passed: &mut bool) {
    // A source-file check makes an empty or missing generated fixture a
    // separate failure from a numeric throughput comparison (R14).
    report(
        "four deterministic parser corpora",
        crate::corpus::verify_sources(),
        passed,
    );
    report(
        "SGR-heavy parse >= 100 MB/s native",
        floor(crate::corpus::sgr_rate(), 100.0),
        passed,
    );
    report(
        "cursor-heavy parse >= 80 MB/s native",
        floor(crate::corpus::cursor_rate(), 80.0),
        passed,
    );
    // Unicode has no M5 floor; it is still measured by `cargo xtask corpus`
    // and recorded in PROGRESS beside the three approved budget rates.
    report("release wasm under 1 MB brotli target", wasm_size(), passed);
}
