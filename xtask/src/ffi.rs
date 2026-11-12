//! Hand-written C header checks and a real native host (M17.ffi, D-43).
//!
//! The source catalogue catches omitted declarations before linking. The C
//! translation unit is compiled independently with strict warnings, so Rust
//! cannot hide a wrong caller signature. Acceptance additionally links the
//! actual static library and runs the host through every exported operation.
//! Native framework flags come from rustc, rather than a second handwritten
//! dependency catalogue which could silently become stale after a GPU update.
//! All build products live below the workspace target directory.
//! A C fixture may split related calls into an included .inc file.
//! Only files actually included by host.c contribute to the call-site catalogue.
//! The same files are compiled by clang in the strict translation unit.
//! Unreferenced helper files cannot claim coverage for a newly exported function.
//! The include scan retains literal local paths rather than enumerating a directory.
//! Each included source is appended only for identifier coverage, not execution.
//! The linked executable remains the independent proof of successful C calls.
//! Header-only declarations and export-only definitions still fail set equality.
//! Adding MIME calls does not relax the existing lifecycle and pointer witnesses.
//! ASan links the same host fixture, including those MIME call sites.
//! Miri separately exercises pointer validation without calling native AppKit.
use std::{collections::BTreeSet, fs, process::Command};
/// ASan and Miri are separate from ordinary debug functional verification.
mod sanitizers;
pub(crate) use sanitizers::{asan, miri};
/// Release-only last-presented snapshot budget and the same portable C consumer.
mod snapshot;
pub(crate) use snapshot::run as snapshot;

/// Header and consumer discovery follows actual local include edges.
mod catalogue;
use catalogue::catalogue;
/// Compiles the actual C program without linking or requiring a display session.
///
/// Compilation uses the authoritative installed header directly, not copied structs.
/// Warning-as-error checks catch mismatched callback and argument declarations.
/// The object stays in workspace target, alongside native acceptance artifacts.
pub(crate) fn verify() -> Result<(), String> {
    // No native window is needed for the check-time declaration/type witness.
    let count = catalogue()?;
    fs::create_dir_all("target/m17-ffi").map_err(|e| e.to_string())?;
    crate::commands::quiet(
        "clang",
        &[
            "-std=c11",
            "-Wall",
            "-Wextra",
            "-Werror",
            "-Icrates/puzzle-ffi/include",
            "-c",
            "crates/puzzle-ffi/tests/host.c",
            "-o",
            "target/m17-ffi/host.o",
        ],
    )?;
    println!("M17_FFI_HEADER {count} exports/declarations/C calls; strict C compile PASS");
    Ok(())
}
/// Builds, links and runs the real AppKit fixture against the selected staticlib.
pub(crate) fn host() -> Result<(), String> {
    verify()?;
    run_host(false, false, false, None)
}
/// Rustc supplies the native dependency flags for exactly this instrumented archive.
///
/// Metal's framework requirements belong to wgpu's selected backend features.
/// Asking rustc avoids maintaining another framework list in the C test harness.
/// Sanitized builds use a separate target subtree because their ABI instrumentation
/// and pinned nightly compiler cannot share ordinary Cargo object artifacts.
fn static_library(
    asan: bool,
    status_protocol: bool,
    extra_protocols: bool,
    release: bool,
) -> Result<(String, String), String> {
    let directory = if asan {
        "target/m17-ffi-asan"
    } else {
        "target"
    };
    // Construct argv directly: framework flags never pass through shell expansion.
    let mut build = Command::new("cargo");
    if asan {
        build.arg("+nightly-2026-08-14");
    }
    build.arg("rustc");
    if asan {
        // Explicit target leaves host build scripts and proc macros uninstrumented.
        build
            .args(["--target", "aarch64-apple-darwin"])
            .env("RUSTFLAGS", "-Zsanitizer=address")
            .env("CARGO_TARGET_DIR", directory);
    }
    build.args(["-p", "puzzle-ffi"]);
    // Absolute copy budgets use the product's optimized archive. Functional
    // host and sanitizer paths keep their established debug compilation mode.
    if release {
        build.arg("--release");
    }
    extra_flags(&mut build, status_protocol, extra_protocols)?;
    build.args(["--", "--print", "native-static-libs"]);
    let output = build.output().map_err(|e| e.to_string())?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !output.status.success() {
        return Err(format!("C host staticlib build: {stderr}"));
    }
    // Rustc emits this note even though it writes the actual archive to target.
    let flags = stderr
        .lines()
        .find_map(|line| line.strip_prefix("note: native-static-libs: "))
        .ok_or("rustc omitted native link flags")?;
    // Explicit cross-target Cargo layouts insert the architecture below target-dir.
    let target = if asan { "/aarch64-apple-darwin" } else { "" };
    let profile = if release { "release" } else { "debug" };
    Ok((
        format!("{directory}{target}/{profile}/libpuzzle_ffi.a"),
        flags.into(),
    ))
}
/// Sanitized and ordinary hosts differ only in instrumentation/build destinations.
///
/// Both compile the same C input, AppKit fixture and authoritative header.
/// Both require the completion token after every operation, including native close.
/// A zero exit without that token cannot accidentally pass an empty host program.
/// The AppKit framework supplies window ownership; Puzzle owns terminal work.
fn run_host(
    asan: bool,
    status_protocol: bool,
    extra_protocols: bool,
    colour: Option<&str>,
) -> Result<(), String> {
    if !cfg!(target_os = "macos") {
        return Err("real C host fixture currently requires the macOS development machine".into());
    }
    let (archive, flags) = static_library(asan, status_protocol, extra_protocols, false)?;
    let binary = if asan {
        "target/m17-ffi/host-asan"
    } else {
        "target/m17-ffi/host"
    };
    // The second host proves runtime registration through the optional C export.
    // The default variant separately proves absence returns Unsupported.
    // Both use identical input, native-lifetime and callback-affinity assertions.
    // ARC keeps the AppKit fixture alive until its explicit teardown call.
    let mut compiler = Command::new("clang");
    compiler.args([
        "-Wall",
        "-Wextra",
        "-Werror",
        "-fobjc-arc",
        "-Icrates/puzzle-ffi/include",
        "crates/puzzle-ffi/tests/host.c",
        "crates/puzzle-ffi/tests/native.m",
    ]);
    extra_defines(&mut compiler, status_protocol, extra_protocols)?;
    // Keep archive before its native libraries; static linkage is order-dependent.
    compiler.arg(archive).args(flags.split_whitespace()).args([
        "-framework",
        "AppKit",
        "-o",
        binary,
    ]);
    if asan {
        sanitizers::instrument_c(&mut compiler)?;
    }
    let output = compiler.output().map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).into_owned());
    }
    // Capture forwards native assertion failures, rather than translating them to PASS.
    let output = host_output(binary, colour)?;
    if !output.contains("M17_C_HOST_PASS") {
        return Err(format!("C host omitted completion: {output}"));
    }
    // Keep the host completion in the acceptance receipt for later reviewer inspection.
    println!("{}", output.trim());
    Ok(())
}
#[cfg(test)]
/// Source-set mismatches fail before costly native initialization.
mod tests;

