//! M6.4d checks logging through a real native process and PTY smoke path.
//!
//! A unit test of the writer alone cannot detect a console layer left on
//! stdout, or a failure to install the second subscriber layer. The smoke
//! process logs one stable lifecycle event and exits after PTY output has
//! reached VT cells, so it tests log routing without opening an OS window.
//! Each run owns files under target and uses an explicit PUZZLE_LOG value.
//!
//! Three process runs expose separate contracts. With no file option,
//! stdout must remain empty while stderr contains the smoke event. With
//! `PUZZLE_LOG_FILE`, both processes still finish their PTY smoke, and
//! the first file must become PATH.1 when the second begins. A failure to
//! rotate can otherwise hide in a successful one-run file test. Finally,
//! a regular file is deliberately used as a parent directory so opening
//! a nested path fails regardless of filesystem permission settings.
//!
//! The formatter's wire form matters to bug reports. The file must not
//! carry ANSI control bytes that would make an editor or parser misread
//! fields. The timestamp is expected in UTC with six fractional digits;
//! a local-time timestamp from the console would be ambiguous when the
//! user sends a recording across time zones. These checks use actual
//! output instead of inspecting the builder flags in source.
//!
//! The size-rollover case uses a unit test with a tiny cap, because
//! writing 64 MiB during every milestone acceptance run would obscure
//! the behavior being checked and slow unrelated development. The unit
//! test calls the same MakeWriter implementation as production. Its exact
//! name is checked so Cargo's zero-selected-test success cannot report
//! PASS after a rename. A failure in one sink does not skip another sink's
//! criterion; the milestone reporter keeps all outcomes visible.

use super::report;
use std::{fs, path::Path, process::Command};

/// Captures stdout and stderr independently with one log-path override.
fn smoke(file: Option<&Path>) -> Result<std::process::Output, String> {
    // cargo run reaches the same binary startup path as the interactive
    // shell while avoiding a GPU window in this deterministic check.
    let mut command = Command::new("cargo");
    command.args(["run", "-p", "puzzle-desktop", "--", "pty-smoke"]);
    // Remove any ambient developer destination first so console-only
    // acceptance remains independent of the caller's environment.
    command
        .env("PUZZLE_LOG", "info")
        .env_remove("PUZZLE_LOG_FILE");
    if let Some(path) = file {
        command.env("PUZZLE_LOG_FILE", path);
    }
    let output = command.output().map_err(|error| error.to_string())?;
    // PTY smoke must pass before its log can count as evidence; an early
    // startup error could also emit a record on stderr.
    output
        .status
        .success()
        .then_some(output)
        .ok_or("PTY smoke failed".into())
}

/// Ensures the console keeps stdout available for command results.
fn stderr_only() -> Result<(), String> {
    let output = smoke(None)?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    // Cargo also writes build messages to stderr. The stable lifecycle
    // event singles out tracing output without relying on Cargo wording.
    if !output.stdout.is_empty() || !stderr.contains("desktop.smoke_started") {
        return Err("smoke log missing from stderr or appeared on stdout".into());
    }
    Ok(())
}

/// Starts twice to prove no ANSI, UTC microseconds and previous-run retention.
fn file_sink() -> Result<(), String> {
    // Files under target belong to this acceptance check; stale previous
    // results cannot stand in for a new process invocation.
    let path = Path::new("target/logging-accept/session.log");
    let previous = Path::new("target/logging-accept/session.log.1");
    let _ = fs::remove_file(path);
    let _ = fs::remove_file(previous);
    smoke(Some(path))?;
    // Retain first-run bytes before the second startup moves them aside.
    let first = fs::read(path).map_err(|error| error.to_string())?;
    smoke(Some(path))?;
    let current = fs::read(path).map_err(|error| error.to_string())?;
    let old = fs::read(previous).map_err(|error| error.to_string())?;
    // Byte equality proves the prior run was kept intact, rather than
    // creating only an empty predecessor with the expected filename.
    if first != old || current.is_empty() || current.contains(&0x1b) {
        return Err("file log did not rotate clean records without ANSI".into());
    }
    // tracing-subscriber's SystemTime prints an ISO UTC timestamp ending
    // in six fractional digits and Z; check that wire shape in each run.
    let text = String::from_utf8(current).map_err(|error| error.to_string())?;
    let stamp = text.split_whitespace().next().ok_or("file log empty")?;
    // The Z suffix names UTC; exactly six digits before it provide the
    // requested microsecond precision for cross-machine comparisons.
    let fraction = stamp
        .split('.')
        .nth(1)
        .ok_or("timestamp lacks microseconds")?;
    if !fraction.ends_with('Z') || fraction.len() != 7 {
        return Err(format!("timestamp is not UTC microseconds: {stamp}"));
    }
    Ok(())
}

/// A bad destination warns once but leaves the PTY command successful.
fn open_failure() -> Result<(), String> {
    // A regular file used as a parent is unwritable on every OS; chmod
    // would be unreliable when a test runs with elevated privileges.
    let blocker = Path::new("target/logging-accept/not-a-directory");
    fs::create_dir_all("target/logging-accept").map_err(|error| error.to_string())?;
    fs::write(blocker, b"file").map_err(|error| error.to_string())?;
    let output = smoke(Some(&blocker.join("session.log")))?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    // One warning reports the startup failure without flooding a log
    // every time the still-running terminal emits another trace event.
    if stderr.matches("desktop.log_file_open_failed").count() != 1 {
        return Err("unwritable log path did not emit exactly one warning".into());
    }
    Ok(())
}

/// Exercises the production writer with a short test cap for size rotation.
fn size_rotation() -> Result<(), String> {
    // Exact-name output is required because Cargo succeeds with no tests
    // when a filter misses the intended writer regression.
    let name = "logging::file::tests::startup_and_size_rotation_keep_one_previous_run";
    let output = crate::commands::capture(
        "cargo",
        &["test", "-p", "puzzle-desktop", name, "--", "--exact"],
    )?;
    output
        .contains(&format!("test {name} ... ok"))
        .then_some(())
        .ok_or_else(|| format!("desktop test {name} did not run"))
}

/// Reports each requested §5.6 behavior even if a different sink fails.
pub(super) fn report_logging(passed: &mut bool) {
    report("M6.4d console logs only to stderr", stderr_only(), passed);
    report(
        "M6.4d log file is plain UTC and keeps PATH.1",
        file_sink(),
        passed,
    );
    report(
        "M6.4d file-open failure warns and starts",
        open_failure(),
        passed,
    );
    report("M6.4d 64 MiB line rotation writer", size_rotation(), passed);
}
