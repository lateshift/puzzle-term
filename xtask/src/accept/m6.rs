//! Incremental M6 acceptance in reviewer work-order sequence.
//!
//! M6.0 first repairs two M5 test gaps. Exact test names prevent Cargo's
//! zero-selected-test success from hiding a deleted regression. Later M6
//! tasks append independent frame, session and live-PTY criteria here.
//! The VT serializer property varies cells and retained history because
//! a simple ASCII grid cannot expose lost sparse metadata or soft wraps.
//! Named-key identity is checked against kitty's published numbers; an
//! arbitrary numeric key passed back unchanged would prove nothing.
//!
//! M6.1 acceptance runs the same offscreen executor used by the eventual
//! session frame. The candidate reference is kept under assets so a fresh
//! GPU render cannot silently become its own expected image. Exact test
//! names guard against Cargo's successful zero-test filter behavior.
//! Draw count and golden bytes are reported separately: a renderer could
//! match the image by issuing a draw per primitive and still violate P5.
//! Each criterion executes independently, preserving R14's full report.
//! The manual-clock case feeds a query on each side of the slice boundary.
//! Keeping budget and reply order as separate report lines makes a failed
//! transport flush visible even if parsing still obeyed its time limit.
//! The key and idle checks distinguish transport work from GPU scheduling:
//! a key must write now, while an undamaged session must stay asleep (P2).
use super::report;
use std::path::Path;
/// System clipboard, host bindings and ordered PTY output (§13.1).
mod clipboard;
/// Configured cursor precedence, colors and live appearance changes (§14.2).
mod cursor;
/// §13.4 raster readback and deliberate stale-row diagnostics.
mod drawn;
/// Drained child status, hold policy and recording replay (§4.7).
mod exit;
/// Continuous-output frame, input, backlog and close measurements.
mod flood;
pub(in crate::accept) mod frame;
/// D-31 grapheme widths, tmux placement and selector invisibility.
mod grapheme;
/// §13.4 raw-input seam and general live-shell scenario checks.
mod keys;
/// Native PTY and winit key adapter checks for the M6.4 STOP.
mod live;
/// M6.4d stderr, file rotation and open-failure checks.
mod logging;
/// D-18 recorded macOS Option-key layout and side policy.
mod option_key;
/// M6.5 layered settings, native persistence and failed-reload safety.
mod settings;
/// D-30 pixel parity, atlas sharing and per-session frame budgets.
mod shared_context;
/// D-32 compiled entry, spawned TERM and tmux synchronized redraws.
mod terminfo;
/// M6.6 source-backed catalogue, contrast and inheritance rules.
mod themes;
/// D-29 measurements and incremental threading acceptance.
pub(super) mod threading;
/// Tmux's DEC region scroll must stay local and independent of history size.
mod vt_scroll;

/// Runs one required VT property or named-key table test by exact name.
fn vt_test(name: &str) -> Result<(), String> {
    // Cargo's process status alone says only that selected tests passed;
    // it also exits zero when a renamed test matches no filter.
    let output = crate::commands::capture(
        "cargo",
        &["test", "-p", "puzzle-vt", "--lib", name, "--", "--exact"],
    )?;
    // Require the exact executable result so a missing case fails R14.
    if output.contains(&format!("test {name} ... ok")) {
        Ok(())
    } else {
        Err(format!("puzzle-vt test {name} did not run"))
    }
}

/// Runs one session test exactly so renaming it cannot create a false PASS.
fn session_test(name: &str) -> Result<(), String> {
    // This helper uses the same exact-output guard as the VT test above;
    // Cargo exits successfully when a filter selects zero tests.
    let output = crate::commands::capture(
        "cargo",
        &["test", "-p", "puzzle-term", "--lib", name, "--", "--exact"],
    )?;
    if output.contains(&format!("test {name} ... ok")) {
        Ok(())
    } else {
        Err(format!("session test {name} did not run"))
    }
}

/// Requires exact instance strides rather than trusting an unused test filter.
fn frame_sizes() -> Result<(), String> {
    // A changed Rust stride would make the current WGSL vertex attributes
    // read neighboring instance fields even if the shader still compiled.
    let output = crate::commands::capture(
        "cargo",
        &["test", "-p", "puzzle-frame", "--lib", "--", "--nocapture"],
    )?;
    for name in [
        "rect_instance_is_thirty_two_bytes",
        "raster_instance_is_forty_bytes",
        "instance_is_fifty_six_bytes",
    ] {
        // The result must name the selected regression, not merely show
        // Cargo's successful exit for an empty test selection.
        if !output.contains(name) {
            return Err(format!("frame stride test {name} did not run"));
        }
    }
    Ok(())
}

/// Renders fresh mixed instances and compares the RGBA result with a reference.
fn frame_scene() -> Result<(), String> {
    // Recreate the attachment and pipeline before comparison. Comparing
    // yesterday's target file would miss a changed shader or upload path.
    let output = crate::commands::capture(
        "cargo",
        &["run", "-p", "puzzle-desktop", "--", "frame-probe"],
    )?;
    if !output.contains("draws=7 ink=") {
        return Err("frame probe did not draw the expected seven batches".into());
    }
    crate::golden::compare(
        // The same fixed §2.7 tolerance applies to all GPU references;
        // M6 adds no special allowance for its new rect pipeline.
        Path::new("target/frame/001-rect-slug.png"),
        Path::new("assets/scenarios/frame/001-rect-slug.png"),
    )
}

