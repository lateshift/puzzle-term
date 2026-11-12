//! M6-K1 acceptance for the recordable raw-input seam (§13.4).
//!
//! The window is the only owner of winit's private KeyEvent fields. The
//! adapter accepts owned RawInput so a headless harness and later replay
//! can drive the exact same command mapping. A source check guards this
//! boundary, while the two-shell scenario catches a single hard-coded
//! nvim command disguised as general `shell` support.
//!
//! Both shell invocations write different visible text through an actual
//! PTY. The acceptance command reads fresh grid dumps produced by that
//! run rather than trusting a process exit that could have skipped the
//! second command. This establishes arbitrary command lines before K2
//! introduces recordings and K4 fills macOS chord sequences from them.
//!
//! K4 runs one real-PTY child for each kitty flag set and checks every
//! Ctrl+letter after a quiet output boundary. The scenario itself asserts
//! exact transport-write calls, modifier labels, absence of RAW input,
//! absence of a settled show-command marker, drawn/grid agreement and
//! cold-frame equality. Merely compiling keyecho would not prove its
//! output reached the same retained GPU path as the live terminal.
//! The nvim witness is separate because an optional external editor cannot
//! be a prerequisite for the self-contained simulator's mechanical matrix.
//! A missing editor is therefore reported as SKIP, never hidden as PASS.

use super::report;
use std::{fs, path::Path};

/// Ensures native winit key reading stays in the one conversion module.
fn raw_boundary() -> Result<(), String> {
    // The conversion module must hold the native event and emit RawInput.
    // This makes replay independent of winit's non-constructible KeyEvent.
    let window = fs::read_to_string("crates/puzzle-desktop/src/window/raw.rs")
        .map_err(|error| error.to_string())?;
    // The adapter must not read native fields after the boundary. Its tests
    // can then supply the same values without opening a real window.
    let adapter = fs::read_to_string("crates/puzzle-desktop/src/keyboard.rs")
        .map_err(|error| error.to_string())?;
    if !window.contains("event: &KeyEvent")
        || !window.contains("RawInput::Key")
        || adapter.contains("use winit")
        || adapter.contains("event: &KeyEvent")
    {
        return Err("winit key fields escaped the window RawInput converter".into());
    }
    Ok(())
}

/// Runs two unrelated command strings through separate real PTY children.
fn two_shells() -> Result<(), String> {
    // Launch the real harness so the shell command crosses a PTY and the
    // session parser, instead of calling the scenario parser in isolation.
    crate::commands::capture(
        "cargo",
        &[
            "run",
            "-p",
            "puzzle-desktop",
            "--",
            "harness",
            "assets/scenarios/shell-two.pzs",
        ],
    )?;
    for (name, expected) in [("first", "first"), ("second", "second")] {
        // Each dump is written after its own shell has exited. Two distinct
        // dumps catch implementations that silently reuse the first command.
        let path = format!("target/harness/shell-two/{name}.grid.txt");
        let content = fs::read_to_string(Path::new(&path)).map_err(|error| error.to_string())?;
        if !content.starts_with(expected) {
            return Err(format!("{path} lacks {expected:?}"));
        }
    }
    Ok(())
}

/// Reports K1's independent platform-boundary and general-shell checks.
pub(super) fn report_k1(passed: &mut bool) {
    report(
        "M6-K1 only window converts winit keys to RawInput",
        raw_boundary(),
        passed,
    );
    report(
        "M6-K1 two arbitrary shell command lines use the PTY",
        two_shells(),
        passed,
    );
}

/// Runs one native replay unit case without requiring a graphical window.
fn replay_test(name: &str) -> Result<(), String> {
    // Clock and retained-atlas tests moved with the executor; replay scripts remain hosts.
    let package = if name.starts_with("window::") {
        "puzzle-embed"
    } else {
        "puzzle-desktop"
    };
    let output =
        crate::commands::capture("cargo", &["test", "-p", package, name, "--", "--exact"])?;
    if output.contains(&format!("test {name} ... ok")) {
        Ok(())
    } else {
        Err(format!("puzzle-desktop test {name} did not run"))
    }
}

/// Runs the retained-atlas witness with the native GPU gate enabled.
fn gpu_replay_test(name: &str) -> Result<(), String> {
    // The general workspace test can run in a GPU-less sandbox. M6 acceptance
    // selects this ignored test explicitly so missing adapter access is a
    // visible failure rather than a silent skip or weakened comparison.
    // The witness draws actual raster atlas pixels after omitting a check;
    // a source-text assertion could not detect a missing upload in wgpu.
    let output = crate::commands::capture(
        "cargo",
        &[
            "test",
            "-p",
            "puzzle-embed",
            name,
            "--",
            "--exact",
            "--ignored",
        ],
    )?;
    output
        .contains(&format!("test {name} ... ok"))
        .then_some(())
        .ok_or_else(|| format!("puzzle-desktop GPU test {name} did not run"))
}

