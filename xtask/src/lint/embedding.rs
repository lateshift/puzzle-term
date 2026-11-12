//! M17 host callbacks use App/Surface; diagnostic owners are an explicit opt-in.
//!
//! Rust privacy closes the default embedding crate. The harness enables
//! test-support for reproducible probes, so compilation alone would let an
//! ordinary input helper borrow a diagnostic endpoint. This second boundary
//! inspects every native window callback and rejects those owner identities.
//!
//! Recording, clocks and counters observe work and cannot mutate Session. They
//! stay available to instrumented hosts; the forbidden names below identify
//! terminal/GPU capabilities, rather than all measurement imports indiscriminately.
//! Probes and the deterministic headless harness intentionally construct owners.
//! Their names distinguish them from the ordinary window/input modules and they
//! may use only the feature-gated diagnostics namespace of puzzle-embed.
//!
//! The scan is deliberately independent of feature unification. Cargo enables
//! test-support for the development binary, so checking only a default build
//! would not detect a callback that starts using a diagnostic Session handle.
//! Identifier matching rejects the canonical name in a renamed `use` statement
//! before its alias is used. Whitespace and explanatory comments are masked by
//! the same lexer as the other architectural gates.
//!
//! New descendants of window/ui inherit this rule automatically. They need no
//! hand-maintained per-file allowlist to remain on the supported host API.
use super::comments::scan_line;
use std::{fs, path::Path};

/// Negative fixtures protect both private owner imports and diagnostic backdoors.
#[cfg(test)]
mod tests;

/// Walks native host sources and verifies the sole feature-gated public module.
pub(super) fn check() -> Result<(), String> {
    // This walk never reaches unrelated projects or generated target output.
    walk(Path::new("crates/puzzle-desktop/src/window"))?;
    for file in ["window.rs", "mouse.rs"] {
        let path = Path::new("crates/puzzle-desktop/src").join(file);
        source(
            &path,
            &fs::read_to_string(&path).map_err(|e| e.to_string())?,
        )?;
    }
    // Module privacy is also checked in the default-feature doc compile tests.
    // Here an accidentally added second public module fails even with test-support on.
    let data =
        fs::read_to_string("crates/puzzle-embed/src/lib.rs").map_err(|error| error.to_string())?;
    for line in data.lines().map(|line| scan_line(line).0) {
        if line.trim().starts_with("pub mod ") && line.trim() != "pub mod diagnostics;" {
            return Err("puzzle-embed exposes an internal module as host API".into());
        }
    }
    Ok(())
}

/// Descendant callbacks inherit the same rule when a new UI helper is added.
fn walk(path: &Path) -> Result<(), String> {
    for entry in fs::read_dir(path).map_err(|error| error.to_string())? {
        let path = entry.map_err(|error| error.to_string())?.path();
        if path.is_dir() {
            walk(&path)?;
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            let data = fs::read_to_string(&path).map_err(|error| error.to_string())?;
            source(&path, &data)?;
        }
    }
    Ok(())
}

/// Named workload/observation modules do not participate in ordinary UI routing.
fn diagnostic(path: &Path) -> bool {
    // A test fixture may construct a private owner but cannot escape Rust privacy.
    path.components().any(|part| {
        let name = part.as_os_str().to_string_lossy();
        let name = name.strip_suffix(".rs").unwrap_or(&name);
        name.ends_with("_probe")
            || matches!(
                name,
                "tests" | "live" | "observer" | "perf" | "record" | "pacing"
            )
    })
}

/// Comments and strings cannot turn a discussion of ownership into an import.
fn source(path: &Path, data: &str) -> Result<(), String> {
    if diagnostic(path) {
        return Ok(());
    }
    let code = data
        .lines()
        .map(|line| scan_line(line).0)
        .collect::<Vec<_>>()
        .join("\n");
    let words = code.split(|ch: char| !ch.is_alphanumeric() && ch != '_');
    // Reject the imported canonical name before `as`, so aliases cannot hide it.
    // Facade module names are checked too: a renamed import of the whole module
    // otherwise hides the endpoint type used on a later line.
    for word in words {
        if matches!(
            word,
            "Session"
                | "SessionMessage"
                | "SurfaceUpdate"
                | "FrameBuilder"
                | "TextContext"
                | "Handle"
                | "Frames"
                | "session_thread"
                | "render_thread"
                | "pty_pool"
                | "system_fonts"
                | "grid_rasterizer"
                | "diagnostics"
        ) {
            return Err(format!(
                "{}: M17 host must use App/Surface, not {word}",
                path.display()
            ));
        }
    }
    // Winit's keyboard module describes OS facts and is legitimate here; only
    // the engine adapter facade would expose a second mutable translation owner.
    let compact: String = code.chars().filter(|ch| !ch.is_whitespace()).collect();
    if compact.contains("crate::keyboard") {
        return Err(format!(
            "{}: native key translation belongs to Surface",
            path.display()
        ));
    }
    Ok(())
}
