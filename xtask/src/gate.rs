//! OS-backed exclusion between independent native/check and Chrome gate processes.
//!
//! Cargo releases its build lock before running xtask, so compilation alone cannot
//! prevent a second invocation from starting Chrome alongside native GPU tests.
//! An advisory file lock survives for the entire top-level gate and releases on
//! process exit, including failures. No stale lock file can strand future runs.
use std::fs::{self, File, OpenOptions};

/// Owns a gate lock independently of any descriptor inherited during process startup.
/// Closing one descriptor alone is insufficient if a concurrent child briefly
/// inherited another reference to the same open-file description before exec.
pub(crate) struct Guard {
    /// Explicit unlock retires the lock before this original descriptor closes.
    file: File,
}
impl Drop for Guard {
    /// Unlock the shared open-file description even when another descriptor survives.
    fn drop(&mut self) {
        // A child may still hold a close-on-exec duplicate; it must not prolong this gate.
        if let Err(error) = self.file.unlock() {
            eprintln!("gate unlock failed: {error}");
        }
    }
}

/// Holds the gate for commands that run native tests, Chrome, or both.
/// Build-only and nested harness helpers do not acquire a second lock: their
/// parent acceptance already holds it throughout its sequential child commands.
pub(crate) fn acquire(args: &[String]) -> Result<Option<Guard>, String> {
    let command = args.first().map(String::as_str);
    let gated = matches!(
        command,
        Some("check" | "accept" | "golden" | "unicode-flood" | "app" | "fuzz" | "ffi")
    ) || command == Some("web") && args.get(1).is_some_and(|arg| arg == "test");
    if !gated {
        return Ok(None);
    }
    fs::create_dir_all("target").map_err(|error| error.to_string())?;
    lock_file(std::path::Path::new("target/puzzle-gates.lock")).map(Some)
}

/// Separate file acquisition permits a hermetic test under an already-held check gate.
fn lock_file(path: &std::path::Path) -> Result<Guard, String> {
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(path)
        .map_err(|error| error.to_string())?;
    // The safe standard-library lock blocks only developer gates, never product input.
    file.lock().map_err(|error| error.to_string())?;
    Ok(Guard { file })
}

#[cfg(test)]
/// OS lock lifetime and command selection are checked without starting a GPU.
mod tests;
