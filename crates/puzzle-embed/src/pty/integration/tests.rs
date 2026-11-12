//! Startup environment construction preserves user values without spawning a shell.
#![allow(
    clippy::expect_used,
    reason = "fixture setup and environment receipts must fail with a useful message"
)]
use super::*;

/// Environment tests exercise the supported route independently of installed bash.
fn apply(command: &mut CommandBuilder, setting: &str) {
    super::apply_with(command, setting, |_| true);
}

/// Version classification follows the user-approved Apple exception, including copies.
#[test]
fn integration_bash_version_policy() {
    let apple = b"GNU bash, version 3.2.57(1)-release (arm64-apple-darwin26)\n";
    assert!(!env_capable(apple, true));
    assert!(env_capable(apple, false));
    assert!(env_capable(
        b"GNU bash, version 5.3.3(1)-release (aarch64-apple-darwin)\n",
        true
    ));
    assert!(!env_capable(b"not a supported bash version", true));
    let mut command = command("/renamed/old-shell");
    let before = command.clone();
    super::apply_with(&mut command, "bash", |_| false);
    assert_eq!(command.get_argv(), before.get_argv());
    assert!(command.get_env("ENV").is_none());
    assert!(command.get_env("PUZZLE_SHELL_INTEGRATION").is_none());
}

/// Isolated builders avoid mutating the test process environment in parallel tests.
fn command(shell: &str) -> CommandBuilder {
    let mut command = CommandBuilder::new(shell);
    command.env_clear();
    command.env("HOME", "/home/test user");
    command.arg("-l");
    command
}

/// Detection uses the executable name, not a substring or inherited terminal brand.
#[test]
fn integration_selection_is_explicit() {
    for (executable, setting, expected) in [
        ("/bin/bash", "detect", Some(Shell::Bash)),
        ("/bin/zsh", "detect", Some(Shell::Zsh)),
        ("/opt/homebrew/bin/fish", "detect", Some(Shell::Fish)),
        ("/bin/sh", "detect", None),
        ("/bin/not-bash", "detect", None),
        ("/bin/zsh", "none", None),
        ("/custom/wrapper", "bash", Some(Shell::Bash)),
        ("/custom/wrapper", "zsh", Some(Shell::Zsh)),
        ("/custom/wrapper", "fish", Some(Shell::Fish)),
    ] {
        assert_eq!(select(OsStr::new(executable), setting), expected);
    }
    for (executable, setting) in [("/bin/sh", "detect"), ("/bin/zsh", "none")] {
        let mut command = command(executable);
        let before = command.clone();
        apply(&mut command, setting);
        assert_eq!(command.get_argv(), before.get_argv());
        assert!(command.get_env("PUZZLE_SHELL_INTEGRATION").is_none());
        assert!(command.get_env("ZDOTDIR").is_none());
    }
}

/// ENV is restored exactly and POSIX startup does not switch the user's history file.
#[test]
fn integration_bash_environment() {
    for original in [None, Some(""), Some("/custom/env file")] {
        let mut command = command("/bin/bash");
        if let Some(value) = original {
            command.env("ENV", value);
        }
        apply(&mut command, "detect");
        assert_eq!(
            command.get_env("PUZZLE_SAVED_ENV"),
            original.map(OsStr::new)
        );
        assert_eq!(command.get_argv(), &["/bin/bash", "--posix", "-l"]);
        assert!(
            command
                .get_env("ENV")
                .expect("bootstrap")
                .to_string_lossy()
                .ends_with("bash/bootstrap.sh")
        );
        assert_eq!(
            command.get_env("HISTFILE"),
            Some(OsStr::new("/home/test user/.bash_history"))
        );
        assert_eq!(
            command.get_env("PUZZLE_UNEXPORT_HISTFILE"),
            Some(OsStr::new("1"))
        );
    }
    let mut command = command("/bin/bash");
    command.env("HISTFILE", "/custom/history");
    apply(&mut command, "detect");
    assert_eq!(
        command.get_env("HISTFILE"),
        Some(OsStr::new("/custom/history"))
    );
    assert!(command.get_env("PUZZLE_UNEXPORT_HISTFILE").is_none());
}

/// Empty ZDOTDIR and unset ZDOTDIR have different shell meanings and stay distinct.
#[test]
fn integration_zsh_environment() {
    for original in [None, Some(""), Some("/custom/startup dir")] {
        let mut command = command("/bin/zsh");
        if let Some(value) = original {
            command.env("ZDOTDIR", value);
        }
        apply(&mut command, "detect");
        assert_eq!(
            command.get_env("PUZZLE_SAVED_ZDOTDIR"),
            original.map(OsStr::new)
        );
        assert!(
            command
                .get_env("ZDOTDIR")
                .expect("bootstrap")
                .to_string_lossy()
                .ends_with("shell-integration/zsh")
        );
        assert_eq!(command.get_argv(), &["/bin/zsh", "-l"]);
    }
}

/// Fish retains every user/system vendor search path and only prepends our entry.
#[test]
fn integration_fish_environment() {
    for original in [None, Some(""), Some("/one dir:/two")] {
        let mut command = command("/opt/homebrew/bin/fish");
        if let Some(value) = original {
            command.env("XDG_DATA_DIRS", value);
        }
        apply(&mut command, "detect");
        assert_eq!(
            command.get_env("PUZZLE_SAVED_XDG_DATA_DIRS"),
            original.map(OsStr::new)
        );
        let paths = command
            .get_env("XDG_DATA_DIRS")
            .expect("paths")
            .to_string_lossy();
        let tail = original
            .filter(|value| !value.is_empty())
            .unwrap_or("/usr/local/share:/usr/share");
        assert!(paths.ends_with(&format!(":{tail}")));
        assert!(paths.contains("shell-integration:"));
        assert_eq!(command.get_argv(), &["/opt/homebrew/bin/fish", "-l"]);
    }
}
