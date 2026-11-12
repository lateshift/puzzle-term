//! Release diagnostic windows preserve full reports and explicit comparison geometry.
//!
//! Ordinary workloads keep their existing ten-pixel fixed-size configuration.
//! The D-47 comparison explicitly maximizes a fifteen-pixel window on the external
//! sixty-Hz display. Shared environment flags cannot silently alter another cohort.
//! Every window has an actual finite PTY producer; empty or stale reports fail.
use std::{fs, process::Command};
/// Owns one diagnostic window and saves its entire raw JSON without replacing any prior run.
///
/// `script` is an existing or locally generated neutral child producer. `workload`
/// selects one of its real commands; `pressure` enables the long full-corpus mode.
/// The explicit empty config keeps personal settings out of comparative evidence.
/// Timing instrumentation only observes ordinary source preparation and submission.
/// A missing fresh report fails even when the native process exits successfully.
/// All process output and structured JSON stay under target, never shipped assets.
pub(super) fn run(
    script: &str,
    workload: Option<&str>,
    pressure: bool,
    label: &str,
    maximized: bool,
) -> Result<serde_json::Value, String> {
    launch(script, workload, pressure, label, maximized, false)
}

/// Observe ordinary cadence without periodic GPU readback; ink is checked separately.
pub(super) fn cadence(script: &str) -> Result<serde_json::Value, String> {
    launch(script, None, true, "m165-cadence", true, true)
}

/// Both cohorts share the real host, producer, geometry and frame executor.
fn launch(
    script: &str,
    workload: Option<&str>,
    pressure: bool,
    label: &str,
    maximized: bool,
    cadence_only: bool,
) -> Result<serde_json::Value, String> {
    // Both directories exist even if the earlier pressure producer failed to initialize.
    fs::create_dir_all("target/m16-2").map_err(|e| e.to_string())?;
    fs::create_dir_all("target/m11-unicode").map_err(|e| e.to_string())?;
    // Default settings establish the same reproducible font and rendering policy.
    fs::write("target/m11-unicode/config.json", "{}").map_err(|e| e.to_string())?;
    // Removing the old report prevents a crashed child from inheriting another run's pass.
    let _ = fs::remove_file("target/m11-unicode/native.json");
    // Dependent warm observations must not reuse a prior successful labelled report.
    let _ = fs::remove_file(format!("target/m16-2/{label}.json"));
    // Keep stderr as well as stdout because startup/PTY failures need their original reason.
    let log =
        fs::File::create(format!("target/m16-2/{label}-window.log")).map_err(|e| e.to_string())?;
    // Launch the native window, including its real toolkit, PTY and process render actor.
    let mut command = Command::new("target/release/puzzle-desktop");
    configure(&mut command, script, label, maximized);
    if cadence_only {
        // This report intentionally cannot satisfy the separate GPU ink criterion.
        command.env("PUZZLE_UNICODE_CADENCE_ONLY", "1");
    }
    command
        .stdout(log.try_clone().map_err(|e| e.to_string())?)
        .stderr(log);
    // A workload cannot accidentally inherit pressure mode from the caller's environment.
    command.env_remove("PUZZLE_UNICODE_PRESSURE");
    if pressure {
        // A failed producer must not inherit a previous vtebench table.
        let _ = fs::remove_file("target/m16-2/puzzle-vtebench.dat");
        command.env("PUZZLE_UNICODE_PRESSURE", "1");
    }
    // The recorded Ctrl+D chord reaches the real app one second after its handshake.
    if let Some(name) = workload {
        command
            .env("PUZZLE_WORKLOAD", name)
            .env("PUZZLE_UNICODE_KEY_DELAY_MS", "1000")
            .env("PUZZLE_UNICODE_KEY", "ctrl+d");
    }
    // The probe owns its finite observation interval and requests an orderly host close.
    if !command.status().map_err(|e| e.to_string())?.success() {
        return Err(format!("{label} window failed"));
    }
    let value = evidence(label)?;
    if pressure {
        // Pair the producer table with this exact window before another cohort starts.
        fs::copy(
            "target/m16-2/puzzle-vtebench.dat",
            format!("target/m16-2/{label}-vtebench.dat"),
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(value)
}

/// Saves the fresh report and screenshot before another diagnostic reuses their paths.
/// The native process has exited, so reading these artifacts cannot perturb timing.
/// Parsing retains all nested raw samples; an absent image or malformed JSON fails.
fn evidence(label: &str) -> Result<serde_json::Value, String> {
    // A fresh missing report remains a failure even when the process exited normally.
    let bytes = fs::read("target/m11-unicode/native.json").map_err(|e| format!("{label}: {e}"))?;
    // Copy before opening another window, which reuses the producer's fixed report path.
    fs::write(format!("target/m16-2/{label}.json"), &bytes).map_err(|e| e.to_string())?;
    // Malformed diagnostic JSON fails acceptance instead of losing fields silently.
    // Retain each workload image before the next probe overwrites its shared PNG path.
    fs::copy(
        "target/m11-unicode/frozen.png",
        format!("target/m16-2/{label}.png"),
    )
    .map_err(|e| e.to_string())?;
    serde_json::from_slice(&bytes).map_err(|e| e.to_string())
}

/// Explicit comparison geometry never leaks into ordinary workload acceptance windows.
fn configure(command: &mut Command, script: &str, label: &str, maximized: bool) {
    // All paths are structured process arguments; no shell concatenation occurs here.
    command
        .args([
            "run",
            "--config",
            "target/m11-unicode/config.json",
            "--set",
            "font.size=10",
        ])
        // An explicit shell bypasses user startup content before the neutral producer.
        .env("SHELL", "/bin/sh")
        .env("LC_ALL", "en_US.UTF-8")
        .env("PUZZLE_UNICODE_PROBE", script)
        // Readiness/appearance observation is opt-in and does not introduce glyph queues.
        .env("PUZZLE_GLYPH_TIMING", "1")
        .env("PUZZLE_LOG_FILE", format!("target/m16-2/{label}.log"));

    // Task launches cannot inherit a fullscreen or maximized diagnostic from their parent.
    command
        .env_remove("PUZZLE_UNICODE_MAXIMIZED")
        .env_remove("PUZZLE_UNICODE_FULLSCREEN")
        // Timing-only cohorts cannot accidentally omit mandatory GPU ink samples.
        .env_remove("PUZZLE_UNICODE_CADENCE_ONLY");
    if maximized {
        command
            .env("PUZZLE_UNICODE_MAXIMIZED", "1")
            .env("PUZZLE_SCROLL_PROBE_MILLIHZ", "60000")
            .args(["--set", "font.size=15"]);
    }
}
