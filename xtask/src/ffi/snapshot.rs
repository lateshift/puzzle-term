//! Snapshot timing uses optimized product code; functional C checks keep their asserts.
//!
//! This runs only the dedicated snapshot branch of the shared native consumer.
//! Building the static library and setting up the 5K pane are outside the copy
//! interval. Per-call measurements remain in the receipt, one per native host kind.
//! The ordinary host suite still uses a debug archive and functional hang guards.
//! Selecting this branch changes only which fixture runs; no production behavior
//! depends on its environment variable. Both branches compile the same header.
//! Pixel setup waits for actual native contents, never an assumed render delay.
//! The receipt includes source/output dimensions so a tiny pane cannot certify
//! the 5K copy budget. The C caller asserts every measured call against 4 ms.
use std::{fs, process::Command};

/// One C process proves both NSView and borrowed-layer paths; no other live cohorts run.
pub(crate) fn run() -> Result<(), String> {
    let binary = build()?;
    // The child selects only snapshot setup/copy checks, not the complete C-host suite.
    // GPU validation errors are fatal here, even though product recovery is nonfatal.
    let output = Command::new(&binary)
        .env("PUZZLE_SNAPSHOT_REPORT", "1")
        .env("PUZZLE_GPU_FAIL_ON_ERROR", "1")
        .output()
        .map_err(|error| error.to_string())?;
    // Retain both streams before interpreting exit status: a native assertion
    // must remain diagnosable instead of becoming an unhelpful missing-token error.
    let receipt = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    fs::write("target/m12-tabs/snapshot/host.log", &receipt).map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(format!("snapshot C host: {receipt}"));
    }
    // One token covers all functional assertions; two measurements prove both
    // target contracts ran. Success without the dedicated branch is insufficient.
    if !receipt.contains("M12_SNAPSHOT_PASS") || receipt.matches("M12_SNAPSHOT_BUDGET").count() != 2
    {
        return Err("missing both native snapshot budget receipts".into());
    }
    print!("{receipt}");
    Ok(())
}
/// Build the shared consumer against optimized Rust without disabling C assertions.
fn build() -> Result<String, String> {
    super::verify()?;
    let root = "target/m12-tabs/snapshot";
    fs::create_dir_all(root).map_err(|error| error.to_string())?;
    // Rustc supplies the archive's actual framework list, keeping Metal and
    // IOSurface requirements synchronized with the workspace dependency graph.
    let (archive, flags) = super::static_library(false, false, false, true)?;
    let binary = format!("{root}/host");
    let output = Command::new("clang")
        .args([
            "-Wall",
            "-Wextra",
            "-Werror",
            "-fobjc-arc",
            "-O2",
            "-Icrates/puzzle-ffi/include",
            "crates/puzzle-ffi/tests/host.c",
            "crates/puzzle-ffi/tests/native.m",
        ])
        // Static archive comes before its native libraries for linker resolution.
        .arg(archive)
        .args(flags.split_whitespace())
        .args(["-framework", "AppKit", "-o", &binary])
        .output()
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).into_owned());
    }
    // This executable is separate from debug/ASan variants; task timing can
    // never accidentally select a previous unoptimized host binary.
    Ok(binary)
}