/// Makes the deferred atlas comparison an explicit native GPU criterion.
fn report_deferred_atlas(passed: &mut bool) {
    // Deferred verification sees some presentations without reading them
    // back. Those presentations still own glyph uploads that later frames
    // reference, so this criterion exercises both retention and detection.
    report(
        "M6-K5b deferred verifier retains glyph uploads and detects stale atlas",
        gpu_replay_test(
            "window::live::verify::tests::skipped_frame_uploads_match_cold_and_stale_atlas_is_detected",
        ),
        passed,
    );
}

/// Keeps K2's frame chronology and changed-byte diagnostics independent.
pub(super) fn report_k2(passed: &mut bool) {
    report(
        "M6-K2 synthetic action recording replays both frames",
        replay_test("harness::replay_actions::tests::synthetic_recording_replays_at_both_frames"),
        passed,
    );
    report(
        "M6-K2 changed adapter byte reports original entry",
        replay_test("harness::replay_actions::tests::changed_adapter_byte_reports_entry"),
        passed,
    );
}

/// Runs one real-PTY key script with the optimized GPU readback path.
fn key_scenario(path: &str) -> Result<(), String> {
    // The scenario invokes its own running executable as `<bin>`, so a
    // release harness cannot accidentally test a stale debug keyecho.
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

/// Reports the complete K4 key matrix and the optional real editor witness.
pub(super) fn report_k4(passed: &mut bool) {
    report(
        "M6-K4 macOS-shaped A-Z across kitty flags 0,1,3,31",
        key_scenario("assets/scenarios/keys-sim.pzs"),
        passed,
    );
    // Nvim is external to the workspace. Its absence is a visible SKIP,
    // while an installed editor with a failing scenario is a hard failure.
    if std::process::Command::new("nvim")
        .arg("--version")
        .output()
        .is_err()
    {
        println!("SKIP M6-K4 nvim key scenario: nvim is not installed");
    } else {
        report(
            "M6-K4 nvim Ctrl+D/U/E/Y/F/B/A/X drawn and cold frames",
            key_scenario("assets/scenarios/keys-nvim.pzs"),
            passed,
        );
    }
}

/// Runs the D-33 scheduler and opt-in mismatch evidence unit cases.
pub(super) fn report_k5(passed: &mut bool) {
    // Each outcome is independent. A correct cold-frame comparison does
    // not prove scheduling, and a fast first frame does not prevent nvim's
    // transient showcmd update from reaching the display.
    for (criterion, test) in [
        // PTY reads two milliseconds apart should feed one late snapshot.
        (
            "M6-K5 late snapshot coalesces two updates",
            "window::pacing::tests::two_updates_two_ms_apart_make_one_late_snapshot",
        ),
        // A marker followed by an erase must never become a visible frame.
        // This directly targets the user's observed Ctrl+D flash.
        (
            "M6-K5 nvim showcmd erase precedes presentation",
            "window::pacing::tests::nvim_showcmd_erase_precedes_the_only_snapshot",
        ),
        // Flood output gets one slot per predicted display refresh even
        // when the parser has many opportunities to wake the UI thread.
        (
            "M6-K5 continuous output presents once per refresh",
            "window::pacing::tests::continuous_damage_never_doubles_a_refresh",
        ),
        // Startup and drag bypass the normal delay so the window never
        // displays an empty first frame or a stretched old backing.
        (
            "M6-K5 startup and resize frames remain immediate",
            "window::pacing::tests::first_and_resize_frames_are_immediate",
        ),
        // The opt-in verifier must identify the changed cell and preserve
        // warm, cold and difference images for a private local diagnosis.
        (
            "M6-K5 frame mismatch names cells and saves PNGs",
            "window::live::verify::tests::mismatch_reports_cell_and_three_pngs",
        ),
    ] {
        report(criterion, replay_test(test), passed);
    }
}

/// Reports the D-33 revision independently of the original K5 regressions.
pub(super) fn report_k5b(passed: &mut bool) {
    // The burst decision belongs to the neutral session crate. A host-only
    // test could accidentally duplicate the rule and leave web rAF behind.
    let neutral = crate::commands::capture(
        "cargo",
        &[
            "test",
            "-p",
            "puzzle-term",
            "--lib",
            "session::presentation::tests::short_mark_and_erase_share_one_snapshot",
            "--",
            "--exact",
        ],
    )
    .and_then(|output| {
        // Cargo exits zero when an exact filter selects no tests, so the
        // named result guards against a renamed or cfg-disabled test.
        output
            .contains(
                "test session::presentation::tests::short_mark_and_erase_share_one_snapshot ... ok",
            )
            .then_some(())
            .ok_or("shared settle test did not run".into())
    });
    report(
        "M6-K5b shared settle rule rejects short transient output",
        neutral,
        passed,
    );
    // The desktop tests below supply phase and Fifo timing. They must
    // agree with the same neutral decision exercised above on the web.
    // The phase sweep tests the entire 60 Hz turn, not one convenient
    // timestamp where nvim's erase happened to beat a scheduled snapshot.
    // Calibration and percentile tests isolate why the original fallback
    // reproduced the user's flash even though the two-ms replay passed.
    for (criterion, test) in [
        // The shared burst rule must never expose gaps shorter than 8 ms.
        (
            "M6-K5b all refresh phases settle 2/5/7 ms erase gaps",
            "window::pacing::tests::settle_window_phase_sweep",
        ),
        // The second unchanged present supplies a phase during idle;
        // a minimized window cannot provide a compositor observation.
        (
            "M6-K5b idle double present calibrates visible phase",
            "window::pacing::tests::idle_double_present_teaches_phase_only_when_visible",
        ),
        // Sparse samples previously indexed a zero-filled 32-entry array
        // and held the frame lead at 1 ms for the first 28 observations.
        (
            "M6-K5b p90 uses 1/10/28/32 actual samples",
            "window::pacing::tests::p90_uses_the_actual_sample_count",
        ),
        // Slow readback belongs after a quiet interval, with frames that
        // arrive meanwhile counted as skipped diagnostic samples.
        (
            "M6-K5b ten-ms verification cannot delay the next frame",
            "window::live::verify::tests::slow_verification_skips_a_new_frame",
        ),
        // The child must generate real 2/5/7-ms separated PTY writes;
        // a parser-only fixture would not exercise that boundary.
        (
            "M6-K5b keyecho accepts measured erase gaps",
            "keyecho::tests::measured_gap_options",
        ),
    ] {
        report(criterion, replay_test(test), passed);
    }
    // The retained GPU witness checks the final drawn cell after each
    // measured gap and rejects a marker left in the bottom status row.
    report(
        "M6-K5b real PTY keyecho gaps 2/5/7 settle without marker",
        key_scenario("assets/scenarios/keys-sim.pzs"),
        passed,
    );
    // The optional browser scene is the only current web draw after font
    // fetch; compiling it proves the rAF callback uses the neutral rule.
    // Its output is captured so acceptance retains the one-line-per-check
    // format even though trunk prints its own build progress and sizes.
    report(
        "M6-K5b browser review scene builds with shared rAF settling",
        crate::commands::capture("cargo", &["xtask", "web", "build-review"]).map(|_| ()),
        passed,
    );
    report_deferred_atlas(passed);
}

/// Reports the held-key burst correction independently of K5b's single keys.
pub(super) fn report_k5c(passed: &mut bool) {
    // The pure rule is checked separately for isolated auto-repeat bursts
    // and uninterrupted animation. A fix that delays every frame would
    // pass the first case while making the terminal feel unresponsive.
    for (label, name) in [
        (
            "M6-K5c 33.4-ms repeat sweep hides 2/5-ms marks at every phase",
            "session::presentation::tests::auto_repeat_never_presents_its_short_lived_mark",
        ),
        (
            "M6-K5c five-ms continuous output presents each refresh",
            "session::presentation::tests::continuous_frames_remain_refresh_driven",
        ),
    ] {
        // Run the neutral tests by exact name so a zero-test Cargo success
        // cannot be mistaken for an implemented burst rule.
        let result = crate::commands::capture(
            "cargo",
            &["test", "-p", "puzzle-term", name, "--", "--exact"],
        )
        .and_then(|output| {
            output
                .contains(&format!("test {name} ... ok"))
                .then_some(())
                .ok_or_else(|| format!("puzzle-term test {name} did not run"))
        });
        report(label, result, passed);
    }
    report(
        "M6-K5c keyecho accepts the fixed 33-ms held-key cadence",
        // The option parser must reject a typo rather than silently running
        // the existing single-key scenario with no repeats at all.
        replay_test("keyecho::tests::held_key_repeat_option"),
        passed,
    );
    // The scenario captures sixty actual PTY mark/erase pairs per gap and
    // replays their cells through every half-ms refresh phase.
    report(
        "M6-K5c real PTY two-second holds present no Ctrl+D marker",
        // The command also checks that many settled frames were presented;
        // suppressing all frames cannot satisfy the real-PTY criterion.
        key_scenario("assets/scenarios/keys-sim.pzs"),
        passed,
    );
}
