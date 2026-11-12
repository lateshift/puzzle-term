//! Final M11 measurements remain separate from human appearance acceptance.
//!
//! The release probe redraws every row of 240×70 emoji/CJK fixtures and records
//! raw samples, bitmap source times and allocated raster pages at both DPRs.
//! Its ASCII fixture retains initialized native fallback state and enforces
//! the existing two-millisecond frame ceiling. Emoji/CJK times are reported
//! without inventing a script-specific frame limit absent from §15.
//! Bitmap preparation includes mapping, shaping, PNG decode and shared fit.
//! Color pixels have no quadratic curve count, so that observation is separate
//! from M11.6's primary-median/per-curve geometry budget. COLR remains measured
//! by the earlier fixture benchmark in this same complete acceptance command.
//! A counting allocator independently checks genuine warm full redraws at both
//! scales. It invalidates rows before rebuilding and retains prepared native ink.
//! Exact filters reject a renamed or missing witness instead of counting no tests.
//! Required output rows prove that each specimen/scale measurement executed.
//! The probe's CSV files retain every trial, even those affected by scheduling.
//! Native appearance and COLR references were confirmed by the user. REVIEW
//! cites that verdict separately from the command’s mechanical evidence.
use super::{report, test};
/// COLR clips and paint rows also obey the approved per-curve scaling rule.
mod colr;
/// Runs final mechanical measurements and lists the authorized desktop STOP.
pub(super) fn run(passed: &mut bool) {
    // Run the exact packer comparison, not only an overlap or pixel-tolerance check.
    report(
        "M11.7 four-edge fast/general band lists and texels identical",
        test(
            "puzzle-slug",
            "atlas::four_edge_tests::four_edge_fast_and_general_pack_identically",
        ),
        passed,
    );
    // M11.6 has just recalibrated the primary; COLR includes each layer's clip work.
    report(
        "M11.7 COLR preparation per curve ≤ primary p95",
        colr::run(),
        passed,
    );
    report(
        "M11.7 native warm emoji/CJK full frames allocate zero",
        test(
            "puzzle-desktop",
            "system_measure::tests::native_warm_full_frames_allocate_nothing",
        ),
        passed,
    );
    report(
        "M11.7 full-grid, bitmap and raster memory measured at DPR 1/2",
        measurements(),
        passed,
    );
    println!(
        "REVIEW M11.7 desktop and COLR references: user confirmed 2026-10-04; all eight assets/scenarios/colr crops accepted; README.md / README.md#m11"
    );
}
/// Requires every final measurement to execute and preserves its labelled output.
fn measurements() -> Result<(), String> {
    let output = crate::commands::capture(
        "cargo",
        &[
            "run",
            "--release",
            "-p",
            "puzzle-desktop",
            "--",
            "coverage-measure",
        ],
    )?;
    // Matching all labelled rows prevents a partial run from masquerading as complete.
    for dpr in [1, 2] {
        for kind in ["emoji", "CJK", "ASCII"] {
            let expected = format!("coverage_frame kind={kind} dpr={dpr} grid=240x70");
            if !output.contains(&expected) {
                return Err(format!("missing final frame measurement: {expected}"));
            }
        }
        // Both installed glyphs must execute at both strike/fit sizes.
        for cluster in ["🦀", "✌️"] {
            let expected = format!("fallback_bitmap cluster={cluster} dpr={dpr}");
            if !output.contains(&expected) {
                return Err(format!("missing final bitmap measurement: {expected}"));
            }
        }
    }
    // CLI success enforces ASCII timing and real bitmap output, not only labels.
    print!("{output}");
    Ok(())
}
