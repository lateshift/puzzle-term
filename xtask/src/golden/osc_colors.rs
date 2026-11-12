//! OSC 17/19 selection references, with independent pixel checks before comparison.
//! Existing selection references stay unchanged; these record the new wire path.
//! Human confirmation is pending M18.9 (R15), never inferred from pixel equality.
//! The harness uses bundled fonts and authored colours, with no user settings.
//! Each output first passes exact paper, visible ink and unchanged outside-pixel
//! assertions. The comparator then applies the unchanged §2.7 tolerances.
//! The filenames are explicit: discovering arbitrary target PNGs would allow
//! stale output to conceal a missing scene. Both device ratios must exist.
//! Blessing copies only those two candidates; it never blesses existing families.
//! Full golden acceptance calls this after the previously reviewed families.
//! Logs and grid dumps stay in target rather than entering the reference set.
use std::{fs, path::Path};

/// Runs the OSC selection scene at both hinting/device paths and checks its references.
pub(crate) fn run(bless: bool, verbose: bool) -> Result<(), String> {
    crate::commands::quiet(
        "cargo",
        &[
            "run",
            "-p",
            "puzzle-desktop",
            "--",
            "harness",
            "assets/scenarios/osc-selection.pzs",
        ],
    )?;
    for (index, dpr) in [1, 2].into_iter().enumerate() {
        let file = format!("{:03}-Block-ink-{dpr}x.png", index + 1);
        let actual = Path::new("target/harness/osc-selection").join(&file);
        let expected = Path::new("assets/scenarios/osc-selection").join(&file);
        if bless {
            // Candidates have already passed authored paper/ink and outside-cell checks.
            fs::create_dir_all("assets/scenarios/osc-selection").map_err(|e| e.to_string())?;
            fs::copy(&actual, &expected).map_err(|e| e.to_string())?;
        } else {
            super::compare(&actual, &expected)?;
        }
        if verbose {
            println!(
                "{} golden OSC selection {file}",
                if bless { "BLESSED" } else { "PASS" }
            );
        }
    }
    Ok(())
}
