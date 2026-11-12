//! Parent terminal markers must not choose graphics adapters in a nested Puzzle.
use super::*;

/// Builder-local contamination avoids mutating process environment in parallel tests.
#[test]
fn terminal_identity_removes_parent_markers_and_preserves_user_environment() {
    let mut command = CommandBuilder::new("/bin/sh");
    let markers = [
        "GHOSTTY_RESOURCES_DIR",
        "GHOSTTY_BIN_DIR",
        "KITTY_WINDOW_ID",
        "KITTY_PID",
        "ITERM_SESSION_ID",
        "ITERM_PROFILE",
        "WEZTERM_PANE",
        "WEZTERM_EXECUTABLE",
        "VTE_VERSION",
        "WT_SESSION",
        "WT_PROFILE_ID",
        "KONSOLE_VERSION",
        "KONSOLE_DBUS_SESSION",
        "ALACRITTY_WINDOW_ID",
        "RIO_WINDOW_ID",
        "TERMINATOR_UUID",
        "TERMINOLOGY",
        "TERM_SESSION_ID",
        "TMUX",
        "TMUX_PANE",
        "STY",
        "WINDOWID",
        "LC_TERMINAL",
        "LC_TERMINAL_VERSION",
    ];
    let markers: Vec<_> = markers
        .into_iter()
        .chain(extra_markers().iter().copied())
        .collect();
    for key in &markers {
        command.env(key, "parent-terminal");
    }
    command.env("TERM", "xterm-ghostty");
    command.env("TERM_PROGRAM", "ghostty");
    command.env("TERM_PROGRAM_VERSION", "parent-version");
    command.env("COLORTERM", "256color");
    command.env("PUZZLE_USER_SETTING", "retained");
    command.env("PATH", "/usr/bin:/bin");
    let name = advertise_terminal(&mut command, None, Appearance::Dark);
    for key in markers {
        assert!(command.get_env(key).is_none(), "inherited {key}");
    }
    for (key, value) in [
        ("TERM", name),
        ("TERM_PROGRAM", "puzzle"),
        ("TERM_PROGRAM_VERSION", env!("CARGO_PKG_VERSION")),
        ("COLORTERM", "truecolor"),
        ("PUZZLE_USER_SETTING", "retained"),
        ("PATH", "/usr/bin:/bin"),
    ] {
        assert_eq!(
            command.get_env(key),
            Some(std::ffi::OsStr::new(value)),
            "{key}"
        );
    }
}

// Extra launch markers are tested only by the distribution which owns them.
include!("identity/optional_items.rs");
