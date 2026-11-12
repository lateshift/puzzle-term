//! Sixteen cell-selection readbacks with independent pixel assertions (§6.9).
//!
//! Candidate generation checks exact selected paper, glyph ink, unchanged
//! outside pixels and row-cache reuse before the golden comparator runs.
//! The fixed name matrix prevents a stale output directory omitting a case.
//! References remain pending human confirmation under R15.
use std::{fs, path::Path};

/// Runs all four kinds at both DPRs with original and overridden selection ink.
pub(crate) fn run(bless: bool, verbose: bool) -> Result<(), String> {
    // Execute the production Session and GPU path on every comparison.
    crate::commands::quiet(
        "cargo",
        &[
            "run",
            "-p",
            "puzzle-desktop",
            "--",
            "harness",
            "assets/scenarios/selection.pzs",
        ],
    )?;
    let mut index = 0;
    // Numbering follows the authored harness matrix, not filesystem enumeration.
    for dpr in [1, 2] {
        // Token absence and explicit white ink have separate references.
        for ink in ["original", "ink"] {
            // Word/line expansion differs from exact and rectangular gestures.
            for kind in ["Simple", "Word", "Line", "Block"] {
                // Every matrix cell gets a stable unique scenario sequence number.
                index += 1;
                let file = format!("{index:03}-{kind}-{ink}-{dpr}x.png");
                // Actual candidates are disposable; only these expected PNGs are assets.
                let actual = Path::new("target/harness/selection").join(&file);
                let expected = Path::new("assets/scenarios/selection").join(&file);
                if bless {
                    // Blessing records candidates and never confirms visual quality.
                    fs::create_dir_all("assets/scenarios/selection").map_err(|e| e.to_string())?;
                    fs::copy(&actual, &expected).map_err(|e| e.to_string())?;
                } else {
                    // Keep the existing ±2-byte and 0.1% tolerance unchanged.
                    super::compare(&actual, &expected)?;
                }
                // Acceptance keeps one criterion line; direct golden runs name each crop.
                if verbose {
                    println!(
                        "{} golden selection {file}",
                        if bless { "BLESSED" } else { "PASS" }
                    );
                }
            }
        }
    }
    Ok(())
}