/// Real acquired-drawable checks through both C native target contracts.
pub(crate) fn surface_colour(path: &str) -> Result<(), String> {
    run_host(false, false, false, Some(path))
}

/// Per-child diagnostic options never mutate the gate's process-global environment.
/// The same executable retains the ordinary full-ABI witness when no colour path
/// is supplied. A colour receipt selects the smaller native presentation fixture.
/// Both modes preserve process exit status and original assertion diagnostics.
/// This helper owns neither native targets nor their lifetime; the C host joins
/// every worker before returning its completion token to the parent gate.
fn host_output(binary: &str, colour: Option<&str>) -> Result<String, String> {
    let output = if let Some(path) = colour {
        // Per-child environment avoids mutating process-global state (Rust 2024/R8).
        let output = Command::new(binary)
            .env("PUZZLE_SURFACE_COLOUR_REPORT", path)
            .env("PUZZLE_GPU_FAIL_ON_ERROR", "1")
            .output()
            .map_err(|e| e.to_string())?;
        // Retain metadata even when a native assertion rejects nil or P3.
        fs::write(format!("{path}.host.log"), &output.stdout).map_err(|e| e.to_string())?;
        // A crashed host, rejected configuration or mapping failure must remain
        // a failing criterion, even if earlier surfaces wrote valid receipts.
        if !output.status.success() {
            return Err(format!(
                "native colour host: {}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            ));
        }
        String::from_utf8_lossy(&output.stdout).into_owned()
    } else {
        traced_host(binary)?
    };
    // The caller verifies the completion token after all native targets close.
    Ok(output)
}

/// Retains scheduler receipts for an intermittent native-to-host takeover failure.
/// The subscriber exists only in debug FFI builds and never changes pacing policy.
fn traced_host(binary: &str) -> Result<String, String> {
    // Per-process identities keep simultaneous sanitizer receipts independent.
    let root = "target/m19-c-host-traces";
    fs::create_dir_all(root).map_err(|error| error.to_string())?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|error| error.to_string())?
        .as_nanos();
    let path = format!("{root}/{}-{stamp}.log", std::process::id());
    // The environment belongs to this C child only, never an absolute-budget binary.
    // Keep scheduler receipts separate from stdout so the last assertion is not lost.
    // The C fixture's unchanged guards remain the only functional hang timeouts.
    let output = Command::new(binary)
        .env("PUZZLE_C_HOST_TRACE", &path)
        .output()
        .map_err(|error| error.to_string())?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    fs::write(format!("{path}.host.log"), format!("{stdout}{stderr}"))
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        // Preserve the actual assertion and add its independently captured clock file.
        return Err(format!(
            "{binary} failed: {stdout}{stderr}; scheduler trace: {path}"
        ));
    }
    println!("M19_C_HOST_TRACE {path}");
    Ok(stdout.into_owned())
}

// Optional build variants do not alter ordinary C compilation or linkage.
include!("ffi/optional_items.rs");