/// Reports the independent session and transport guarantees from M6.2.
fn report_session(passed: &mut bool) {
    // The same boundary case protects two distinct contracts: the parser
    // stops at the time budget, and replies leave before the next slice.
    report(
        "M6.2 64 KiB feed budget with manual clock",
        session_test("session::tests::feed_budget_and_replies_are_slice_ordered"),
        passed,
    );
    report(
        "M6.2 replies reach transport in slice order",
        session_test("session::tests::feed_budget_and_replies_are_slice_ordered"),
        passed,
    );
    // Input work is checked without a presentation call; an idle session
    // separately proves it does not ask the platform for a redraw.
    report(
        "M6.2 key bytes bypass the render frame",
        session_test("session::tests::key_reaches_transport_before_any_frame"),
        passed,
    );
    report(
        "M6.2 idle session schedules no frame",
        session_test("session::tests::idle_session_requests_no_frames"),
        passed,
    );
    // A pending suffix must continue on a scheduled turn even if the PTY
    // delivers no fresh packet after the first bounded feed call.
    report(
        "M6.2 queued suffix resumes without new PTY input",
        session_test("session::tests::pending_suffix_resumes_on_next_frame"),
        passed,
    );
    // Terminal-origin OSC requests traverse interceptors, while facts
    // reach their typed observers after the parser completes its slice.
    report(
        "M6.2 VT facts reach typed bus observers",
        session_test("session::tests::vt_facts_are_forwarded_in_order"),
        passed,
    );
    report(
        "M6.2 terminal host requests retain policy origin",
        session_test("session::tests::osc_clipboard_request_passes_through_interceptors"),
        passed,
    );
}

/// Reports each implemented M6 criterion and leaves review to the user.
pub(super) fn run() -> bool {
    let mut passed = true;
    // M6 inherits the full native/wasm gate while only M6.0 exists;
    // later acceptance lines cannot hide a platform regression.
    report(
        "native and wasm check",
        crate::commands::check(),
        &mut passed,
    );
    // The two reviewer findings are independent acceptance failures.
    report(
        "M6.0 styled VT round trip with scrollback",
        vt_test("view::tests::random_screen_vt_round_trip"),
        &mut passed,
    );
    report(
        "M6.0 kitty named key to published code table",
        vt_test("input::tests::kitty_all_published_named_pua_rows"),
        &mut passed,
    );
    report("M6.1 GPU instance byte strides", frame_sizes(), &mut passed);
    report(
        "M6.1 rect and Slug RenderFrame golden",
        frame_scene(),
        &mut passed,
    );
    report("M6.1 seven batched draw calls", frame_scene(), &mut passed);
    report_session(&mut passed);
    frame::report_frame(&mut passed);
    live::report_live(&mut passed);
    flood::report_flood(&mut passed);
    threading::report_baseline(&mut passed);
    threading::report_snapshot(&mut passed);
    threading::report_pool(&mut passed);
    threading::report_parse(&mut passed);
    threading::report_main(&mut passed);
    vt_scroll::report_scroll(&mut passed);
    grapheme::report_grapheme(&mut passed);
    logging::report_logging(&mut passed);
    terminfo::report_terminfo(&mut passed);
    keys::report_k1(&mut passed);
    keys::report_k2(&mut passed);
    drawn::report_k3(&mut passed);
    keys::report_k4(&mut passed);
    keys::report_k5(&mut passed);
    // K5b's phase sweep and real-PTY gaps have their own outcomes because
    // the original two-ms K5 case passed before the live display failed.
    keys::report_k5b(&mut passed);
    exit::report_exit(&mut passed);
    // K5c covers held-key bursts that the single-key K5b sweep missed.
    keys::report_k5c(&mut passed);
    option_key::report_option_key(&mut passed);
    clipboard::report_clipboard(&mut passed);
    cursor::report_cursor(&mut passed);
    shared_context::report_shared_context(&mut passed);
    settings::report_settings(&mut passed);
    themes::report_themes(&mut passed);
    // Human review remains visible even when a mechanical check fails.
    println!("REVIEW M6 comment quality: README.md review steps");
    println!("REVIEW M6.1 frame reference: assets/scenarios/frame/001-rect-slug.png");
    println!("REVIEW M6.3 terminal references: assets/scenarios/terminal-frame/*.png");
    println!("REVIEW M6.4 live terminal: cargo xtask run; checklist in README.md");
    println!("REVIEW M6.4h German Mac Option text: cargo xtask run; steps in README.md");
    println!("REVIEW M6.4i Cmd+V shell and nvim paste: cargo xtask run; steps in README.md");
    println!("REVIEW M6.4g cursor colors: target/harness/cursor-settings/*.png");
    println!("REVIEW M6.4b shared atlas: target/harness/shared-context/*.png");
    println!(
        "REVIEW M6.6 theme gallery: assets/scenarios/theme-gallery/*.png (confirmed by user 2026-10-01)"
    );
    println!("REVIEW M6 flood: cargo xtask run; run /usr/local/bin/DOOM-fire and follow README.md");
    println!("REVIEW M6-K5 live nvim and cold frames: commands and private evidence in README.md");
    println!("REVIEW M6-K5 comment quality: recognizer, keyecho and pacing paths in README.md");
    println!("REVIEW M6-K5b live nvim/vim pacing: ten presses each; steps in README.md");
    println!("REVIEW M6-K5b comment quality: settle, schedule and PTY replay in README.md");
    println!("REVIEW M6-K5c held-key nvim pacing: steps in README.md");
    passed
}
