//! Selects OS conventions; the effective table and matcher live in puzzle-term.
//!
//! Core parsing resolves primary before layer merge, so config, CLI and live
//! input use identical trigger identities. No exact-match default lookup
//! remains in the window adapter (M7.3, §14.9).
pub use puzzle_term::keybind::Platform;

/// Chooses the user OS here, keeping target checks outside the core (R9).
pub fn current() -> Platform {
    if cfg!(target_os = "macos") {
        Platform::Mac
    } else if cfg!(target_os = "windows") {
        Platform::Windows
    } else {
        Platform::Linux
    }
}

#[cfg(test)]
/// The M6 byte contracts now exercise the shared table instead of a host copy.
mod tests;
