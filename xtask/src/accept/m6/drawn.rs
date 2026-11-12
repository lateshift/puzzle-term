//! K3 acceptance checks pixel-inferred text and cache-fault detection (§13.4).
//!
//! Existing preview and terminal-frame scripts are augmented in target/ so
//! every established screen is recognized without modifying its golden PNG.
//! At DPR 2, fixed DPR-1 pixel probes are omitted; all cell, ink and drawn
//! assertions still run. The damage fault uses a new word so the reported
//! old and new cells cannot be confused by a blank-image false positive.
//!
//! The scenario compiler inserts an assertion immediately after each
//! `render-terminal`. That placement is significant: a later feed could
//! repair a stale row before the detector sees it. Running the existing
//! references also gives the recognizer real color, cursor, box and editor
//! states rather than only a synthetic alphabet chart.
//!
//! The fault test is negative by design. A passing harness process would
//! mean the detector missed a real stale row, so this acceptance helper
//! requires failure text naming all three changed columns. The cold path
//! additionally has to leave PNG evidence for independent inspection.

use super::super::report;
use std::{fs, path::Path};

/// Builds a temporary copy with drawn assertions after every terminal frame.
///
/// Generated scripts live under ignored target/ because they are derived
/// checks, not new golden references awaiting visual approval. The source
/// scenario remains the reviewer-accepted sequence of fixture bytes.
fn scenario(source: &str, label: &str, dpr_two: bool) -> Result<String, String> {
    let input = fs::read_to_string(source).map_err(|error| error.to_string())?;
    let mut script = String::new();
    let mut count = 0;
    for line in input.lines() {
        // Pixel probes in the original frame script name physical points at
        // DPR 1. Reusing them at DPR 2 would test the wrong positions;
        // cell-relative ink and drawn checks retain their meaning.
        if dpr_two && line.starts_with("expect-pixel ") {
            continue;
        }
        script.push_str(line);
        script.push('\n');
        if dpr_two && line.starts_with("terminal ") {
            // Set DPR after terminal creation so preview's reset behavior
            // cannot return the second pass to its default 1× scale.
            script.push_str("dpr 2\n");
        }
        if line == "render-terminal" {
            // Assert before a screenshot or next feed can replace the
            // attachment whose pixels are supposed to be recognized.
            script.push_str("expect-drawn-grid\n");
            count += 1;
        }
    }
    if count < 2 {
        // A renamed render command must not turn the acceptance check into
        // a successful no-op over an otherwise parseable script.
        return Err(format!("{source} has too few terminal frames"));
    }
    fs::create_dir_all("target/drawn").map_err(|error| error.to_string())?;
    let path = format!("target/drawn/{label}.pzs");
    fs::write(&path, script).map_err(|error| error.to_string())?;
    Ok(path)
}

/// Runs one augmented GPU scenario, preserving its exact failure location.
///
/// The release profile keeps large fixture recognition from spending time
/// in debug-mode Slug preparation; this is still the normal headless GPU
/// harness and uses the same shaders and fonts as the original scenes.
fn run_scenario(path: &str) -> Result<(), String> {
    crate::commands::quiet(
        "cargo",
        &[
            "run",
            "--release",
            "-p",
            "puzzle-desktop",
            "--",
            "harness",
            path,
        ],
    )
}

/// Requires recognition on preview fixtures and decorated cursor scenes.
///
/// Preview already includes paired DPR 1 and DPR 2 screenshots of five
/// different captured programs. Terminal-frame covers the M6 decorations
/// and cursor styles; an extra pass supplies DPR 2 for those exact states.
fn self_test() -> Result<(), String> {
    // Preview explicitly renders each capture at 1× and 2×. The frame
    // scenario is repeated because its references were originally 1× only.
    let cases = [
        ("assets/scenarios/preview.pzs", "preview-readback", false),
        (
            "assets/scenarios/terminal-frame.pzs",
            "frame-readback-1x",
            false,
        ),
        (
            "assets/scenarios/terminal-frame.pzs",
            "frame-readback-2x",
            true,
        ),
    ];
    for (source, label, doubled) in cases {
        // Stop at the first failing screen but retain its source line in the
        // error; the top-level accept runner continues with other criteria.
        run_scenario(&scenario(source, label, doubled)?)?;
    }
    Ok(())
}

/// Expects both detectors to name the three changed cells after damage loss.
///
/// The word change `old` to `new` preserves length and row geometry. A
/// broad ink-count or a bounding-box-only check could miss it, which is why
/// the assertion requires the three physical columns by name.
fn stale_detection() -> Result<(), String> {
    fs::create_dir_all("target/drawn").map_err(|error| error.to_string())?;
    let prefix = "terminal 8x2\nfeed \"old\"\nrender-terminal\nfeed \"\\rnew\"\n\
        test-skip-row-damage 0\nrender-terminal\n";
    for (label, assertion) in [
        ("stale-drawn", "expect-drawn-grid"),
        ("stale-cold", "expect-cold-match"),
    ] {
        let path = format!("target/drawn/{label}.pzs");
        fs::write(&path, format!("{prefix}{assertion}\n")).map_err(|error| error.to_string())?;
        let failure = run_scenario(&path)
            .err()
            .ok_or("skipped damage passed unexpectedly")?;
        // Both assertions must diagnose the same storage-versus-image split.
        // A failure from a missing font or GPU adapter lacks these cells and
        // must not be mistaken for a successful negative test.
        for cell in ["0:0", "0:1", "0:2"] {
            if !failure.contains(cell) {
                return Err(format!("{label} omitted cell {cell}"));
            }
        }
    }
    // The cold path must save inspectable GPU evidence, not merely compare
    // a row version or claim that a byte buffer differs.
    let folder = Path::new("target/harness/stale-cold");
    for name in ["001-incremental.png", "002-cold.png", "003-difference.png"] {
        if !folder.join(name).is_file() {
            return Err(format!("missing {name}"));
        }
    }
    Ok(())
}

/// Reports K3's independently failing recognition and stale-cache criteria.
pub(super) fn report_k3(passed: &mut bool) {
    report(
        "M6-K3 drawn recognizer matches preview and frame screens at 1x/2x",
        self_test(),
        passed,
    );
    report(
        "M6-K3 skipped row damage fails drawn-grid and cold match at cells 0:0-0:2",
        stale_detection(),
        passed,
    );
}
