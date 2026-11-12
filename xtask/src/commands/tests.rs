//! Run-profile selection is checked without starting a GUI process.
use super::live_run_args;

/// The interactive default uses the optimized, incremental profile.
#[test]
fn live_run_is_optimized() {
    assert_eq!(
        live_run_args(&[]).ok().map(|args| args[1]),
        Some("--profile")
    );
    assert_eq!(live_run_args(&[]).ok().map(|args| args[2]), Some("live"));
}

/// Explicit debugging preserves Cargo's ordinary dev profile.
#[test]
fn debug_run_uses_dev() {
    assert_eq!(
        live_run_args(&["--debug"]).ok(),
        Some(vec!["run", "-p", "puzzle-desktop", "--", "run"])
    );
}

/// A requested file path reaches the native binary after Cargo's `--`.
#[test]
fn log_file_is_forwarded() {
    assert_eq!(
        live_run_args(&["--log-file", "target/user.log"]).ok(),
        Some(vec![
            "run",
            "--profile",
            "live",
            "-p",
            "puzzle-desktop",
            "--",
            "run",
            "--log-file",
            "target/user.log"
        ])
    );
}

/// Recording without a path uses the binary's timestamped target file.
#[test]
fn recording_is_forwarded() {
    assert_eq!(
        live_run_args(&["--record", "--debug"]).ok(),
        Some(vec!["run", "-p", "puzzle-desktop", "--", "run", "--record"])
    );
    assert_eq!(
        live_run_args(&["--record", "target/recordings/ctrl-d.pzrec"]).ok(),
        Some(vec![
            "run",
            "--profile",
            "live",
            "-p",
            "puzzle-desktop",
            "--",
            "run",
            "--record",
            "target/recordings/ctrl-d.pzrec"
        ])
    );
}

/// File, profile and repeated session overrides reach the native parser.
#[test]
fn settings_options_are_forwarded() {
    assert_eq!(
        live_run_args(&[
            "--config",
            "my config.json",
            "--profile",
            "present",
            "--set",
            "font.size=18",
            "--set",
            "cursor.blink=false"
        ])
        .ok(),
        Some(vec![
            "run",
            "--profile",
            "live",
            "-p",
            "puzzle-desktop",
            "--",
            "run",
            "--config",
            "my config.json",
            "--profile",
            "present",
            "--set",
            "font.size=18",
            "--set",
            "cursor.blink=false"
        ])
    );
}

/// The documented wrapper must not reject native headless listing flags.
#[test]
fn introspection_options_are_forwarded() {
    assert_eq!(
        live_run_args(&["--debug", "--list-keybindings", "--list-actions"]).ok(),
        Some(vec![
            "run",
            "-p",
            "puzzle-desktop",
            "--",
            "run",
            "--list-keybindings",
            "--list-actions"
        ])
    );
}
