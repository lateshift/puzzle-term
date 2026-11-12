//! Kitty image reference candidates retain the established byte/pixel tolerance.
//!
//! Source PNGs are generated from authored pixels; no external assets enter the
//! repository. The harness independently asserts colors, anchors and clipping.
//!
//! The twelve original filenames pin six semantic cases at DPR one and two;
//! four appended crops cover placeholders and seams without changing that order.
//! Directory enumeration cannot accidentally bless a failed-run
//! artifact. The same comparator as text and Slug retains the ±2/255 and 0.1%
//! limits; images do not introduce a new tolerance or waive any old reference.
//! The image-specific selector supports targeted task verification, while the
//! global golden command also includes this matrix after the older scenarios.
use std::{fs, path::Path};
/// Executes every required case afresh before comparing or blessing its crop.
pub(crate) fn run(bless: bool, verbose: bool) -> Result<(), String> {
    // Each comparison uses a newly rendered publication, never a previous PNG receipt.
    crate::commands::quiet(
        "cargo",
        &[
            "run",
            "-p",
            "puzzle-desktop",
            "--",
            "harness",
            "assets/scenarios/images.pzs",
        ],
    )?;
    // Scenario numbering restarts at one for its first actual screenshot.
    let mut index = 0;
    for dpr in [1, 2] {
        for name in [
            "rgba",
            "png-alpha",
            "negative-z",
            "source-crop",
            "half-viewport",
            "reflow",
        ] {
            index += 1;
            // Stable labels keep references tied to the intended source/crop combination.
            compare_case(&format!("{index:03}-{name}-{dpr}x.png"), bless, verbose)?;
        }
    }
    // Append text-backed and seam witnesses without renumbering older references.
    for name in ["placeholders", "placeholder-seams"] {
        for dpr in [1, 2] {
            index += 1;
            compare_case(&format!("{index:03}-{name}-{dpr}x.png"), bless, verbose)?;
        }
    }
    Ok(())
}

/// Uses the same established comparator for physical placements and text-backed tiles.
fn compare_case(file: &str, bless: bool, verbose: bool) -> Result<(), String> {
    // Read the candidate produced by this invocation, not a cached prior run.
    let actual = Path::new("target/harness/images").join(file);
    // References live separately so normal checks cannot modify their baseline.
    let expected = Path::new("assets/scenarios/images").join(file);
    if bless {
        // Copying a mechanically valid candidate never grants R15 approval.
        fs::create_dir_all("assets/scenarios/images").map_err(|e| e.to_string())?;
        fs::copy(&actual, &expected).map_err(|e| e.to_string())?;
    } else {
        // Existing per-byte and changed-pixel limits apply to image tiles too.
        // Comparison reads candidates only; it never overwrites approved assets.
        super::compare(&actual, &expected)?;
    }
    // Acceptance emits its own criterion; interactive runs name each crop.
    if verbose {
        println!(
            "{} golden images {file}",
            if bless { "BLESSED" } else { "PASS" }
        );
    }
    Ok(())
}
