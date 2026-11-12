//! A PTY child belongs to Puzzle, even when its host started inside another terminal.
//!
//! Terminal-specific environment markers can override protocol detection in real
//! clients such as yazi. Remove only those markers, keeping PATH, locale, shell
//! configuration and user application settings. The caller then writes Puzzle's
//! TERM, COLORTERM, TERM_PROGRAM and version before spawning the login shell.
use portable_pty::CommandBuilder;

/// Removes inherited terminal identities/private handoffs, never host environment.
pub(super) fn clean(command: &mut CommandBuilder) {
    // Additional launch-policy cleanup is independent of terminal identity.
    strip_extra(command);
    // Iterate the builder's full environment: explicit host command overrides can
    // carry the same stale markers as the process environment inherited at creation.
    let keys: Vec<_> = command
        .iter_full_env_as_str()
        .filter(|(key, _)| foreign(key))
        .map(|(key, _)| key.to_owned())
        .collect();
    // Collect before removal because the iterator borrows the command's map.
    // This runs once per child, outside parser, frame and steady-state input work.
    for key in keys {
        command.env_remove(key);
    }
}

/// Prefixes cover terminal versions, window IDs, IPC sockets and integration hints.
fn foreign(key: &str) -> bool {
    // Uppercase comparison also matches Windows' case-insensitive environment keys.
    let key = key.to_ascii_uppercase();
    // IPC endpoints and resource directories can select a client's proprietary
    // terminal fast path even after TERM_PROGRAM has been replaced with Puzzle.
    [
        "GHOSTTY_",
        "KITTY_",
        "ITERM_",
        "WEZTERM_",
        "KONSOLE_",
        "ALACRITTY_",
        "RIO_",
        "WT_",
    ]
    .iter()
    .any(|prefix| key.starts_with(prefix))
        || matches!(
            key.as_str(),
            "TERM_PROGRAM"
                | "TERM_PROGRAM_VERSION"
                | "TERM_SESSION_ID"
                | "LC_TERMINAL"
                | "LC_TERMINAL_VERSION"
                | "VTE_VERSION"
                | "TERMINATOR_UUID"
                | "TERMINOLOGY"
                // A newly owned PTY is outside the launcher's multiplexer pane.
                // Keeping these makes clients wrap graphics for the wrong byte stream.
                | "TMUX"
                | "TMUX_PANE"
                | "STY"
                | "WINDOWID"
        )
}

// Optional launch environment is a complete replaceable cold-path component.
include!("identity/optional_items.rs");
