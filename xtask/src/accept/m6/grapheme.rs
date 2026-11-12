//! D-31 acceptance runs real tmux bytes and the streaming width table.
//!
//! The recorded pane divider exposes late VS16 widening: a unit test of
//! isolated emoji cannot show tmux's backspace-induced stale cell. The
//! scenario asserts physical columns and ink after GPU presentation.
//!
//! M6.4c has three distinct failure modes. The width table isolates the
//! Unicode stream printer, including mode 2027 being off. The reset test
//! isolates session policy from the live mode state: RIS and DECSTR must
//! restore the configured startup choice. The tmux replay exercises the
//! interaction between cursor movement and width changes after bytes
//! arrive through a real application's control sequence stream.
//!
//! The final `ab✌️cd|` scene crosses the VT/text/rendering boundary.
//! It asserts the cell indexes before presentation, then checks actual GPU
//! ink so a missing glyph cannot pass because the terminal retained the
//! correct Unicode string. The font may show the base as tofu until an
//! emoji fallback exists, but U+FE0F must not create a second tofu glyph.
//!
//! Each line runs its own command. A failed VT test should not suppress
//! the scenario's independent evidence, and a renamed test must not pass
//! merely because Cargo exits zero after selecting no tests. This is why
//! the helper checks the named `... ok` result in captured test output.

use super::report;

/// Runs one conformance case and rejects Cargo's empty test selection.
fn unicode_test(name: &str) -> Result<(), String> {
    // Integration tests live in `tests/conformance.rs`; `--lib` would
    // silently select zero cases while still returning successful status.
    let output = crate::commands::capture(
        "cargo",
        &[
            "test",
            "-p",
            "puzzle-vt",
            "--test",
            "conformance",
            name,
            "--",
            "--exact",
        ],
    )?;
    output
        // Check the exact test name, not Cargo's process status alone.
        .contains(&format!("test {name} ... ok"))
        .then_some(())
        .ok_or_else(|| format!("Unicode conformance test {name} did not run"))
}

/// Replays the privacy-checked tmux split and selector ink scene.
fn grapheme_scene() -> Result<(), String> {
    // The script replays the checked-in tmux bytes and creates fresh GPU
    // screenshots. Old PNGs are never accepted in place of a new run.
    crate::commands::capture(
        "cargo",
        &[
            "run",
            "-p",
            "puzzle-desktop",
            "--",
            "harness",
            "assets/scenarios/grapheme.pzs",
        ],
    )?;
    Ok(())
}

/// Reports the width, terminal placement and visible glyph contracts.
pub(super) fn report_grapheme(passed: &mut bool) {
    // This table includes selector order, ZWJ families, flags and keycaps
    // in both clustered and legacy modes; isolated VS16 alone is too weak.
    report(
        "M6.4c grapheme widths in mode 2027 and legacy",
        unicode_test("unicode::grapheme_width_parity_table"),
        passed,
    );
    report(
        "M6.4c configured default survives RIS and DECSTR",
        // A live mode reset must return to SessionConfig's startup policy.
        unicode_test("unicode::grapheme_default_is_restored_by_both_resets"),
        passed,
    );
    report(
        "M6.4c tmux divider, joined emoji and legacy stale cell",
        // One pane shifting the divider fails even if the other pane fits.
        grapheme_scene(),
        passed,
    );
    report(
        "M6.4c VS16 has no extra tofu and leaves cd| in columns 4-6",
        // This replays the same script but reports the visual boundary
        // separately from the pane-placement contract.
        grapheme_scene(),
        passed,
    );
}
