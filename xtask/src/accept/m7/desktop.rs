//! Desktop parity witnesses and effective-table CLI checks for the M7.3 STOP.
//!
//! CLI output is checked through the executable with a neutral temporary
//! config, not the developer's file. The table must preserve four sources.
//!
//! The pointer witnesses submit typed Mouse commands through a real Session,
//! so an adapter that produces a correct number but never reaches the PTY
//! cannot pass. Literal bytes distinguish classic, UTF-8, SGR, urxvt and
//! pixel-SGR; coordinate conversion is exercised at DPR 2.
//! The motion test replaces several OS samples in one frame, then checks
//! that cell encodings suppress a subcell move while pixel encoding keeps it.
//! Click counts are local metadata for future selection, not extra PTY bytes.
//! Fractional wheel input must accumulate instead of disappearing on rounding.
//! Host actions are tested against a file-backed store: a zoom reset restores
//! the current profile size, and a failed profile switch preserves that profile.
//! None of these checks requires a live display. The separate STOP review
//! verifies real tmux/htop callbacks and clipboard gestures on the user's OS.
//! Those observations cannot be substituted by passing synthetic byte tables.
//! Future selection remains unavailable; this task only prepares click metadata.
use super::test;
use crate::accept::report;

/// Cleans up a CLI fixture even when a process fails its criterion.
struct Config(std::path::PathBuf);
/// Acceptance must not leave synthetic configuration in a crate directory.
impl Drop for Config {
    /// Only this temporary file is owned by the witness.
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// Introspection must show the effective table, not just schema defaults.
fn listing() -> Result<(), String> {
    // The process id isolates this neutral fixture from another concurrent
    // acceptance run; Drop removes it on both success and early failure.
    let config =
        Config(std::env::temp_dir().join(format!("puzzle-m7-list-{}.json", std::process::id())));
    std::fs::write(&config.0, r#"{"keyboard":{"bindings":["ctrl+KeyA=sendText:user"]},"profiles":{"review":{"keyboard":{"bindings":["ctrl+KeyB=sendText:profile"]}}}}"#).map_err(|e| e.to_string())?;
    let path = config.0.to_str().ok_or("config path is not UTF-8")?;
    // Exercise the shipping argument parser and store loading, not a second
    // formatter that might hide missing profile/session provenance.
    let output = crate::commands::capture(
        "cargo",
        &[
            "run",
            "-p",
            "xtask",
            "--",
            "run",
            "--debug",
            "--list-keybindings",
            "--list-actions",
            "--config",
            path,
            "--profile",
            "review",
            "--set",
            r#"keyboard.bindings=["ctrl+KeyC=sendText:session"]"#,
        ],
    )?;
    // Every layer contributes a different physical trigger. A flattened
    // user list would falsely label profile/session/default entries alike.
    for row in [
        "ctrl+KeyA\tSendText(\"user\")\tUser",
        "ctrl+KeyB\tSendText(\"profile\")\tProfile",
        "ctrl+KeyC\tSendText(\"session\")\tSession",
        "\tDefaults",
    ] {
        if !output.contains(row) {
            return Err(format!("effective table missing {row}"));
        }
    }
    // Unimplemented actions remain visible with their milestone ownership.
    if !output
        .lines()
        .any(|line| line.starts_with("copy\t") && line.contains("\tM8\t"))
    {
        return Err("action catalogue omitted deferred copy".into());
    }
    Ok(())
}

/// Mouse tables use the actual desktop adapter and Session transport lane.
pub(super) fn run(passed: &mut bool) {
    for (label, name) in [
        (
            // Each report travels through active DEC modes, not a hard-coded
            // default encoder that could ignore application negotiations.
            "desktop mouse buttons/wheel and all encodings with modifiers",
            "desktop_mouse_encoding_table",
        ),
        (
            // Normal tracking must not flood the child when the cursor moves;
            // any-motion must emit only the last eligible position per turn.
            "desktop motion: 1000 suppressed, 1003 reported and frame-coalesced",
            "desktop_motion_tracking_and_coalescing",
        ),
        (
            // M8 selection will need both double/triple counts and complete
            // wheel ticks after several fractional trackpad callbacks.
            "desktop click counts and fractional wheel accumulation",
            "desktop_click_counts_and_wheel_fraction",
        ),
    ] {
        // The synthetic callback table requires no live window or OS layout.
        report(
            label,
            test("puzzle-desktop", &format!("mouse::tests::{name}")),
            passed,
        );
    }
    report(
        "desktop --list-keybindings source layers and --list-actions catalogue",
        listing(),
        passed,
    );
    report(
        "desktop zoom/reset, profile and appearance actions preserve layers",
        test(
            "puzzle-embed",
            "settings::tests::binding_host_actions_preserve_layers_and_zoom_base",
        ),
        passed,
    );
    report(
        // Native controls must use the same xterm identity as the synthetic
        // byte table, and child EOF must prevent a later pointer write.
        "desktop native button identities and no pointer writes after exit",
        test(
            "puzzle-desktop",
            "mouse::tests::native_button_identity_table",
        )
        .and_then(|()| {
            test(
                "puzzle-desktop",
                "mouse::tests::pointer_after_exit_is_inert",
            )
        }),
        passed,
    );
}
