//! Reject crate-local build or evidence directories before lint traverses sources.
//!
//! Cargo outputs and test screenshots share workspace `target/`. Cargo's config
//! controls compiler output, but tests must still anchor their own artifact paths.
//! Failing here catches either regression without scanning generated Rust code.
use std::{fs, path::Path};

/// Filesystem fixtures protect the crate/workspace distinction without running Cargo.
#[cfg(test)]
mod tests;

/// Reports every misplaced target directory directly inside a workspace crate.
pub(super) fn check(crates: &Path) -> Result<(), String> {
    let mut misplaced = Vec::new();
    for entry in fs::read_dir(crates).map_err(|error| error.to_string())? {
        // Inspect direct crate children only; unrelated trees and workspace target stay untouched.
        let path = entry
            .map_err(|error| error.to_string())?
            .path()
            .join("target");
        // Following directory symlinks for this check also rejects redirected crate-local builds.
        if path.is_dir() {
            misplaced.push(path.display().to_string());
        }
    }
    // Stable ordering makes multi-crate failures reproducible on every filesystem.
    misplaced.sort();
    if misplaced.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "crate-local target directories: {}; use workspace target/ for builds and test evidence",
            misplaced.join(", ")
        ))
    }
}
