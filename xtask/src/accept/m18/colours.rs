//! M18.1 independent gates: generated data, wire grammar, theme and actual GPU ink.
//! A parser-only success cannot certify selection colours reach the renderer.
//! The golden fixture starts with deliberately different theme tokens and checks
//! selected paper/ink plus every unselected pixel before reference comparison.
//! Native and browser runs are orchestrated sequentially; this task has no
//! browser skip hidden in a parser gate. The milestone still owns its full run.
//!
//! The generated-data check prevents editing the packed lookup independently
//! of its approved source. Parser tests visit all source spellings and their
//! normalized aliases, plus numeric boundaries, malformed UTF-8 and allocation.
//! Wire tests feed complete packets through Terminal::advance, never directly
//! into colour helpers, so routing, terminators and reply order stay covered.
//! Separate per-number files make an omitted reset number visible in review.
//! Session tests exercise theme replacement and the existing fade contract:
//! queries switch to the target immediately, rendering interpolates defaults.
//! A sealed-table regression mutates the live VT after capture and verifies
//! cursor text/selection still use the snapshot's earlier values.
//! The GPU witness checks actual paper/ink with both hinting/device paths.
//! Its baseline uses a conflicting theme colour so ignoring OSC cannot pass.
//! All checks retain their original assertions; the gate only aggregates them.
//! Failures remain visible even if a later criterion succeeds. Human approval
//! of the new reference PNGs is listed separately, never inferred here.
use super::report;

/// Reports every colour obligation, retaining failures from earlier criteria.
pub(super) fn run(passed: &mut bool) {
    // Derived bytes must agree with the approved source on every acceptance run.
    report(
        "M18.1 generated X11 table matches source",
        crate::commands::quiet("python3", &["tools/generate-x11-colours.py", "--check"]),
        passed,
    );
    report(
        "M18.1 shared allocation-free colour parser",
        crate::commands::quiet(
            "cargo",
            &["test", "-p", "puzzle-vt", "--lib", "color_spec::tests"],
        ),
        passed,
    );
    // Per-number files pin sets, queries, resets, both terminators and bad input.
    report(
        "M18.1 every colour OSC and ordered shared-table replies",
        crate::commands::quiet(
            "cargo",
            &[
                "test",
                "-p",
                "puzzle-vt",
                "--test",
                "conformance",
                "colors::",
            ],
        ),
        passed,
    );
    report(
        "M18.1 theme/reset ownership and sealed render colours",
        session_tests(),
        passed,
    );
    report(
        "M18.1 OSC 17/19 selection pixels and references",
        crate::golden::osc_colors::run(false, false),
        passed,
    );
    println!(
        "REVIEW M18.1 OSC selection references: assets/scenarios/osc-selection; pending M18.9"
    );
}

/// Exercises appearance/fade behavior as well as renderer-side table isolation.
fn session_tests() -> Result<(), String> {
    // All theme regressions remain enabled, including target queries during a fade.
    crate::commands::quiet(
        "cargo",
        &[
            "test",
            "-p",
            "puzzle-term",
            "--lib",
            "session::tests::theme::",
        ],
    )?;
    crate::commands::quiet(
        "cargo",
        &[
            "test",
            "-p",
            "puzzle-term",
            "--lib",
            "frame_builder::colors::tests::",
        ],
    )
}
