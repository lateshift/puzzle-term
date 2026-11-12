//! Login-shell options for native embedding hosts (D-43).
use super::*;
use std::{
    ffi::{OsStr, OsString},
    path::Path,
};

/// Spawns a login shell with host-selected cwd and additional environment values.
/// Puzzle applies its terminal identity afterward, so overrides cannot advertise unsupported modes.
pub fn spawn_login_options(
    size: PtySize,
    appearance: Appearance,
    shell: Option<&OsStr>,
    cwd: Option<&Path>,
    env: &[(OsString, OsString)],
) -> Result<SpawnedPty, String> {
    // Diagnostic callers use the same default as a new native surface. App's
    // configured path below supplies its resolved next-session setting instead.
    spawn_login_settings(size, appearance, shell, cwd, env, "detect")
}

/// Applies next-session integration policy before native spawn; host transports never call here.
pub(crate) fn spawn_login_settings(
    size: PtySize,
    appearance: Appearance,
    shell: Option<&OsStr>,
    cwd: Option<&Path>,
    env: &[(OsString, OsString)],
    integration: &str,
) -> Result<SpawnedPty, String> {
    // Native shell selection accepts OS strings, including non-UTF-8 executable paths.
    // An unset SHELL preserves the same portable /bin/sh fallback as the harness.
    let default_shell = std::env::var_os("SHELL").unwrap_or_else(|| "/bin/sh".into());
    let mut command = CommandBuilder::new(shell.unwrap_or(&default_shell));
    if cfg!(unix) {
        // A native host gets the same shell initialization files as the harness.
        command.arg("-l");
    }
    // An absent cwd inherits the embedding process directory; there is no hidden
    // home-directory change that could surprise an editor or SSH front end.
    if let Some(cwd) = cwd {
        command.cwd(cwd);
    }
    // Environment values stay native OsStrings, so Unix hosts need not round-trip
    // path bytes through UTF-8 merely to start their existing shell.
    for (key, value) in env {
        command.env(key, value);
    }
    // Startup redirects belong after host overrides: preserved ENV/ZDOTDIR/XDG
    // values must be the ones this child would otherwise inherit. No shell
    // command is interpolated, so spaces and native path bytes remain literal.
    #[cfg(unix)]
    super::integration::apply(&mut command, integration);
    #[cfg(not(unix))]
    let _ = integration;
    // The common constructor installs terminfo and COLORFGBG after host additions.
    spawn_with_appearance(command, size, None, appearance, None)
}

// Additional child constructors are isolated from login-shell semantics.
include!("login/optional_items.rs");
