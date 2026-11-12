//! Third-party notice generation from Cargo's resolved dependency tree (design §2.6).
//!
//! Cargo's tree command already knows the versions and license expressions
//! selected by the lockfile. A `name|license` delimiter preserves SPDX
//! expressions containing spaces such as `MIT OR Apache-2.0`.
//! The `--prefix none` option removes tree glyphs so each record can be read
//! without guessing how many dependency levels precede the package name.
//! Repeated transitive appearances are collapsed before the notice is written.
//!
//! Slug, font and theme attributions are authored ahead of the crate section;
//! regenerating dependencies preserves that header and its pinned sources.
use std::{collections::BTreeSet, fs, process::Command};
/// Regenerates the crate list while retaining permanent asset attribution.
/// The output is sorted so the same lockfile yields a stable review diff.
pub(crate) fn run() -> Result<(), String> {
    // Cargo supplies resolved package IDs and license expressions; no parser crate is needed.
    let output = Command::new("cargo")
        .args([
            "tree",
            "--workspace",
            // Optional shipped protocols need notices even when today's host
            // default feature set does not link them (M19.3/R4).
            "--all-features",
            "--prefix",
            "none",
            "--format",
            "{p}|{l}",
        ])
        .output()
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).into_owned());
    }
    // A set removes repeated appearances of transitive crates.
    let mut crates = BTreeSet::new();
    // Cargo emits one package with its SPDX license expression per line.
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let trimmed = line.trim_end_matches(" (*)");
        if trimmed.starts_with("puzzle-") || trimmed.starts_with("xtask ") {
            // Workspace-owned code is covered by the root MIT license.
            continue;
        }
        if let Some((name, license)) = trimmed.split_once('|')
            && !name.is_empty()
            && license != "Unknown"
        {
            crates.insert(format!("- {name} — {license}"));
        }
    }
    // Palette sources are authored separately from Cargo dependencies. Keep
    // every asset credit verbatim instead of rebuilding a stale static header.
    let existing = fs::read_to_string("THIRD_PARTY_NOTICES.md").map_err(|e| e.to_string())?;
    let (credits, crate_section) = existing
        .split_once("## Crates\n")
        .ok_or("missing Crates section")?;
    // Retain the generation description; only Cargo's list is regenerated.
    let description = crate_section
        .split_once("- ")
        .ok_or("missing crate list")?
        .0;
    let mut notice = format!("{credits}## Crates\n{description}");
    for line in crates {
        notice.push_str(&line);
        notice.push('\n');
    }
    // The generated result is a single reviewable artifact.
    fs::write("THIRD_PARTY_NOTICES.md", notice).map_err(|error| error.to_string())
}
