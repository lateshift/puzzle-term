//! Immutable failure bundles survive retries of the same scenario name.
//!
//! The ordinary harness output folder is intentionally predictable for acceptance.
//! Its next run overwrites log.txt and numbered screenshots. Before returning a
//! command failure, copy the observation into an atomically reserved subdirectory
//! that no later run reuses, even when the fresh and retained pixels are identical.
//!
//! Three observations deliberately remain separate: the child's byte transcript,
//! parsed VT contents, and the incremental GPU attachment. A correct input-byte
//! assertion does not prove correct rendering, and matching cold pixels do not
//! prove the recognizer named a glyph correctly. Keeping all three permits that
//! distinction without trying to infer the cause from a later successful retry.
use super::Scenario;
use crate::png;
use std::{
    fs,
    path::{Path, PathBuf},
};

/// Reserves a never-reused evidence folder without a clock or process-global counter.
fn reserve(root: &Path) -> Result<PathBuf, String> {
    fs::create_dir_all(root).map_err(|error| error.to_string())?;
    // Atomic creation also separates two concurrently launched scenario processes.
    for ordinal in 0u64.. {
        let path = root.join(format!("{}-{ordinal}", std::process::id()));
        match fs::create_dir(&path) {
            Ok(()) => return Ok(path),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error.to_string()),
        }
    }
    Err("failure evidence identities exhausted".into())
}

/// Saves the exact failing state, including failures that are recognizer-only.
pub(super) fn capture(state: &Scenario, error: &str, source: &str) -> Result<PathBuf, String> {
    let path = reserve(&state.output.join("failures"))?;
    // Store the error and authored steps before any further GPU work can fail.
    fs::write(path.join("failure.txt"), error).map_err(|error| error.to_string())?;
    fs::write(path.join("scenario.pzs"), source).map_err(|error| error.to_string())?;
    // A prefix can fail despite cold equality. Save the warm attachment always,
    // rather than relying on the cold-diff path to notice a recognition failure.
    if let Some(image) = &state.image {
        png::write(&path.join("warm.png"), image)?;
    }
    // The human-readable grid accompanies a machine-replayable VT serialization.
    // Neither contains the GPU result, so neither substitutes for warm.png.
    if let Some(term) = &state.terminal {
        fs::write(path.join("grid.txt"), super::preview::grid_dump(term))
            .map_err(|error| error.to_string())?;
        // Cursor, palette and modes remain in the VT serialization, unlike a plain grid.
        let view = term.buffer();
        if let Some(range) = view.line_ids() {
            fs::write(
                path.join("contents.vt"),
                view.serialize(range, puzzle_term::vt::view::Format::Vt),
            )
            .map_err(|error| error.to_string())?;
        }
        // A failed cold render must not erase the original error or warm pixels.
        match super::drawn::cold_image(state) {
            Ok(image) => png::write(&path.join("cold.png"), &image)?,
            Err(error) => {
                fs::write(path.join("cold-error.txt"), error).map_err(|error| error.to_string())?
            }
        }
    }
    // These strings came from the failing pass; capture never invokes OCR again.
    if let Some(readback) = &state.drawn_cache {
        fs::write(path.join("drawn.txt"), readback.text()).map_err(|error| error.to_string())?;
        fs::write(path.join("drawn-notes.txt"), readback.notes.join("\n"))
            .map_err(|error| error.to_string())?;
    }
    super::drawn::recognize::failure::capture(state, &path)?;
    // The shell owns the exact input/output transcript; a later child cannot replace it.
    if let Some(super::shell::ShellRun::Active(probe)) = &state.shell {
        probe.save_failure(&path)?;
    }
    Ok(path)
}
