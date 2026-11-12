//! OSC 66 reference candidates through the ordinary frame and glyph pipelines.
//!
//! The scene asserts occupied ink and blank neighbouring rectangles, then compares
//! incremental frames with independent cold builders. All eight snapshots are
//! named explicitly so an old target artifact cannot conceal a skipped reflow.
//! Human visual approval remains M18.9; numeric equality never supplies it (R15).
//! Existing reference families and their channel/pixel tolerances are unchanged.
//! Only this family's outputs can be copied by its bless path. Grid dumps and
//! timings remain evidence under target, outside the distributed reference set.
use std::{fs, path::Path};

/// Complete authored matrix; grid dumps also increment the scenario file ordinal.
const FILES: [&str; 8] = [
    "001-sizing-1x.png",
    "003-sizing-2x.png",
    "005-reflow-8-1x.png",
    "006-reflow-5-1x.png",
    "007-reflow-back-1x.png",
    "008-reflow-8-2x.png",
    "009-reflow-5-2x.png",
    "010-reflow-back-2x.png",
];

/// Regenerates mechanical witnesses before comparing or copying candidates.
pub(crate) fn run(bless: bool, verbose: bool) -> Result<(), String> {
    // A fresh run supplies outputs from today's sealed-snapshot and glyph code.
    crate::commands::quiet(
        "cargo",
        &[
            "run",
            "-p",
            "puzzle-desktop",
            "--",
            "harness",
            "assets/scenarios/text-sizing.pzs",
        ],
    )?;
    // Enumerating the authored matrix catches missing DPR or resize checkpoints.
    for file in FILES {
        // Resolve only authored output names; unrelated captures are never blessed.
        let actual = Path::new("target/harness/text-sizing").join(file);
        let expected = Path::new("assets/scenarios/text-sizing").join(file);
        if bless {
            // This path runs only after the scene's ink, clipping and cold checks pass.
            fs::create_dir_all("assets/scenarios/text-sizing").map_err(|e| e.to_string())?;
            fs::copy(&actual, &expected).map_err(|e| e.to_string())?;
        } else {
            // Keep the same numerical tolerance as every previously reviewed family.
            super::compare(&actual, &expected)?;
        }
        // Quiet acceptance still performs every comparison; verbosity affects logs only.
        if verbose {
            println!(
                "{} golden OSC 66 {file}",
                if bless { "BLESSED" } else { "PASS" }
            );
        }
    }
    Ok(())
}
