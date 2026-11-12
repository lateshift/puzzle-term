//! Instrumented ABI tests use the user-approved pinned nightly (D-43).
//!
//! ASan runs every Rust FFI test plus the real C host with both languages
//! instrumented. Miri interprets only the handle and pointer logic: native GPU
//! startup is cfg-skipped, never replaced by fake acceptance. Neither command
//! installs tools; the approved Miri component is installed separately.
//!
//! Each sanitizer returns the original subprocess output on failure, making a
//! memory violation distinguishable from a compiler or toolchain problem.
//! A successful process must also contain the test harness completion record;
//! command startup alone cannot certify that interpreted tests actually ran.
//! The native caller is rebuilt against the instrumented archive rather than
//! accidentally linking a previously built ordinary library.
//! Separate workspace target subdirectories keep incompatible compiler flags
//! from invalidating the normal host artifacts or taking a crate-local build lock.
//! Miri deliberately excludes native App construction via cfg(miri); it still
//! checks range validation, UTF-8 copying and generation retirement directly.
use std::process::Command;
/// Executes all exported-call tests and the native C host with address instrumentation.
pub(crate) fn asan() -> Result<(), String> {
    asan_features(false)
}
/// Both languages link the matching feature-enabled instrumented archive.
pub(super) fn asan_features(extra_protocols: bool) -> Result<(), String> {
    let mut command = Command::new("cargo");
    command.args([
        "+nightly-2026-08-14",
        "test",
        "--target",
        "aarch64-apple-darwin",
        "-p",
        "puzzle-ffi",
        "--lib",
    ]);
    // The selected test graph must match the corresponding independently linked C archive.
    super::extra_flags(&mut command, false, extra_protocols)?;
    let output = command
        // Keep incompatible sanitizer artifacts away from ordinary workspace builds.
        .env("CARGO_TARGET_DIR", "target/m17-ffi-asan")
        .env("RUSTFLAGS", "-Zsanitizer=address")
        .output()
        .map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err(format!(
            "ASan FFI tests:\n{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    super::run_host(true, false, extra_protocols, None)
}
/// Interprets pure handles and marshalling, including stale/null/oversized misuse.
pub(crate) fn miri() -> Result<(), String> {
    miri_features(false)
}
/// Feature selection never installs a tool or substitutes native GPU work for Miri.
pub(super) fn miri_features(extra_protocols: bool) -> Result<(), String> {
    let mut command = Command::new("cargo");
    command.args([
        "+nightly-2026-08-14",
        "miri",
        "test",
        "-p",
        "puzzle-ffi",
        "--lib",
    ]);
    super::extra_flags(&mut command, false, extra_protocols)?;
    // Miri's sysroot is independent of the host target and maintained by rustup.
    let output = command
        .env("CARGO_TARGET_DIR", "target/m17-ffi-miri")
        .output()
        .map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err(format!(
            "Miri FFI tests:\n{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    if !stdout.contains("test result: ok.") {
        return Err("Miri did not execute tests".into());
    }
    println!("{}", stdout.trim());
    Ok(())
}

/// Uses the pinned Rust LLVM runtime for both languages rather than loading two ASans.
///
/// The dylib comes from the approved installed nightly, never a downloaded tool.
/// An absolute link path and runtime search path keep a different Xcode installation
/// from silently substituting Apple's incompatible compiler-brand version guard.
/// Rust's own version check remains linked and validates the upstream runtime ABI.
/// C loads/stores, stack redzones and allocation interceptors remain instrumented.
/// A missing runtime is an acceptance error, not a fallback to unsanitized linking.
pub(super) fn instrument_c(compiler: &mut Command) -> Result<(), String> {
    // Apple Clang names its version guard differently from upstream LLVM.
    // Its address checks use the same ASan ABI; Rust objects retain the v8 guard.
    // Suppress only the redundant C compiler-brand guard, never memory instrumentation.
    let output = Command::new("rustc")
        .args(["+nightly-2026-08-14", "--print", "target-libdir"])
        .output()
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).into_owned());
    }
    let directory = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    let runtime = format!("{directory}/librustc-nightly_rt.asan.dylib");
    if !std::path::Path::new(&runtime).is_file() {
        return Err(format!("pinned ASan runtime missing: {runtime}"));
    }
    compiler
        .args([
            "-fsanitize=address",
            "-fno-sanitize-link-runtime",
            "-mllvm",
            "-asan-guard-against-version-mismatch=0",
        ])
        .arg(runtime)
        .arg(format!("-Wl,-rpath,{directory}"));
    Ok(())
}
