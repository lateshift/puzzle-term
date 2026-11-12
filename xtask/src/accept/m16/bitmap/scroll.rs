//! The bitmap path exposes new history without allocating or changing the CPU ceiling.
//!
//! The unit witness counts the first row-cache miss after ordinary startup warms
//! glyphs and storage. The release example measures all 202 exact row builds, at
//! DPR 1 and 2, including fractional edge changes. Its own maximum assertion
//! rejects any build above 0.2 ms before this acceptance wrapper reads the median.
//! The explicit environment flag selects the bitmap fixture only for this child.
//! Startup warms glyphs and frame storage without building the measured history
//! rows. Their first exposure must still miss exactly one row cache. Fractional
//! scroll changes exercise both edge rows and retain the same scratch capacity.
//! The timing scope matches the native actor's build_snapshot after the separately
//! budgeted Session capture. File writes and CSV formatting stay outside that timer.
//! A failed probe retains every raw sample before returning its unchanged error.
//! No chosen median can hide a single maximum above the one-row CPU ceiling.
use std::{fs, process::Command};

/// Preserves the original allocation and every-sample exposed-row timing requirements.
pub(super) fn run() -> Result<(), String> {
    super::super::super::m7::test(
        "puzzle-term",
        "frame_builder::tests::scrolling::bitmap_first_exposed_history_row_reuses_warmed_frame_storage",
    )?;
    let output = Command::new("cargo")
        .args([
            "run",
            "--release",
            "-p",
            "puzzle-term",
            "--example",
            "scroll_bench",
        ])
        .env("PUZZLE_GRID_BITMAP_BENCH", "1")
        .output()
        .map_err(|e| e.to_string())?;
    // Retain both streams on failure; compiler and budget diagnostics explain different causes.
    let mut log = output.stdout.clone();
    log.extend_from_slice(&output.stderr);
    fs::create_dir_all("target/m16-4").map_err(|e| e.to_string())?;
    fs::write("target/m16-4/bitmap-scroll.log", log).map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err(
            "bitmap exposed-row release probe failed; target/m16-4/bitmap-scroll.log".into(),
        );
    }
    let text = String::from_utf8(output.stdout).map_err(|e| e.to_string())?;
    // The existing example prints only its full-call median; maximum checks execute internally.
    let millis: f64 = text
        .trim()
        .strip_prefix("scroll_ms=")
        .ok_or("scroll median absent")?
        .parse()
        .map_err(|_| "invalid scroll median")?;
    if !millis.is_finite() || millis > 0.2 {
        return Err(format!("bitmap scroll frame {millis} ms exceeds 0.2"));
    }
    println!(
        "M16.4 bitmap scroll median={millis:.6} ms; every exact build ≤0.2 ms; target/m16-4/exposed-row-build.csv"
    );
    Ok(())
}
