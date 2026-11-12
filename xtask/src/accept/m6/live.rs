//! M6.4 PTY and native key checks before the mandatory live-window STOP.
//!
//! A fake transport would only retest Session. The smoke command instead
//! starts `/bin/sh` on an actual OS PTY, reads the master, and checks VT
//! cells through `BufferView`. The key test uses winit logical names and
//! the real M5 encoder, including application cursor mode and modifiers.
//! Both processes are separate acceptance criteria, so a PTY failure still
//! leaves the key table result visible to the reviewer (R14).
//!
//! Startup geometry is tested at DPR 1 and 2 because a surface's winit
//! inner size is physical pixels while font size is configured in logical
//! pixels. A mismatch makes the initial shell report too few columns even
//! when later manual resizing corrects it. The PTY smoke additionally
//! checks the child sees the grid that the session was created with.
//!
//! Padding is a GPU property. A unit test can prove the frame requested
//! the theme background, but only a readback can prove the attachment was
//! actually cleared to it outside the clipped grid. The padding scenario
//! allocates an extra subcell strip at the right and bottom edges so that
//! all three regions—cell grid, inset, remainder—exist in one image.
//!
//! Nvim is a real keyboard-protocol client. It queries kitty support on
//! its alternate screen, requests progressive flags, and interprets
//! Ctrl+D as a half-page scroll. The scenario records both outgoing mode
//! negotiation and each transport write from the key event. One correct
//! CSI u write plus a second legacy C0 write can still scroll and show
//! `^D`; checking the final grid alone would miss the duplicate.
//!
//! Each criterion runs independently after a failure. This is important
//! during a native regression because GPU, PTY and key mapping can fail
//! for unrelated reasons and each needs its own actionable PASS/FAIL line.
use super::report;
use std::path::Path;
/// Real editor regression for kitty Ctrl+D and duplicate IME text.
mod nvim;

/// Runs a filtered test and rejects Cargo's zero-selected-test success.
/// Cargo exits successfully when a filter selects no tests, so the output
/// must contain the specific selected name before this gate can pass.
fn keyboard_table() -> Result<(), String> {
    let name = "keyboard::tests::winit_keys_encode_with_current_terminal_modes";
    let output = crate::commands::capture(
        "cargo",
        &["test", "-p", "puzzle-embed", name, "--", "--exact"],
    )?;
    // A test rename cannot silently remove this acceptance criterion.
    output
        .contains(&format!("test {name} ... ok"))
        .then_some(())
        .ok_or_else(|| format!("desktop test {name} did not run"))
}

/// Proves control-byte IME commits are rejected after modifier release.
fn ime_control_filter() -> Result<(), String> {
    let name = "keyboard::tests::control_commit_is_rejected_without_modifier_state";
    let output = crate::commands::capture(
        "cargo",
        &["test", "-p", "puzzle-embed", name, "--", "--exact"],
    )?;
    output
        .contains(&format!("test {name} ... ok"))
        .then_some(())
        .ok_or_else(|| format!("desktop test {name} did not run"))
}

/// Requires the shared physical-pixel calculation at simulated DPR 1 and 2.
/// A live maximize test needs user hardware; this arithmetic test checks
/// both the initial and later resize paths without opening a window.
fn startup_grid() -> Result<(), String> {
    let name = "window::live::tests::startup_and_resize_use_physical_pixels_at_both_dprs";
    let output = crate::commands::capture(
        "cargo",
        &["test", "-p", "puzzle-embed", name, "--", "--exact"],
    )?;
    output
        .contains(&format!("test {name} ... ok"))
        .then_some(())
        .ok_or_else(|| format!("desktop test {name} did not run"))
}

/// Executes the fixed shell through the same local PTY adapter as the UI.
/// The child reports its tty size before printing a marker. This checks
/// actual OS PTY geometry as well as parser delivery into visible cells.
fn pty_smoke() -> Result<(), String> {
    // No window or GPU is needed for this command: it isolates process
    // spawning, byte transport and VT parsing from visual presentation.
    crate::commands::capture("cargo", &["run", "-p", "puzzle-desktop", "--", "pty-smoke"])?;
    Ok(())
}

/// Renders spare backing pixels and compares their clear colour on the GPU.
/// The reference is a review item; the scenario also has exact pixel and
/// ink assertions so a newly blank image cannot pass by coincidence.
fn padding_scene() -> Result<(), String> {
    // The offscreen renderer uses the same FrameRenderer clear operation
    // as the live surface, without relying on macOS window automation.
    crate::commands::capture(
        "cargo",
        &[
            "run",
            "-p",
            "puzzle-desktop",
            "--",
            "harness",
            "assets/scenarios/padding.pzs",
        ],
    )?;
    crate::golden::compare(
        Path::new("target/harness/padding/001-padding.png"),
        Path::new("assets/scenarios/padding/001-padding.png"),
    )
}

/// Adds one line per M6.4 mechanical contract to the milestone report.
/// Keeping the readback and frame-color unit tests separate localizes
/// whether a failure is in frame production or render execution.
pub(super) fn report_live(passed: &mut bool) {
    report(
        "M6 startup grid and PTY use physical pixels at DPR 1 and 2",
        startup_grid(),
        passed,
    );
    report(
        "M6 frame clear fills padding with default background",
        super::session_test("frame_builder::tests::frame_clear_matches_default_background"),
        passed,
    );
    report(
        "M6 frame clear colour equals default background in GPU screenshot",
        padding_scene(),
        passed,
    );
    report(
        "M6.4 drag resize keeps GPU scissors inside the surface",
        super::session_test(
            "frame_builder::tests::pending_resize_never_scissors_beyond_backing_pixels",
        ),
        passed,
    );
    report(
        "M6.4 real PTY shell output reaches VT cells",
        pty_smoke(),
        passed,
    );
    report(
        "M6.4 winit key identities use M5 encoding",
        keyboard_table(),
        passed,
    );
    report(
        "M6.4 nvim Ctrl+D sends one kitty key and scrolls",
        nvim::ctrl_d(),
        passed,
    );
    report(
        "M6.4 IME C0 and DEL commits never double-send",
        ime_control_filter(),
        passed,
    );
}
