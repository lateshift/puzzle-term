//! Platform config.json paths and document I/O (§14.4).
//!
//! The store owns policy. This file layer only handles missing files,
//! timestamps and pretty JSON. It does not turn a parse failure into an
//! empty document, which would erase the last good live configuration.
use serde_json::Value as Json;
use std::{
    fs,
    path::{Path, PathBuf},
    time::SystemTime,
};

/// Selects the documented XDG/AppData configuration directory.
pub(super) fn default_path() -> PathBuf {
    if cfg!(target_os = "windows") {
        // APPDATA is the roaming per-user configuration root on Windows;
        // the product directory disambiguates us from other Puzzle tools.
        // A missing environment variable uses cwd only as a last resort,
        // avoiding a guessed home directory with the wrong permissions.
        return std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."))
            .join("Puzzle Terminal/config.json");
    }
    // XDG_CONFIG_HOME wins on macOS and Linux when explicitly provided.
    // Otherwise follow the documented ~/.config/puzzle location so an
    // SSH session and graphical launch find the same personal file.
    let root = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
        .unwrap_or_else(|| PathBuf::from("."));
    root.join("puzzle/config.json")
}

/// Returns None for an absent file and a parse error with line and column.
pub(super) fn read(path: &Path) -> Result<Option<Json>, String> {
    // A missing file is a normal first launch and means schema defaults.
    // Permission and UTF-8 failures are different: they deserve a warning
    // while leaving the already working store untouched.
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("{}: {error}", path.display())),
    };
    // serde_json supplies the editor-friendly line and column of a syntax
    // error; dropping that location would make a broken config hard to fix.
    serde_json::from_str(&text).map(Some).map_err(|error| {
        format!(
            "{}:{}:{}: {error}",
            path.display(),
            error.line(),
            error.column()
        )
    })
}

/// Reports the file's current mtime or None when it does not exist.
pub(super) fn modified(path: &Path) -> Result<Option<SystemTime>, String> {
    // Focus callbacks compare this timestamp; an idle window never polls
    // the filesystem merely to discover a user settings edit (P2).
    match fs::metadata(path) {
        Ok(metadata) => metadata
            .modified()
            .map(Some)
            .map_err(|error| error.to_string()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("{}: {error}", path.display())),
    }
}

/// Writes two-space-indented JSON while preserving unknown user keys.
pub(super) fn write(path: &Path, document: &Json) -> Result<(), String> {
    // The first save or --init-config may need to create the configured
    // parent directory. The file itself is written only after conversion
    // and pretty printing succeed, so those errors cannot truncate it.
    let parent = path.parent().ok_or("config path has no parent")?;
    fs::create_dir_all(parent).map_err(|error| format!("{}: {error}", parent.display()))?;
    let mut text = serde_json::to_string_pretty(document).map_err(|error| error.to_string())?;
    // A trailing newline keeps the generated starter pleasant to edit in
    // ordinary text editors without changing its JSON semantics.
    text.push('\n');
    fs::write(path, text).map_err(|error| format!("{}: {error}", path.display()))
}
