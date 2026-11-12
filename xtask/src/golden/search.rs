//! Literal-match and current-match paper references at both DPR paths (§6.10).
//!
//! The scenario asserts every expected cell has exact token paper and glyph ink,
//! with byte-identical outside pixels and complete count/navigation transitions.
//! References record candidates only: human visual confirmation remains pending.
use std::{fs, path::Path};
/// Executes the real Session and renderer before comparing four pinned crops.
pub(crate) fn run(bless: bool, verbose: bool) -> Result<(), String> {
    // Stale output files cannot satisfy a modified engine or frame assembler.
    crate::commands::quiet(
        "cargo",
        &[
            "run",
            "-p",
            "puzzle-desktop",
            "--",
            "harness",
            "assets/scenarios/search.pzs",
        ],
    )?;
    // Both ratios exercise pack/unhinted and hinted glyph paths independently.
    // Current references differ from the ordinary-match images at exactly one hit.
    // The independent scenario checker validates that change before comparison.
    for (index, name) in ["matches-1x", "current-1x", "matches-2x", "current-2x"]
        .iter()
        .enumerate()
    {
        // Pin scenario numbering independently of directory enumeration.
        let file = format!("{:03}-{name}.png", index + 1);
        let actual = Path::new("target/harness/search").join(&file);
        let expected = Path::new("assets/scenarios/search").join(&file);
        if bless {
            // R15: copying these candidates does not approve their appearance.
            // Only this explicit matrix is copied, never timing or failure images.
            fs::create_dir_all("assets/scenarios/search").map_err(|e| e.to_string())?;
            fs::copy(&actual, &expected).map_err(|e| e.to_string())?;
        } else {
            // Keep the shared ±2-byte/0.1% comparator and dimensions unchanged.
            super::compare(&actual, &expected)?;
        }
        if verbose {
            // Direct runs name each crop; acceptance retains one criterion line.
            println!(
                "{} golden search {file}",
                if bless { "BLESSED" } else { "PASS" }
            );
        }
    }
    Ok(())
}
