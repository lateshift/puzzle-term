//! The approved fuzz workspace has its own version catalogue (M19.2b/R4).
//! Product dependencies still inherit the root catalogue. Only this exact
//! unpublished, isolated manifest declares the two approved fuzz versions;
//! its actual dependencies must inherit that local catalogue as usual.
use std::path::Path;

/// Validates the new catalogue without exempting actual dependency declarations.
/// Returning true identifies only the catalogue section to the ordinary scanner;
/// libfuzzer-sys.workspace/arbitrary.workspace still undergo inheritance checks.
/// Product manifests get false and retain all their existing root-workspace rules.
/// Literal versions/features are checked here because they define, rather than
/// inherit, this separate workspace's approved development-only catalogue.
pub(super) fn manifest(path: &Path, data: &str) -> Result<bool, String> {
    if path != Path::new("fuzz/Cargo.toml") {
        return Ok(false);
    }
    // Nested version ownership is permitted only because cargo-fuzz is
    // explicitly isolated from the distributed workspace and root lockfile.
    if !data.lines().any(|line| line.trim() == "[workspace]")
        || !data.lines().any(|line| line.trim() == "publish = false")
    {
        return Err("fuzz workspace must be isolated and unpublished".into());
    }
    // Inspect section headers, so comments mentioning a dependency cannot
    // authorize its version. The tiny catalogue has one declaration per line.
    let mut catalogue = false;
    let mut entries = Vec::new();
    for line in data.lines().map(str::trim) {
        if line.starts_with('[') {
            catalogue = line == "[workspace.dependencies]";
        } else if catalogue && !line.is_empty() && !line.starts_with('#') {
            entries.push(line);
        }
    }
    // A wildcard, extra crate or newly enabled derive feature cannot slip into
    // the tool workspace merely by inheriting its own unreviewed declaration.
    let approved = [
        "libfuzzer-sys = { version = \"=0.4.13\", default-features = false, features = [\"link_libfuzzer\"] }",
        "arbitrary = { version = \"=1.4.2\", default-features = false }",
    ];
    if entries != approved {
        return Err("unapproved fuzz dependency catalogue/version/features".into());
    }
    Ok(true)
}

#[cfg(test)]
/// Behavioral fixtures remain separate from the production command/manifest scanner.
mod tests;
