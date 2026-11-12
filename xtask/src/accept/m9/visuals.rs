//! Cursor/status obligations use exact Session witnesses and native GPU images.
//!
//! Tests pin source ownership, committed-input ordering and finite scheduling.
//! A named test must really execute; a successful empty Cargo filter is rejected.
//! Harness specimens independently assert annotation ink at both device scales.
//! These task receipts extend full M9 acceptance rather than replacing any line.
//! Reference appearance is reported separately for human review under R15.
//!
//! Composition and suggestions must remain outside the terminal grid and transport.
//! The explicit acceptance witness also checks that a child which never echoes input
//! cannot leave accepted suggestion ink behind. Progress is checked through parsed OSC,
//! not by manufacturing rectangle instances. Bell tests inject model time, removing
//! scheduler variability while proving finite reduced-motion removal and idle cost.
//! The GPU specimens exercise the same compiler used by a live native surface.
use super::super::{m7::test, report};

/// Runs each completed cursor/status obligation without hiding later failures.
pub(super) fn run(passed: &mut bool) {
    for (criterion, name) in [
        (
            "preedit and ghost stay outside VT/PTY; explicit acceptance only",
            "preedit_and_ghost_are_public_cursor_overlays_not_terminal_output",
        ),
        (
            "OSC progress renders and clears from the source",
            "osc_progress_draws_and_clears_from_its_source",
        ),
        (
            "reduced-motion bell is static, finite and allocation-free while idle",
            "bell_reduced_motion_is_static_and_finishes_with_zero_idle_allocations",
        ),
    ] {
        // The exact path prevents a rename from silently dropping the contract.
        report(
            &format!("M9.5 {criterion}"),
            test(
                "puzzle-term",
                &format!("session::tests::builtin_visuals::{name}"),
            ),
            passed,
        );
    }
    for scene in ["overlay-ghost", "overlay-preedit", "overlay-status"] {
        // The production compiler and renderer must submit real annotation ink.
        report(
            &format!("M9.5 {scene} GPU ink at DPR 1/2"),
            crate::commands::quiet(
                "cargo",
                &["xtask", "harness", &format!("assets/scenarios/{scene}.pzs")],
            ),
            passed,
        );
    }
    println!(
        "REVIEW M9.5 cursor/status appearance: pending; target/harness/overlay-ghost and overlay-preedit; README.md"
    );
}
