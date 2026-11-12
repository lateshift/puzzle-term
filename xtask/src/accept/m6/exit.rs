//! M6.4f acceptance over a real PTY and a typed replayed exit command.
//!
//! Separate lines keep an early abnormal exit from being hidden by a normal
//! close result. Each PTY probe starts a fixed shell child and waits for EOF
//! before delivering ChildExited, so it checks the ordering at the native
//! transport seam as well as the neutral Session policy.
use super::super::report;

/// Runs one fixed real-PTY child lifecycle without opening a window.
fn probe(case: &str) -> Result<(), String> {
    // The command exits after its own finite PTY deadline, so acceptance
    // cannot hang if the reader fails to observe EOF on this platform.
    crate::commands::quiet(
        "cargo",
        &["run", "-p", "puzzle-desktop", "--", "exit-probe", case],
    )
}

/// Requires a named unit test to execute rather than matching zero tests.
fn exact_test(package: &str, name: &str) -> Result<(), String> {
    // Cargo test filters are substring matches by default; --exact makes a
    // renamed or omitted regression test fail this acceptance criterion.
    let mut args = vec!["test", "-p", package];
    // The desktop tests belong to its binary target, whereas Session's
    // lifecycle tests live in the platform-neutral library target.
    if package == "puzzle-desktop" {
        args.extend(["--bin", "puzzle-desktop"]);
    } else {
        args.push("--lib");
    }
    args.extend([name, "--", "--exact"]);
    // A zero-test cargo success is not evidence that the named contract ran.
    let output = crate::commands::capture("cargo", &args)?;
    if output.contains(&format!("test {name} ... ok")) {
        Ok(())
    } else {
        Err(format!("{name} did not run"))
    }
}

/// Reports every current §4.7 criterion even if an earlier child fails.
pub(super) fn report_exit(passed: &mut bool) {
    // The real child proves drain order at the PTY boundary; the Session test
    // proves the bus publishes its fact after the final VT mutation.
    // Each report call executes independently: a failed shell spawn does not
    // suppress the replay or origin checks later in this group.
    report(
        "M6.4f final PTY output precedes SessionExited code 3 and close",
        probe("close").and_then(|_| {
            exact_test(
                "puzzle-term",
                "session::tests::child_exit_drains_before_session_exited",
            )
        }),
        passed,
    );
    // A held notice is inspected in the grid, where frame building will
    // obtain it, instead of merely checking a policy flag.
    // The status text includes the numeric code, preventing a generic
    // "process ended" placeholder from satisfying the held-screen rule.
    report(
        "M6.4f hold paints dim exit notice and waits for a key",
        probe("hold"),
        passed,
    );
    report_hold_regressions(passed);
    // Fast failures use the default Close preference to exercise the
    // abnormal-runtime exception rather than the ordinary Hold setting.
    report(
        "M6.4f exit 1 under 250 ms holds despite close policy",
        probe("abnormal"),
        passed,
    );
    // An Arc write counter sees the actual transport after Session owns it;
    // the neutral test also checks the emitted close-request event.
    report(
        "M6.4f post-exit key requests close without transport write",
        probe("no-write").and_then(|_| {
            exact_test(
                "puzzle-term",
                "session::tests::held_exit_resets_keyboard_and_key_requests_close_without_write",
            )
        }),
        passed,
    );
    // The replay parser must preserve the trusted platform boundary and
    // status payload after all recorded output chunks.
    report(
        "M6.4f recording replay ends with the same ChildExited",
        exact_test(
            "puzzle-desktop",
            "harness::replay_actions::tests::recorded_child_exit_replays_after_last_output",
        ),
        passed,
    );
    // API users can observe the exit fact but cannot impersonate the child
    // process or terminate the session by submitting an internal command.
    report(
        "M6.4f API origin cannot forge child exit",
        exact_test(
            "puzzle-term",
            "session::tests::api_origin_cannot_end_child_session",
        ),
        passed,
    );
}

/// Guards the final grid content that remains visible under the Hold policy.
fn report_hold_regressions(passed: &mut bool) {
    // A notice appended after a full grid must leave the last error visible
    // above it, while preserving the normal primary-screen history motion.
    report(
        "M6.4f held notice preserves a full-screen final error",
        exact_test(
            "puzzle-term",
            "session::tests::held_exit_preserves_full_screen_final_error",
        ),
        passed,
    );
    // Resetting input protocols cannot reset DECSCNM or invent an exit code
    // when the PTY API reports only EOF.
    report(
        "M6.4f held screen retains display modes and unknown status",
        exact_test(
            "puzzle-term",
            "session::tests::held_exit_keeps_display_modes_and_handles_unknown_code",
        ),
        passed,
    );
}
