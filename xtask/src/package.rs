//! Enforce the client-only distribution boundary (design §12.5–§12.6).
//!
//! The native PTY bridge can launch a shell and is only a local development
//! tool. Checking the actual package tree catches an accidental copy step,
//! including nested Cargo artifacts and links to a bridge outside the tree.
//! D-37 adds a test-font boundary across all dist outputs, including web assets.
//! The font supports deterministic paint tests, but live emoji use system fonts.
//! Checking copied files catches a future broad fonts-directory copy step.
use std::path::Path;

/// Checks existing package output; absence is valid before M10 packaging exists.
/// Filesystem errors fail the gate instead of treating unreadable output as clean.
/// This inspects copied artifacts, independent of Cargo's selected build target.
pub(crate) fn verify() -> Result<(), String> {
    let root = Path::new("dist");
    // Ordinary source checks must not create a distribution just to inspect it.
    if !root.exists() {
        return Ok(());
    }
    inspect(root)
}

/// Walks directories without following links, preventing filesystem cycles.
/// A failure includes the offending path so the packaging copy step can be fixed.
/// Children are checked recursively because bundles may nest native executables.
fn inspect(path: &Path) -> Result<(), String> {
    // Both Cargo's underscore library name and the executable's hyphen name
    // identify bridge artifacts, even when a copy added a suffix or hash.
    if path
        .components()
        .any(|c| c.as_os_str() == "fonts" && path.to_string_lossy().contains("fonts/test"))
        || path
            .file_name()
            .is_some_and(|n| n == "test_glyphs-glyf_colr_1.ttf")
    {
        // D-37 fixtures are never copied into any shipped distribution tree.
        return Err(format!(
            "test font fixture in distribution: {}",
            path.display()
        ));
    }
    if bridge_name(path) {
        return Err(format!(
            "development bridge artifact in package: {}",
            path.display()
        ));
    }
    let kind = std::fs::symlink_metadata(path).map_err(|error| error.to_string())?;
    if kind.is_symlink() {
        // A harmless filename must not conceal a link to the native tool.
        let target = std::fs::read_link(path).map_err(|error| error.to_string())?;
        if bridge_name(&target) {
            return Err(format!(
                "package link exposes development bridge: {}",
                path.display()
            ));
        }
    } else if kind.is_dir() {
        for entry in std::fs::read_dir(path).map_err(|error| error.to_string())? {
            inspect(&entry.map_err(|error| error.to_string())?.path())?;
        }
    }
    Ok(())
}

/// Searches every component so a Cargo output directory cannot hide its payload.
fn bridge_name(path: &Path) -> bool {
    path.components().any(|component| {
        let name = component.as_os_str().to_string_lossy();
        name.contains("puzzle-pty-bridge") || name.contains("puzzle_pty_bridge")
    })
}

/// Negative package fixtures prevent the check from becoming a no-op.
#[cfg(test)]
mod tests;
