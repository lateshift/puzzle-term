//! Exact witness gates retain counts, pixels, failures and every supporting run.
//!
//! K09/K10 use injected ownership/clock tests, not probabilistic retries as proof.
//! K19's historical masks were lost. The reviewer permits continuing with that
//! cause open: a recurrence after capture was added blocks M17.close until the
//! preserved bundle explains it. These repeats count observations, not a cure.
//! Diagnostic GPU tests run serially and finish before any live scenario starts.
//! No Chrome work is needed for these desktop-only fixture changes (D-46).
use super::report;
use std::{collections::BTreeMap, fs};

/// Checks each ownership/detector contract, then records repeated actual scenarios.
pub(super) fn run(passed: &mut bool) {
    for (criterion, name) in [
        // A second probe used to delete the first child’s global key file.
        // This witness retains a failed five-receipt journal across that launch.
        // It also rejects a receipt earlier than its issued timestamp.
        // Six-to-five pairing must fail before any zip can discard the sixth key.
        (
            "K09 private exact-six journals survive another probe",
            "window::unicode_probe::receipts::tests::independent_probes_preserve_failed_six_key_history",
        ),
        // A detached timer outlived the window whose input it measured.
        // Dropping its cancellation authority now wakes the pending wait.
        // Channel state supplies the clock-independent witness; no sleeps guess
        // whether a worker happened to reach its cancellation branch in time.
        (
            "K09 closed probe cancels remaining keys",
            "window::unicode_probe::receipts::tests::closed_probe_cancels_every_remaining_key",
        ),
        // Normal expiry and final flushing once sent the same value twice.
        // The injected final-frame costs straddle the existing throttle deadline.
        // Both routes must consume one pending notification, and a second close
        // must find nothing left to send. Each pending final notification is still consumed exactly once.
        (
            "K10 throttled notifications at either close edge",
            "harness::thread_storm::runtime::quiet_resize::tests::throttle_notifications_are_consumed_once_before_or_after_close",
        ),
        // Consuming the first throttle window must not suppress a later drag.
        // The second group owns a fresh last size and first-event deadline.
        // This distinguishes correct ownership from a process-wide sent flag
        // that would pass one drag by dropping every later PTY notification.
        (
            "K10 independent resize groups remain independent",
            "harness::thread_storm::runtime::quiet_resize::tests::completed_group_does_not_hide_a_later_resize",
        ),
        // The status text is at the historical physical row and both DPRs.
        // Its c glyph competes with independently rendered ASCII templates.
        // A forced prefix failure must save images even when cold pixels agree.
        // A later pass may not overwrite that earlier failed observation.
        (
            "K19 historical status prefix and immutable failure bundle",
            "harness::drawn::tests::status_recognition_and_failed_pixels_survive_retry",
        ),
        // The fresh builder must differ only in cache history.
        // Explicit cursor and theme preferences are presentation inputs too.
        // Default preferences cannot stand in for the source frame’s settings;
        // that would make a correct incremental image fail cold equality.
        (
            "K19 cold reference keeps presentation settings",
            "harness::drawn::tests::cold_reference_keeps_cursor_and_theme_settings",
        ),
        // Translation invariance is checked rather than assumed for thin ink.
        // Every physical location uses the same independent candidate sheet.
        // Both hinted 1× and unhinted 2× cells retain the original confidence
        // ceiling and ambiguity margin; no threshold is tuned to the status row.
        (
            "K19 independent thin-glyph candidates at every cell",
            "harness::drawn::tests::thin_status_glyph_matches_at_every_cell",
        ),
        // Matching the current VT scalar directly would conceal stale ink.
        // The injected old c/new d cell must fail through actual GPU pixels.
        // The saved c mask must be the exact mask scored during that failure,
        // not a later rerender that could hide an intermittent sheet defect.
        (
            "K19 stale ink rejected with exact original masks",
            "harness::drawn::tests::failed_grid_retains_original_candidate_masks",
        ),
    ] {
        // Each exact filter must execute one test, not silently match zero names.
        report(criterion, witness(name), passed);
    }
    // A single failing run stops that repeated witness and remains a gate failure.
    // report still evaluates the other independent criteria, as required by R14.
    report(
        "K10 three throttled GPU storm witnesses",
        repeat("thread-storm"),
        passed,
    );
    report(
        "K19 three actual PTY/GPU keys-sim witnesses",
        repeat("keys-sim"),
        passed,
    );
    // This is the binding reviewer disposition, not self-certification of a cause.
    println!(
        "REVIEW K19: cause open; any recurrence blocks M17.close until explained; run counts in target/m17-probe-accept/keys-sim/"
    );
}

