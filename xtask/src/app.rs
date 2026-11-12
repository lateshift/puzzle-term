//! D-56: build the native C library and the independently maintained AppKit host.
//!
//! The Xcode configuration reads the authoritative header in place and links one
//! release archive below workspace target/apps-macos. No copied header or crate-
//! local target directory can drift from the ABI checked by the Rust task gates.
//! Host scripts own their tests; this command invokes them without editing them.
//!
//! Puzzle.xcconfig expects target/apps-macos/release/libpuzzle_ffi.a relative to
//! the repository root. An absolute Cargo target path preserves that contract
//! even though Xcode and the scripts run from the app's project directory.
//! PUZZLE_ENABLED=YES prevents a successful placeholder-only build from standing
//! in for the real static-library integration. Code signing is unnecessary for
//! local test builds; the app keeps its own hardened-runtime configuration.
//! Metadata tests validate host model and C records without opening windows.
//! The separate native script needs the logged-in desktop and dispatches AppKit.
//! Both scripts must succeed; compilation alone is not the native lifecycle gate.
//! They run sequentially under xtask's native/Chrome exclusion, without a second
//! gate lock in this nested helper that would deadlock milestone acceptance.
use std::{path::Path, process::Command};

/// Builds the opted-in app, then runs its headless and native integration scripts.
pub(crate) fn run(args: &[String]) -> Result<(), String> {
    if args != ["macos"] {
        return Err("usage: cargo xtask app macos".into());
    }
    if !cfg!(target_os = "macos") {
        return Err("the AppKit example requires macOS and Xcode".into());
    }
    // Resolve from the workspace, never an inherited crate-local Cargo directory.
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or("xtask has no workspace parent")?;
    let target = root.join("target/apps-macos");
    let host = root.join("apps/macos");
    let mut cargo = Command::new("cargo");
    cargo.current_dir(root).env("CARGO_TARGET_DIR", &target);
    cargo.args(["build", "--release", "-p", "puzzle-ffi"]);
    execute(&mut cargo)?;
    // Missing linkage is a build failure, never a fallback to placeholder mode.
    let mut xcode = Command::new("xcodebuild");
    xcode
        .current_dir(&host)
        .args([
            "-project",
            "PuzzleShell.xcodeproj",
            "-scheme",
            "PuzzleShell",
            "-configuration",
            "Debug",
            "-destination",
            "platform=macOS",
            "-derivedDataPath",
        ])
        .arg(target.join("xcode"))
        .args(["PUZZLE_ENABLED=YES", "CODE_SIGNING_ALLOWED=NO", "build"]);
    execute(&mut xcode)?;
    for script in ["Scripts/test-metadata.sh", "Scripts/test.sh"] {
        // The second script intentionally runs actual NSWindows, not compile-only.
        execute(Command::new("bash").current_dir(&host).arg(script))?;
    }
    println!("M17_APP_MACOS_PASS release staticlib, enabled Xcode build and host test scripts");
    Ok(())
}

/// Keeps every child failure visible and stops dependent build/test operations.
fn execute(command: &mut Command) -> Result<(), String> {
    // Inherit output: native test assertions and Xcode diagnostics belong in receipts.
    let status = command.status().map_err(|error| error.to_string())?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{command:?} failed: {status}"))
    }
}
