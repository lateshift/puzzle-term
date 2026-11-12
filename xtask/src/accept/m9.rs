//! Ordered M9 acceptance, extended only after each work-order task is built.
//!
//! Exact named witnesses prevent Cargo's successful empty-filter behavior
//! from accepting a removed contract. Visual and browser approvals remain
//! human review items; this command never certifies them from test status.
//!
//! M9.1 acceptance names tracked anchor lifetime, logical/device geometry,
//! idle allocation, callback handoff and snapshot isolation independently.
//! The full task gate runs first but failures accumulate: a failed lint must
//! not hide whether the actual behavioral witnesses executed.
//!
//! Integration witnesses use the public registry and VT API. Session tests
//! exercise the external builder handoff used by desktop, including parser
//! output arriving after capture. Those tests compare neutral metadata with
//! submitted glyph instances rather than reconstructing placement math.
//!
//! Frozen snapshot tests protect the user-approved visible-row contract on
//! both targets. Warm metadata/cluster copies are checked with the existing
//! approved allocation counter. Clip tests inspect the GPU-neutral contract
//! so an empty or leaked scissor fails before browser or native rendering.
//!
//! Further work-order obligations are added here as their tasks land. The
//! authorized STOP after M9.4 is an implementation boundary, not milestone
//! acceptance: pending reference and desktop/Chrome reviews remain visible.
//!
//! Task selectors run the same witness functions as milestone acceptance. They
//! do not waive historical lines; final acceptance still executes the complete set.
//! Cursor/status specimens check real GPU ink independently of source-model tests.
/// OSC duration and policy-preserving output copy.
mod blocks;
/// Declaration source lifetimes and shared pointer ownership.
mod decorations;
/// Exact matcher continuations and the Session-owned shortcut query mode.
mod help;
/// Opt-in completed-frame statistics and debug painter.
mod hud;
/// Shape, text, effect and bitmap-only exception contracts.
mod ink;
/// Presented link identity and final opener policy.
mod links;
/// Typical dirty/idle cost and the normal shipped wasm artifact.
mod measurements;
/// Scrollbar, following pill and native query mode on the existing M8 models.
mod models;
/// Real interactive bash, zsh and fish semantic boundaries.
mod shells;
/// Cursor/status built-ins grow the task inventory without bypassing full acceptance.
mod visuals;
use super::{m7, report};

/// Executes one integration witness and checks that the named test actually ran.
fn core_test(name: &str) -> Result<(), String> {
    let output = crate::commands::capture(
        "cargo",
        &[
            "test",
            "-p",
            "puzzle-overlay",
            "--test",
            "core",
            name,
            "--",
            "--exact",
        ],
    )?;
    // A typo or later test rename cannot turn an obligation into zero executed tests.
    if !output.contains(&format!("test {name} ... ok")) {
        return Err(format!("overlay witness {name} did not run"));
    }
    Ok(())
}

/// Checks all implemented core obligations without hiding failures after the first.
fn core(passed: &mut bool) {
    for (criterion, name) in [
        (
            "M9.1 anchors follow output/reflow/eviction and auto_remove",
            "anchors_follow_output_reflow_and_eviction",
        ),
        (
            "M9.1 idle registered providers: zero work and allocation",
            "registered_idle_provider_does_no_work_or_allocation",
        ),
        (
            "M9.1 logical cells/ranges/hits and device helpers at DPR 1/2",
            "logical_cells_ranges_and_hits_at_two_dprs",
        ),
        (
            "M9.1 external-frame registrations and animation endpoints",
            "registration_survives_skipped_frames_and_animation_finishes",
        ),
        (
            "M9.1 frozen BufferView: visible rows only, reusable copies",
            "visible_snapshot_is_frozen_and_warm_copy_is_allocation_free",
        ),
    ] {
        // Each witness protects an independent public guarantee in §11.1–§11.5.
        report(criterion, core_test(name), passed);
    }
    report(
        "M9.1 geometry equals grid placements at DPR 1/2",
        m7::test(
            "puzzle-term",
            "session::tests::overlays::geometry_equals_grid_placements_at_two_dprs",
        ),
        passed,
    );
    report(
        "M9.1 nested scissors and stable z-slot ordering",
        m7::test(
            "puzzle-term",
            "session::tests::overlays::nested_clips_restore_and_slots_keep_submission_order",
        ),
        passed,
    );
}

/// Runs the authorized task range, followed by pending human reviews (R15).
pub(super) fn run(browser: super::BrowserGate) -> bool {
    // Mechanical status remains separate from the pending editorial review.
    // The caller propagates any failure as a nonzero process exit.
    let mut passed = true;
    // The task gate includes native GPU tests, wasm Clippy and source rules.
    report(
        "native and wasm check",
        crate::commands::check(),
        &mut passed,
    );
    core(&mut passed);
    // Native readback tests Top padding independently from grid clipping.
    report(
        "M9.1 public provider harness pixels at DPR 1/2",
        crate::commands::quiet(
            "cargo",
            &["xtask", "harness", "assets/scenarios/overlay-core.pzs"],
        ),
        &mut passed,
    );
    // U2 schedules real PTY/socket witnesses beside the Chrome closure gates.
    super::bridge::closure(browser, &mut passed);
    ink::run(&mut passed, browser);
    decorations::run(&mut passed);
    models::run(&mut passed);
    visuals::run(&mut passed);
    help::run(&mut passed);
    links::run(&mut passed);
    blocks::run(&mut passed);
    hud::run(&mut passed);
    shells::run(&mut passed);
    // Final measurements follow the behavioral witnesses, so a failed timing
    // cannot suppress evidence about shell zones, pointer ownership or ink.
    measurements::run(&mut passed);
    println!(
        "REVIEW M9 core comment quality: pending; crates/puzzle-overlay and frame_builder/overlays; README.md"
    );
    passed
}

/// Task receipts omit unrelated historical cohorts but enforce their own exact witnesses.
pub(super) fn task(args: &[String]) -> bool {
    let mut passed = true;
    match args {
        [flag, name] if flag == "--task" && name == "visuals" => visuals::run(&mut passed),
        [flag, name] if flag == "--task" && name == "help" => help::run(&mut passed),
        [flag, name] if flag == "--task" && name == "links" => links::run(&mut passed),
        [flag, name] if flag == "--task" && name == "blocks" => blocks::run(&mut passed),
        [flag, name] if flag == "--task" && name == "hud" => hud::run(&mut passed),
        [flag, name] if flag == "--task" && name == "shells" => shells::run(&mut passed),
        [flag, name] if flag == "--task" && name == "measurements" => {
            measurements::run(&mut passed)
        }
        _ => report(
            "M9 task arguments",
            Err("usage: accept M9 --task visuals|help|links|blocks|hud|shells|measurements".into()),
            &mut passed,
        ),
    }
    passed
}
