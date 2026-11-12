//! Incremental M6.6 checks for source-backed theme resolution (§14.6).
//!
//! The popular pack keeps authored values even when a contrast check would
//! prefer a different one. Only Puzzle's four palettes carry the mandatory
//! WCAG floors; custom themes report poor contrast without rewriting bytes.
//! The live appearance check is separate from static palette tests: a
//! catalogue can be valid while an OS switch leaves stale OSC overrides or
//! forgets to tell an application that opted into mode 2031. The PTY
//! environment check is separate again because COLORFGBG is observed at
//! child spawn, before any renderer or command lane can repair it.
//! Generated documentation is a criterion in its own right. A setting
//! added this week must appear in Markdown, JSON Schema and TypeScript,
//! even if the runtime test for that setting happens in an earlier task.
//! The gallery harness asserts a source-backed clear pixel and visible
//! ink before saving each candidate PNG. The comparator then checks those
//! pixels against committed references; a human still decides whether the
//! palette and glyph contrast look right in the full contact sheet (R15).
//!
//! The appearance-cycle probe checks cursor and clear pixels under the
//! real Session command path, rather than simply comparing two palettes.
//! Its own midpoint expectations come from source tokens. The separate
//! blink-table test reaches the off phase and the 15-second cutoff, where
//! a correct color token alone cannot prove that a cursor is drawn.
//! Reporting both criteria identifies color and visibility failures apart.
use super::super::report;
use std::path::Path;

/// Requires an exact test result rather than Cargo's zero-match success.
fn exact(name: &str) -> Result<(), String> {
    let output = crate::commands::capture(
        "cargo",
        &["test", "-p", "puzzle-term", "--lib", name, "--", "--exact"],
    )?;
    if output.contains(&format!("test {name} ... ok")) {
        Ok(())
    } else {
        Err(format!("theme test {name} did not run"))
    }
}

/// Reports source catalogue and derived-color invariants independently.
pub(super) fn report_themes(passed: &mut bool) {
    // A generated file with fewer than ten external palettes would still
    // compile; this count and known-source bytes protect D-25's scope.
    report(
        "M6.6 fourteen source-backed built-in palettes",
        exact("theme::tests::builtin_catalogue_has_fourteen_sources"),
        passed,
    );
    // The pack ships as its authors designed it. Contrast is enforced only
    // for the four Puzzle originals, which own this readability promise.
    report(
        "M6.6 Puzzle theme contrast policy",
        exact("theme::tests::puzzle_contrast_policy"),
        passed,
    );
    // Derivation follows the final inherited terminal colors, so changing
    // only a custom background updates UI overlays coherently.
    report(
        "M6.6 inherited colors derive UI tokens",
        exact("theme::tests::inheritance_derives_after_override"),
        passed,
    );
    // A cycle must be a recoverable report for an untrusted config file,
    // rather than recursing until the process aborts on startup.
    report(
        "M6.6 theme inheritance cycles rejected",
        exact("theme::tests::inheritance_cycles_are_rejected"),
        passed,
    );
    // The host fact must clear prior application color overrides and send
    // mode 2031's exact bytes only after the child enabled that mode.
    report(
        "M6.6 appearance switch clears OSC colors and reports mode 2031",
        exact("session::tests::theme::appearance_switch_reports_2031_and_recolors_queries"),
        passed,
    );
    // The same protocol report follows a settings-selected palette, but
    // a reload with unchanged resolved colors must emit no duplicate.
    report(
        "M6.6 settings theme change reports mode 2031 once",
        exact("session::tests::theme::settings_theme_change_reports_2031_once_per_resolved_change"),
        passed,
    );
    // A nonzero user duration affects only rendered color; protocol
    // replies move to the final theme before the first blend sample.
    report(
        "M6.6 configured appearance fade reaches exact OKLab endpoints",
        exact("session::tests::theme::configured_transition_samples_oklab_without_delaying_2031"),
        passed,
    );
    report_cursor_recovery(passed);
    // A public API caller can decode this command but cannot impersonate
    // the platform origin that authorizes an OS appearance change.
    report(
        "M6.6 appearance command requires platform origin",
        exact("session::tests::theme::appearance_command_requires_platform_origin"),
        passed,
    );
    // The shell's startup environment is fixed before its first prompt;
    // a later mode-2031 report cannot repair a wrong initial COLORFGBG.
    report(
        "M6.6 spawned COLORFGBG matches light and dark",
        desktop_exact("pty::tests::spawned_colorfgbg_matches_initial_appearance"),
        passed,
    );
    // The checked-in guide, JSON Schema and TS declarations must all be
    // regenerated from the same typed catalogue after a new setting lands.
    report(
        "M6.6 generated settings docs, schema and TS types are current",
        crate::settings_docs::check(),
        passed,
    );
    // Readback assertions reject a blank theme; committed references keep
    // a later shader or palette change visible until a person reviews it.
    report(
        "M6.6 theme gallery covers every built-in and both appearance modes",
        gallery(),
        passed,
    );
}

