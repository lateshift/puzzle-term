//! COLR gradient and layer crops with independent CPU/GPU oracle guards.
//!
//! The probe runs before comparison, captures real interpolation coordinates,
//! and rejects a one-byte gradient/clip error. References remain R15 candidates.
use std::{fs, path::Path};
/// Checks four brush/layer families at both device scales.
/// The probe must pass before any output is compared or copied as a candidate.
pub(crate) fn run(bless: bool, verbose: bool) -> Result<(), String> {
    // This also tests paint texture growth and real device destroy/recreation.
    crate::commands::quiet(
        "cargo",
        &["run", "-p", "puzzle-desktop", "--", "colr-probe"],
    )?;
    for dpr in [1, 2] {
        // Linear, sweep, radial/layered, and transformed nested clipping crops.
        for glyph in [8, 12, 156, 205] {
            let file = format!("{glyph}-{dpr}x.png");
            let actual = Path::new("target/colr").join(&file);
            let expected = Path::new("assets/scenarios/colr").join(&file);
            if bless {
                // Only the pinned crop matrix enters version control. Neither
                // failed images nor timings are acceptable golden references.
                fs::create_dir_all("assets/scenarios/colr").map_err(|e| e.to_string())?;
                fs::copy(&actual, &expected).map_err(|e| e.to_string())?;
            } else {
                // Keep the reviewed global ±2/255, 0.1% regression tolerance.
                super::compare(&actual, &expected)?;
            }
            if verbose {
                println!(
                    "{} golden COLR {file}",
                    if bless { "BLESSED" } else { "PASS" }
                );
            }
        }
    }
    Ok(())
}
