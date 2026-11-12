//! Native embedding API receipts, distinct from unfinished shared-resource acceptance.
//!
//! Calls are checked in every build, not merely with debug assertions. The
//! optimized misuse witness guards that distinction. Lifecycle tests use real
//! PTYs and held children but no drawable: their assertions are functional,
//! and a generous deadlock guard never serves as a performance budget.
//!
//! This selector records only the facade contracts. It does not claim the
//! two-window cadence/key integration or the complete M17 milestone passed.
//!
//! The harness enables test-support for observational probes. Its live callbacks
//! must still use the checked Surface API, so a separate lint fixture attempts
//! to import diagnostic owners from an ordinary callback module and must fail.
//! The raw-input witness then exercises the real control inbox: configuration,
//! modifiers and composition retain order even across cloned Surface endpoints.
//! Default-feature documentation checks prove those private owner paths remain
//! unavailable to native hosts, independently of the harness's feature set.
//!
//! The functional lifetime cases keep real children and real drain/writer
//! threads. They deliberately do not acquire a drawable: worker cancellation
//! must not depend on a display being available or a compositor releasing one.
//! A separate shared-resource receipt performs actual GPU uploads and readback.
//! Neither category replaces the two-window integration's measured cadence.
//!
//! A stale endpoint is exercised again after a replacement surface exists.
//! That distinguishes rejecting a freed identity from merely accepting its
//! input into a disconnected queue. The release run also checks wrong-thread
//! rejection, since that contract must survive removal of debug assertions.
use crate::accept::report;

/// Executes each exact witness; missing test filters remain acceptance failures.
pub(super) fn run(passed: &mut bool) {
    for (criterion, test) in [
        (
            "checked surface thread/lifetime errors",
            "wrong_thread_and_closed_surfaces_are_rejected",
        ),
        (
            "host-thread tick event delivery",
            "tick_delivers_child_facts_and_effects_on_host_thread",
        ),
        (
            "App drop joins flooding and held Sessions",
            "app_drop_joins_open_surfaces",
        ),
    ] {
        // Exact names move with the owner; the helper rejects a zero-test success.
        let name = format!("app::tests::{test}");
        report(
            criterion,
            crate::accept::m7::test("puzzle-embed", &name),
            passed,
        );
    }
    // Runtime affinity must not disappear when debug assertions compile away.
    report(
        "checked surface misuse in release",
        super::release_test("app::tests::wrong_thread_and_closed_surfaces_are_rejected"),
        passed,
    );
    report(
        "host callbacks cannot import diagnostic owners",
        crate::accept::m7::test(
            "xtask",
            "lint::embedding::tests::ordinary_callbacks_cannot_import_owners_or_diagnostic_modules",
        ),
        passed,
    );
    report(
        "raw input uses one Session-owned composition and policy stream",
        crate::accept::m7::test(
            "puzzle-embed",
            "session_thread::tests::input::raw_input_policy_composition_and_focus_are_owner_ordered",
        ),
        passed,
    );
    report(
        "default host API and private owner doc tests",
        default_host(),
        passed,
    );
    // The host find UI receives bounded progress through its event stream.
    // Scanning remains refresh-driven; a host callback never borrows Session.
    report(
        "search count/progress/navigation/clear events are change-only",
        crate::accept::m7::test(
            "puzzle-term",
            "session::tests::host_search::search_progress_navigation_and_clear_are_change_only",
        ),
        passed,
    );
}

/// A feature-enabled harness build cannot substitute for checking a native host build.
fn default_host() -> Result<(), String> {
    // No test-support in this command: its only public modules must be App/Surface types.
    let output = crate::commands::capture(
        "cargo",
        &[
            "test",
            "-p",
            "puzzle-embed",
            "--no-default-features",
            "--doc",
        ],
    )?;
    // Two compile-fail examples independently reject the Session and render owners.
    if output.matches("compile fail ... ok").count() != 2 {
        return Err("private owner doc tests did not both execute".into());
    }
    // Clippy checks the feature-off library and its retained functional fixtures too.
    crate::commands::quiet(
        "cargo",
        &[
            "clippy",
            "-p",
            "puzzle-embed",
            "--all-targets",
            "--no-default-features",
            "--",
            "-D",
            "warnings",
        ],
    )
}

/// GPU contracts prove sharing through real uploads and actual device destruction.
/// The first witness checks both format changes and last-user release. The second
/// changes the glyph allowance and DPR without mutating its neighbour, then destroys
/// the real device and compares ink for every retained context after replacement.
/// These are functional pixel contracts; their debug runtimes have no speed limit.
/// Neither receipt substitutes for the separate two-window cadence/key measurement.
/// A successful empty filter must not hide a renamed resource-sharing test.
pub(super) fn resources(passed: &mut bool) {
    for (criterion, test) in [
        (
            "matching surfaces reuse prepared ink across formats",
            "matching_surfaces_share_ink_and_release_the_last_context",
        ),
        (
            "surface policy isolation and shared-device recovery",
            "policy_changes_keep_other_surfaces_and_device_recovery_restores_all_contexts",
        ),
    ] {
        // The exact filter rejects an accidentally deleted or renamed GPU witness.
        report(
            criterion,
            crate::accept::m7::test(
                "puzzle-embed",
                &format!("render_thread::resources::tests::{test}"),
            ),
            passed,
        );
    }
}