/// Checks pixel color and the blink invariant separately from palette data.
fn report_cursor_recovery(passed: &mut bool) {
    // The sampled GPU cursor follows the destination token through the
    // user's second dark-mode switch at both instant and faded settings.
    report(
        "M6.6 repeated appearance cursor pixels match active theme",
        crate::commands::quiet(
            "cargo",
            &[
                "run",
                "-p",
                "puzzle-desktop",
                "--",
                "harness",
                "assets/scenarios/appearance-cycle.pzs",
            ],
        ),
        passed,
    );
    // A settings re-arm after the idle cutoff has no future wakeup; it
    // must discard the old invisible phase before drawing another frame.
    report(
        "M6.6 host changes and idle stop cannot strand a hidden cursor",
        exact(
            "session::tests::visibility::host_changes_and_idle_stop_never_strand_a_hidden_cursor",
        ),
        passed,
    );
}

/// Requires the real native PTY test rather than a zero-match Cargo success.
fn desktop_exact(name: &str) -> Result<(), String> {
    // Native-only PTY behavior cannot be proven by the neutral session
    // fixture's in-memory Transport implementation.
    let output = crate::commands::capture(
        "cargo",
        &["test", "-p", "puzzle-embed", "--lib", name, "--", "--exact"],
    )?;
    // Cargo succeeds on an unmatched filter, so require the named result
    // line as evidence that the process actually spawned and reported.
    if output.contains(&format!("test {name} ... ok")) {
        Ok(())
    } else {
        Err(format!("desktop test {name} did not run"))
    }
}

/// Rerenders both pair switches and every built-in before comparing images.
fn gallery() -> Result<(), String> {
    crate::commands::quiet(
        "cargo",
        &[
            "run",
            "-p",
            "puzzle-desktop",
            "--",
            "harness",
            "assets/scenarios/theme-gallery.pzs",
        ],
    )?;
    // Three switch images precede the source-ordered built-in catalogue.
    // The fixed names catch a script that silently drops an appearance.
    for (index, name) in ["auto-light", "auto-dark", "forced-light"]
        .iter()
        .enumerate()
    {
        let file = format!("{:03}-{name}.png", index + 1);
        crate::golden::compare(
            &Path::new("target/harness/theme-gallery").join(&file),
            &Path::new("assets/scenarios/theme-gallery").join(&file),
        )?;
    }
    for (index, name) in puzzle_term::theme::Theme::builtin_names()
        .iter()
        .enumerate()
    {
        // The generator's order is stable and captures all 14 names.
        // Each scene already asserted clear RGB and at least 100 ink pixels.
        let file = format!("{:03}-{name}.png", index + 4);
        crate::golden::compare(
            &Path::new("target/harness/theme-gallery").join(&file),
            &Path::new("assets/scenarios/theme-gallery").join(&file),
        )?;
    }
    Ok(())
}