/// Uses diagnostic clocks only where the fixture itself is feature-gated.
fn witness(name: &str) -> Result<(), String> {
    let output = crate::commands::capture(
        "cargo",
        &[
            "test",
            "-p",
            "puzzle-desktop",
            "--features",
            "lock-metrics",
            name,
            "--",
            "--exact",
        ],
    )?;
    // Exit zero alone would allow a renamed test to disappear from acceptance.
    if !output.contains(&format!("test {name} ... ok"))
        || !output.contains("test result: ok. 1 passed;")
    {
        return Err(format!("exact witness {name} did not execute"));
    }
    Ok(())
}

/// Saves every repetition separately before checking its original assertions.
fn repeat(stem: &str) -> Result<(), String> {
    let root = receipt_directory(stem)?;
    for run in 1..=3 {
        let scenario = format!("assets/scenarios/{stem}.pzs");
        let result = crate::commands::capture(
            "cargo",
            &[
                "run",
                "--release",
                "-p",
                "puzzle-desktop",
                "--",
                "harness",
                &scenario,
            ],
        );
        // Preserve the failing subprocess message before propagating its status.
        // The harness itself keeps immutable pixel/transcript failure bundles.
        let log = result
            .as_ref()
            .map_or_else(|error| error.as_str(), String::as_str);
        fs::write(format!("{root}/run-{run}.log"), log).map_err(|e| e.to_string())?;
        result?;
        if stem == "thread-storm" {
            // Copy the ordinary latest reports before another trial replaces them.
            // Both successful and failing numeric receipts remain inspectable.
            let data = fs::read_to_string("target/harness/thread-storm/after.txt")
                .map_err(|e| e.to_string())?;
            fs::write(format!("{root}/run-{run}-after.txt"), &data).map_err(|e| e.to_string())?;
            fs::copy(
                "target/harness/thread-storm/owner-publications.json",
                format!("{root}/run-{run}-owner-publications.json"),
            )
            .map_err(|e| e.to_string())?;
            storm(&data)?;
        }
    }
    Ok(())
}

/// Keeps exact event/count/final-grid contracts beside the unchanged flood limits.
fn storm(data: &str) -> Result<(), String> {
    let fields = data
        .lines()
        .filter_map(|line| line.split_once('='))
        .collect::<BTreeMap<_, _>>();
    let number = |key: &str| -> Result<f64, String> {
        let value = fields
            .get(key)
            .ok_or_else(|| format!("missing {key}"))?
            .parse::<f64>()
            .map_err(|e| e.to_string())?;
        value
            .is_finite()
            .then_some(value)
            .ok_or_else(|| format!("nonfinite {key}"))
    };
    // Checking both axes prevents a correct final width from hiding stale height.
    // D-29 throttles during the three-second drag: at most one per 25-ms
    // window plus the final fixture flush; more than one rejects trailing starvation.
    if number("storm_resize_events")? != 180.0
        || number("storm_pty_calls")? <= 1.0
        || number("storm_pty_calls")? > (3_000 / 25 + 1) as f64
        || number("storm_grid_cols")? != number("storm_pty_cols")?
        || number("storm_grid_rows")? != number("storm_pty_rows")?
    {
        return Err(format!("throttled PTY/grid mismatch: {data}"));
    }
    let steady = number("steady_throughput_mb_s")?;
    // No ratio can pass against a stopped parser; the denominator must be positive.
    // These are the existing M6/M16 storm bounds, not probe-specific replacements.
    if steady <= 0.0
        || number("storm_throughput_mb_s")? < 0.9 * steady
        || number("storm_fps")? < 30.0
    {
        return Err(format!("resize storm budget: {data}"));
    }
    Ok(())
}

/// Atomically reserves each acceptance invocation so later runs cannot erase it.
fn receipt_directory(stem: &str) -> Result<String, String> {
    let parent = format!("target/m17-probe-accept/{stem}");
    fs::create_dir_all(&parent).map_err(|e| e.to_string())?;
    // A PID can be reused; skip existing ordinals rather than replacing evidence.
    // The gate lock excludes other gates but does not make old directories reusable.
    for ordinal in 0u64.. {
        let path = format!("{parent}/{}-{ordinal}", std::process::id());
        // Creation, not an existence check, reserves the identity without a race.
        match fs::create_dir(&path) {
            Ok(()) => return Ok(path),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error.to_string()),
        }
    }
    // Exhaustion is a setup failure, never permission to overwrite a prior trial.
    Err("probe acceptance identities exhausted".into())
}
