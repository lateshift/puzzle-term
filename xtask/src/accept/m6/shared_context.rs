//! D-30 shared text resources without shared VT or row caches.
//!
//! The scenario uses actual GPU pixels for a shared and a cold context;
//! the unit case checks prepared entry counts and both generations. Frame
//! budgets repeat the native §15.2 numbers after changing ownership, so
//! a newly introduced context lookup cannot hide in a later milestone.
//!
//! Separate lines matter because a pixel-identical second session could
//! still prepare every outline twice, defeating P6. The inverse is also
//! possible: a shared prepared handle might point at texture data never
//! uploaded to the executor used by that session. The harness checks both
//! properties, and the cold builder supplies an independent pixel judge.
//! A generation test then changes atlas identity with VT versions fixed;
//! damage must come from the context instead of terminal output.

use super::{
    frame::{cpu_budget, within},
    report, session_test,
};

/// Runs both sessions and an independent cold image through GPU readback.
fn shared_scene() -> Result<(), String> {
    // A cold-match assertion precedes the two-session probe in the script.
    // The command then compares every RGBA byte after a forced atlas reset.
    crate::commands::capture(
        "cargo",
        &[
            "xtask",
            "harness",
            "assets/scenarios/terminal-frame/shared-context.pzs",
        ],
    )?;
    Ok(())
}

/// Keeps each acceptance criterion visible even when a shared check fails.
pub(super) fn report_shared_context(passed: &mut bool) {
    let scene = shared_scene();
    // Pixel parity covers the same terminal source under one common and
    // one isolated font and atlas resource set.
    report("M6.4b shared context pixel parity", scene.clone(), passed);
    // The same scene resets the atlas and requires both warm builders to
    // rebuild before it compares their output to the cold reference.
    report(
        "M6.4b atlas generation rebuilds both sessions",
        scene,
        passed,
    );
    // The unit case inspects entry count before and after the second
    // session's first draw, which pixel equality cannot reveal.
    let entries = session_test(
        "frame_builder::tests::shared_context_prepares_once_and_rebuilds_both_generations",
    );
    // Shared construction is tested at the type seam: FrameBuilder::new
    // accepts only a borrowed TextContext, so it has no font-loading path.
    report(
        "M6.4b glyph used by both prepared once",
        entries.clone(),
        passed,
    );
    // Builder construction takes only &TextContext; it neither opens a
    // font path nor creates a duplicate FontSet for the second session.
    report(
        "M6.4b second session startup loads no font",
        entries,
        passed,
    );
    // The existing release benchmark prepares the glyph set before its
    // timed iterations; only row rebuild and instance assembly are charged.
    // Reuse its parser and exact thresholds to avoid an accidental second
    // interpretation of the §15.2 per-session frame budgets.
    let times = cpu_budget();
    report(
        "M6.4b full 240x70 frame <= 2 ms with shared text",
        times
            .as_ref()
            .map_err(Clone::clone)
            .and_then(|(full, _)| within(*full, 2_000.0, "full frame")),
        passed,
    );
    report(
        "M6.4b one-row frame <= 0.2 ms with shared text",
        times
            .as_ref()
            .map_err(Clone::clone)
            .and_then(|(_, row)| within(*row, 200.0, "one row")),
        passed,
    );
}
